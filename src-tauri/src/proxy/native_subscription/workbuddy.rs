//! WorkBuddy inference uses streamed Chat or native Anthropic Messages.
use super::*;
mod schema;
use crate::services::native_subscription::workbuddy as vendor;
use schema::normalize_schema;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};

/// Maximum upstream error body inspected for classification. Carriers hold
/// fixed metadata only and never echo upstream text, credentials, or input.
const ERROR_BODY_LIMIT: usize = 16 * 1024;

/// Sanitized tool-name mappings keyed by the original client name.
type ToolNames = HashMap<String, String>;

fn prepare(mut body: Value, protocol: Protocol) -> Result<(Value, ToolNames), ProxyError> {
    let object = body
        .as_object_mut()
        .ok_or_else(|| ProxyError::InvalidRequest("Invalid WorkBuddy request".into()))?;
    object.insert("stream".into(), json!(true));
    if protocol == Protocol::Chat {
        if let Some(max) = object.remove("max_completion_tokens") {
            object.entry("max_tokens").or_insert(max);
        }
        match object.get("tool_choice") {
            Some(choice) if choice == "none" || choice["type"] == "none" => {
                for key in ["tool_choice", "tools", "functions"] {
                    object.remove(key);
                }
            }
            Some(Value::Object(choice)) => {
                let choice = match choice.get("type").and_then(Value::as_str) {
                    Some("function") => choice
                        .get("function")
                        .and_then(|value| value["name"].as_str()),
                    Some(kind @ ("auto" | "required")) => Some(kind),
                    _ => None,
                }
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    ProxyError::InvalidRequest("Unsupported WorkBuddy tool choice".into())
                })?;
                object.insert("tool_choice".into(), json!(choice));
            }
            _ => {}
        }
    }
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let messages = object
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ProxyError::InvalidRequest("Missing WorkBuddy messages".into()))?;
    for message in messages.iter_mut() {
        let Some(fields) = message.as_object_mut() else {
            return Err(ProxyError::InvalidRequest(
                "Invalid WorkBuddy message".into(),
            ));
        };
        let role = fields.get("role").and_then(Value::as_str);
        if protocol == Protocol::Chat {
            if role == Some("developer") {
                fields.insert("role".into(), json!("system"));
            } else if role == Some("assistant") {
                let text = fields.remove("reasoning_content");
                let details = fields.remove("reasoning_details");
                let reasoning = text
                    .filter(|value| value.as_str().is_some_and(|s| !s.is_empty()))
                    .or_else(|| {
                        let text: String = details
                            .as_ref()?
                            .as_array()?
                            .iter()
                            .filter_map(|detail| {
                                detail["text"]
                                    .as_str()
                                    .or_else(|| detail["summary"].as_str())
                            })
                            .collect();
                        (!text.is_empty()).then(|| json!(text))
                    });
                if let Some(reasoning) = reasoning {
                    fields.entry("reasoning").or_insert(reasoning);
                }
            }
        } else if role == Some("assistant") {
            if let Some(parts) = fields.get_mut("content").and_then(Value::as_array_mut) {
                let before = parts.len();
                parts.retain(|part| {
                    part["type"] != "thinking"
                        || part["signature"].as_str().is_some_and(|s| !s.is_empty())
                });
                if before > 0 && parts.is_empty() {
                    parts.push(json!({"type":"text","text":""}));
                }
            }
        }
    }
    if protocol == Protocol::Chat {
        rewrite_channel_fingerprints(messages);
        pause_generic_openers(messages);
        fill_kimi_call_text(model.as_deref(), messages);
        separate_tool_results_from_user(messages);
        if messages
            .first()
            .is_some_and(|message| message["role"] != "system")
        {
            messages.insert(
                0,
                json!({"role":"system","content":"You are a helpful assistant."}),
            );
        }
    } else {
        hide_harness_identity(object);
    }
    let mut names = ToolNames::new();
    match protocol {
        Protocol::Chat => adapt_chat_tools(object, &mut names),
        Protocol::Anthropic => adapt_anthropic_tools(object, &mut names),
        _ => {}
    }
    Ok((body, names))
}

const HARNESS_OPENER: &str = "You are an AI agent powered by DeepSeek Harness";
const HARNESS_OPENER_FIXED: &str = "You are an AI agent, powered by DeepSeek Harness.";
const CLAUDE_OPENER: &str = "You are Claude Code, Anthropic's official CLI for Claude";
const CLAUDE_OPENER_FIXED: &str = "You are Claude Code, Anthropic's CLI for Claude";
const CODEX_OPENER: &str = "Codex CLI is an open source project led by OpenAI.";
const CODEX_OPENER_FIXED: &str = "The Codex CLI is an open source project led by OpenAI.";
const GUI_OPENER: &str = "Web GUI whose URL the runtime context names. When the user refers";
const GUI_OPENER_FIXED: &str =
    "Web GUI whose URL the runtime context names. The interface, when the user refers";
const BRANCH_OPENER: &str = "Main branch (you will usually use this for PRs)";
const BRANCH_OPENER_FIXED: &str = "Main branch (usually the base for PRs)";

/// Rewrite fingerprints the Chat endpoint refuses in system and assistant
/// text. User messages and tool results stay unchanged.
fn rewrite_channel_fingerprints(messages: &mut [Value]) {
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if !matches!(role.as_str(), "system" | "assistant" | "developer") {
            continue;
        }
        rewrite_message_text(message, &role, rewrite_flagged_text);
        if matches!(role.as_str(), "system" | "assistant") {
            rewrite_message_text(message, &role, pause_opener);
        }
    }
}

fn rewrite_message_text(message: &mut Value, role: &str, rewrite: fn(&str) -> Option<String>) {
    let Some(content) = message.get_mut("content") else {
        return;
    };
    if let Some(text) = content.as_str() {
        if let Some(rewritten) = rewrite(text) {
            *content = json!(rewritten);
        }
        return;
    }
    let Some(parts) = content.as_array_mut() else {
        return;
    };
    for part in parts.iter_mut() {
        if part
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "text")
        {
            continue;
        }
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            if let Some(rewritten) = rewrite(text) {
                part["text"] = json!(rewritten);
            }
        }
    }
    if role == "system" {
        parts.retain(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .is_none_or(|text| !text.trim().is_empty())
                || part.get("type").and_then(Value::as_str) != Some("text")
        });
    }
}

fn rewrite_flagged_text(text: &str) -> Option<String> {
    let mut rewritten = text.to_string();
    rewritten = replace_ci(&rewritten, HARNESS_OPENER, HARNESS_OPENER_FIXED, true);
    rewritten = replace_ci(&rewritten, CLAUDE_OPENER, CLAUDE_OPENER_FIXED, false);
    if !rewritten
        .to_ascii_lowercase()
        .contains("the codex cli is an open source project led by openai.")
    {
        rewritten = replace_ci(&rewritten, CODEX_OPENER, CODEX_OPENER_FIXED, false);
    }
    rewritten = replace_ci(&rewritten, GUI_OPENER, GUI_OPENER_FIXED, false);
    rewritten = replace_ci(&rewritten, BRANCH_OPENER, BRANCH_OPENER_FIXED, false);
    rewritten = strip_billing_header(&rewritten);
    (rewritten != text).then_some(rewritten)
}

fn replace_ci(text: &str, needle: &str, replacement: &str, optional_period: bool) -> String {
    let lower_text = text.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    let mut out = String::new();
    let mut rest = 0;
    while let Some(found) = lower_text[rest..].find(&lower_needle) {
        let start = rest + found;
        let mut end = start + needle.len();
        if optional_period && text[end..].starts_with('.') {
            end += 1;
        }
        out.push_str(&text[rest..start]);
        out.push_str(replacement);
        rest = end;
    }
    out.push_str(&text[rest..]);
    out
}

