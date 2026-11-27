//! Build each upstream wire format directly from the shared conversation IR.
use super::*;
use crate::proxy::ProxyError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

fn invalid() -> ProxyError {
    ProxyError::InvalidRequest("Content cannot be represented in the selected protocol".into())
}

const ANTHROPIC_ID_MARK: &str = "vary_id_";
const INTERRUPTED_TOOL_MESSAGE: &str =
    "[The result of this tool call is unavailable: the turn was interrupted.]";

fn bound_call_id(id: &str) -> String {
    if id.len() <= 64 {
        return id.to_owned();
    }
    format!("{:x}", Sha256::digest(id.trim().as_bytes()))
}

fn anthropic_out_id(id: &str) -> String {
    if id.is_empty()
        || (id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            && !id.starts_with(ANTHROPIC_ID_MARK))
    {
        return id.to_owned();
    }
    format!(
        "{ANTHROPIC_ID_MARK}{}",
        URL_SAFE_NO_PAD.encode(id.as_bytes())
    )
}

fn is_openai_host(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host == "api.openai.com" || host.ends_with(".openai.azure.com")
}

fn is_openai_model(model: &str) -> bool {
    let bare = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    bare.starts_with("gpt-")
        || bare.starts_with("chatgpt-")
        || bare.starts_with("codex-")
        || (bare.len() > 1 && bare.starts_with('o') && bare.as_bytes()[1].is_ascii_digit())
}

fn responses_replays_unsigned_reasoning(host: Option<&str>, model: &str) -> bool {
    if is_openai_host(host) || is_openai_model(model) {
        return false;
    }
    chat_replays_reasoning(host, model)
        || model.to_ascii_lowercase().contains("grok")
        || host.is_some_and(|h| h.to_ascii_lowercase().contains("x.ai"))
}

fn attachment_text(media_type: &str, name: Option<&str>) -> String {
    if let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) {
        format!("[Attached file: {name}]")
    } else if !media_type.trim().is_empty() {
        format!("[Attached file ({})]", media_type.trim())
    } else {
        "[Attached file]".to_owned()
    }
}

pub(crate) fn request(r: &Request, target: Protocol) -> Result<Value, ProxyError> {
    request_for_host(r, target, None)
}

pub(crate) fn request_for_host(
    r: &Request,
    target: Protocol,
    host: Option<&str>,
) -> Result<Value, ProxyError> {
    let needs_portable = r
        .tools
        .iter()
        .any(|t| matches!(t.kind.as_str(), "namespace" | "custom" | "tool_search"))
        || r.messages.iter().flat_map(|m| &m.parts).any(|p| {
            matches!(
                p,
                Part::NamespacedToolCall { .. }
                    | Part::CustomToolCall { .. }
                    | Part::CustomToolResult { .. }
                    | Part::ToolSearchCall { .. }
                    | Part::ToolSearchResult { .. }
            )
        })
        || matches!(
            r.tool_choice,
            Some(
                ToolChoice::Namespaced { .. } | ToolChoice::Custom { .. } | ToolChoice::ToolSearch
            )
        );
    let portable = if target != Protocol::Responses && needs_portable {
        Some(portable_request(r)?)
    } else {
        None
    };
    let r = portable.as_ref().unwrap_or(r);
    let mut body = match target {
        Protocol::Chat => chat(r, host)?,
        Protocol::Responses => responses(r, host)?,
        Protocol::Anthropic => anthropic(r)?,
        Protocol::Gemini => gemini(r)?,
    };
    if target != Protocol::Gemini {
        body["model"] = json!(r.model);
        body["stream"] = json!(r.stream);
        if let Some(value) = &r.temperature {
            body["temperature"] = value.clone();
        }
        if let Some(value) = &r.top_p {
            body["top_p"] = value.clone();
        }
        if let Some(value) = &r.native.service_tier {
            if target != Protocol::Anthropic || r.source == Protocol::Anthropic {
                body["service_tier"] = json!(value);
            }
        }
    }
    Ok(body)
}