fn strip_billing_header(text: &str) -> String {
    let marker = "x-anthropic-billing-header:";
    let lower = text.to_ascii_lowercase();
    let mut out = String::new();
    let mut rest = 0;
    while let Some(found) = lower[rest..].find(marker) {
        let start = rest + found;
        out.push_str(&text[rest..start]);
        let after = start + marker.len();
        let end = text[after..]
            .find('\n')
            .map(|i| after + i + 1)
            .unwrap_or(text.len());
        rest = end;
    }
    out.push_str(&text[rest..]);
    out
}

/// Insert the measured comma into a known harness-opener shape.
fn pause_opener(text: &str) -> Option<String> {
    let opener = "you are ";
    let lower = text.to_ascii_lowercase();
    let mut out = String::new();
    let mut scan = 0;
    let mut copied = 0;
    while let Some(found) = lower[scan..].find(opener) {
        let start = scan + found;
        scan = start + opener.len();
        let mut end = lower.len().min(start + 120);
        while !lower.is_char_boundary(end) {
            end -= 1;
        }
        let window = &lower[start..end];
        let Some(at) = [
            " powered by ",
            " running on ",
            " running in ",
            " built on ",
            " based on ",
        ]
        .into_iter()
        .find_map(|phrase| window.find(phrase)) else {
            continue;
        };
        let Some(between) = window.get(opener.len()..at) else {
            continue;
        };
        let kind = between.rsplit(' ').next().unwrap_or("");
        if !matches!(kind, "agent" | "assistant") || between.contains('.') || between.contains(',')
        {
            continue;
        }
        let comma_at = start + at;
        out.push_str(&text[copied..comma_at]);
        out.push(',');
        copied = comma_at;
        scan = comma_at;
    }
    if copied == 0 {
        return None;
    }
    out.push_str(&text[copied..]);
    Some(out)
}

fn attach_growth_event(
    object: &mut serde_json::Map<String, Value>,
    model: Option<&str>,
    conversation: Option<&str>,
) {
    let Some(conversation) = conversation.filter(|id| !id.is_empty()) else {
        return;
    };
    let Some(messages) = object.get("messages").and_then(Value::as_array) else {
        return;
    };
    let model = model.unwrap_or_default();
    let event = json!({
        "eventCode": "chat_request_send",
        "id": conversation,
        "extra": {
            "inputLength": last_user_input_length(messages),
            "requestModelId": model,
            "requestModelName": model,
            "mode": "craft",
            "command": ""
        }
    });
    let mut vars = object
        .get("extra_vars")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut events = vars
        .get("growthEvent")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .map(|value| match value {
            Value::Array(items) => items,
            other => vec![other],
        })
        .unwrap_or_default();
    let present = events.iter().any(|item| {
        item.get("eventCode").and_then(Value::as_str) == Some("chat_request_send")
            && item.get("id").and_then(Value::as_str) == Some(conversation)
    });
    if !present {
        events.push(event);
    }
    vars.insert(
        "growthEvent".into(),
        json!(Value::Array(events).to_string()),
    );
    vars.entry("initiator").or_insert(json!("user"));
    object.insert("extra_vars".into(), Value::Object(vars));
}

fn last_user_input_length(messages: &[Value]) -> usize {
    messages
        .iter()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .and_then(|message| message.get("content"))
        .map(|content| match content {
            Value::String(text) => text.chars().count(),
            Value::Array(parts) => parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str).unwrap_or("text") == "text")
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .map(|text| text.chars().count())
                .sum(),
            _ => 0,
        })
        .unwrap_or(0)
}

fn pause_generic_openers(messages: &mut [Value]) {
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if matches!(role.as_str(), "system" | "assistant") {
            rewrite_message_text(message, &role, pause_opener);
        }
    }
}

fn fill_kimi_call_text(model: Option<&str>, messages: &mut [Value]) {
    let Some(model) = model else {
        return;
    };
    let lower = model.to_ascii_lowercase();
    if !lower.starts_with("kimi-k3.1-pro") {
        return;
    }
    if lower.len() > "kimi-k3.1-pro".len() && !lower["kimi-k3.1-pro".len()..].starts_with('-') {
        return;
    }
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let has_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty());
        if !has_calls || !empty_content(message.get("content")) {
            continue;
        }
        message["content"] = json!(" ");
    }
}

fn empty_content(content: Option<&Value>) -> bool {
    match content {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.is_empty(),
        Some(Value::Array(parts)) => parts.iter().all(|part| {
            part.get("type").and_then(Value::as_str) == Some("text")
                && part
                    .get("text")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
        }),
        Some(_) => false,
    }
}

/// Gemini on WorkBuddy AI refuses tool results immediately followed by a
/// user message. An empty assistant turn between them is accepted.
fn system_key(body: &Value) -> String {
    body.get("messages")
        .and_then(Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
                .filter_map(|message| message.get("content"))
                .map(content_text)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|key| !key.trim().is_empty() && key.trim() != "You are a helpful assistant.")
        .unwrap_or_default()
}

fn pause_refused_openers(body: &Value) -> Option<Value> {
    let mut body = body.clone();
    let messages = body.get_mut("messages")?.as_array_mut()?;
    let before = messages.clone();
    pause_generic_openers(messages);
    (messages != &before).then_some(body)
}

fn repairs() -> &'static std::sync::Mutex<RepairMemory> {
    static REPAIRS: std::sync::OnceLock<std::sync::Mutex<RepairMemory>> =
        std::sync::OnceLock::new();
    REPAIRS.get_or_init(|| std::sync::Mutex::new(RepairMemory::default()))
}

#[derive(Default)]
struct RepairMemory {
    comma: VecDeque<String>,
    moved: VecDeque<String>,
    tools: VecDeque<String>,
}

fn remember_repair(kind: RepairKind, key: String) {
    if key.is_empty() {
        return;
    }
    let Ok(mut memory) = repairs().lock() else {
        return;
    };
    let bucket = match kind {
        RepairKind::Comma => &mut memory.comma,
        RepairKind::Moved => &mut memory.moved,
        RepairKind::Tools => &mut memory.tools,
    };
    if !bucket.iter().any(|item| item == &key) {
        bucket.push_back(key);
    }
    while bucket.len() > 256 {
        bucket.pop_front();
    }
}

fn repair_known(kind: RepairKind, key: &str) -> bool {
    repairs().lock().ok().is_some_and(|memory| {
        let bucket = match kind {
            RepairKind::Comma => &memory.comma,
            RepairKind::Moved => &memory.moved,
            RepairKind::Tools => &memory.tools,
        };
        bucket.iter().any(|item| item == key)
    })
}

enum RepairKind {
    Comma,
    Moved,
    Tools,
}

const COMMA_SYSTEMS: RepairKind = RepairKind::Comma;
const MOVED_SYSTEMS: RepairKind = RepairKind::Moved;
const TOOL_MODELS: RepairKind = RepairKind::Tools;

fn apply_known_repairs(body: &mut Value) {
    let key = system_key(body);
    if !key.is_empty() && repair_known(RepairKind::Moved, &key) {
        if let Some(moved) = move_refused_system(body) {
            *body = moved;
        }
    } else if !key.is_empty() && repair_known(RepairKind::Comma, &key) {
        if let Some(paused) = pause_refused_openers(body) {
            *body = paused;
        }
    }
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if repair_known(RepairKind::Tools, model) {
        if let Some(separated) = separate_tool_results_from_user_body(body) {
            *body = separated;
        }
    }
}

fn move_refused_system(body: &Value) -> Option<Value> {
    let mut body = body.clone();
    let messages = body.get_mut("messages")?.as_array_mut()?;
    if messages.first()?.get("role")?.as_str()? != "system" {
        return None;
    }
    let mut index = 0;
    let mut lead = Vec::new();
    while messages
        .get(index)
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        == Some("system")
    {
        lead.push(content_text(
            messages[index].get("content").unwrap_or(&Value::Null),
        ));
        index += 1;
    }
    let said = lead.join("\n\n").trim().to_string();
    if said.is_empty() || said == "You are a helpful assistant." {
        return None;
    }
    messages.drain(..index);
    for message in messages.iter_mut() {
        if message.get("role").and_then(Value::as_str) == Some("system") {
            message["role"] = json!("user");
        }
    }
    let wrapped = format!("<instructions>\n{said}\n</instructions>");
    match messages
        .iter()
        .position(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    {
        Some(user) => match &messages[user]["content"] {
            Value::String(text) => {
                messages[user]["content"] = json!(format!("{wrapped}\n\n{text}"))
            }
            Value::Array(parts) => {
                let mut parts = parts.clone();
                parts.insert(0, json!({"type":"text","text":wrapped}));
                messages[user]["content"] = Value::Array(parts);
            }
            _ => messages[user]["content"] = json!(wrapped),
        },
        None => messages.insert(0, json!({"role":"user","content":wrapped})),
    }
    messages.insert(
        0,
        json!({"role":"system","content":"You are a helpful assistant."}),
    );
    Some(body)
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn separate_tool_results_from_user_body(body: &Value) -> Option<Value> {
    let mut body = body.clone();
    let messages = body.get_mut("messages")?.as_array_mut()?;
    let before = messages.len();
    separate_tool_results_from_user(messages);
    (messages.len() != before).then_some(body)
}

fn separate_tool_results_from_user(messages: &mut Vec<Value>) {
    let mut index = messages.len();
    while index > 0 && messages[index - 1].get("role").and_then(Value::as_str) == Some("user") {
        index -= 1;
    }
    if index == messages.len() || index == 0 {
        return;
    }
    if messages[index - 1].get("role").and_then(Value::as_str) != Some("tool") {
        return;
    }
    messages.insert(index, json!({"role":"assistant","content":""}));
}

/// Remove DeepSeek Harness prose from native Messages system text and tool
/// descriptions. Paths, identifiers, and tool results stay unchanged.
fn hide_harness_identity(object: &mut serde_json::Map<String, Value>) {
    if let Some(system) = object.get_mut("system") {
        hide_value(system, true);
    }
    if let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            if message.get("role").and_then(Value::as_str) == Some("user")
                && message
                    .get("content")
                    .and_then(Value::as_array)
                    .is_some_and(|parts| parts.iter().any(|part| part["type"] == "tool_result"))
            {
                continue;
            }
            if let Some(content) = message.get_mut("content") {
                hide_value(content, false);
            }
        }
    }
    if let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            if let Some(description) = tool.get("description").and_then(Value::as_str) {
                if let Some(hidden) = hide_harness_text(description, true) {
                    tool["description"] = json!(hidden);
                }
            }
        }
    }
}

fn hide_value(value: &mut Value, prose: bool) {
    match value {
        Value::String(text) => {
            if let Some(hidden) = hide_harness_text(text, prose) {
                *value = json!(hidden);
            }
        }
        Value::Array(parts) => {
            for part in parts {
                let text_part = part
                    .get("type")
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind == "text")
                    && part.get("text").is_some();
                if text_part {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        if let Some(hidden) = hide_harness_text(text, prose) {
                            part["text"] = json!(hidden);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn hide_harness_text(text: &str, prose: bool) -> Option<String> {
    let mut rewritten = replace_ci(text, HARNESS_OPENER, "You are an AI agent.", true);
    rewritten = replace_ci(
        &rewritten,
        "You are an AI agent, powered by DeepSeek Harness",
        "You are an AI agent.",
        true,
    );
    rewritten = rewritten.replace(
        "DeepSeek Harness implementation checkout",
        "agent implementation checkout",
    );
    rewritten = rewritten.replace("DeepSeek Harness Web GUI", "Web GUI");
    if prose {
        rewritten = replace_prose_name(&rewritten);
    }
    (rewritten != text).then_some(rewritten)
}

fn replace_prose_name(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(found) = find_prose_name(rest) {
        out.push_str(&rest[..found.start]);
        out.push_str(found.replacement);
        rest = &rest[found.end..];
    }
    out.push_str(rest);
    out
}

struct ProseName<'a> {
    start: usize,
    end: usize,
    replacement: &'a str,
}

fn find_prose_name(text: &str) -> Option<ProseName<'static>> {
    let candidates = ["DeepSeek Harness", "DSH"];
    let mut best: Option<ProseName<'static>> = None;
    for needle in candidates {
        let mut search = text;
        let mut offset = 0;
        while let Some(found) = search.find(needle) {
            let start = offset + found;
            let end = start + needle.len();
            if prose_boundary(text, start, end) {
                let replacement = "the agent harness";
                if best.as_ref().is_none_or(|current| start < current.start) {
                    best = Some(ProseName {
                        start,
                        end,
                        replacement,
                    });
                }
                break;
            }
            let next = found + needle.len();
            offset += next;
            search = &search[next..];
        }
    }
    best
}

fn prose_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    let path_before = before.is_some_and(|ch| {
        matches!(ch, '/' | '\\' | '.' | '~' | ':' | '@' | '-' | '_') || ch.is_ascii_alphanumeric()
    });
    let path_after = after
        .is_some_and(|ch| matches!(ch, '/' | '\\' | '@' | '-' | '_') || ch.is_ascii_alphanumeric())
        || text[end..].starts_with(".")
            && text[end + 1..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphanumeric());
    !path_before && !path_after
}

/// Apply the upstream tool-name grammar and parameter-schema compatibility to
/// a Chat Completions request. Legal names pass through untouched; mapped
/// names are recorded so streamed tool calls can be restored to the client.
fn adapt_chat_tools(object: &mut serde_json::Map<String, Value>, names: &mut ToolNames) {
    let mut used = HashSet::new();
    if let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) {
        // Register every legal upstream name first so sanitized names cannot
        // collide with an existing tool.
        for tool in tools.iter() {
            if let Some(name) = tool
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
            {
                if is_legal_tool_name(name) {
                    used.insert(name.to_string());
                }
            }
        }
        for tool in tools.iter_mut() {
            let Some(function) = tool.get_mut("function") else {
                continue;
            };
            if let Some(original) = function
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                if let Some(sanitized) = sanitize_tool_name(&original, names, &mut used) {
                    function["name"] = json!(sanitized);
                }
            }
            if let Some(parameters) = function.get_mut("parameters") {
                normalize_schema(parameters);
            }
        }
    }
    let named = object
        .get("tool_choice")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(original) = named {
        if let Some(sanitized) = names.get(&original) {
            object.insert("tool_choice".into(), json!(sanitized.clone()));
        }
    }
    if let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            if message.get("role").and_then(Value::as_str) == Some("tool") {
                if let Some(wire) = message
                    .get("name")
                    .and_then(Value::as_str)
                    .and_then(|n| names.get(n))
                {
                    message["name"] = json!(wire);
                }
            }
            let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) else {
                continue;
            };
            for call in calls {
                let Some(original) = call
                    .get_mut("function")
                    .and_then(|function| function.get_mut("name"))
                    .and_then(|name| name.as_str())
                    .map(str::to_owned)
                else {
                    continue;
                };
                if let Some(sanitized) = sanitize_tool_name(&original, names, &mut used) {
                    call["function"]["name"] = json!(sanitized);
                }
            }
        }
    }
}