fn qualified_name(namespace: &str, name: &str) -> String {
    crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(namespace, name)
}
fn portable_part(part: &Part) -> Part {
    match part {
        Part::ToolSearchCall { id, arguments, .. } => Part::ToolCall {
            id: id.clone(),
            name: crate::proxy::providers::transform_codex_chat::TOOL_SEARCH_PROXY_NAME.into(),
            arguments: arguments.clone(),
            signature: None,
        },
        Part::ToolSearchResult { call_id, native } => Part::ToolResult {
            call_id: call_id.clone(),
            name: Some(
                crate::proxy::providers::transform_codex_chat::TOOL_SEARCH_PROXY_NAME.into(),
            ),
            parts: vec![Part::Text {
                text: native.to_string(),
                signature: None,
            }],
            is_error: false,
        },
        Part::NamespacedToolCall {
            id,
            namespace,
            name,
            arguments,
            signature,
        } => Part::ToolCall {
            id: id.clone(),
            name: qualified_name(namespace, name),
            arguments: arguments.clone(),
            signature: signature.clone(),
        },
        Part::CustomToolCall {
            id,
            name,
            input,
            namespace,
        } => Part::ToolCall {
            id: id.clone(),
            name: namespace
                .as_ref()
                .map(|ns| qualified_name(ns, name))
                .unwrap_or_else(|| name.clone()),
            arguments: json!({"input":input}),
            signature: None,
        },
        Part::CustomToolResult { call_id, parts } => Part::ToolResult {
            call_id: call_id.clone(),
            name: None,
            parts: parts.iter().map(portable_part).collect(),
            is_error: false,
        },
        Part::ToolResult {
            call_id,
            name,
            parts,
            is_error,
        } => Part::ToolResult {
            call_id: call_id.clone(),
            name: name.clone(),
            parts: parts.iter().map(portable_part).collect(),
            is_error: *is_error,
        },
        _ => part.clone(),
    }
}
fn custom_function(native: &Value, namespace: Option<&str>) -> Result<Tool, ProxyError> {
    let name = native["name"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(invalid)?;
    let name = namespace
        .map(|ns| qualified_name(ns, name))
        .unwrap_or_else(|| name.to_owned());
    let description=format!("{}\nOriginal custom tool definition: {}\nPass its complete unmodified raw input in the input string.",native["description"].as_str().unwrap_or_default(),native);
    Ok(Tool {
        kind: "function".into(),
        native: None,
        name,
        description,
        schema: json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"]}),
        strict: None,
        deferred: native["defer_loading"].as_bool().unwrap_or(false),
    })
}
fn portable_tool(tool: &Tool) -> Result<Vec<Tool>, ProxyError> {
    match tool.kind.as_str() {
        "tool_search" => {
            let native = tool.native.as_ref().ok_or_else(invalid)?;
            if !client_tool_search(native) {
                return Err(invalid());
            }
            let (description, schema) =
                crate::proxy::providers::transform_codex_chat::tool_search_definition(native);
            if !schema.is_object() && !schema.is_boolean() {
                return Err(invalid());
            }
            Ok(vec![Tool {
                kind: "function".into(),
                native: None,
                name: crate::proxy::providers::transform_codex_chat::TOOL_SEARCH_PROXY_NAME.into(),
                description,
                schema,
                strict: None,
                deferred: false,
            }])
        }
        "custom" => Ok(vec![custom_function(
            tool.native.as_ref().ok_or_else(invalid)?,
            None,
        )?]),
        "namespace" => {
            let native = tool.native.as_ref().ok_or_else(invalid)?;
            let namespace = native["name"]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or_else(invalid)?;
            let children = native
                .get("tools")
                .or_else(|| native.get("children"))
                .and_then(Value::as_array)
                .ok_or_else(invalid)?;
            let mut out = Vec::new();
            for child in children {
                if child["type"] == "custom" {
                    out.push(custom_function(child, Some(namespace))?);
                    continue;
                }
                if child["type"] != "function" {
                    return Err(invalid());
                }
                let name = child["name"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .ok_or_else(invalid)?;
                let schema = child
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object","properties":{}}));
                if !schema.is_object() && !schema.is_boolean() {
                    return Err(invalid());
                }
                out.push(Tool {
                    kind: "function".into(),
                    native: None,
                    name: qualified_name(namespace, name),
                    description: child["description"].as_str().unwrap_or_default().to_owned(),
                    schema,
                    strict: child["strict"].as_bool(),
                    deferred: child["defer_loading"].as_bool().unwrap_or(false),
                });
            }
            Ok(out)
        }
        _ => Ok(vec![tool.clone()]),
    }
}
fn client_tool_search(native: &Value) -> bool {
    native
        .get("execution")
        .filter(|value| !value.is_null())
        .is_none_or(|value| value == "client")
}
fn portable_request(r: &Request) -> Result<Request, ProxyError> {
    for part in r.messages.iter().flat_map(|message| &message.parts) {
        if let Part::ToolSearchCall { native, .. } | Part::ToolSearchResult { native, .. } = part {
            if !client_tool_search(native) {
                return Err(invalid());
            }
        }
    }
    let mut portable = r.clone();
    portable.system = r.system.iter().map(portable_part).collect();
    portable.messages = r
        .messages
        .iter()
        .map(|m| Message {
            role: m.role.clone(),
            parts: m.parts.iter().map(portable_part).collect(),
        })
        .collect();
    portable.tools = r
        .tools
        .iter()
        .map(portable_tool)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    portable.tool_choice = match &r.tool_choice {
        Some(ToolChoice::ToolSearch) => Some(ToolChoice::Named(
            crate::proxy::providers::transform_codex_chat::TOOL_SEARCH_PROXY_NAME.into(),
        )),
        Some(ToolChoice::Namespaced { namespace, name }) => {
            Some(ToolChoice::Named(qualified_name(namespace, name)))
        }
        Some(ToolChoice::Custom { name, namespace }) => Some(ToolChoice::Named(
            namespace
                .as_ref()
                .map(|ns| qualified_name(ns, name))
                .unwrap_or_else(|| name.clone()),
        )),
        other => other.clone(),
    };
    Ok(portable)
}

fn source(media: &str, data: &Option<String>, url: &Option<String>) -> Result<String, ProxyError> {
    match (data, url) {
        (Some(data), _) => Ok(format!("data:{media};base64,{data}")),
        (_, Some(url)) => Ok(url.clone()),
        _ => Err(invalid()),
    }
}
fn text(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|p| match p {
            Part::Text { text, .. } => Some(text.clone()),
            Part::ToolReference { name } => Some(format!("Tool available: {name}")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn tool_functions(r: &Request) -> impl Iterator<Item = &Tool> {
    r.tools.iter().filter(|t| t.kind == "function")
}
fn native_tool(tool: &Tool, target: Protocol) -> Result<Value, ProxyError> {
    let native = tool.native.as_ref().ok_or_else(invalid)?;
    if tool.kind == "web_search" {
        return Ok(match target {
            Protocol::Responses => {
                if matches!(
                    native["type"].as_str(),
                    Some("web_search" | "web_search_preview")
                ) {
                    native.clone()
                } else {
                    json!({"type":"web_search"})
                }
            }
            Protocol::Anthropic => {
                if native["type"]
                    .as_str()
                    .is_some_and(|v| v.starts_with("web_search_"))
                    && native["name"] == "web_search"
                {
                    native.clone()
                } else {
                    json!({"type":"web_search_20250305","name":"web_search"})
                }
            }
            Protocol::Gemini => {
                if native.get("googleSearch").is_some()
                    || native.get("googleSearchRetrieval").is_some()
                {
                    native.clone()
                } else {
                    json!({"googleSearch":{}})
                }
            }
            Protocol::Chat => native
                .get("web_search_options")
                .cloned()
                .unwrap_or_else(|| json!({})),
        });
    }
    let kind = native["type"].as_str().unwrap_or_default();
    match target {
        Protocol::Responses
            if matches!(
                kind,
                "custom"
                    | "namespace"
                    | "file_search"
                    | "computer_use_preview"
                    | "computer"
                    | "code_interpreter"
                    | "mcp"
                    | "image_generation"
                    | "tool_search"
                    | "shell"
                    | "local_shell"
                    | "apply_patch"
            ) =>
        {
            Ok(native.clone())
        }
        Protocol::Anthropic
            if kind.starts_with("computer_")
                || kind.starts_with("bash_")
                || kind.starts_with("text_editor_")
                || kind.starts_with("web_fetch_")
                || kind.starts_with("code_execution_")
                || kind.starts_with("tool_search_") =>
        {
            Ok(native.clone())
        }
        Protocol::Gemini
            if ["codeExecution", "urlContext"]
                .iter()
                .any(|key| native.get(*key).is_some()) =>
        {
            Ok(native.clone())
        }
        _ => Err(invalid()),
    }
}
fn effort_from_budget(budget: i64) -> &'static str {
    match budget {
        ..=0 => "",
        1..=4096 => "low",
        4097..=12000 => "medium",
        12001..=24000 => "high",
        _ => "xhigh",
    }
}
fn thinking_budget(effort: &str) -> i64 {
    match effort {
        "none" | "minimal" | "low" => 4096,
        "high" => 24000,
        "xhigh" | "max" | "ultra" => 32000,
        _ => 10000,
    }
}
fn claude_version(model: &str) -> Option<(u32, u32)> {
    let name = model.to_ascii_lowercase();
    let name = name.split("claude-").nth(1)?;
    let segments = name.split(['-', '.']).collect::<Vec<_>>();
    for pair in segments.windows(2) {
        if pair[0].len() <= 2 && pair[1].len() <= 2 {
            if let (Ok(major), Ok(minor)) = (pair[0].parse(), pair[1].parse()) {
                return Some((major, minor));
            }
        }
    }
    None
}
fn anthropic_thinking(r: &Request, body: &mut Value) {
    let reasoning = &r.reasoning;
    if reasoning.enabled == Some(false) || reasoning.effort.as_deref() == Some("none") {
        body["thinking"] = json!({"type":"disabled"});
        return;
    }
    if reasoning.effort.is_none()
        && reasoning.budget.is_none()
        && reasoning.enabled != Some(true)
        && !reasoning.visible
    {
        return;
    }
    let inferred = reasoning
        .budget
        .map(effort_from_budget)
        .filter(|v| !v.is_empty());
    let effort = reasoning.effort.as_deref().or(inferred).unwrap_or("medium");
    let version = claude_version(&r.model);
    if version.is_some_and(|(major, minor)| major > 4 || (major == 4 && minor >= 6)) {
        body["thinking"] = json!({"type":"adaptive","display":"summarized"});
        body["output_config"]["effort"] = json!(if effort == "xhigh" && version == Some((4, 6)) {
            "max"
        } else if effort == "minimal" {
            "low"
        } else {
            effort
        });
        return;
    }
    if r.max_tokens.is_some_and(|cap| cap <= 2048) {
        return;
    }
    let mut budget = reasoning
        .budget
        .filter(|b| *b > 0)
        .unwrap_or_else(|| thinking_budget(effort));
    if let Some(cap) = r.max_tokens {
        budget = budget.min(i64::try_from(cap.saturating_sub(1024)).unwrap_or(i64::MAX));
    } else {
        body["max_tokens"] = json!(8192_i64.max(budget.saturating_add(4096)));
    }
    if budget > 0 {
        body["thinking"] = json!({"type":"enabled","budget_tokens":budget});
    }
}
fn push_gemini_turn(contents: &mut Vec<Value>, role: &str, parts: Vec<Value>, results: bool) {
    if parts.is_empty() {
        return;
    }
    let previous_same_kind = contents
        .last()
        .and_then(|last| last["parts"].as_array())
        .is_some_and(|parts| parts.iter().all(|p| p.get("functionResponse").is_some()) == results);
    if let Some(last) = contents
        .last_mut()
        .filter(|last| last["role"] == role && previous_same_kind)
    {
        if let Some(existing) = last["parts"].as_array_mut() {
            existing.extend(parts);
        }
    } else {
        contents.push(json!({"role":role,"parts":parts}));
    }
}
fn choice(r: &Request, target: Protocol) -> Option<Value> {
    r.tool_choice.as_ref().map(|choice| match (target, choice) {
        (Protocol::Responses, ToolChoice::ToolSearch) => json!({"type":"tool_search"}),
        (Protocol::Responses, ToolChoice::Namespaced { namespace, name }) => {
            json!({"type":"function","namespace":namespace,"name":name})
        }
        (Protocol::Responses, ToolChoice::Custom { name, namespace }) => {
            let mut choice = json!({"type":"custom","name":name});
            if let Some(ns) = namespace {
                choice["namespace"] = json!(ns);
            }
            choice
        }
        (Protocol::Anthropic, ToolChoice::Named(name)) => json!({"type":"tool","name":name}),
        (Protocol::Anthropic, c) => {
            json!({"type": match c { ToolChoice::Auto=>"auto",ToolChoice::None=>"none",_=>"any" }})
        }
        (Protocol::Chat, ToolChoice::Named(name)) => {
            json!({"type":"function","function":{"name":name}})
        }
        (Protocol::Responses, ToolChoice::Named(name)) => json!({"type":"function","name":name}),
        (_, ToolChoice::Auto) => json!("auto"),
        (_, ToolChoice::None) => json!("none"),
        (_, ToolChoice::Required) => json!("required"),
        _ => Value::Null,
    })
}
fn openai_parts(
    parts: &[Part],
    responses: bool,
    assistant: bool,
    allow_native_file: bool,
) -> Result<Vec<Value>, ProxyError> {
    let mut out = Vec::new();
    for p in parts {
        match p {
            Part::Text { text, .. } => out.push(json!({"type":if responses { if assistant {"output_text"} else {"input_text"} } else {"text"},"text":text})),
            Part::Image { media_type, data, url, detail } => {
                let uri = source(media_type, data, url)?;
                let mut image = if responses { json!({"type":"input_image","image_url":uri}) } else { json!({"type":"image_url","image_url":{"url":uri}}) };
                if let Some(detail) = detail { if responses {image["detail"] = json!(detail)} else {image["image_url"]["detail"] = json!(detail)} }
                out.push(image);
            }
            Part::File { media_type, data, url, id, name } => {
                if responses {
                    let supports_native = !assistant
                        && (allow_native_file
                            || (media_type.eq_ignore_ascii_case("application/pdf")
                                && data.is_some()))
                        && (id.is_some() || url.is_some() || data.is_some());
                    if supports_native {
                        let mut file = json!({"type": "input_file"});
                        if let Some(id) = id {
                            file["file_id"] = json!(id);
                        } else if let Some(url) = url {
                            file["file_url"] = json!(url);
                        } else if data.is_some() {
                            file["file_data"] = json!(source(media_type, data, url)?);
                        }
                        if let Some(name) = name {
                            file["filename"] = json!(name);
                        }
                        out.push(file);
                    } else {
                        out.push(json!({
                            "type": if assistant { "output_text" } else { "input_text" },
                            "text": attachment_text(media_type, name.as_deref())
                        }));
                    }
                } else if media_type.eq_ignore_ascii_case("application/pdf") && data.is_some() {
                    let mut file = json!({"file_data": source(media_type, data, url)?});
                    if let Some(name) = name {
                        file["filename"] = json!(name);
                    }
                    out.push(json!({"type": "file", "file": file}));
                } else {
                    out.push(json!({
                        "type": "text",
                        "text": attachment_text(media_type, name.as_deref())
                    }));
                }
            }
            Part::Search { query, hits, .. } => out.push(json!({"type":if responses {"input_text"} else {"text"},"text":format!("{query}\n{}",json!(hits))})),
            Part::ToolReference { name } => out.push(json!({"type":if responses {"input_text"} else {"text"},"text":format!("Tool available: {name}")})),
            Part::Thinking { .. } | Part::ToolCall { .. } | Part::ToolResult { .. }
            | Part::NamespacedToolCall{..} | Part::CustomToolCall{..} | Part::CustomToolResult{..}
            | Part::ToolSearchCall{..} | Part::ToolSearchResult{..} => {}
        }
    }
    Ok(out)
}

fn tool_result_chat_content(
    call_id: &str,
    parts: &[Part],
    pending_media: &mut Vec<Value>,
) -> Result<String, ProxyError> {
    let (media_ir, text_ir): (Vec<_>, Vec<_>) = parts
        .iter()
        .cloned()
        .partition(|part| matches!(part, Part::Image { .. } | Part::File { .. }));
    let mut text_chunks: Vec<String> = text_ir
        .iter()
        .filter_map(|part| match part {
            Part::Text { text, .. } => Some(text.clone()),
            Part::ToolReference { name } => Some(format!("Tool available: {name}")),
            Part::Search { query, hits, .. } => Some(format!("{query}\n{}", json!(hits))),
            _ => None,
        })
        .collect();
    if !media_ir.is_empty() {
        let mut media_parts = Vec::new();
        for part in media_ir {
            match part {
                Part::Image { .. } => {
                    media_parts.extend(openai_parts(&[part], false, false, false)?);
                }
                Part::File {
                    media_type,
                    data,
                    url,
                    id,
                    name,
                } => {
                    if media_type.eq_ignore_ascii_case("application/pdf") && data.is_some() {
                        media_parts.extend(openai_parts(
                            &[Part::File {
                                media_type,
                                data,
                                url,
                                id,
                                name,
                            }],
                            false,
                            false,
                            false,
                        )?);
                    } else {
                        text_chunks.push(attachment_text(&media_type, name.as_deref()));
                    }
                }
                _ => {}
            }
        }
        if !media_parts.is_empty() {
            text_chunks.push(crate::proxy::tool_media::TOOL_RESULT_MEDIA_MOVED_MARKER.into());
            crate::proxy::tool_media::queue_chat_tool_output_media(
                pending_media,
                &bound_call_id(call_id),
                media_parts,
            );
        }
    }
    Ok(text_chunks.join("\n"))
}

fn chat_wants_reasoning(host: Option<&str>, model: &str) -> bool {
    let host = host.unwrap_or_default().to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    host.contains("deepseek") || model.contains("deepseek") || host == "qoder" || host == "qoder-cn"
}

fn chat_replays_reasoning(host: Option<&str>, model: &str) -> bool {
    chat_wants_reasoning(host, model) || host.is_some_and(|h| h.eq_ignore_ascii_case("devin"))
}

// Responses emits text, reasoning, and each call as separate items. Chat
// expects one assistant turn per step so reasoning, text, and tool calls
// stay together for both plain replies and tool-call batches.
fn responses_chat_messages(mut tail: &[Message]) -> Vec<std::borrow::Cow<'_, Message>> {
    use std::borrow::Cow;
    let mut messages = Vec::with_capacity(tail.len());
    while let Some((message, rest)) = tail.split_first() {
        if message.role != "assistant" {
            messages.push(Cow::Borrowed(message));
            tail = rest;
            continue;
        }
        let count = 1 + rest.iter().take_while(|m| m.role == "assistant").count();
        let (assistants, next) = tail.split_at(count);
        if count > 1 {
            messages.push(Cow::Owned(Message {
                role: message.role.clone(),
                parts: assistants.iter().flat_map(|m| &m.parts).cloned().collect(),
            }));
        } else {
            messages.push(Cow::Borrowed(message));
        }
        tail = next;
    }
    messages
}

fn silent_chat_assistant(m: &Value) -> bool {
    if m["role"] != "assistant" {
        return false;
    }
    if m["prefix"] == true
        || m["partial"] == true
        || m.get("function_call").is_some_and(|v| !v.is_null())
        || m.get("audio").is_some_and(|v| !v.is_null())
        || m["refusal"].as_str().is_some_and(|s| !s.is_empty())
    {
        return false;
    }
    if m["tool_calls"].as_array().is_some_and(|c| !c.is_empty()) {
        return false;
    }
    match &m["content"] {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(parts) => parts.iter().all(|p| {
            p["type"] == "text" && p["text"].as_str().is_some_and(|t| t.trim().is_empty())
        }),
        _ => false,
    }
}

fn joined_user_content(a: &Value, b: &Value) -> Option<Value> {
    if let (Some(as_str), Some(bs_str)) = (a.as_str(), b.as_str()) {
        return Some(json!(format!("{as_str}\n\n{bs_str}")));
    }
    let to_parts = |c: &Value| -> Option<Vec<Value>> {
        match c {
            Value::String(s) => {
                if s.trim().is_empty() {
                    Some(Vec::new())
                } else {
                    Some(vec![json!({"type": "text", "text": s})])
                }
            }
            Value::Array(parts) => Some(parts.clone()),
            _ => None,
        }
    };
    let mut parts = to_parts(a)?;
    parts.extend(to_parts(b)?);
    Some(Value::Array(parts))
}

fn without_silent_assistants(msgs: Vec<(Value, bool)>) -> Vec<(Value, bool)> {
    if !msgs.iter().any(|(m, _)| silent_chat_assistant(m)) {
        return msgs;
    }
    let mut out: Vec<(Value, bool)> = Vec::with_capacity(msgs.len());
    let mut dropped = false;
    for (m, is_reminder) in msgs {
        if silent_chat_assistant(&m) {
            dropped = true;
            continue;
        }
        if dropped
            && out
                .last()
                .is_some_and(|(prev, _)| prev["role"] == "user" && m["role"] == "user")
        {
            if let Some(last) = out.last_mut() {
                if let Some(joined) = joined_user_content(&last.0["content"], &m["content"]) {
                    last.0["content"] = joined;
                    last.1 &= is_reminder;
                    continue;
                }
            }
        }
        dropped = false;
        out.push((m, is_reminder));
    }
    out
}

fn empty_chat_assistant(m: &Value) -> bool {
    if m["tool_calls"].as_array().is_some_and(|c| !c.is_empty()) {
        return false;
    }
    if m["reasoning_content"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return false;
    }
    m["content"].as_str().is_none_or(|s| s.trim().is_empty())
}

fn chat_content_text(c: &Value) -> String {
    match c {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn pair_chat_tool_messages(msgs: Vec<(Value, bool)>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(msgs.len());
    let mut pending: Vec<String> = Vec::new();
    let mut order: HashMap<String, usize> = HashMap::new();
    let mut answered: HashSet<String> = HashSet::new();
    let mut tools: Vec<Value> = Vec::new();
    let mut held: Vec<Value> = Vec::new();

    let flush_pending = |out: &mut Vec<Value>,
                         pending: &mut Vec<String>,
                         order: &mut HashMap<String, usize>,
                         answered: &mut HashSet<String>,
                         tools: &mut Vec<Value>,
                         held: &mut Vec<Value>| {
        if !tools.is_empty() {
            tools.sort_by_key(|t| {
                t["tool_call_id"]
                    .as_str()
                    .and_then(|id| order.get(id).copied())
                    .unwrap_or(usize::MAX)
            });
            out.append(tools);
        }
        for id in pending.iter() {
            if !answered.contains(id) {
                out.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": INTERRUPTED_TOOL_MESSAGE
                }));
            }
        }
        pending.clear();
        order.clear();
        answered.clear();
        out.append(held);
    };

    let tool_as_user = |out: &mut Vec<Value>, m: &Value| {
        let mut text = chat_content_text(&m["content"]);
        if text.trim().is_empty() {
            text = "Tool result received.".to_owned();
        }
        if let Some(last) = out.last_mut().filter(|prev| prev["role"] == "user") {
            if let Some(s) = last["content"].as_str() {
                last["content"] = json!(format!("{s}\n\n{text}"));
                return;
            }
        }
        out.push(json!({"role": "user", "content": text}));
    };

    for (m, is_reminder) in msgs {
        let role = m["role"].as_str().unwrap_or_default();
        match role {
            "assistant" => {
                let calls = m["tool_calls"]
                    .as_array()
                    .filter(|calls| !calls.is_empty())
                    .cloned();
                if let Some(calls) = calls {
                    flush_pending(
                        &mut out,
                        &mut pending,
                        &mut order,
                        &mut answered,
                        &mut tools,
                        &mut held,
                    );
                    out.push(m);
                    for (i, call) in calls.iter().enumerate() {
                        let id = call["id"].as_str().unwrap_or_default().to_owned();
                        order.insert(id.clone(), i);
                        pending.push(id);
                    }
                } else {
                    if !pending.is_empty() && empty_chat_assistant(&m) {
                        continue;
                    }
                    flush_pending(
                        &mut out,
                        &mut pending,
                        &mut order,
                        &mut answered,
                        &mut tools,
                        &mut held,
                    );
                    out.push(m);
                }
            }
            "tool" => {
                let id = m["tool_call_id"].as_str().unwrap_or_default();
                if !order.contains_key(id) {
                    flush_pending(
                        &mut out,
                        &mut pending,
                        &mut order,
                        &mut answered,
                        &mut tools,
                        &mut held,
                    );
                    tool_as_user(&mut out, &m);
                    continue;
                }
                if !answered.insert(id.to_owned()) {
                    continue;
                }
                tools.push(m);
            }
            _ if is_reminder || matches!(role, "system" | "developer") => {
                if !pending.is_empty() {
                    held.push(m);
                } else {
                    out.push(m);
                }
            }
            _ => {
                flush_pending(
                    &mut out,
                    &mut pending,
                    &mut order,
                    &mut answered,
                    &mut tools,
                    &mut held,
                );
                out.push(m);
            }
        }
    }
    flush_pending(
        &mut out,
        &mut pending,
        &mut order,
        &mut answered,
        &mut tools,
        &mut held,
    );
    out
}

fn chat(r: &Request, host: Option<&str>) -> Result<Value, ProxyError> {
    let mut tagged_messages: Vec<(Value, bool)> = Vec::new();
    let mut system = openai_parts(&r.system, false, false, false)?;
    let mut replied = false;
    let mut after_tool = false;
    let replay = chat_replays_reasoning(host, &r.model);
    let wants = chat_wants_reasoning(host, &r.model);
    let mut pending_tool_media: Vec<Value> = Vec::new();
    let flush_media = |tagged: &mut Vec<(Value, bool)>, pending: &mut Vec<Value>| {
        if pending.is_empty() {
            return;
        }
        let mut flushed = Vec::new();
        crate::proxy::tool_media::flush_pending_chat_tool_media(&mut flushed, pending);
        tagged.extend(flushed.into_iter().map(|msg| (msg, false)));
    };
    let responses_messages;
    let history: Box<dyn Iterator<Item = &Message>> = if r.source == Protocol::Responses {
        responses_messages = responses_chat_messages(&r.messages);
        Box::new(responses_messages.iter().map(std::borrow::Cow::as_ref))
    } else {
        Box::new(r.messages.iter())
    };
    for m in history {
        replied |= m.role == "assistant"
            || m.parts.iter().any(|part| {
                matches!(
                    part,
                    Part::ToolCall { .. } | Part::ToolResult { .. } | Part::Thinking { .. }
                )
            });
        if matches!(m.role.as_str(), "system" | "developer") {
            let mut content = openai_parts(&m.parts, false, false, false)?;
            if !replied {
                system.extend(content);
            } else if !content.is_empty() {
                flush_media(&mut tagged_messages, &mut pending_tool_media);
                // Chat gateways may reject developer roles and later system
                // messages. Keep updates in place using Magpie's reminder form.
                let content = if content.iter().all(|part| part["type"] == "text") {
                    json!(format!(
                        "<system-reminder>\n{}\n</system-reminder>",
                        text(&m.parts)
                    ))
                } else {
                    content.insert(0, json!({"type":"text","text":"<system-reminder>\n"}));
                    content.push(json!({"type":"text","text":"\n</system-reminder>"}));
                    json!(content)
                };
                after_tool = false;
                tagged_messages.push((json!({"role":"user","content":content}), true));
            }
            continue;
        }
        if m.role == "assistant" {
            let calls: Vec<_> = m
                .parts
                .iter()
                .filter_map(|p| {
                    if let Part::ToolCall {
                        id,
                        name,
                        arguments,
                        signature,
                    } = p
                    {
                        let mut call = json!({
                            "id": bound_call_id(id),
                            "type": "function",
                            "function": {"name": name, "arguments": arguments.to_string()}
                        });
                        if let Some(signature) = signature {
                            call["extra_content"] =
                                json!({"google":{"thought_signature":signature}});
                        }
                        Some(call)
                    } else {
                        None
                    }
                })
                .collect();
            let content = openai_parts(&m.parts, false, true, false)?;
            let all_text = content.iter().all(|part| part["type"] == "text");
            let plain_text = if all_text {
                content
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            };
            let thoughts: Vec<_> = m
                .parts
                .iter()
                .filter_map(|p| match p {
                    Part::Thinking { text, .. } if !text.is_empty() => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let has_tool_results = m.parts.iter().any(|p| matches!(p, Part::ToolResult { .. }));
            if !calls.is_empty() || !content.is_empty() || !thoughts.is_empty() || !has_tool_results
            {
                let mut message = json!({"role": "assistant"});
                if !calls.is_empty() {
                    message["tool_calls"] = json!(calls);
                }
                if !thoughts.is_empty() && replay {
                    message["reasoning_content"] = json!(thoughts.join("\n"));
                } else if wants && (!calls.is_empty() || after_tool) {
                    message["reasoning_content"] = json!(" ");
                }
                if !content.is_empty() && !all_text {
                    message["content"] = json!(content);
                } else if !plain_text.trim().is_empty() {
                    message["content"] = json!(plain_text);
                } else if calls.is_empty() {
                    message["content"] = json!("");
                }
                flush_media(&mut tagged_messages, &mut pending_tool_media);
                after_tool = false;
                tagged_messages.push((message, false));
            }
            for p in &m.parts {
                if let Part::ToolResult { call_id, parts, .. } = p {
                    let content =
                        tool_result_chat_content(call_id, parts, &mut pending_tool_media)?;
                    tagged_messages.push((
                        json!({"role":"tool","tool_call_id":bound_call_id(call_id),"content":content}),
                        false,
                    ));
                    after_tool = true;
                }
            }
            continue;
        }
        for p in &m.parts {
            if let Part::ToolResult { call_id, parts, .. } = p {
                let content = tool_result_chat_content(call_id, parts, &mut pending_tool_media)?;
                tagged_messages.push((
                    json!({"role":"tool","tool_call_id":bound_call_id(call_id),"content":content}),
                    false,
                ));
                after_tool = true;
            }
        }
        let content = openai_parts(&m.parts, false, false, false)?;
        if !content.is_empty() {
            let wire_content = match content.as_slice() {
                [part] if part["type"] == "text" => part["text"].clone(),
                _ => json!(content),
            };
            flush_media(&mut tagged_messages, &mut pending_tool_media);
            after_tool = false;
            tagged_messages.push((json!({"role": m.role, "content": wire_content}), false));
        }
    }
    flush_media(&mut tagged_messages, &mut pending_tool_media);
    let tagged_messages = without_silent_assistants(tagged_messages);
    let mut messages = pair_chat_tool_messages(tagged_messages);
    if !system.is_empty() {
        let content = if system.iter().all(|part| part["type"] == "text") {
            json!(system
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n\n"))
        } else {
            json!(system)
        };
        messages.insert(0, json!({"role":"system","content":content}));
    }
    let mut body = json!({"messages":messages});
    if let Some(n) = r.max_tokens {
        if is_openai_host(host) {
            body["max_completion_tokens"] = json!(n);
        } else {
            body["max_tokens"] = json!(n);
        }
    }
    if !r.stop.is_empty() {
        body["stop"] = json!(r.stop);
    }
    let mut tools = Vec::new();
    let mut omitted_search = false;
    for t in &r.tools {
        if t.kind == "web_search" {
            // An optional hosted-search declaration does not establish Chat
            // search support. Codex sends it on ordinary turns too; generic
            // Chat gateways reject an invented web_search_options field.
            omitted_search = true;
            continue;
        }
        if t.kind != "function" {
            return Err(invalid());
        }
        // Match Magpie's generic Chat conversion: the complete schema is
        // portable, but a strict flag is not supported by every Chat gateway.
        let f = json!({"name":t.name,"description":t.description,"parameters":t.schema});
        tools.push(json!({"type":"function","function":f}));
    }
    if omitted_search && tools.is_empty() && r.tool_choice == Some(ToolChoice::Required) {
        return Err(invalid());
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if body.get("tools").is_some()
        || (!omitted_search && r.native.responses_tool_allowlist.is_none())
    {
        if let Some(value) = choice(r, Protocol::Chat) {
            body["tool_choice"] = value;
        }
        if let Some(value) = r.parallel {
            body["parallel_tool_calls"] = json!(value);
        }
    }
    if let Some(value) = &r.native.user {
        body["user"] = json!(value);
    }
    if let Some(value) = &r.native.prompt_cache_key {
        body["prompt_cache_key"] = json!(value);
    }
    if r.reasoning.enabled == Some(false) {
        body["reasoning_effort"] = json!("none");
    } else if let Some(value) = &r.reasoning.effort {
        body["reasoning_effort"] = json!(value);
    }
    // Chat Completions does not support Responses' reasoning_summary (matches Magpie buildChat).
    if let Some(format) = &r.format {
        body["response_format"] = if format.kind == "json_schema" {
            json!({"type":"json_schema","json_schema":{"name":format.name.as_deref().unwrap_or("response"),"schema":format.schema,"strict":format.strict.unwrap_or(false)}})
        } else {
            json!({"type":format.kind})
        };
    }
    if r.stream {
        body["stream_options"] = json!({"include_usage":true});
    }
    Ok(body)
}

fn responses_output_text(output: &Value) -> String {
    match output {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn pair_responses_tool_items(items: Vec<Value>) -> Vec<Value> {
    let mut seen_calls = HashSet::new();
    let mut answered = HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let kind = item["type"].as_str().unwrap_or_default();
        match kind {
            "function_call" | "custom_tool_call" | "tool_search_call" => {
                if let Some(id) = item["call_id"].as_str().filter(|id| !id.is_empty()) {
                    seen_calls.insert(id.to_owned());
                }
                out.push(item);
            }
            "function_call_output" | "custom_tool_call_output" => {
                let id = item["call_id"].as_str().unwrap_or_default();
                if !seen_calls.contains(id) {
                    let mut text = responses_output_text(&item["output"]);
                    if text.trim().is_empty() {
                        text = "Tool result received.".to_owned();
                    }
                    out.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": [{"type": "input_text", "text": text}]
                    }));
                } else if answered.insert(id.to_owned()) {
                    out.push(item);
                }
            }
            "tool_search_output" => {
                let id = item["call_id"].as_str().unwrap_or_default();
                if seen_calls.contains(id) && answered.insert(id.to_owned()) {
                    out.push(item);
                }
            }
            _ => out.push(item),
        }
    }
    out
}

fn responses(r: &Request, host: Option<&str>) -> Result<Value, ProxyError> {
    let mut input = Vec::new();
    let mut instructions_parts = Vec::new();
    let native_responses = r.source == Protocol::Responses;
    if !r.system.is_empty() {
        if r.system
            .iter()
            .all(|part| matches!(part, Part::Text { .. } | Part::ToolReference { .. }))
        {
            let text = text(&r.system);
            if !text.is_empty() {
                instructions_parts.push(text);
            }
        } else {
            let parts = openai_parts(&r.system, true, false, native_responses)?;
            if !parts.is_empty() {
                input.push(json!({"type":"message","role":"developer","content":parts}));
            }
        }
    }
    let replay_reasoning = responses_replays_unsigned_reasoning(host, &r.model);
    let mut replied = false;
    for m in &r.messages {
        replied |= m.role == "assistant"
            || m.parts.iter().any(|part| {
                matches!(
                    part,
                    Part::ToolCall { .. } | Part::ToolResult { .. } | Part::Thinking { .. }
                )
            });
        if !native_responses && matches!(m.role.as_str(), "system" | "developer") {
            let all_text = m
                .parts
                .iter()
                .all(|part| matches!(part, Part::Text { .. } | Part::ToolReference { .. }));
            if !replied && all_text {
                let text = text(&m.parts);
                if !text.is_empty() {
                    instructions_parts.push(text);
                }
            } else {
                let content = openai_parts(&m.parts, true, false, native_responses)?;
                if !content.is_empty() {
                    input.push(json!({"type":"message","role":"developer","content":content}));
                }
            }
            continue;
        }
        let mut pending = Vec::new();
        for p in &m.parts {
            let item = match p {
                Part::ToolSearchCall { native, .. } | Part::ToolSearchResult { native, .. } => {
                    let mut item = native.clone();
                    if let Some(id) = item.get("call_id").and_then(Value::as_str) {
                        item["call_id"] = json!(bound_call_id(id));
                    }
                    Some(item)
                }
                Part::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } => Some(json!({
                    "type":"function_call",
                    "call_id":bound_call_id(id),
                    "name":name,
                    "arguments":arguments.to_string()
                })),
                Part::NamespacedToolCall {
                    id,
                    namespace,
                    name,
                    arguments,
                    ..
                } => Some(json!({
                    "type":"function_call",
                    "call_id":bound_call_id(id),
                    "namespace":namespace,
                    "name":name,
                    "arguments":arguments.to_string()
                })),
                Part::CustomToolCall {
                    id,
                    name,
                    input,
                    namespace,
                } => {
                    let mut call = json!({
                        "type":"custom_tool_call",
                        "call_id":bound_call_id(id),
                        "name":name,
                        "input":input
                    });
                    if let Some(ns) = namespace {
                        call["namespace"] = json!(ns);
                    }
                    Some(call)
                }
                Part::CustomToolResult { call_id, parts } => {
                    let output = if parts.iter().all(|p| matches!(p, Part::Text { .. })) {
                        json!(text(parts))
                    } else {
                        json!(openai_parts(parts, true, false, native_responses)?)
                    };
                    Some(json!({
                        "type":"custom_tool_call_output",
                        "call_id":bound_call_id(call_id),
                        "output":output
                    }))
                }
                Part::ToolResult { call_id, parts, .. } => {
                    let output = if parts.iter().all(|p| matches!(p, Part::Text { .. })) {
                        json!(text(parts))
                    } else {
                        json!(openai_parts(parts, true, false, native_responses)?)
                    };
                    Some(json!({
                        "type":"function_call_output",
                        "call_id":bound_call_id(call_id),
                        "output":output
                    }))
                }
                Part::Thinking {
                    text,
                    source,
                    sealed,
                    ..
                } => {
                    if *source == Protocol::Responses && sealed.is_some() {
                        Some(sealed.clone().ok_or_else(invalid)?)
                    } else if replay_reasoning && !text.is_empty() {
                        Some(
                            json!({"type":"reasoning","summary":[{"type":"summary_text","text":text}]}),
                        )
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(item) = item {
                if !pending.is_empty() {
                    let content =
                        openai_parts(&pending, true, m.role == "assistant", native_responses)?;
                    if !content.is_empty() {
                        input.push(json!({"role":m.role,"content":content}));
                    }
                    pending.clear();
                }
                input.push(item);
            } else {
                pending.push(p.clone());
            }
        }
        if !pending.is_empty() {
            let content = openai_parts(&pending, true, m.role == "assistant", native_responses)?;
            if !content.is_empty() {
                input.push(json!({"role":m.role,"content":content}));
            }
        }
    }
    let input = pair_responses_tool_items(input);
    let mut body = json!({"input":input});
    if !instructions_parts.is_empty() {
        body["instructions"] = json!(instructions_parts.join("\n\n"));
    }
    if let Some(n) = r.max_tokens {
        body["max_output_tokens"] = json!(n);
    }
    if !r.stop.is_empty() {
        body["stop"] = json!(r.stop);
    }
    if let Some(value) = &r.native.metadata {
        body["metadata"] = value.clone();
    }
    if let Some(value) = &r.native.user {
        body["user"] = json!(value);
    }
    let mut tools = Vec::new();
    for t in &r.tools {
        if t.kind != "function" {
            tools.push(native_tool(t, Protocol::Responses)?);
            continue;
        }
        let mut f = json!({"type":"function","name":t.name,"description":t.description,"parameters":t.schema});
        if let Some(strict) = t.strict {
            f["strict"] = json!(strict);
        }
        // Magpie's responsesTool offers translated functions eagerly.
        // A Responses client's own native search contract stays on its wire.
        if t.deferred
            && t.native
                .as_ref()
                .is_some_and(|tool| tool["type"] == "function")
        {
            f["defer_loading"] = json!(true);
        }
        tools.push(f);
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if let Some(v) = r
        .native
        .responses_tool_allowlist
        .clone()
        .or_else(|| choice(r, Protocol::Responses))
    {
        body["tool_choice"] = v;
    }
    if let Some(v) = r.parallel {
        body["parallel_tool_calls"] = json!(v);
    }
    if let Some(v) = &r.native.prompt_cache_key {
        body["prompt_cache_key"] = json!(v);
    }
    for (key, value) in [
        ("include", &r.native.include),
        ("client_metadata", &r.native.client_metadata),
        ("text", &r.native.text),
        ("access_programs", &r.native.access_programs),
    ] {
        if let Some(v) = value {
            body[key] = v.clone();
        }
    }
    if let Some(v) = &r.native.cache_retention {
        body["prompt_cache_retention"] = json!(v);
    }
    if r.reasoning.enabled == Some(false) {
        body["reasoning"]["effort"] = json!("none");
    } else if let Some(v) = &r.reasoning.effort {
        body["reasoning"]["effort"] = json!(v);
    }
    if let Some(v) = &r.reasoning.summary {
        body["reasoning"]["summary"] = json!(v);
    }
    if let Some(f) = &r.format {
        let mut format = json!({"type":f.kind});
        if let Some(v) = &f.name {
            format["name"] = json!(v);
        }
        if let Some(v) = &f.schema {
            format["schema"] = v.clone();
        }
        if let Some(v) = f.strict {
            format["strict"] = json!(v);
        }
        body["text"]["format"] = format;
    }
    Ok(body)
}

fn anthropic_parts(parts: &[Part], native_anthropic: bool) -> Result<Vec<Value>, ProxyError> {
    let mut out = Vec::new();
    for p in parts {
        match p {
            Part::Text { text, .. } => {
                if !text.trim().is_empty() {
                    out.push(json!({"type":"text","text":text}));
                }
            }
            Part::Image {
                media_type,
                data,
                url,
                ..
            } => {
                let source = if let Some(data) = data {
                    json!({"type":"base64","media_type":media_type,"data":data})
                } else if let Some(url) = url {
                    json!({"type":"url","url":url})
                } else {
                    return Err(invalid());
                };
                out.push(json!({"type":"image","source":source}));
            }
            Part::File {
                media_type,
                data,
                url,
                name,
                ..
            } => {
                if (media_type.eq_ignore_ascii_case("application/pdf") || native_anthropic)
                    && (data.is_some() || url.is_some())
                {
                    let source = if let Some(data) = data {
                        json!({"type":"base64","media_type":media_type,"data":data})
                    } else if let Some(url) = url {
                        json!({"type":"url","url":url})
                    } else {
                        return Err(invalid());
                    };
                    out.push(json!({"type":"document","source":source}));
                } else {
                    out.push(json!({
                        "type": "text",
                        "text": attachment_text(media_type, name.as_deref())
                    }));
                }
            }
            Part::ToolCall {
                id,
                name,
                arguments,
                ..
            } => out.push(
                json!({"type":"tool_use","id":anthropic_out_id(id),"name":name,"input":arguments}),
            ),
            Part::ToolResult {
                call_id,
                parts,
                is_error,
                ..
            } => out.push(json!({
                "type":"tool_result",
                "tool_use_id":anthropic_out_id(call_id),
                "content":anthropic_parts(parts, native_anthropic)?,
                "is_error":is_error
            })),
            Part::Thinking {
                text,
                signature,
                source,
                sealed,
            } => {
                if *source == Protocol::Anthropic {
                    if let Some(sealed) = sealed {
                        out.push(sealed.clone());
                        continue;
                    }
                    let mut block = json!({"type":"thinking","thinking":text});
                    if let Some(signature) = signature {
                        block["signature"] = json!(signature);
                    }
                    out.push(block);
                }
            }
            Part::ToolReference { name } => {
                out.push(json!({"type":"tool_reference","tool_name":name}));
            }
            Part::Search { query, hits, .. } => {
                out.push(json!({"type":"text","text":format!("{query}\n{}",json!(hits))}));
            }
            Part::NamespacedToolCall { .. }
            | Part::CustomToolCall { .. }
            | Part::CustomToolResult { .. }
            | Part::ToolSearchCall { .. }
            | Part::ToolSearchResult { .. } => return Err(invalid()),
        }
    }
    Ok(out)
}

fn blocks_calls_last(blocks: Vec<Value>) -> Vec<Value> {
    let mut head = Vec::new();
    let mut calls = Vec::new();
    let mut called = false;
    let mut moved = false;
    for block in &blocks {
        let kind = block["type"].as_str().unwrap_or_default();
        if kind == "tool_use" {
            called = true;
            calls.push(block.clone());
        } else if !called {
            head.push(block.clone());
        } else if matches!(kind, "thinking" | "redacted_thinking") {
            calls.push(block.clone());
        } else {
            moved = true;
            if kind == "text" && block["text"].as_str().is_some_and(|t| t.trim().is_empty()) {
                continue;
            }
            head.push(block.clone());
        }
    }
    if !moved {
        return blocks;
    }
    head.extend(calls);
    head
}

fn anthropic_result_said(block: &Value) -> Vec<Value> {
    match &block["content"] {
        Value::String(s) => {
            let text = if s.trim().is_empty() {
                "Tool result received."
            } else {
                s.as_str()
            };
            vec![json!({"type": "text", "text": text})]
        }
        Value::Array(blocks) if !blocks.is_empty() => blocks.clone(),
        _ => vec![json!({"type": "text", "text": "Tool result received."})],
    }
}

fn pair_anthropic_tool_blocks(msgs: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(msgs.len());
    let mut pending: Vec<String> = Vec::new();
    let mut open: HashSet<String> = HashSet::new();
    let mut answered: HashSet<String> = HashSet::new();

    let synthetic = |id: &str| {
        json!({
            "type": "tool_result",
            "tool_use_id": id,
            "content": INTERRUPTED_TOOL_MESSAGE,
            "is_error": true
        })
    };

    let close_exchange = |out: &mut Vec<Value>,
                          pending: &mut Vec<String>,
                          open: &mut HashSet<String>,
                          answered: &mut HashSet<String>| {
        let blocks: Vec<Value> = pending
            .iter()
            .filter(|id| !answered.contains(*id))
            .map(|id| synthetic(id))
            .collect();
        if !blocks.is_empty() {
            out.push(json!({"role": "user", "content": blocks}));
        }
        pending.clear();
        open.clear();
        answered.clear();
    };

    for mut m in msgs {
        if m["role"] == "assistant" {
            close_exchange(&mut out, &mut pending, &mut open, &mut answered);
            let blocks = m["content"].as_array().cloned().unwrap_or_default();
            for b in &blocks {
                if b["type"] == "tool_use" {
                    if let Some(id) = b["id"].as_str().filter(|id| !id.is_empty()) {
                        pending.push(id.to_owned());
                        open.insert(id.to_owned());
                    }
                }
            }
            m["content"] = Value::Array(blocks_calls_last(blocks));
            out.push(m);
            continue;
        }
        let blocks = m["content"].as_array().cloned().unwrap_or_default();
        let mut kept = Vec::new();
        let mut rest = Vec::new();
        for b in blocks {
            if b["type"] != "tool_result" {
                rest.push(b);
                continue;
            }
            let id = b["tool_use_id"].as_str().unwrap_or_default();
            if !open.contains(id) {
                rest.extend(anthropic_result_said(&b));
                continue;
            }
            if !answered.insert(id.to_owned()) {
                continue;
            }
            kept.push(b);
        }
        for id in &pending {
            if !answered.contains(id) {
                kept.push(synthetic(id));
            }
        }
        kept.extend(rest);
        out.push(json!({"role": "user", "content": kept}));
        pending.clear();
        open.clear();
        answered.clear();
    }
    close_exchange(&mut out, &mut pending, &mut open, &mut answered);
    out
}

fn anthropic(r: &Request) -> Result<Value, ProxyError> {
    let native_anthropic = r.source == Protocol::Anthropic;
    let mut system = anthropic_parts(&r.system, native_anthropic)?;
    let mut messages: Vec<Value> = Vec::new();
    for m in &r.messages {
        if matches!(m.role.as_str(), "system" | "developer") {
            system.extend(anthropic_parts(&m.parts, native_anthropic)?);
            continue;
        }
        let mut parts = m
            .parts
            .iter()
            .filter(|p| matches!(p, Part::ToolResult { .. }))
            .cloned()
            .collect::<Vec<_>>();
        parts.extend(
            m.parts
                .iter()
                .filter(|p| !matches!(p, Part::ToolResult { .. }))
                .cloned(),
        );
        let content = anthropic_parts(&parts, native_anthropic)?;
        if content.is_empty() {
            continue;
        }
        let role = if m.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        if let Some(last) = messages.last_mut().filter(|last| last["role"] == role) {
            if let Some(blocks) = last["content"].as_array_mut() {
                blocks.extend(content);
            }
        } else {
            messages.push(json!({"role":role,"content":content}));
        }
    }
    let mut messages = pair_anthropic_tool_blocks(messages);
    if !native_anthropic {
        let ephemeral = json!({"type": "ephemeral"});
        if let Some(last_sys) = system.last_mut() {
            last_sys["cache_control"] = ephemeral.clone();
        }
        if let Some(last_msg) = messages.last_mut() {
            if let Some(blocks) = last_msg["content"].as_array_mut() {
                for block in blocks.iter_mut().rev() {
                    if !matches!(
                        block["type"].as_str(),
                        Some("thinking" | "redacted_thinking")
                    ) {
                        block["cache_control"] = ephemeral;
                        break;
                    }
                }
            }
        }
    }
    let mut body = json!({"messages":messages,"max_tokens":r.max_tokens.unwrap_or(8192)});
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if !r.stop.is_empty() {
        body["stop_sequences"] = json!(r.stop);
    }
    let mut tools = Vec::new();
    for t in &r.tools {
        if t.kind != "function" {
            tools.push(native_tool(t, Protocol::Anthropic)?);
            continue;
        }
        let mut tool = json!({"name":t.name,"description":t.description,"input_schema":t.schema});
        if t.strict == Some(true) {
            tool["strict"] = json!(true);
        }
        if t.deferred {
            tool["defer_loading"] = json!(true);
        }
        tools.push(tool);
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if let Some(v) = choice(r, Protocol::Anthropic) {
        body["tool_choice"] = v;
    }
    if r.parallel == Some(false)
        && (body.get("tools").is_some() || r.native.responses_tool_allowlist.is_none())
    {
        if body.get("tool_choice").is_none() {
            body["tool_choice"] = json!({"type":"auto"});
        }
        body["tool_choice"]["disable_parallel_tool_use"] = json!(true);
    }
    anthropic_thinking(r, &mut body);
    if let Some(v) = &r.native.metadata {
        body["metadata"] = v.clone();
    } else if let Some(v) = &r.native.user {
        body["metadata"] = json!({"user_id":v});
    }
    if let Some(v) = &r.native.safeguards {
        body["safeguards"] = v.clone();
    }
    if let Some(f) = &r.format {
        if let Some(schema) = &f.schema {
            body["output_config"]["format"] = json!({"type":"json_schema","schema":schema});
        } else if f.kind == "json_object" {
            // Messages accepts a concrete schema, not OpenAI's unconstrained JSON-object mode.
            let instruction =
                json!({"type":"text","text":"Return the answer as a valid JSON object."});
            if let Some(system) = body.get_mut("system").and_then(Value::as_array_mut) {
                system.push(instruction);
            } else {
                body["system"] = json!([instruction]);
            }
        }
    }
    Ok(body)
}

fn gemini_parts(parts: &[Part]) -> Result<Vec<Value>, ProxyError> {
    let mut out = Vec::new();
    for p in parts {
        let mut block = match p {
            Part::Text { text, signature } => {
                let mut block = json!({"text":text});
                if let Some(signature) = signature {
                    block["thoughtSignature"] = json!(signature);
                }
                block
            }
            Part::Image {
                media_type,
                data,
                url,
                ..
            }
            | Part::File {
                media_type,
                data,
                url,
                ..
            } => {
                if let Some(data) = data {
                    json!({"inlineData":{"mimeType":media_type,"data":data}})
                } else if let Some(url) = url {
                    json!({"fileData":{"mimeType":media_type,"fileUri":url}})
                } else {
                    return Err(invalid());
                }
            }
            Part::ToolCall {
                id,
                name,
                arguments,
                signature,
            } => {
                let mut block = json!({"functionCall":{"id":id,"name":name,"args":arguments}});
                if let Some(signature) = signature {
                    block["thoughtSignature"] = json!(signature);
                }
                block
            }
            Part::ToolResult {
                call_id,
                name,
                parts,
                is_error,
            } => {
                let label = name.as_ref().ok_or_else(invalid)?;
                let mut response = json!({"output":text(parts)});
                if *is_error {
                    response = json!({"error":text(parts)});
                }
                let media = parts
                    .iter()
                    .filter(|p| matches!(p, Part::Image { .. } | Part::File { .. }))
                    .cloned()
                    .collect::<Vec<_>>();
                let mut function = json!({"id":call_id,"name":label,"response":response});
                if !media.is_empty() {
                    function["parts"] = json!(gemini_parts(&media)?);
                }
                json!({"functionResponse":function})
            }
            Part::Thinking {
                text,
                signature,
                source,
                ..
            } => {
                if text.is_empty() {
                    continue;
                }
                let mut block = json!({"text":text,"thought":true});
                if *source == Protocol::Gemini {
                    if let Some(signature) = signature {
                        block["thoughtSignature"] = json!(signature);
                    }
                }
                block
            }
            Part::Search {
                query,
                hits,
                gemini,
            } => gemini
                .clone()
                .unwrap_or_else(|| json!({"text":format!("{query}\n{}",json!(hits))})),
            Part::ToolReference { name } => json!({"text":format!("Tool available: {name}")}),
            Part::NamespacedToolCall { .. }
            | Part::CustomToolCall { .. }
            | Part::CustomToolResult { .. }
            | Part::ToolSearchCall { .. }
            | Part::ToolSearchResult { .. } => return Err(invalid()),
        };
        // Google's signatures are opaque; values are only carried, never synthesized.
        if block.is_null() {
            block = json!({"text":""});
        }
        out.push(block);
    }
    Ok(out)
}
fn gemini(r: &Request) -> Result<Value, ProxyError> {
    let mut system = gemini_parts(&r.system)?;
    let mut contents: Vec<Value> = Vec::new();
    let mut names = std::collections::HashMap::new();
    for m in &r.messages {
        for p in &m.parts {
            if let Part::ToolCall { id, name, .. } = p {
                names.insert(id.clone(), name.clone());
            }
        }
        if matches!(m.role.as_str(), "system" | "developer") {
            system.extend(gemini_parts(&m.parts)?);
            continue;
        }
        let parts = m
            .parts
            .iter()
            .cloned()
            .map(|mut p| {
                if let Part::ToolResult { call_id, name, .. } = &mut p {
                    if name.is_none() {
                        *name = names.get(call_id).cloned();
                    }
                }
                p
            })
            .collect::<Vec<_>>();
        let role = if m.role == "assistant" {
            "model"
        } else {
            "user"
        };
        let (results, ordinary): (Vec<_>, Vec<_>) = parts
            .into_iter()
            .partition(|p| matches!(p, Part::ToolResult { .. }));
        if !results.is_empty() {
            push_gemini_turn(&mut contents, "user", gemini_parts(&results)?, true);
        }
        if !ordinary.is_empty() {
            push_gemini_turn(&mut contents, role, gemini_parts(&ordinary)?, false);
        }
    }
    let mut body = json!({"contents":contents});
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts":system});
    }
    let mut config = json!({});
    for (key, value) in [
        ("temperature", r.temperature.clone()),
        ("topP", r.top_p.clone()),
        ("maxOutputTokens", r.max_tokens.map(Value::from)),
        ("topK", r.native.top_k.clone()),
        ("imageConfig", r.native.image_config.clone()),
        ("responseModalities", r.native.modalities.clone()),
    ] {
        if let Some(v) = value {
            config[key] = v;
        }
    }
    if !r.stop.is_empty() {
        config["stopSequences"] = json!(r.stop);
    }
    if let Some(f) = &r.format {
        if matches!(f.kind.as_str(), "json_object" | "json_schema") {
            config["responseMimeType"] = json!("application/json");
            if let Some(schema) = &f.schema {
                config["responseJsonSchema"] = schema.clone();
            }
        }
    }
    if let Some(thinking) = gemini_thinking(r) {
        config["thinkingConfig"] = thinking;
    }
    if config.as_object().is_some_and(|v| !v.is_empty()) {
        body["generationConfig"] = config;
    }
    let declarations = tool_functions(r)
        .map(|t| json!({"name":t.name,"description":t.description,"parametersJsonSchema":t.schema}))
        .collect::<Vec<_>>();
    let combined = !declarations.is_empty() && r.tools.iter().any(|t| t.kind != "function");
    let mut tools = Vec::new();
    if !declarations.is_empty() {
        tools.push(json!({"functionDeclarations":declarations}));
    }
    for t in r.tools.iter().filter(|t| t.kind != "function") {
        tools.push(native_tool(t, Protocol::Gemini)?);
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if combined {
        body["toolConfig"]["includeServerSideToolInvocations"] = json!(true);
    }
    let default_choice = ToolChoice::Auto;
    if let Some(c) = r
        .tool_choice
        .as_ref()
        .or(combined.then_some(&default_choice))
    {
        body["toolConfig"]["functionCallingConfig"] = match c {
            ToolChoice::Auto if combined => json!({"mode":"VALIDATED"}),
            ToolChoice::Auto => json!({"mode":"AUTO"}),
            ToolChoice::None => json!({"mode":"NONE"}),
            ToolChoice::Required => json!({"mode":"ANY"}),
            ToolChoice::Named(name) => json!({"mode":"ANY","allowedFunctionNames":[name]}),
            ToolChoice::Namespaced { .. } | ToolChoice::Custom { .. } | ToolChoice::ToolSearch => {
                return Err(invalid())
            }
        };
    }
    if let Some(v) = &r.native.safety_settings {
        body["safetySettings"] = v.clone();
    }
    Ok(body)
}

fn gemini_thinking(r: &Request) -> Option<Value> {
    let reasoning = &r.reasoning;
    if reasoning.effort.is_none()
        && reasoning.budget.is_none()
        && reasoning.enabled.is_none()
        && !reasoning.visible
        && reasoning.summary.is_none()
    {
        return None;
    }
    let model = r.model.rsplit('/').next().unwrap_or(&r.model);
    let generation = model
        .strip_prefix("gemini-")
        .and_then(|v| v.split(['.', '-']).next())
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    let off = reasoning.enabled == Some(false) || reasoning.effort.as_deref() == Some("none");
    let mut config = json!({});
    if generation >= 3 {
        let effort = reasoning.effort.as_deref().unwrap_or("high");
        let level = if off {
            if model.contains("image") {
                "HIGH"
            } else {
                "LOW"
            }
        } else {
            match effort {
                "minimal" | "low" => "LOW",
                "medium" if model.contains("flash") || generation > 3 => "MEDIUM",
                _ => "HIGH",
            }
        };
        config["thinkingLevel"] = json!(level);
    } else {
        let budget = if off {
            if model.contains("pro") {
                128
            } else {
                0
            }
        } else {
            reasoning
                .budget
                .unwrap_or_else(|| thinking_budget(reasoning.effort.as_deref().unwrap_or("medium")))
        };
        config["thinkingBudget"] = json!(budget);
    }
    if reasoning.visible || reasoning.summary.is_some() {
        config["includeThoughts"] = json!(true);
    }
    Some(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_deferred_client_tool_discovery_survives_protocol_conversion(
    ) -> Result<(), crate::proxy::ProxyError> {
        let body = json!({"model":"synthetic","messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":"synthetic-search","name":"ToolSearch","input":{"query":"lookup"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-search","content":[{"type":"tool_reference","tool_name":"lookup"}]}]}
        ],"tools":[{"name":"ToolSearch","description":"Find a tool","input_schema":{"type":"object"}}, {"name":"lookup","description":"Synthetic lookup","input_schema":{"type":"object"},"defer_loading":true}]});
        let parsed = super::super::parse::request(Protocol::Anthropic, &body, "/v1/messages")?;
        for target in [
            Protocol::Chat,
            Protocol::Responses,
            Protocol::Gemini,
            Protocol::Anthropic,
        ] {
            let wire = request(&parsed, target)?;
            let encoded = wire.to_string();
            assert!(encoded.contains("lookup"));
            assert!(encoded.contains("ToolSearch"));
            if target != Protocol::Anthropic {
                assert!(encoded.contains("Tool available: lookup"));
            }
            if target == Protocol::Responses {
                assert!(wire["tools"][1].get("defer_loading").is_none());
            }
        }
        Ok(())
    }
    #[test]
    fn chat_maps_initial_and_late_instruction_roles_without_losing_content(
    ) -> Result<(), crate::proxy::ProxyError> {
        let body = json!({
            "model":"synthetic", "instructions":"Synthetic root instruction",
            "input":[
                {"role":"developer","content":"Synthetic initial instruction"},
                {"role":"user","content":"Synthetic first question"},
                {"role":"system","content":"Synthetic initial context"},
                {"role":"assistant","content":"Synthetic first answer"},
                {"role":"developer","content":"Synthetic updated instruction"},
                {"role":"system","content":"Synthetic updated context"},
                {"role":"user","content":"Synthetic followup"}
            ]
        });
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let before = parsed.clone();
        let wire = request(&parsed, Protocol::Chat)?;
        assert_eq!(
            wire["messages"],
            json!([
                {"role":"system","content":"Synthetic root instruction\n\nSynthetic initial instruction\n\nSynthetic initial context"},
                {"role":"user","content":"Synthetic first question"},
                {"role":"assistant","content":"Synthetic first answer"},
                {"role":"user","content":"<system-reminder>\nSynthetic updated instruction\n</system-reminder>"},
                {"role":"user","content":"<system-reminder>\nSynthetic updated context\n</system-reminder>"},
                {"role":"user","content":"Synthetic followup"}
            ])
        );
        assert_eq!(
            parsed, before,
            "Chat conversion must not mutate the shared IR"
        );
        let native = request(&parsed, Protocol::Responses)?;
        assert_eq!(native["instructions"], "Synthetic root instruction");
        assert_eq!(native["input"][0]["role"], "developer");
        assert_eq!(native["input"][4]["role"], "developer");
        Ok(())
    }

    #[test]
    fn chat_keeps_instruction_updates_after_tool_turns_in_place(
    ) -> Result<(), crate::proxy::ProxyError> {
        let body = json!({"model":"synthetic", "input":[
            {"role":"developer","content":"Synthetic initial instruction"},
            {"role":"user","content":"Synthetic question"},
            {"type":"function_call","call_id":"synthetic-call","name":"lookup","arguments":"{}"},
            {"type":"function_call_output","call_id":"synthetic-call","output":"Synthetic tool result"},
            {"role":"developer","content":[
                {"type":"input_text","text":"Synthetic visual context"},
                {"type":"input_image","image_url":"https://synthetic.invalid/image.png"}
            ]},
            {"role":"user","content":"Synthetic followup"}
        ]});
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let wire = request(&parsed, Protocol::Chat)?;
        assert_eq!(
            wire["messages"][0]["content"],
            "Synthetic initial instruction"
        );
        assert_eq!(wire["messages"][2]["tool_calls"][0]["id"], "synthetic-call");
        assert_eq!(wire["messages"][3]["tool_call_id"], "synthetic-call");
        assert_eq!(wire["messages"][4]["role"], "user");
        assert_eq!(
            wire["messages"][4]["content"],
            json!([
                {"type":"text","text":"<system-reminder>\n"},
                {"type":"text","text":"Synthetic visual context"},
                {"type":"image_url","image_url":{"url":"https://synthetic.invalid/image.png"}},
                {"type":"text","text":"\n</system-reminder>"}
            ])
        );
        assert_eq!(wire["messages"][5]["content"], "Synthetic followup");
        Ok(())
    }

    #[test]
    fn responses_chat_keeps_parallel_tool_calls_and_reasoning_before_their_results(
    ) -> Result<(), ProxyError> {
        let body = json!({"model":"deepseek-reasoner", "input":[
            {"role":"user","content":"Synthetic question"},
            {"type":"function_call","call_id":"call-a","name":"lookup","arguments":"{}"},
            {"type":"function_call","call_id":"call-b","name":"lookup","arguments":"{}"},
            {"role":"assistant","content":"Synthetic progress"},
            {"type":"reasoning","summary":[{"type":"summary_text","text":"Synthetic thought"}]},
            {"type":"function_call_output","call_id":"call-a","output":"Synthetic first result"},
            {"type":"function_call_output","call_id":"call-b","output":"Synthetic second result"},
            {"role":"assistant","content":"Synthetic answer"}
        ]});
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let wire = request(&parsed, Protocol::Chat)?;
        assert_eq!(wire["messages"].as_array().map(Vec::len), Some(5));
        assert_eq!(wire["messages"][1]["content"], "Synthetic progress");
        assert_eq!(
            wire["messages"][1]["reasoning_content"],
            "Synthetic thought"
        );
        assert_eq!(wire["messages"][1]["tool_calls"][0]["id"], "call-a");
        assert_eq!(wire["messages"][1]["tool_calls"][1]["id"], "call-b");
        assert_eq!(wire["messages"][2]["tool_call_id"], "call-a");
        assert_eq!(wire["messages"][3]["tool_call_id"], "call-b");
        assert_eq!(wire["messages"][4]["content"], "Synthetic answer");
        Ok(())
    }

    #[test]
    fn chat_single_text_uses_plain_content_and_keeps_multi_part_content() {
        for (parts, plain) in [
            (json!([{"type":"input_text","text":"Synthetic text"}]), true),
            (
                json!([{"type":"input_text","text":"Synthetic first"},{"type":"input_text","text":"Synthetic second"}]),
                false,
            ),
            (
                json!([{"type":"input_text","text":"Synthetic caption"},{"type":"input_image","image_url":"data:image/png;base64,c3ludGhldGlj"}]),
                false,
            ),
        ] {
            let parsed = super::super::parse::request(
                Protocol::Responses,
                &json!({
                    "model":"synthetic", "input":[{"role":"user","content":parts}]
                }),
                "/v1/responses",
            )
            .unwrap();
            let body = request(&parsed, Protocol::Chat).unwrap();
            assert_eq!(body["messages"][0]["content"].is_string(), plain);
            if !plain {
                assert!(body["messages"][0]["content"].is_array());
            }
        }
    }
    #[test]
    fn anthropic_omits_disabled_strict_and_preserves_requested_enforcement() {
        for strict in [false, true] {
            let body = json!({"model":"synthetic","input":"Synthetic request","tools":[{
                "type":"function","name":"read_file","strict":strict,"parameters":{
                    "type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false
                }
            }]});
            let parsed =
                super::super::parse::request(Protocol::Responses, &body, "/v1/responses").unwrap();
            let wire = request(&parsed, Protocol::Anthropic).unwrap();
            assert_eq!(
                wire["tools"][0]["input_schema"],
                body["tools"][0]["parameters"]
            );
            assert_eq!(
                wire["tools"][0].get("strict"),
                strict.then_some(&Value::Bool(true))
            );
        }
    }
    #[test]
    fn anthropic_json_object_format_remains_an_explicit_instruction() {
        let input = json!({"model":"claude-sonnet-4-5","messages":[{"role":"system","content":"Synthetic existing system"},{"role":"user","content":"Synthetic request"}],"response_format":{"type":"json_object"}});
        let parsed =
            super::super::parse::request(Protocol::Chat, &input, "/v1/chat/completions").unwrap();
        let wire = request(&parsed, Protocol::Anthropic).unwrap();
        assert_eq!(wire["system"][0]["text"], "Synthetic existing system");
        assert!(wire["system"]
            .as_array()
            .unwrap()
            .iter()
            .any(|part| part["text"]
                .as_str()
                .is_some_and(|text| text.contains("JSON object"))));
        assert!(wire.pointer("/output_config/format").is_none());
    }
    #[test]
    fn namespace_and_custom_tool_choices_follow_the_call_name_mapping() {
        for choice in [
            json!({"type":"function","namespace":"ns","name":"lookup"}),
            json!({"type":"custom","name":"patch"}),
        ] {
            let original = json!({"model":"synthetic","input":"Synthetic","tools":[
                {"type":"namespace","name":"ns","tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}]},
                {"type":"custom","name":"patch","format":{"type":"text"}}
            ],"tool_choice":choice});
            let parsed =
                super::super::parse::request(Protocol::Responses, &original, "/v1/responses")
                    .unwrap();
            assert_eq!(
                request(&parsed, Protocol::Responses).unwrap()["tool_choice"],
                choice
            );
            for target in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
                let wire = request(&parsed, target).unwrap();
                let wanted = if choice["type"] == "custom" {
                    "patch"
                } else {
                    "ns__lookup"
                };
                match target {
                    Protocol::Chat => assert_eq!(wire["tool_choice"]["function"]["name"], wanted),
                    Protocol::Anthropic => assert_eq!(wire["tool_choice"]["name"], wanted),
                    Protocol::Gemini => assert_eq!(
                        wire["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
                        wanted
                    ),
                    _ => unreachable!(),
                }
            }
        }
    }
    #[test]
    fn anthropic_tool_results_use_user_role_and_precede_mixed_text() {
        let parsed=super::super::parse::request(Protocol::Chat,&json!({"model":"synthetic","messages":[
            {"role":"assistant","tool_calls":[{"type":"function","id":"call-a","function":{"name":"lookup","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"call-a","content":"Synthetic output"},
            {"role":"user","content":"Synthetic followup"}
        ]}),"/v1/chat/completions").unwrap();
        let wire = request(&parsed, Protocol::Anthropic).unwrap();
        assert_eq!(wire["messages"][1]["role"], "user");
        assert_eq!(wire["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(
            wire["messages"][1]["content"][1]["text"],
            "Synthetic followup"
        );
    }
    #[test]
    fn gemini_mixed_function_results_and_text_are_separate_user_turns() {
        let parsed=super::super::parse::request(Protocol::Anthropic,&json!({"model":"gemini-3.8-flash","messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":"call-a","name":"lookup","input":{}}]},
            {"role":"user","content":[{"type":"text","text":"Synthetic followup"},{"type":"tool_result","tool_use_id":"call-a","content":"Synthetic output"}]}
        ]}),"/v1/messages").unwrap();
        let wire = request(&parsed, Protocol::Gemini).unwrap();
        assert_eq!(
            wire["contents"][1]["parts"][0]["functionResponse"]["name"],
            "lookup"
        );
        assert_eq!(wire["contents"][1]["parts"].as_array().unwrap().len(), 1);
        assert_eq!(
            wire["contents"][2]["parts"][0]["text"],
            "Synthetic followup"
        );
    }
    #[test]
    fn translated_chat_tools_follow_magpie_without_strict_flags() {
        for strict in [Some(true), Some(false), None] {
            let mut tool = json!({"type":"function","name":"lookup","description":"Synthetic lookup",
                "parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}});
            if let Some(value) = strict {
                tool["strict"] = json!(value);
            }
            let body = json!({"model":"gpt-6.1-sol","input":"Synthetic prompt","tools":[tool],"tool_choice":"required"});
            let parsed =
                super::super::parse::request(Protocol::Responses, &body, "/v1/responses").unwrap();
            let chat = request(&parsed, Protocol::Chat).unwrap();
            assert!(chat["tools"][0]["function"].get("strict").is_none());
            assert_eq!(
                chat["tools"][0]["function"]["parameters"],
                body["tools"][0]["parameters"]
            );
            assert_eq!(
                chat["tools"][0]["function"]["description"],
                "Synthetic lookup"
            );
            assert_eq!(chat["tool_choice"], "required");
            assert_eq!(
                request(&parsed, Protocol::Responses).unwrap()["tools"],
                body["tools"]
            );
        }
    }

    #[test]
    fn ordinary_chat_models_do_not_imply_native_search_support() {
        for model in ["gpt-6.1-sol", "claude-sonnet-5-5", "synthetic-model"] {
            for choice in ["auto", "none", "required"] {
                let body = json!({"model":model,"input":"Synthetic prompt","stream":true,
                    "tools":[{"type":"web_search"},{"type":"function","name":"lookup","description":"Synthetic lookup",
                        "parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]},"strict":false}],
                    "tool_choice":choice,"parallel_tool_calls":false,"reasoning":{"effort":"high"}});
                let parsed =
                    super::super::parse::request(Protocol::Responses, &body, "/v1/responses")
                        .unwrap();
                let chat = request(&parsed, Protocol::Chat).unwrap();
                assert!(chat.get("web_search_options").is_none());
                assert_eq!(chat["tools"][0]["function"]["name"], "lookup");
                assert_eq!(
                    chat["tools"][0]["function"]["description"],
                    "Synthetic lookup"
                );
                assert_eq!(
                    chat["tools"][0]["function"]["parameters"],
                    body["tools"][1]["parameters"]
                );
                assert!(chat["tools"][0]["function"].get("strict").is_none());
                assert_eq!(chat["tool_choice"], choice);
                assert_eq!(chat["parallel_tool_calls"], false);
                assert_eq!(chat["reasoning_effort"], "high");
                assert_eq!(chat["messages"][0]["content"], "Synthetic prompt");
                assert_eq!(
                    request(&parsed, Protocol::Responses).unwrap()["tools"],
                    body["tools"]
                );
            }
        }
    }

    #[test]
    fn grok_chat_omits_optional_native_search_but_keeps_function_tools() {
        for model in ["grok-4.5", "grok-4.7", "xai/GROK-4.6"] {
            for required in [false, true] {
                let body = json!({"model":model,"input":"Synthetic prompt","stream":true,
                    "tools":[{"type":"web_search"},{"type":"function","name":"lookup",
                        "parameters":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]},"strict":true}],
                    "tool_choice":if required {"required"} else {"auto"},"parallel_tool_calls":true,
                    "reasoning":{"effort":"high"}});
                let parsed =
                    super::super::parse::request(Protocol::Responses, &body, "/v1/responses")
                        .unwrap();
                let chat = request(&parsed, Protocol::Chat).unwrap();
                assert!(chat.get("web_search_options").is_none());
                assert_eq!(chat["tools"].as_array().unwrap().len(), 1);
                assert_eq!(chat["tools"][0]["function"]["name"], "lookup");
                assert_eq!(
                    chat["tools"][0]["function"]["parameters"],
                    body["tools"][1]["parameters"]
                );
                assert!(chat["tools"][0]["function"].get("strict").is_none());
                assert_eq!(chat["messages"][0]["content"], "Synthetic prompt");
                assert_eq!(chat["tool_choice"], body["tool_choice"]);
                assert_eq!(chat["parallel_tool_calls"], true);
                assert_eq!(chat["reasoning_effort"], "high");
                assert_eq!(chat["stream"], true);
                // Native search still belongs to Responses, not the retired Chat API.
                assert_eq!(
                    request(&parsed, Protocol::Responses).unwrap()["tools"][0],
                    body["tools"][0]
                );
                let mut other = parsed;
                other.model = "synthetic-model".into();
                assert!(request(&other, Protocol::Chat)
                    .unwrap()
                    .get("web_search_options")
                    .is_none());
            }
        }
    }

    #[test]
    fn chat_does_not_silently_ignore_required_search_or_leave_dangling_choices() {
        for (model, choice) in ["grok-4.5", "gpt-6.1-sol", "synthetic-model"]
            .into_iter()
            .flat_map(|model| ["auto", "none", "required"].map(|choice| (model, choice)))
        {
            let body = json!({"model":model,"input":"Synthetic prompt",
                "tools":[{"type":"web_search"}],"tool_choice":choice,"parallel_tool_calls":true});
            let parsed =
                super::super::parse::request(Protocol::Responses, &body, "/v1/responses").unwrap();
            let result = request(&parsed, Protocol::Chat);
            if choice == "required" {
                assert!(result.is_err());
            } else {
                let chat = result.unwrap();
                for key in [
                    "web_search_options",
                    "tools",
                    "tool_choice",
                    "parallel_tool_calls",
                ] {
                    assert!(chat.get(key).is_none(), "unexpected {key}");
                }
            }
        }
    }

    #[test]
    fn web_search_declarations_translate_to_eligible_native_tool_spellings() {
        for (source, body) in [
            (
                Protocol::Responses,
                json!({"model":"synthetic","input":[],"tools":[{"type":"web_search","external_web_access":false}]}),
            ),
            (
                Protocol::Anthropic,
                json!({"model":"synthetic","messages":[],"tools":[{"type":"web_search_20250305","name":"web_search","max_uses":3}]}),
            ),
            (
                Protocol::Gemini,
                json!({"model":"synthetic","contents":[],"tools":[{"googleSearch":{}}]}),
            ),
        ] {
            let parsed = super::super::parse::request(source, &body, "/v1/messages").unwrap();
            assert_eq!(
                request(&parsed, Protocol::Responses).unwrap()["tools"][0]["type"],
                "web_search"
            );
            assert!(
                request(&parsed, Protocol::Anthropic).unwrap()["tools"][0]["type"]
                    .as_str()
                    .unwrap()
                    .starts_with("web_search_")
            );
            assert!(request(&parsed, Protocol::Gemini).unwrap()["tools"][0]
                .get("googleSearch")
                .is_some());
            assert!(request(&parsed, Protocol::Chat)
                .unwrap()
                .get("web_search_options")
                .is_none());
        }
    }
    #[test]
    fn gemini_combined_search_and_functions_enable_context_circulation() {
        let function = json!({"type":"function","name":"synthetic_echo","parameters":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"]}});
        let search = json!({"type":"web_search"});
        for (choice, mode) in [
            (None, "VALIDATED"),
            (Some(json!("auto")), "VALIDATED"),
            (Some(json!("none")), "NONE"),
            (Some(json!("required")), "ANY"),
            (
                Some(json!({"type":"function","name":"synthetic_echo"})),
                "ANY",
            ),
        ] {
            let mut input = json!({"model":"gemini-3.6-flash","input":"Synthetic request","tools":[function,search]});
            if let Some(choice) = choice {
                input["tool_choice"] = choice;
            }
            let parsed =
                super::super::parse::request(Protocol::Responses, &input, "/v1/responses").unwrap();
            let body = request(&parsed, Protocol::Gemini).unwrap();
            assert_eq!(body["toolConfig"]["includeServerSideToolInvocations"], true);
            assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], mode);
            assert_eq!(
                body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"],
                function["parameters"]
            );
            assert!(body["tools"][1].get("googleSearch").is_some());
            if input["tool_choice"].is_object() {
                assert_eq!(
                    body["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"],
                    json!(["synthetic_echo"])
                );
            }
        }
        for tools in [json!([]), json!([function]), json!([search])] {
            let input = json!({"model":"gemini-3.6-flash","input":"Synthetic request","tools":tools,"tool_choice":"auto"});
            let parsed =
                super::super::parse::request(Protocol::Responses, &input, "/v1/responses").unwrap();
            let body = request(&parsed, Protocol::Gemini).unwrap();
            assert!(body["toolConfig"]
                .get("includeServerSideToolInvocations")
                .is_none());
            assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "AUTO");
        }
    }

    #[test]
    fn unknown_native_tools_are_rejected_instead_of_leaking_other_api_objects() {
        let parsed = super::super::parse::request(
            Protocol::Gemini,
            &json!({"model":"synthetic","contents":[],"tools":[{"codeExecution":{}}]}),
            "/v1beta/models/synthetic:generateContent",
        )
        .unwrap();
        assert!(request(&parsed, Protocol::Responses).is_err());
        assert!(request(&parsed, Protocol::Anthropic).is_err());
    }
    #[test]
    fn gemini_budget_levels_match_the_pinned_reference() {
        for (effort, budget) in [
            ("low", 4096),
            ("medium", 10000),
            ("high", 24000),
            ("xhigh", 32000),
            ("max", 32000),
        ] {
            let parsed = super::super::parse::request(
                Protocol::Chat,
                &json!({"model":"gemini-2.5-flash","messages":[],"reasoning_effort":effort}),
                "/v1/chat/completions",
            )
            .unwrap();
            assert_eq!(
                request(&parsed, Protocol::Gemini).unwrap()["generationConfig"]["thinkingConfig"]
                    ["thinkingBudget"],
                budget
            );
        }
    }
    #[test]
    fn gemini_future_flash_models_do_not_receive_unsupported_minimal_level() {
        for model in ["gemini-3.7-flash", "gemini-3.8-flash", "gemini-4-argon"] {
            for effort in ["none", "minimal"] {
                let parsed = super::super::parse::request(
                    Protocol::Chat,
                    &json!({"model":model,"messages":[],"reasoning_effort":effort}),
                    "/v1/chat/completions",
                )
                .unwrap();
                assert_eq!(
                    request(&parsed, Protocol::Gemini).unwrap()["generationConfig"]
                        ["thinkingConfig"]["thinkingLevel"],
                    "LOW"
                );
            }
        }
    }
    #[test]
    fn anthropic_nonadaptive_thinking_fits_the_output_cap() {
        let parsed=super::super::parse::request(Protocol::Chat,&json!({"model":"claude-sonnet-4-5","messages":[],"max_tokens":8192,"reasoning_effort":"high"}),"/v1/chat/completions").unwrap();
        let wire = request(&parsed, Protocol::Anthropic).unwrap();
        assert_eq!(wire["thinking"]["type"], "enabled");
        assert_eq!(wire["thinking"]["budget_tokens"], 7168);
        assert_eq!(wire["max_tokens"], 8192);
    }
    #[test]
    fn anthropic_adaptive_models_keep_visible_thinking_and_fit_xhigh() {
        let parsed = super::super::parse::request(
            Protocol::Chat,
            &json!({"model":"claude-opus-4-6","messages":[],"reasoning_effort":"xhigh"}),
            "/v1/chat/completions",
        )
        .unwrap();
        let wire = request(&parsed, Protocol::Anthropic).unwrap();
        assert_eq!(
            wire["thinking"],
            json!({"type":"adaptive","display":"summarized"})
        );
        assert_eq!(wire["output_config"]["effort"], "max");
    }
    #[test]
    fn stop_sequences_and_request_metadata_roundtrip_without_private_diagnostics() {
        let body = json!({"model":"synthetic","input":[],"stop":["Synthetic stop"],"metadata":{"synthetic":"value"},"user":"synthetic-user","prompt_cache_key":"synthetic-cache"});
        let parsed =
            super::super::parse::request(Protocol::Responses, &body, "/v1/responses").unwrap();
        let wire = request(&parsed, Protocol::Responses).unwrap();
        assert_eq!(wire["stop"], body["stop"]);
        assert_eq!(wire["metadata"], body["metadata"]);
        assert_eq!(wire["user"], body["user"]);
    }
    #[test]
    fn signed_empty_anthropic_thinking_keeps_required_text_and_signature_fields() {
        let body = json!({"model":"synthetic","messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"","signature":"synthetic-signature"}]}]});
        let parsed =
            super::super::parse::request(Protocol::Anthropic, &body, "/v1/messages").unwrap();
        let wire = request(&parsed, Protocol::Anthropic).unwrap();
        assert_eq!(
            wire["messages"][0]["content"][0],
            body["messages"][0]["content"][0]
        );
    }
    #[test]
    fn tool_result_images_survive_all_target_builders() {
        let body = json!({"model":"synthetic","messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":"call-a","name":"lookup","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"call-a","content":[{"type":"text","text":"Synthetic"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"c3ludGhldGlj"}}]}]}
        ]});
        let parsed =
            super::super::parse::request(Protocol::Anthropic, &body, "/v1/messages").unwrap();
        for target in [
            Protocol::Chat,
            Protocol::Responses,
            Protocol::Anthropic,
            Protocol::Gemini,
        ] {
            let built = request(&parsed, target).unwrap();
            assert!(built.to_string().contains("c3ludGhldGlj"));
            if target == Protocol::Chat {
                let messages = built["messages"].as_array().unwrap();
                assert_eq!(messages[1]["role"], "tool");
                let tool_text = messages[1]["content"].as_str().unwrap();
                assert!(tool_text.contains("Synthetic"));
                assert!(
                    tool_text.contains(crate::proxy::tool_media::TOOL_RESULT_MEDIA_MOVED_MARKER)
                );
                assert_eq!(messages[2]["role"], "user");
                let trailing = messages[2].to_string();
                assert!(trailing.contains("[cc-switch: media output of tool call call-a]"));
                assert!(trailing.contains("c3ludGhldGlj"));
            }
        }
    }

    #[test]
    fn chat_builder_moves_tool_result_media_to_trailing_user_message_preserving_tool_contiguity() {
        let responses_body = json!({
            "model": "synthetic",
            "input": [
                {"type":"function_call","call_id":"call-1","name":"browser_cmd","arguments":"{}"},
                {"type":"function_call","call_id":"call-2","name":"screenshot","arguments":"{}"},
                {"type":"function_call","call_id":"call-3","name":"ping","arguments":"{}"},
                {"type":"function_call_output","call_id":"call-1","output":[
                    {"type":"input_text","text":"Page summary"},
                    {"type":"input_image","image_url":"data:image/png;base64,img1"}
                ]},
                {"type":"function_call_output","call_id":"call-2","output":[
                    {"type":"input_image","image_url":"data:image/png;base64,img2"}
                ]},
                {"type":"function_call_output","call_id":"call-3","output":"plain text"},
                {"type":"message","role":"user","content":[{"type":"input_text","text":"Continue"}]}
            ]
        });
        let parsed =
            super::super::parse::request(Protocol::Responses, &responses_body, "/v1/responses")
                .unwrap();
        let built = request(&parsed, Protocol::Chat).unwrap();
        let messages = built["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[0]["role"], "assistant");
        let calls = messages[0]["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 3);
        for (call, id) in calls.iter().zip(["call-1", "call-2", "call-3"]) {
            assert_eq!(call["id"], id);
        }
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call-1");
        let c1 = messages[1]["content"].as_str().unwrap();
        assert!(c1.contains("Page summary"));
        assert!(c1.contains(crate::proxy::tool_media::TOOL_RESULT_MEDIA_MOVED_MARKER));
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call-2");
        assert_eq!(
            messages[2]["content"].as_str(),
            Some(crate::proxy::tool_media::TOOL_RESULT_MEDIA_MOVED_MARKER)
        );
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "call-3");
        assert_eq!(messages[3]["content"].as_str(), Some("plain text"));
        assert_eq!(messages[4]["role"], "user");
        let media_msg = messages[4].to_string();
        assert!(media_msg.contains("[cc-switch: media output of tool call call-1]"));
        assert!(media_msg.contains("data:image/png;base64,img1"));
        assert!(media_msg.contains("[cc-switch: media output of tool call call-2]"));
        assert!(media_msg.contains("data:image/png;base64,img2"));
        assert_eq!(messages[5]["role"], "user");
        assert!(messages[5].to_string().contains("Continue"));

        // Mixed Anthropic user message: [tool_result(image), text] flushes media before text user msg.
        let anthropic_mixed = json!({
            "model": "synthetic",
            "messages": [
                {"role":"assistant","content":[{"type":"tool_use","id":"call-mix","name":"shot","input":{}}]},
                {"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"call-mix","content":[
                        {"type":"image","source":{"type":"base64","media_type":"image/png","data":"bWl4"}}
                    ]},
                    {"type":"text","text":"Follow-up note"}
                ]}
            ]
        });
        let parsed_mix =
            super::super::parse::request(Protocol::Anthropic, &anthropic_mixed, "/v1/messages")
                .unwrap();
        let built_mix = request(&parsed_mix, Protocol::Chat).unwrap();
        let mix_msgs = built_mix["messages"].as_array().unwrap();
        assert_eq!(mix_msgs[1]["role"], "tool");
        assert_eq!(
            mix_msgs[1]["content"].as_str(),
            Some(crate::proxy::tool_media::TOOL_RESULT_MEDIA_MOVED_MARKER)
        );
        assert_eq!(mix_msgs[2]["role"], "user");
        assert!(mix_msgs[2].to_string().contains("bWl4"));
        assert_eq!(mix_msgs[3]["role"], "user");
        assert!(mix_msgs[3].to_string().contains("Follow-up note"));

        // File inside tool result is also moved out of role:tool string content.
        let file_body = json!({
            "model": "synthetic",
            "input": [
                {"type":"function_call","call_id":"call-f","name":"read_pdf","arguments":"{}"},
                {"type":"function_call_output","call_id":"call-f","output":[
                    {"type":"input_file","filename":"doc.pdf","file_data":"data:application/pdf;base64,cGRm"}
                ]}
            ]
        });
        let parsed_file =
            super::super::parse::request(Protocol::Responses, &file_body, "/v1/responses").unwrap();
        let built_file = request(&parsed_file, Protocol::Chat).unwrap();
        let file_msgs = built_file["messages"].as_array().unwrap();
        assert_eq!(file_msgs[1]["role"], "tool");
        assert_eq!(
            file_msgs[1]["content"].as_str(),
            Some(crate::proxy::tool_media::TOOL_RESULT_MEDIA_MOVED_MARKER)
        );
        assert_eq!(file_msgs[2]["role"], "user");
        assert!(file_msgs[2].to_string().contains("cGRm"));
    }
    #[test]
    fn codex_tool_search_preserves_the_client_discovery_contract() -> Result<(), ProxyError> {
        let declaration = json!({
            "type":"tool_search", "execution":"client",
            "description":"Search the synthetic records and calendar namespaces when needed.",
            "parameters":{"type":"object","properties":{
                "query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":5}
            },"required":["query","limit"],"additionalProperties":false}
        });
        let source = json!({"model":"synthetic","input":"Hello","tools":[declaration]});
        let parsed = super::super::parse::request(Protocol::Responses, &source, "/v1/responses")?;
        for target in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
            let built = request(&parsed, target)?;
            let portable = super::super::parse::request(
                target,
                &built,
                "/v1beta/models/synthetic:generateContent",
            )?;
            let search = portable.tools.first().ok_or_else(invalid)?;
            assert_eq!(search.name, "tool_search");
            assert_eq!(search.description, declaration["description"]);
            assert_eq!(search.schema, declaration["parameters"]);
        }
        Ok(())
    }

    #[test]
    fn responses_client_tool_search_supports_initial_and_loaded_tool_turns() {
        let declaration = json!({"type":"tool_search","execution":"client"});
        let call = json!({"type":"tool_search_call","id":"search-item","call_id":"search-call","execution":"client","status":"completed","arguments":{"query":"synthetic lookup","limit":2}});
        let result = json!({"type":"tool_search_output","call_id":"search-call","execution":"client","status":"completed","tools":[{"type":"namespace","name":"synthetic_ns","tools":[{"type":"function","name":"lookup","description":"Synthetic lookup","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}]}]});
        for history in [false, true] {
            let mut source = json!({"model":"synthetic","tools":[declaration],"tool_choice":{"type":"tool_search"},"input":[{"role":"user","content":"Synthetic request"}]});
            if history {
                source["input"]
                    .as_array_mut()
                    .unwrap()
                    .extend([call.clone(), result.clone()]);
            }
            let parsed =
                super::super::parse::request(Protocol::Responses, &source, "/v1/responses")
                    .unwrap();
            let native = request(&parsed, Protocol::Responses).unwrap();
            assert_eq!(native["tools"][0], declaration);
            assert_eq!(native["tool_choice"], source["tool_choice"]);
            if history {
                assert_eq!(native["input"][1], call);
                assert_eq!(native["input"][2], result);
            }
            for target in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
                let built = request(&parsed, target).unwrap();
                let portable = super::super::parse::request(
                    target,
                    &built,
                    "/v1beta/models/synthetic:generateContent",
                )
                .unwrap();
                let search = portable
                    .tools
                    .iter()
                    .find(|tool| tool.name == "tool_search")
                    .unwrap();
                assert_eq!(search.schema["properties"]["query"]["type"], "string");
                assert_eq!(search.schema["required"], json!(["query"]));
                assert_eq!(
                    portable.tool_choice,
                    Some(ToolChoice::Named("tool_search".into()))
                );
                if history {
                    assert!(portable
                        .tools
                        .iter()
                        .any(|tool| tool.name == "synthetic_ns__lookup"));
                    assert!(portable.messages.iter().flat_map(|message| &message.parts).any(|part| matches!(part, Part::ToolCall { id, name, arguments, .. } if id=="search-call" && name=="tool_search" && arguments==&call["arguments"])));
                    assert!(portable.messages.iter().flat_map(|message| &message.parts).any(|part| matches!(part, Part::ToolResult { call_id, parts, .. } if call_id=="search-call" && text(parts).contains("synthetic_ns"))));
                }
            }
        }
    }

    #[test]
    fn server_tool_search_cannot_be_relabelled_as_a_client_function() {
        let source = json!({"model":"synthetic","input":"Synthetic request","tools":[{"type":"tool_search","execution":"server"}]});
        let parsed =
            super::super::parse::request(Protocol::Responses, &source, "/v1/responses").unwrap();
        assert_eq!(
            request(&parsed, Protocol::Responses).unwrap()["tools"],
            source["tools"]
        );
        for target in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
            assert!(request(&parsed, target).is_err());
        }
    }

    #[test]
    fn namespaced_and_custom_tools_keep_native_identity_and_portable_wrappers() {
        let raw = "*** Begin Synthetic Patch\n+  preserve spacing\n*** End Synthetic Patch\n";
        let source = json!({"model":"synthetic","input":[
            {"type":"function_call","call_id":"lookup-call","namespace":"synthetic_ns","name":"lookup","arguments":"{\"agent\":\"synthetic\"}"},
            {"type":"function_call_output","call_id":"lookup-call","output":"Synthetic result"},
            {"type":"custom_tool_call","call_id":"patch-call","name":"apply_patch","input":raw},
            {"type":"custom_tool_call_output","call_id":"patch-call","output":"Synthetic patch applied"}
        ],"tools":[
            {"type":"namespace","name":"synthetic_ns","tools":[{"type":"function","name":"lookup","strict":false,"parameters":{"type":"object","properties":{"agent":{"type":"string"}}}}]},
            {"type":"custom","name":"apply_patch","description":"Apply a synthetic patch","format":{"type":"text"}}
        ]});
        let parsed =
            super::super::parse::request(Protocol::Responses, &source, "/v1/responses").unwrap();
        assert!(
            matches!(&parsed.messages[0].parts[0],Part::NamespacedToolCall{namespace,name,..} if namespace=="synthetic_ns"&&name=="lookup")
        );
        assert!(
            matches!(&parsed.messages[2].parts[0],Part::CustomToolCall{input,..} if input==raw)
        );
        let native = request(&parsed, Protocol::Responses).unwrap();
        assert_eq!(native["input"][0]["namespace"], "synthetic_ns");
        assert_eq!(native["input"][0]["name"], "lookup");
        assert_eq!(native["input"][2]["type"], "custom_tool_call");
        assert_eq!(native["input"][2]["input"], raw);
        assert_eq!(native["input"][3]["type"], "custom_tool_call_output");
        assert_eq!(native["tools"], source["tools"]);
        for target in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
            let portable = portable_request(&parsed).unwrap();
            assert_eq!(portable.tools[0].name, "synthetic_ns__lookup");
            assert_eq!(portable.tools[0].strict, Some(false));
            assert_eq!(
                portable.tools[1].schema["properties"]["input"]["type"],
                "string"
            );
            assert!(
                matches!(&portable.messages[0].parts[0],Part::ToolCall{name,..} if name=="synthetic_ns__lookup")
            );
            assert!(
                matches!(&portable.messages[2].parts[0],Part::ToolCall{arguments,..} if arguments["input"]==raw)
            );
            let built = request(&parsed, target).unwrap();
            assert!(built.to_string().contains("synthetic_ns__lookup"));
        }
    }
    #[test]
    fn full_chat_request_builds_each_protocol_without_a_wire_intermediate() {
        let body = json!({"model":"gemini-3.6-flash","reasoning_effort":"high","messages":[{"role":"system","content":"Synthetic system"},{"role":"user","content":"Synthetic user","agent":"synthetic"},{"role":"assistant","content":null,"reasoning_content":"Synthetic thought","tool_calls":[{"id":"call-a","type":"function","function":{"name":"lookup","arguments":"{\"agent\":\"synthetic\"}"}}]},{"role":"tool","tool_call_id":"call-a","content":"Synthetic result"}],"tools":[{"type":"function","function":{"name":"lookup","strict":true,"parameters":{"type":"object","properties":{"agent":{"type":"string"}}}}}],"parallel_tool_calls":false,"response_format":{"type":"json_schema","json_schema":{"name":"answer","schema":{"type":"object"},"strict":true}},"max_completion_tokens":128});
        let ir =
            super::super::parse::request(Protocol::Chat, &body, "/v1/chat/completions").unwrap();
        for protocol in [
            Protocol::Chat,
            Protocol::Responses,
            Protocol::Anthropic,
            Protocol::Gemini,
        ] {
            let wire = request(&ir, protocol).unwrap();
            assert!(wire.is_object());
            assert!(!wire.to_string().contains("conversationRequestId"));
            if protocol == Protocol::Gemini {
                assert_eq!(
                    wire["generationConfig"]["thinkingConfig"]["thinkingLevel"],
                    "HIGH"
                );
                assert_eq!(
                    wire["contents"][2]["parts"][0]["functionResponse"]["name"],
                    "lookup"
                );
                assert_eq!(
                    wire["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]
                        ["properties"]["agent"]["type"],
                    "string"
                );
            }
        }
    }
    #[test]
    fn chat_translation_omits_reasoning_summary() {
        let body = json!({
            "model": "gpt-6.2-sol",
            "input": [{"role": "user", "content": "hi"}],
            "reasoning": {"effort": "high", "summary": "auto"},
            "stream": true
        });
        let ir = super::super::parse::request(Protocol::Responses, &body, "/v1/responses").unwrap();
        let chat = request(&ir, Protocol::Chat).unwrap();
        assert_eq!(chat["reasoning_effort"], "high");
        assert!(chat.get("reasoning_summary").is_none());
    }
    #[test]
    fn responses_chat_merges_reasoning_with_assistant_text_and_omits_reasoning_for_gpt(
    ) -> Result<(), ProxyError> {
        let body = json!({
            "model": "gpt-6.2-sol",
            "input": [
                {"role": "user", "content": "First question"},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Hidden thought"}]},
                {"role": "assistant", "content": "First answer"},
                {"role": "user", "content": "Second question"},
                {"type": "reasoning", "summary": [], "encrypted_content": "synthetic-seal"},
                {"type": "reasoning", "summary": []},
                {"role": "assistant", "content": [
                    {"type": "output_text", "text": "Part one"},
                    {"type": "output_text", "text": "Part two"}
                ]},
                {"role": "user", "content": "Third question"},
                {"type": "reasoning", "summary": [], "encrypted_content": "synthetic-seal-alone"}
            ]
        });
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let wire = request(&parsed, Protocol::Chat)?;
        let messages = wire["messages"].as_array().expect("messages array");
        assert_eq!(messages.len(), 5);
        for message in messages {
            assert!(
                message.get("reasoning_content").is_none(),
                "non-deepseek Chat message must omit reasoning_content: {message}"
            );
            if message.get("tool_calls").is_none() {
                assert!(
                    !message["content"].is_null(),
                    "non-tool Chat message must not send content: null: {message}"
                );
            }
        }
        assert_eq!(
            messages[0],
            json!({"role": "user", "content": "First question"})
        );
        assert_eq!(
            messages[1],
            json!({"role": "assistant", "content": "First answer"})
        );
        assert_eq!(
            messages[2],
            json!({"role": "user", "content": "Second question"})
        );
        assert_eq!(
            messages[3],
            json!({"role": "assistant", "content": "Part one\nPart two"})
        );
        assert_eq!(
            messages[4],
            json!({"role": "user", "content": "Third question"})
        );
        Ok(())
    }
    #[test]
    fn responses_chat_deepseek_replays_reasoning_and_fills_missing_tool_turn_reasoning(
    ) -> Result<(), ProxyError> {
        let input = json!([
            {"role": "user", "content": "Check files"},
            {"type": "reasoning", "content": [{"type": "reasoning_text", "text": "Thinking via content"}]},
            {"type": "function_call", "call_id": "call-1", "name": "lookup", "arguments": "{}"},
            {"type": "function_call_output", "call_id": "call-1", "output": "ok"},
            {"type": "function_call", "call_id": "call-2", "name": "lookup", "arguments": "{}"},
            {"type": "function_call_output", "call_id": "call-2", "output": "done"},
            {"role": "assistant", "content": "All set"}
        ]);
        let deepseek_ir = super::super::parse::request(
            Protocol::Responses,
            &json!({"model": "custom-reasoner", "input": input}),
            "/v1/responses",
        )?;
        let deepseek_wire =
            request_for_host(&deepseek_ir, Protocol::Chat, Some("api.deepseek.com"))?;
        let ds_msgs = deepseek_wire["messages"].as_array().expect("messages");
        assert_eq!(ds_msgs[1]["reasoning_content"], "Thinking via content");
        assert_eq!(ds_msgs[3]["reasoning_content"], " ");
        assert_eq!(ds_msgs[5]["reasoning_content"], " ");
        assert_eq!(ds_msgs[5]["content"], "All set");

        let gpt_wire = request_for_host(&deepseek_ir, Protocol::Chat, Some("api.openai.com"))?;
        let gpt_msgs = gpt_wire["messages"].as_array().expect("messages");
        for message in gpt_msgs {
            assert!(message.get("reasoning_content").is_none());
        }
        Ok(())
    }
    #[test]
    fn responses_anthropic_skips_empty_reasoning_messages() -> Result<(), ProxyError> {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "input": [
                {"role": "user", "content": "Hello"},
                {"type": "reasoning", "summary": [], "encrypted_content": "synthetic-seal"},
                {"type": "reasoning", "summary": []},
                {"role": "assistant", "content": "Hi there"}
            ]
        });
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let wire = request(&parsed, Protocol::Anthropic)?;
        let messages = wire["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(
            messages[1]["content"],
            json!([{"type": "text", "text": "Hi there", "cache_control": {"type": "ephemeral"}}])
        );
        Ok(())
    }
    #[test]
    fn chat_drops_silent_reasoning_only_assistant_even_for_deepseek_and_merges_adjacent_users(
    ) -> Result<(), ProxyError> {
        let body = json!({
            "model": "deepseek-reasoner",
            "input": [
                {"role": "user", "content": "First ask"},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Interrupted thought"}]},
                {"role": "assistant", "content": "   "},
                {"role": "user", "content": "Retry ask"}
            ]
        });
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let wire = request(&parsed, Protocol::Chat)?;
        let messages = wire["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "First ask\n\nRetry ask");
        Ok(())
    }
    #[test]
    fn chat_pairs_out_of_order_orphaned_duplicate_unanswered_and_mid_tool_developer_messages(
    ) -> Result<(), ProxyError> {
        let body = json!({
            "model": "gpt-6.2-sol",
            "input": [
                {"type": "function_call_output", "call_id": "orphan-1", "output": "Prior orphaned result"},
                {"role": "user", "content": "Continue"},
                {"type": "function_call", "call_id": "call-1", "name": "first", "arguments": "{}"},
                {"type": "function_call", "call_id": "call-2", "name": "second", "arguments": "{}"},
                {"type": "function_call", "call_id": "call-3", "name": "third", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call-2", "output": "second-first"},
                {"type": "function_call_output", "call_id": "call-2", "output": "second-duplicate"},
                {"role": "developer", "content": "Mid-tool reminder"},
                {"type": "function_call_output", "call_id": "call-1", "output": "first-ok"},
                {"role": "user", "content": "Next turn"}
            ]
        });
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let wire = request(&parsed, Protocol::Chat)?;
        let messages = wire["messages"].as_array().expect("messages");
        assert_eq!(
            messages[0],
            json!({"role": "user", "content": "Prior orphaned result"})
        );
        assert_eq!(messages[1], json!({"role": "user", "content": "Continue"}));
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(
            messages[3],
            json!({"role": "tool", "tool_call_id": "call-1", "content": "first-ok"})
        );
        assert_eq!(
            messages[4],
            json!({"role": "tool", "tool_call_id": "call-2", "content": "second-first"})
        );
        assert_eq!(
            messages[5],
            json!({"role": "tool", "tool_call_id": "call-3", "content": INTERRUPTED_TOOL_MESSAGE})
        );
        assert_eq!(
            messages[6],
            json!({"role": "user", "content": "<system-reminder>\nMid-tool reminder\n</system-reminder>"})
        );
        assert_eq!(messages[7], json!({"role": "user", "content": "Next turn"}));
        assert_eq!(messages.len(), 8);
        Ok(())
    }
    #[test]
    fn chat_uses_max_completion_tokens_only_for_openai_hosts_and_bounds_long_call_ids(
    ) -> Result<(), ProxyError> {
        let long_id = format!("call-{}-suffix", "x".repeat(80));
        assert!(long_id.len() > 64);
        let body = json!({
            "model": "gpt-6.2-sol",
            "max_output_tokens": 512,
            "input": [
                {"role": "user", "content": "Go"},
                {"type": "function_call", "call_id": long_id, "name": "lookup", "arguments": "{}"},
                {"type": "function_call_output", "call_id": long_id, "output": "done"}
            ]
        });
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let relay = request_for_host(&parsed, Protocol::Chat, Some("relay.example"))?;
        assert_eq!(relay["max_tokens"], 512);
        assert!(relay.get("max_completion_tokens").is_none());
        let openai = request_for_host(&parsed, Protocol::Chat, Some("api.openai.com"))?;
        assert_eq!(openai["max_completion_tokens"], 512);
        assert!(openai.get("max_tokens").is_none());
        let call_id = openai["messages"][1]["tool_calls"][0]["id"]
            .as_str()
            .expect("call id");
        assert_eq!(call_id.len(), 64);
        assert_eq!(openai["messages"][2]["tool_call_id"], call_id);
        Ok(())
    }
    #[test]
    fn chat_and_responses_and_anthropic_degrade_unsupported_files_safely() -> Result<(), ProxyError>
    {
        let body = json!({
            "model": "gpt-6.2-sol",
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Read this"},
                    {"type": "input_file", "filename": "notes.txt", "file_data": "data:text/plain;base64,aGk="}
                ]
            }]
        });
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let chat_wire = request(&parsed, Protocol::Chat)?;
        assert_eq!(
            chat_wire["messages"][0]["content"],
            json!([
                {"type": "text", "text": "Read this"},
                {"type": "text", "text": "[Attached file: notes.txt]"}
            ])
        );
        let anthropic_wire = request(&parsed, Protocol::Anthropic)?;
        assert_eq!(
            anthropic_wire["messages"][0]["content"][1],
            json!({
                "type": "text",
                "text": "[Attached file: notes.txt]",
                "cache_control": {"type": "ephemeral"}
            })
        );
        let chat_source = json!({
            "model": "gpt-6.2-sol",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "file",
                    "file": {"file_id": "file_remote_123", "filename": "report.csv", "mime_type": "text/csv"}
                }]
            }]
        });
        let chat_parsed =
            super::super::parse::request(Protocol::Chat, &chat_source, "/v1/chat/completions")?;
        let responses_wire = request(&chat_parsed, Protocol::Responses)?;
        assert_eq!(
            responses_wire["input"][0]["content"][0],
            json!({"type": "input_text", "text": "[Attached file: report.csv]"})
        );
        Ok(())
    }
    #[test]
    fn responses_folds_cross_protocol_system_into_instructions_drops_unsigned_thinking_and_pairs_orphaned_outputs(
    ) -> Result<(), ProxyError> {
        let body = json!({
            "model": "gpt-6.2-sol",
            "messages": [
                {"role": "system", "content": "Top system rule"},
                {"role": "tool", "tool_call_id": "orphan-call", "content": "Orphan output"},
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi", "reasoning_content": "Unsigned chat thought"},
                {"role": "system", "content": "Late rule"}
            ]
        });
        let parsed = super::super::parse::request(Protocol::Chat, &body, "/v1/chat/completions")?;
        let wire = request(&parsed, Protocol::Responses)?;
        assert_eq!(wire["instructions"], "Top system rule");
        let input = wire["input"].as_array().expect("input");
        assert!(input.iter().all(|item| item["role"] != "system"));
        assert!(input.iter().all(|item| item["type"] != "reasoning"));
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["text"], "Orphan output");
        assert_eq!(input.last().expect("last")["role"], "developer");
        Ok(())
    }
    #[test]
    fn anthropic_pairs_tool_blocks_sanitizes_ids_skips_blank_text_drops_cross_model_thinking_omits_service_tier_and_adds_cache_control(
    ) -> Result<(), ProxyError> {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "instructions": "Base instructions",
            "service_tier": "priority",
            "input": [
                {"type": "function_call_output", "call_id": "orphan:0#1", "output": "Orphan text"},
                {"role": "user", "content": [{"type": "input_text", "text": "   "}, {"type": "input_text", "text": "Real ask"}]},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Cross-model thought"}]},
                {"type": "function_call", "call_id": "Bash:0#1", "name": "run", "arguments": "{}"},
                {"role": "assistant", "content": "Running now"}
            ]
        });
        let parsed = super::super::parse::request(Protocol::Responses, &body, "/v1/responses")?;
        let wire = request(&parsed, Protocol::Anthropic)?;
        assert!(wire.get("service_tier").is_none());
        assert_eq!(
            wire["system"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        let messages = wire["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(
            messages[0]["content"],
            json!([
                {"type": "text", "text": "Orphan text"},
                {"type": "text", "text": "Real ask"}
            ])
        );
        assert_eq!(messages[1]["role"], "assistant");
        let assistant_blocks = messages[1]["content"].as_array().expect("assistant blocks");
        assert_eq!(assistant_blocks.len(), 2);
        assert_eq!(
            assistant_blocks[0],
            json!({"type": "text", "text": "Running now"})
        );
        assert_eq!(assistant_blocks[1]["type"], "tool_use");
        let safe_id = assistant_blocks[1]["id"].as_str().expect("id");
        assert!(safe_id.starts_with(ANTHROPIC_ID_MARK));
        assert!(safe_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')));
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(messages[2]["content"][0]["tool_use_id"], safe_id);
        assert_eq!(messages[2]["content"][0]["is_error"], true);
        assert_eq!(
            messages[2]["content"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        Ok(())
    }
    #[test]
    fn gemini_model_function_response_preserves_tool_result_and_media_in_chat(
    ) -> Result<(), ProxyError> {
        let body = json!({
            "contents": [
                {"role": "user", "parts": [{"text": "Read it"}]},
                {"role": "model", "parts": [{"functionCall": {"id": "history-call", "name": "read", "args": {"path": "history"}}}]},
                {"role": "model", "parts": [{"functionResponse": {
                    "id": "history-call",
                    "name": "read",
                    "response": {"result": "history-result"},
                    "parts": [{"inlineData": {"mimeType": "image/png", "data": "dG9vbA=="}}]
                }}]}
            ]
        });
        let parsed = super::super::parse::request(
            Protocol::Gemini,
            &body,
            "/v1beta/models/fixture:generateContent",
        )?;
        let chat_wire = request(&parsed, Protocol::Chat)?;
        let messages = chat_wire["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "history-call");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "history-call");
        assert!(messages[2]["content"]
            .as_str()
            .is_some_and(|s| s.contains("history-result")));
        assert_eq!(messages[3]["role"], "user");
        assert!(messages[3]["content"].to_string().contains("dG9vbA=="));
        Ok(())
    }
}