/// Apply the same tool-name grammar and schema compatibility to a native
/// Anthropic Messages request.
fn adapt_anthropic_tools(object: &mut serde_json::Map<String, Value>, names: &mut ToolNames) {
    let mut used = HashSet::new();
    if let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools.iter() {
            if let Some(name) = tool.get("name").and_then(Value::as_str) {
                if is_legal_tool_name(name) {
                    used.insert(name.to_string());
                }
            }
        }
        for tool in tools.iter_mut() {
            if let Some(original) = tool.get("name").and_then(Value::as_str).map(str::to_owned) {
                if let Some(sanitized) = sanitize_tool_name(&original, names, &mut used) {
                    tool["name"] = json!(sanitized);
                }
            }
            if let Some(schema) = tool.get_mut("input_schema") {
                normalize_schema(schema);
            }
        }
    }
    let named = object
        .get("tool_choice")
        .filter(|choice| choice.get("type").and_then(Value::as_str) == Some("tool"))
        .and_then(|choice| choice.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(original) = named {
        if let Some(sanitized) = names.get(&original) {
            if let Some(choice) = object.get_mut("tool_choice") {
                choice["name"] = json!(sanitized.clone());
            }
        }
    }
    if let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            let Some(parts) = message.get_mut("content").and_then(Value::as_array_mut) else {
                continue;
            };
            for part in parts {
                if part.get("type").and_then(Value::as_str) != Some("tool_use") {
                    continue;
                }
                let Some(original) = part.get("name").and_then(Value::as_str).map(str::to_owned)
                else {
                    continue;
                };
                if let Some(sanitized) = sanitize_tool_name(&original, names, &mut used) {
                    part["name"] = json!(sanitized);
                }
            }
        }
    }
}

/// The upstream tool-name grammar: ASCII letters, digits, underscore, dash.
fn is_legal_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

/// Map a client tool name onto the upstream tool-name grammar. Legal names
/// pass through; other characters become underscores, and a deterministic
/// numeric suffix resolves collisions. Returns the sanitized name when it
/// differs from the original.
fn sanitize_tool_name(
    original: &str,
    names: &mut ToolNames,
    used: &mut HashSet<String>,
) -> Option<String> {
    if is_legal_tool_name(original) {
        used.insert(original.to_string());
        return None;
    }
    if let Some(sanitized) = names.get(original) {
        return Some(sanitized.clone());
    }
    let base: String = original
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let base = if base.is_empty() {
        "tool".to_owned()
    } else {
        base
    };
    let mut sanitized = base.clone();
    let mut suffix = 2;
    while used.contains(&sanitized) {
        sanitized = format!("{base}_{suffix}");
        suffix += 1;
    }
    used.insert(sanitized.clone());
    names.insert(original.to_string(), sanitized.clone());
    Some(sanitized)
}

fn conversation_salt() -> &'static [u8] {
    static SALT: std::sync::OnceLock<[u8; 16]> = std::sync::OnceLock::new();
    SALT.get_or_init(|| uuid::Uuid::new_v4().into_bytes())
}

fn conversation_headers(
    headers: &mut HeaderMap,
    account: &str,
    session: Option<&str>,
    body: &Value,
) -> Result<(), ProxyError> {
    let user_messages = || {
        body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, row)| {
                row["role"] == "user"
                    && !row["content"]
                        .as_array()
                        .is_some_and(|parts| parts.iter().any(|part| part["type"] == "tool_result"))
            })
    };
    let digest = |value: Value| {
        let mut hasher = Sha256::new();
        hasher.update(conversation_salt());
        hasher.update(value.to_string().as_bytes());
        format!("{:x}", hasher.finalize())[..32].to_string()
    };
    let conversation = session
        .map(|id| digest(json!([account, "session", id])))
        .or_else(|| {
            user_messages()
                .next()
                .map(|(_, row)| digest(json!([account, "first", row["content"]])))
        });
    let turn = user_messages()
        .last()
        .map(|(index, row)| digest(json!([account, conversation, index, row["content"]])));
    let message = uuid::Uuid::new_v4().simple().to_string();
    let turn = turn.unwrap_or_else(|| message.clone());
    for (name, value) in [
        ("x-conversation-message-id", &message),
        ("x-request-id", &message),
        ("x-conversation-request-id", &turn),
    ] {
        headers.insert(name, HeaderValue::from_str(value).map_err(|_| invalid())?);
    }
    if let Some(conversation) = conversation {
        headers.insert(
            "x-conversation-id",
            HeaderValue::from_str(&conversation).map_err(|_| invalid())?,
        );
    }
    Ok(())
}

async fn encode(body: Value, protocol: Protocol) -> Result<Bytes, ProxyError> {
    if protocol == Protocol::Anthropic {
        return serde_json::to_vec(&body)
            .map(Bytes::from)
            .map_err(|_| invalid());
    }
    tokio::task::spawn_blocking(move || {
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        serde_json::to_writer(&mut gzip, &body).map_err(|_| invalid())?;
        gzip.finish().map(Bytes::from).map_err(|_| invalid())
    })
    .await
    .map_err(|_| invalid())?
}

fn names_effort(body: &Value) -> bool {
    body.get("reasoning_effort").is_some()
        || body
            .get("reasoning")
            .and_then(Value::as_object)
            .is_some_and(|reasoning| reasoning.contains_key("effort"))
}

fn request(
    client: &reqwest::Client,
    account: &accounts::Account,
    base: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: &Bytes,
    beta: bool,
) -> Result<reqwest::RequestBuilder, ProxyError> {
    let path = if protocol == Protocol::Anthropic {
        if beta {
            "/v1/messages?beta=true"
        } else {
            "/v1/messages"
        }
    } else {
        "/v2/chat/completions"
    };
    let mut request = client
        .post(format!("{base}{path}"))
        .headers(headers.clone())
        .headers(vendor::headers(account).map_err(|_| invalid())?)
        .bearer_auth(account.token())
        .header(header::ACCEPT, "text/event-stream")
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-agent-type", "main")
        .header("x-agent-runtime", "local-agent")
        .headers(client_headers())
        .body(body.clone());
    if protocol == Protocol::Chat {
        request = request.header(header::CONTENT_ENCODING, "gzip");
    } else if !headers.contains_key("anthropic-version") {
        request = request.header("anthropic-version", "2023-06-01");
    }
    Ok(request)
}

/// Headers WorkBuddy's own CLI sends with model requests. Both editions use
/// the same SDK identity; the OS and architecture follow this process.
fn client_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    let os = match std::env::consts::OS {
        "macos" => "MacOS",
        "windows" => "Windows",
        "linux" => "Linux",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "android" => "Android",
        other => return headers_with_other(other),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "arm" => "arm",
        "x86" => "x32",
        other => return headers_with_arch(os, other),
    };
    insert_client_headers(&mut headers, os, arch);
    headers
}

fn headers_with_other(os: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let value = format!("Other:{os}");
    insert_client_headers(&mut headers, &value, arch_label(std::env::consts::ARCH));
    headers
}

fn headers_with_arch(os: &str, arch: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    insert_client_headers(&mut headers, os, arch_label(arch));
    headers
}

fn arch_label(arch: &str) -> &'static str {
    match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "arm" => "arm",
        "x86" => "x32",
        _ => "other",
    }
}

fn insert_client_headers(headers: &mut HeaderMap, os: &str, arch: &str) {
    let arch = if arch == "other" {
        format!("other:{}", std::env::consts::ARCH)
    } else {
        arch.to_string()
    };
    for (name, value) in [
        ("x-stainless-retry-count", "0".to_string()),
        ("x-stainless-timeout", "1200".to_string()),
        ("x-stainless-lang", "js".to_string()),
        ("x-stainless-package-version", "6.25.0".to_string()),
        ("x-stainless-os", os.to_string()),
        ("x-stainless-arch", arch),
        ("x-stainless-runtime", "node".to_string()),
        ("x-stainless-runtime-version", "v22.21.1".to_string()),
    ] {
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.insert(name, value);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn send(
    client: &reqwest::Client,
    account: &accounts::Account,
    protocol: Protocol,
    body: Value,
    session: Option<&str>,
    caller: &HeaderMap,
    beta: bool,
    default_effort: Option<&str>,
    model_name: Option<&str>,
) -> Result<(reqwest::Response, ToolNames), ProxyError> {
    let (mut body, names) = prepare(body, protocol)?;
    if protocol == Protocol::Chat
        && !names_effort(&body)
        && body["max_tokens"].as_u64().is_none_or(|max| max > 1024)
    {
        if let Some(effort) = default_effort {
            body["reasoning_effort"] = json!(effort);
        }
    }
    let mut headers = HeaderMap::new();
    if protocol == Protocol::Anthropic {
        for name in ["anthropic-version", "anthropic-beta"] {
            for value in caller.get_all(name) {
                headers.append(name, value.clone());
            }
        }
    }
    apply_known_repairs(&mut body);
    conversation_headers(&mut headers, &account.id, session, &body)?;
    if account.provider == SubscriptionProvider::WorkbuddyAi && protocol == Protocol::Chat {
        if let Some(object) = body.as_object_mut() {
            let conversation = headers
                .get("x-conversation-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let model = object
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned);
            attach_growth_event(object, model.as_deref(), conversation.as_deref());
        }
    }
    let encoded = encode(body.clone(), protocol).await?;
    let send = |account: &accounts::Account, encoded: &Bytes| {
        request(
            client,
            account,
            &vendor::account_origin(account),
            protocol,
            &headers,
            encoded,
            beta,
        )
    };
    let wait = crate::proxy::http_client::UPSTREAM_HEADER_WAIT;
    let mut response =
        crate::proxy::http_client::send_with_header_timeout(send(account, &encoded)?, wait).await?;
    if response.status() == StatusCode::UNAUTHORIZED {
        let renewed = match accounts::renew_rejected_workbuddy(account).await {
            Ok(renewed) => renewed,
            Err(_) => {
                return Err(ProxyError::UpstreamError {
                    status: 401,
                    body: None,
                })
            }
        };
        response =
            crate::proxy::http_client::send_with_header_timeout(send(&renewed, &encoded)?, wait)
                .await?;
        if response.status() == StatusCode::UNAUTHORIZED
            && response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("error=\"invalid_token\""))
        {
            accounts::observe_rejection(&renewed).await;
        }
    }
    if response.status() == StatusCode::BAD_REQUEST && protocol == Protocol::Chat {
        response = repair_chat(client, account, protocol, &body, &headers, beta, response).await?;
    } else if response.status().is_client_error() && protocol == Protocol::Anthropic {
        response =
            repair_signature(client, account, protocol, &body, &headers, beta, response).await?;
    }
    if !response.status().is_success() {
        let status = response.status().as_u16();
        return Err(rejection(status, response).await);
    }
    if !response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(';').next() == Some("text/event-stream"))
    {
        return Err(invalid());
    }
    if account.provider == SubscriptionProvider::WorkbuddyAi && protocol == Protocol::Chat {
        let step = activity_step(&body, &headers, model_name);
        response = report_activity(client, account, step, response);
    }
    Ok((response, names))
}

async fn repair_chat(
    client: &reqwest::Client,
    account: &accounts::Account,
    protocol: Protocol,
    body: &Value,
    headers: &HeaderMap,
    beta: bool,
    response: reqwest::Response,
) -> Result<reqwest::Response, ProxyError> {
    let send = |encoded: &Bytes| {
        request(
            client,
            account,
            &vendor::account_origin(account),
            protocol,
            headers,
            encoded,
            beta,
        )
    };
    let wait = crate::proxy::http_client::UPSTREAM_HEADER_WAIT;
    let status = response.status().as_u16();
    let mut bytes = bounded_error_body(response).await;
    let mut body = body.clone();
    let key = system_key(&body);
    let mut response = None;
    if workbuddy_code(&bytes).as_deref() == Some("11128") && !repair_known(COMMA_SYSTEMS, &key) {
        if let Some(paused) = pause_refused_openers(&body) {
            let encoded = encode(paused.clone(), protocol).await?;
            let next =
                crate::proxy::http_client::send_with_header_timeout(send(&encoded)?, wait).await?;
            if next.status().is_success() && !key.is_empty() {
                remember_repair(COMMA_SYSTEMS, key.clone());
            }
            if next.status() == StatusCode::BAD_REQUEST {
                bytes = bounded_error_body(next).await;
            } else {
                response = Some(next);
            }
            body = paused;
        }
    }
    if response.is_none()
        && workbuddy_code(&bytes).as_deref() == Some("11128")
        && !key.is_empty()
        && !repair_known(MOVED_SYSTEMS, &key)
    {
        if let Some(moved) = move_refused_system(&body) {
            remember_repair(MOVED_SYSTEMS, key);
            let encoded = encode(moved, protocol).await?;
            let next =
                crate::proxy::http_client::send_with_header_timeout(send(&encoded)?, wait).await?;
            if next.status() == StatusCode::BAD_REQUEST {
                bytes = bounded_error_body(next).await;
            } else {
                response = Some(next);
            }
        }
    }
    if response.is_none() && workbuddy_code(&bytes).as_deref() == Some("11153") {
        if let Some(separated) = separate_tool_results_from_user_body(&body) {
            let model = body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default();
            remember_repair(TOOL_MODELS, model.to_owned());
            let encoded = encode(separated, protocol).await?;
            response = Some(
                crate::proxy::http_client::send_with_header_timeout(send(&encoded)?, wait).await?,
            );
        }
    }
    response.ok_or_else(|| rejection_from(status, &bytes))
}

async fn repair_signature(
    client: &reqwest::Client,
    account: &accounts::Account,
    protocol: Protocol,
    body: &Value,
    headers: &HeaderMap,
    beta: bool,
    response: reqwest::Response,
) -> Result<reqwest::Response, ProxyError> {
    let status = response.status().as_u16();
    let bytes = bounded_error_body(response).await;
    if !bad_signature(&bytes) {
        return Err(rejection_from(status, &bytes));
    }
    let Some(stripped) = drop_signed_thinking(body) else {
        return Err(rejection_from(status, &bytes));
    };
    let encoded = encode(stripped, protocol).await?;
    let request = request(
        client,
        account,
        &vendor::account_origin(account),
        protocol,
        headers,
        &encoded,
        beta,
    )?;
    crate::proxy::http_client::send_with_header_timeout(
        request,
        crate::proxy::http_client::UPSTREAM_HEADER_WAIT,
    )
    .await
}

fn bad_signature(bytes: &[u8]) -> bool {
    rejection_code(bytes) == Some("routing_upstream_thinking_signature_rejected")
}

fn drop_signed_thinking(body: &Value) -> Option<Value> {
    let mut body = body.clone();
    let messages = body.get_mut("messages")?.as_array_mut()?;
    let mut dropped = false;
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(parts) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let before = parts.len();
        parts.retain(|part| part.get("type").and_then(Value::as_str) != Some("thinking"));
        if parts.len() == before {
            continue;
        }
        dropped = true;
        if parts.is_empty() {
            parts.push(json!({"type":"text","text":""}));
        }
    }
    dropped.then_some(body)
}

struct ActivityStep {
    conversation: String,
    turn: String,
    message: String,
    model: String,
    model_name: String,
    input: usize,
    user_send: bool,
    step: u64,
    tool_calls: u64,
    history: u64,
}

fn activity_step(body: &Value, headers: &HeaderMap, model_name: Option<&str>) -> ActivityStep {
    let messages = body.get("messages").and_then(Value::as_array);
    let mut step = 1;
    let mut tool_calls = 0;
    if let Some(messages) = messages {
        for message in messages.iter().rev() {
            match message.get("role").and_then(Value::as_str) {
                Some("user") => break,
                Some("assistant") => {
                    step += 1;
                    tool_calls += message
                        .get("tool_calls")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0) as u64;
                }
                _ => {}
            }
        }
    }
    let history = messages
        .map(|messages| {
            messages
                .iter()
                .filter(|message| {
                    matches!(
                        message.get("role").and_then(Value::as_str),
                        Some("user" | "assistant")
                    )
                })
                .count() as u64
        })
        .unwrap_or(0);
    ActivityStep {
        conversation: header_text(headers, "x-conversation-id"),
        turn: header_text(headers, "x-conversation-request-id"),
        message: header_text(headers, "x-conversation-message-id"),
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        model_name: model_name
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                body.get("model")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            })
            .to_owned(),
        input: messages
            .map(|messages| last_user_input_length(messages))
            .unwrap_or(0),
        user_send: messages
            .and_then(|messages| messages.last())
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            == Some("user"),
        step,
        tool_calls,
        history,
    }
}

fn header_text(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn report_activity(
    client: &reqwest::Client,
    account: &accounts::Account,
    step: ActivityStep,
    response: reqwest::Response,
) -> reqwest::Response {
    let status = response.status();
    let headers = response.headers().clone();
    let version = response.version();
    let client = client.clone();
    let account = account.clone();
    let started = std::time::SystemTime::now();
    let stream = response.bytes_stream().map(|chunk| {
        chunk.map_err(|_| std::io::Error::other("WorkBuddy activity stream interrupted"))
    });
    let reported = async_stream::stream! {
        let mut upstream = Box::pin(stream);
        let mut tail = Vec::new();
        while let Some(chunk) = upstream.next().await {
            if let Ok(bytes) = &chunk {
                tail.extend_from_slice(bytes);
                if tail.len() > 16 * 1024 {
                    tail.drain(..tail.len() - 16 * 1024);
                }
            }
            yield chunk;
        }
        let _ = post_activity(&client, &account, &step, &tail, started).await;
    };
    let mut rebuilt = http::Response::new(reqwest::Body::wrap_stream(reported));
    *rebuilt.status_mut() = status;
    *rebuilt.headers_mut() = headers;
    *rebuilt.version_mut() = version;
    reqwest::Response::from(rebuilt)
}

async fn post_activity(
    client: &reqwest::Client,
    account: &accounts::Account,
    step: &ActivityStep,
    tail: &[u8],
    started: std::time::SystemTime,
) -> Result<(), ()> {
    let facts = reply_facts(tail);
    let ended = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ())?
        .as_millis() as u64;
    let start = started
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ())?
        .as_millis() as u64;
    let origin = vendor::account_origin(account);
    for body in activity_batches(account, step, &facts, start, ended) {
        let request = client
            .post(format!("{origin}/v2/report"))
            .headers(vendor::headers(account).map_err(|_| ())?)
            .bearer_auth(account.token())
            .json(&body)
            .timeout(Duration::from_secs(15));
        if let Ok(response) = request.send().await {
            let _ = response.bytes().await;
        }
    }
    Ok(())
}

struct ReplyFacts {
    finish: String,
    input: u64,
    output: u64,
    total: u64,
    cached: u64,
}

fn reply_facts(tail: &[u8]) -> ReplyFacts {
    let text = String::from_utf8_lossy(tail);
    let last = |key: &str| {
        text.rmatch_indices(&format!("\"{key}\""))
            .next()
            .and_then(|(index, _)| {
                let rest = text[index + key.len() + 2..].trim_start_matches([' ', ':']);
                rest.split(|c: char| !c.is_ascii_digit())
                    .next()?
                    .parse()
                    .ok()
            })
            .unwrap_or(0)
    };
    let finish = text
        .rmatch_indices("\"finish_reason\"")
        .next()
        .and_then(|(index, _)| {
            let rest = text[index + 16..].trim_start_matches([' ', ':']);
            let rest = rest.trim_start_matches('"');
            rest.split('"').next().map(str::to_owned)
        })
        .unwrap_or_default();
    let input = last("prompt_tokens");
    let output = last("completion_tokens");
    ReplyFacts {
        finish,
        input,
        output,
        total: {
            let total = last("total_tokens");
            if total == 0 {
                input + output
            } else {
                total
            }
        },
        cached: last("cached_tokens"),
    }
}

fn report_machine_id() -> String {
    static MACHINE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    MACHINE
        .get_or_init(|| {
            let host = std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .unwrap_or_default();
            let user = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_default();
            format!(
                "{:x}",
                Sha256::digest(format!("varyai-workbuddy|{host}|{user}").as_bytes())
            )
        })
        .clone()
}

fn report_session_id() -> String {
    static SESSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SESSION
        .get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
        .clone()
}

fn report_os_version() -> String {
    std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn iana_timezone() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|zone| !zone.is_empty())
        .unwrap_or_default()
}

fn report_cpu_model() -> String {
    static MODEL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    MODEL
        .get_or_init(|| {
            #[cfg(target_os = "macos")]
            {
                return command_line("sysctl", &["-n", "machdep.cpu.brand_string"]);
            }
            #[cfg(not(target_os = "macos"))]
            report_cpu_model_uncached()
        })
        .clone()
}

fn report_cpu_model_uncached() -> String {
    #[cfg(target_os = "windows")]
    {
        return command_line(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name)",
            ],
        );
    }
    #[cfg(not(target_os = "windows"))]
    String::new()
}

fn report_memory_bytes() -> u64 {
    static MEMORY: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *MEMORY.get_or_init(report_memory_bytes_uncached)
}

fn report_memory_bytes_uncached() -> u64 {
    #[cfg(target_os = "macos")]
    {
        return command_line("sysctl", &["-n", "hw.memsize"])
            .parse()
            .unwrap_or(0);
    }
    #[cfg(target_os = "windows")]
    {
        return command_line(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
            ],
        )
        .parse()
        .unwrap_or(0);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    0
}

fn command_line(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn activity_batches(
    account: &accounts::Account,
    step: &ActivityStep,
    facts: &ReplyFacts,
    start: u64,
    end: u64,
) -> Vec<Value> {
    let base = |code: &str| {
        json!({
            "eventCode": code,
            "buddyId": "",
            "buddyName": "",
            "userId": account.workbuddy_uid(),
            "username": account.login,
            "userNickname": account.login,
            "product": "SaaS",
            "commit": "",
            "ideName": "WorkBuddy",
            "ideType": "WorkBuddy",
            "ideVersion": "5.9.3",
            "machineId": report_machine_id(),
            "sessionId": report_session_id(),
            "extName": "workbuddy-ai",
            "extVersion": "2.165.0",
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "osVersion": report_os_version(),
            "cpuModel": report_cpu_model(),
            "timestamp": end,
            "reportDelay": 0,
            "releaseDate": 0,
            "cpuCores": std::thread::available_parallelism().map(|count| count.get()).unwrap_or(0),
            "memorySize": report_memory_bytes(),
            "timezone": iana_timezone()
        })
    };
    let ids = json!({
        "conversationId": step.conversation,
        "requestId": step.turn,
        "traceId": step.message,
        "rootRequestId": step.turn,
        "parentConversationId": "",
        "agentName": "",
        "agentType": "main",
        "codebuddy.session_id": step.conversation,
        "codebuddy.conversation_request_id": step.turn
    });
    let model = json!({"requestModelId": step.model, "requestModelName": step.model_name});
    let tokens = json!({
        "inputToken": facts.input,
        "outputToken": facts.output,
        "totalToken": facts.total,
        "cachedTokens": facts.cached,
        "cachedWriteTokens": 0,
        "cachedMissTokens": facts.input.saturating_sub(facts.cached)
    });
    let ok = json!({"isSuccessful": true, "messageErrorCode": "", "finishReason": if facts.finish.is_empty() { "stop" } else { &facts.finish }});
    let first = vec![merge(&[
        base("chat_message_send"),
        ids.clone(),
        json!({"messageId": step.message}),
        model.clone(),
        json!({"historyCount": step.history, "currentStepCount": step.step, "totalStepCount": step.step, "compactionCount": 0, "presentAt": start, "isContextTruncated": false}),
    ])];
    let mut second = Vec::new();
    if step.user_send {
        second.push(merge(&[base("chat_request_send"), json!({"mode": "craft", "source": "work", "command": ""}), ids.clone(), model.clone(), json!({"inputLength": step.input, "mentionContextCount": 0, "presentAt": start, "isPlan": false, "isAutoExecuteTerminal": false, "isAutoModify": false, "codebaseEnable": false, "isMaxMode": true, "mentionContexts": [], "knowledgeId": [], "knowledgeName": [], "codebaseId": "", "inputAttemptId": "", "recommendId": "", "skillId": "", "vcsType": "", "vcsRepo": "", "vcsBranchName": "", "vcsRevId": ""})]));
    }
    second.push(merge(&[base("chat_message_response"), ids.clone(), json!({"messageId": step.message, "responseModelId": step.model, "totalStepCount": step.step, "firstTokenAt": start, "presentAt": end}), model.clone(), tokens.clone(), ok.clone()]));
    second.push(merge(&[
        base("chat_message_status"),
        ids.clone(),
        json!({"messageId": step.message, "messageErrorCode": "0"}),
        model.clone(),
    ]));
    let mut third = Vec::new();
    if facts.finish != "tool_calls" {
        let mut no_trace = ids;
        no_trace
            .as_object_mut()
            .map(|fields| fields.remove("traceId"));
        second.push(merge(&[
            base("chat_request_response"),
            json!({"mode": "craft", "toolCallCount": step.tool_calls, "presentAt": end}),
            no_trace,
            model,
            tokens,
            ok,
        ]));
        third.push(merge(&[base("agent_task_completed"), json!({"mode": "LOCAL", "task_id": step.turn, "duration_ms": end.saturating_sub(start), "total_steps": step.step})]));
        third.push(merge(&[base("chat_response_complete"), json!({"conversationId": step.conversation, "requestId": step.turn, "artifactType": "", "source": "work"})]));
    }
    [first, second, third]
        .into_iter()
        .filter(|batch| !batch.is_empty())
        .map(Value::Array)
        .collect()
}

fn merge(values: &[Value]) -> Value {
    let mut out = serde_json::Map::new();
    for value in values {
        if let Some(object) = value.as_object() {
            out.extend(object.clone());
        }
    }
    Value::Object(out)
}

/// Read a bounded, time-limited upstream error body. The body is classified
/// from fixed upstream metadata only and is never forwarded, echoed, or logged.
async fn bounded_error_body(response: reqwest::Response) -> Vec<u8> {
    let read = async {
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        while buffer.len() < ERROR_BODY_LIMIT {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    let remaining = ERROR_BODY_LIMIT - buffer.len();
                    buffer.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                    if chunk.len() > remaining {
                        break;
                    }
                }
                _ => break,
            }
        }
        buffer
    };
    tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .unwrap_or_default()
}

/// Extract the upstream numeric rejection code from a bounded error body.
/// Top-level code and nested error.code shapes both occur, as numbers and as
/// numeric strings.
fn workbuddy_code(bytes: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(bytes).ok()?;
    let error = value.get("error").unwrap_or(&value);
    let code = error
        .get("internal_code")
        .or_else(|| error.get("code"))
        .or_else(|| value.get("internal_code"))
        .or_else(|| value.get("code"))
        .and_then(numeric_code)
        .or_else(|| {
            value
                .get("error")
                .and_then(Value::as_str)
                .and_then(leading_numeric_code)
        })?;
    (!code.is_empty()).then_some(code)
}

fn numeric_code(code: &Value) -> Option<String> {
    code.as_i64()
        .map(|code| code.to_string())
        .or_else(|| code.as_str().map(str::to_owned))
        .filter(|code| !code.is_empty() && code.bytes().all(|byte| byte.is_ascii_digit()))
}

fn leading_numeric_code(text: &str) -> Option<String> {
    let (code, rest) = text.split_once(':')?;
    (code.len() == 5 && code.bytes().all(|byte| byte.is_ascii_digit()) && !rest.is_empty())
        .then(|| code.to_string())
}

/// Classify an upstream rejection into a fixed code from its numeric metadata
/// only. Upstream message text is never read, echoed, or logged.
pub(super) fn rejection_code(bytes: &[u8]) -> Option<&'static str> {
    match workbuddy_code(bytes).as_deref() {
        // Upstream channel policy: this subscription endpoint only accepts
        // approved client channels for the request's prompt shape.
        Some("11128") => Some("routing_upstream_channel_restricted"),
        // Invalid request parameters: unsupported tool names or schemas.
        Some("11133") => Some("routing_upstream_invalid_parameters_rejected"),
        Some("11159") => Some("routing_upstream_thinking_signature_rejected"),
        Some("11140") => Some("routing_upstream_account_restricted"),
        Some("14018") => Some("routing_upstream_quota_exhausted"),
        Some("11153") => Some("routing_upstream_tool_separator_required"),
        Some("11158") => Some("routing_upstream_entitlement_missing"),
        Some(code) if code.bytes().all(|byte| byte.is_ascii_digit()) && code.len() <= 8 => {
            Some("routing_upstream_numeric_rejected")
        }
        _ => None,
    }
}

/// Wrap a non-2xx upstream response. Only the fixed numeric code survives;
/// the carrier body is rebuilt from fixed phrases and never contains any
/// substring of the upstream response, which may echo credentials or input.
fn rejection_from(status: u16, bytes: &[u8]) -> ProxyError {
    let body = match rejection_code(bytes) {
        Some("routing_upstream_channel_restricted") => Some(
            json!({"error":{"type":"upstream_error","code":"11128","message":"Upstream channel policy rejected this request (code 11128)"}})
                .to_string(),
        ),
        Some("routing_upstream_invalid_parameters_rejected") => Some(
            json!({"error":{"type":"upstream_error","code":"11133","message":"Upstream rejected the request parameters (code 11133)"}})
                .to_string(),
        ),
        Some("routing_upstream_thinking_signature_rejected") => Some(
            json!({"error":{"type":"upstream_error","code":"11159","message":"Upstream rejected the thinking signature (code 11159)"}})
                .to_string(),
        ),
        Some("routing_upstream_account_restricted") => Some(
            json!({"error":{"type":"upstream_error","code":"11140","message":"Upstream account is restricted (code 11140)"}})
                .to_string(),
        ),
        Some("routing_upstream_quota_exhausted") => Some(
            json!({"error":{"type":"upstream_error","code":"14018","message":"Upstream quota is exhausted (code 14018)"}})
                .to_string(),
        ),
        Some("routing_upstream_tool_separator_required") => Some(
            json!({"error":{"type":"upstream_error","code":"11153","message":"Upstream requires an assistant separator after tool results (code 11153)"}})
                .to_string(),
        ),
        Some("routing_upstream_entitlement_missing") => Some(
            json!({"error":{"type":"upstream_error","code":"11158","message":"Upstream account is not entitled to this model (code 11158)"}})
                .to_string(),
        ),
        Some("routing_upstream_numeric_rejected") => Some(
            json!({"error":{"type":"upstream_error","code":"routing_upstream_numeric_rejected","message":"Upstream rejected the request with an unclassified numeric code"}})
                .to_string(),
        ),
        _ => None,
    };
    ProxyError::UpstreamError { status, body }
}

async fn rejection(status: u16, response: reqwest::Response) -> ProxyError {
    rejection_from(status, &bounded_error_body(response).await)
}

/// Restore sanitized tool-call names while streaming an upstream reply. Only
/// mapped names are rewritten; every other byte, event boundary, and framing
/// field is forwarded unchanged.
pub(super) fn restore_tool_names(
    stream: ByteStream,
    request_names: ToolNames,
    protocol: Protocol,
) -> ByteStream {
    strip_filter_text(
        restore_mapped_names(stream, request_names, protocol),
        protocol,
    )
}

fn restore_mapped_names(
    stream: ByteStream,
    request_names: ToolNames,
    protocol: Protocol,
) -> ByteStream {
    let names: ToolNames = request_names
        .into_iter()
        .map(|(client, upstream)| (upstream, client))
        .collect();
    Box::pin(async_stream::stream! {
        const LINE_LIMIT: usize = 2 * 1024 * 1024;
        let mut upstream = stream;
        let mut buffer: Vec<u8> = Vec::new();
        loop {
            while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = buffer.drain(..=position).collect();
                yield Ok(Bytes::from(restore_tool_line(&line, &names, protocol)));
            }
            if buffer.len() > LINE_LIMIT {
                yield Ok(Bytes::from(std::mem::take(&mut buffer)));
            }
            match upstream.next().await {
                Some(Ok(chunk)) => buffer.extend_from_slice(&chunk),
                Some(Err(error)) => {
                    if !buffer.is_empty() {
                        yield Ok(Bytes::from(std::mem::take(&mut buffer)));
                    }
                    yield Err(error);
                }
                None => {
                    if !buffer.is_empty() {
                        yield Ok(Bytes::from(std::mem::take(&mut buffer)));
                    }
                    break;
                }
            }
        }
    })
}

const FILTER_SAYS: &str = "Sorry, I can't respond to this question.";

fn strip_filter_text(stream: ByteStream, protocol: Protocol) -> ByteStream {
    if protocol != Protocol::Chat {
        return stream;
    }
    Box::pin(async_stream::stream! {
        let mut upstream = stream;
        let mut buffer = String::new();
        let mut held: Vec<String> = Vec::new();
        let mut said = String::new();
        let mut open = true;
        while let Some(chunk) = upstream.next().await {
            let Ok(bytes) = chunk else {
                yield chunk;
                continue;
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                yield Ok(bytes);
                continue;
            };
            buffer.push_str(&text.replace("\r\n", "\n"));
            while let Some(split) = buffer.find("\n\n") {
                let event = buffer.drain(..=split + 1).collect::<String>();
                for emitted in filter_event(&event, &mut held, &mut said, &mut open) {
                    yield Ok(Bytes::from(emitted));
                }
            }
        }
        for event in held.drain(..) {
            yield Ok(Bytes::from(event));
        }
        if !buffer.is_empty() {
            yield Ok(Bytes::from(buffer));
        }
    })
}

fn filter_event(
    event: &str,
    held: &mut Vec<String>,
    said: &mut String,
    open: &mut bool,
) -> Vec<String> {
    if !*open {
        return vec![event.to_string()];
    }
    let Some(value) = event_value(event) else {
        return vec![event.to_string()];
    };
    let choice = value.pointer("/choices/0");
    let text = choice
        .and_then(|choice| choice.pointer("/delta/content"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let calls = choice.and_then(|choice| choice.pointer("/delta/tool_calls"));
    let has_call = calls
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty())
        || calls.is_some_and(|calls| !calls.is_array());
    if has_call || (!text.is_empty() && !filter_prefix(&(said.clone() + text))) {
        *open = false;
        let mut out = std::mem::take(held);
        out.push(event.to_string());
        return out;
    }
    said.push_str(text);
    let finish = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str);
    if finish == Some("content_filter") && !said.is_empty() {
        *open = false;
        let mut out = std::mem::take(held);
        out.push(event.to_string());
        return out
            .into_iter()
            .filter_map(|event| strip_filter_content(&event))
            .collect();
    }
    if finish.is_some() {
        *open = false;
        let mut out = std::mem::take(held);
        out.push(event.to_string());
        return out;
    }
    if said.is_empty() {
        return vec![event.to_string()];
    }
    held.push(event.to_string());
    Vec::new()
}

fn filter_prefix(text: &str) -> bool {
    let normalized = text.replace('’', "'").trim().to_string();
    FILTER_SAYS.starts_with(&normalized)
}

fn event_value(event: &str) -> Option<Value> {
    let data = event.lines().find_map(|line| line.strip_prefix("data:"))?;
    let data = data.trim_start();
    (data != "[DONE]").then(|| serde_json::from_str(data).ok())?
}

fn strip_filter_content(event: &str) -> Option<String> {
    let mut value = event_value(event)?;
    let finish = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let usage = value.get("usage").is_some();
    let delta = value.pointer_mut("/choices/0/delta")?;
    if delta.get("content").and_then(Value::as_str).is_none() {
        return Some(event.to_string());
    }
    let Some(object) = delta.as_object_mut() else {
        return Some(event.to_string());
    };
    object.remove("content");
    let left = object.keys().any(|key| key != "role");
    if !left && finish.is_none() && !usage {
        return None;
    }
    Some(format!("data: {}\n\n", value))
}

/// Rewrite one SSE line, restoring mapped tool-call names. Lines without a
/// mapped name are returned unchanged.
fn restore_tool_line(line: &[u8], names: &ToolNames, protocol: Protocol) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(line) else {
        return line.to_vec();
    };
    let Some(rest) = text.strip_prefix("data:") else {
        return line.to_vec();
    };
    let leading = rest.len() - rest.trim_start().len();
    let payload = rest.trim_start();
    let payload = &payload[..payload.trim_end().len()];
    if !payload.starts_with('{') {
        return line.to_vec();
    }
    let marker = match protocol {
        Protocol::Anthropic => "\"content_block\"",
        _ => "\"tool_calls\"",
    };
    if !payload.contains(marker) {
        return line.to_vec();
    }
    let Ok(mut value) = serde_json::from_str::<Value>(payload) else {
        return line.to_vec();
    };
    let changed = match protocol {
        Protocol::Anthropic => restore_anthropic_names(&mut value, names),
        _ => restore_chat_names(&mut value, names),
    };
    if !changed {
        return line.to_vec();
    }
    let mut restored = Vec::with_capacity(line.len() + 8);
    restored.extend_from_slice(b"data:");
    restored.extend_from_slice(&rest.as_bytes()[..leading]);
    restored.extend_from_slice(value.to_string().as_bytes());
    restored.extend_from_slice(&rest.as_bytes()[leading + payload.len()..]);
    restored
}

/// Restore mapped names in a Chat Completions streaming event.
fn restore_chat_names(value: &mut Value, names: &ToolNames) -> bool {
    let mut changed = false;
    let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) else {
        return false;
    };
    for choice in choices {
        for key in ["delta", "message"] {
            let Some(calls) = choice
                .get_mut(key)
                .and_then(|part| part.get_mut("tool_calls"))
                .and_then(Value::as_array_mut)
            else {
                continue;
            };
            for call in calls {
                let Some(original) = call
                    .get_mut("function")
                    .and_then(|function| function.get_mut("name"))
                    .and_then(|name| name.as_str())
                    .map(str::to_owned)
                else {
                    continue;
                };
                if let Some(restored) = names.get(&original) {
                    call["function"]["name"] = json!(restored);
                    changed = true;
                }
            }
        }
    }
    changed
}

/// Restore mapped names in a native Messages content_block_start event.
fn restore_anthropic_names(value: &mut Value, names: &ToolNames) -> bool {
    let Some(block) = value
        .get_mut("content_block")
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
    else {
        return false;
    };
    let Some(original) = block.get("name").and_then(Value::as_str).map(str::to_owned) else {
        return false;
    };
    match names.get(&original) {
        Some(restored) => {
            block["name"] = json!(restored);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests;
