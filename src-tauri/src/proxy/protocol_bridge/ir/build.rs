//! Build each upstream wire format directly from the shared conversation IR.
use super::*;
use crate::proxy::ProxyError;
use serde_json::{json, Value};

fn invalid() -> ProxyError {
    ProxyError::InvalidRequest("Content cannot be represented in the selected protocol".into())
}

pub(crate) fn request(r: &Request, target: Protocol) -> Result<Value, ProxyError> {
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
        Protocol::Chat => chat(r)?,
        Protocol::Responses => responses(r)?,
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
            body["service_tier"] = json!(value);
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
            if !client_tool_search(tool.native.as_ref().ok_or_else(invalid)?) {
                return Err(invalid());
            }
            Ok(vec![Tool {
                kind: "function".into(),
                native: None,
                name: crate::proxy::providers::transform_codex_chat::TOOL_SEARCH_PROXY_NAME.into(),
                description: crate::proxy::providers::transform_codex_chat::TOOL_SEARCH_DESCRIPTION
                    .into(),
                schema: crate::proxy::providers::transform_codex_chat::tool_search_parameters(),
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
                let mut file = json!({});
                if let Some(id) = id { file["file_id"] = json!(id); }
                else if let Some(url) = url { file["file_url"] = json!(url); }
                else if data.is_some() { file["file_data"] = json!(source(media_type,data,url)?); }
                else { return Err(invalid()); }
                if let Some(name) = name { file["filename"] = json!(name); }
                out.push(if responses { file["type"] = json!("input_file");file } else { json!({"type":"file","file":file}) });
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

fn chat(r: &Request) -> Result<Value, ProxyError> {
    let mut messages = Vec::new();
    if !r.system.is_empty() {
        messages.push(json!({"role":"system","content":openai_parts(&r.system,false,false)?}));
    }
    for m in &r.messages {
        for p in &m.parts {
            if let Part::ToolResult { call_id, parts, .. } = p {
                let content = openai_parts(parts, false, false)?;
                messages.push(json!({"role":"tool","tool_call_id":call_id,"content":if parts.iter().all(|p|matches!(p,Part::Text{..})) {json!(text(parts))} else {json!(content)}}));
            }
        }
        let content = openai_parts(&m.parts, false, m.role == "assistant")?;
        let wire_content = match content.as_slice() {
            [] => Value::Null,
            [part] if part["type"] == "text" => part["text"].clone(),
            _ => json!(content),
        };
        let mut message = json!({"role":m.role,"content":wire_content});
        let calls: Vec<_> = m.parts.iter().filter_map(|p| if let Part::ToolCall {id,name,arguments,signature} = p {
            let mut call = json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}});
            if let Some(signature)=signature {call["extra_content"] = json!({"google":{"thought_signature":signature}});}
            Some(call)
        } else {None}).collect();
        if !calls.is_empty() {
            message["tool_calls"] = json!(calls);
        }
        let thoughts: Vec<_> = m
            .parts
            .iter()
            .filter_map(|p| {
                if let Part::Thinking { text, .. } = p {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        if !thoughts.is_empty() {
            message["reasoning_content"] = json!(thoughts.join("\n"));
        }
        if !content.is_empty()
            || message.get("tool_calls").is_some()
            || !thoughts.is_empty()
            || m.parts.is_empty()
        {
            messages.push(message);
        }
    }
    let mut body = json!({"messages":messages});
    if let Some(n) = r.max_tokens {
        body["max_completion_tokens"] = json!(n);
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
        let mut f = json!({"name":t.name,"description":t.description,"parameters":t.schema});
        if let Some(strict) = t.strict {
            f["strict"] = json!(strict);
        }
        tools.push(json!({"type":"function","function":f}));
    }
    if omitted_search && tools.is_empty() && r.tool_choice == Some(ToolChoice::Required) {
        return Err(invalid());
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if !omitted_search || body.get("tools").is_some() {
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
    if let Some(value) = &r.reasoning.summary {
        body["reasoning_summary"] = json!(value);
    }
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

fn responses(r: &Request) -> Result<Value, ProxyError> {
    let mut input = Vec::new();
    if !r.system.is_empty() {
        input.push(json!({"role":"system","content":openai_parts(&r.system,true,false)?}));
    }
    for m in &r.messages {
        let mut pending = Vec::new();
        for p in &m.parts {
            let item = match p {
                Part::ToolSearchCall { native, .. } | Part::ToolSearchResult { native, .. } => {
                    Some(native.clone())
                }
                Part::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } => Some(
                    json!({"type":"function_call","call_id":id,"name":name,"arguments":arguments.to_string()}),
                ),
                Part::NamespacedToolCall {
                    id,
                    namespace,
                    name,
                    arguments,
                    ..
                } => Some(
                    json!({"type":"function_call","call_id":id,"namespace":namespace,"name":name,"arguments":arguments.to_string()}),
                ),
                Part::CustomToolCall {
                    id,
                    name,
                    input,
                    namespace,
                } => {
                    let mut call =
                        json!({"type":"custom_tool_call","call_id":id,"name":name,"input":input});
                    if let Some(ns) = namespace {
                        call["namespace"] = json!(ns);
                    }
                    Some(call)
                }
                Part::CustomToolResult { call_id, parts } => Some(
                    json!({"type":"custom_tool_call_output","call_id":call_id,"output":if parts.iter().all(|p|matches!(p,Part::Text{..})) {json!(text(parts))} else {json!(openai_parts(parts,true,false)?)}}),
                ),
                Part::ToolResult { call_id, parts, .. } => Some(
                    json!({"type":"function_call_output","call_id":call_id,"output":if parts.iter().all(|p|matches!(p,Part::Text{..})) {json!(text(parts))} else {json!(openai_parts(parts,true,false)?)}}),
                ),
                Part::Thinking {
                    text,
                    source,
                    sealed,
                    ..
                } => Some(if *source == Protocol::Responses && sealed.is_some() {
                    sealed.clone().ok_or_else(invalid)?
                } else {
                    json!({"type":"reasoning","summary":[{"type":"summary_text","text":text}]})
                }),
                _ => None,
            };
            if let Some(item) = item {
                if !pending.is_empty() {
                    input.push(json!({"role":m.role,"content":openai_parts(&pending,true,m.role=="assistant")?}));
                    pending.clear();
                }
                input.push(item);
            } else {
                pending.push(p.clone());
            }
        }
        if !pending.is_empty() {
            input.push(
                json!({"role":m.role,"content":openai_parts(&pending,true,m.role=="assistant")?}),
            );
        }
    }
    let mut body = json!({"input":input});
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
    if let Some(v) = choice(r, Protocol::Responses) {
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

fn anthropic_parts(parts: &[Part]) -> Result<Vec<Value>, ProxyError> {
    let mut out = Vec::new();
    for p in parts {
        match p {
            Part::Text{text,..}=>out.push(json!({"type":"text","text":text})),
            Part::Image{media_type,data,url,..}|Part::File{media_type,data,url,..}=>{
                let source=if let Some(data)=data {json!({"type":"base64","media_type":media_type,"data":data})} else if let Some(url)=url {json!({"type":"url","url":url})} else {return Err(invalid());};
                out.push(json!({"type":if matches!(p,Part::Image{..}) {"image"} else {"document"},"source":source}));
            }
            Part::ToolCall{id,name,arguments,..}=>out.push(json!({"type":"tool_use","id":id,"name":name,"input":arguments})),
            Part::ToolResult{call_id,parts,is_error,..}=>out.push(json!({"type":"tool_result","tool_use_id":call_id,"content":anthropic_parts(parts)?,"is_error":is_error})),
            Part::Thinking{text,signature,source,sealed}=>{
                if *source==Protocol::Anthropic {
                    if let Some(sealed)=sealed {out.push(sealed.clone());continue;}
                    let mut block=json!({"type":"thinking","thinking":text});
                    if let Some(signature)=signature {block["signature"]=json!(signature);}
                    out.push(block);
                } else if !text.is_empty() {out.push(json!({"type":"text","text":text}));}
            }
            Part::ToolReference{name}=>out.push(json!({"type":"tool_reference","tool_name":name})),
            Part::Search{query,hits,..}=>out.push(json!({"type":"text","text":format!("{query}\n{}",json!(hits))})),
            Part::NamespacedToolCall{..}|Part::CustomToolCall{..}|Part::CustomToolResult{..}
            |Part::ToolSearchCall{..}|Part::ToolSearchResult{..}=>return Err(invalid()),
        }
    }
    Ok(out)
}
fn anthropic(r: &Request) -> Result<Value, ProxyError> {
    let mut system = anthropic_parts(&r.system)?;
    let mut messages: Vec<Value> = Vec::new();
    for m in &r.messages {
        if matches!(m.role.as_str(), "system" | "developer") {
            system.extend(anthropic_parts(&m.parts)?);
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
        let content = anthropic_parts(&parts)?;
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
    if r.parallel == Some(false) {
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
                assert_eq!(chat["tools"][0]["function"]["strict"], false);
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
                assert_eq!(chat["tools"][0]["function"]["strict"], true);
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
            assert!(request(&parsed, target)
                .unwrap()
                .to_string()
                .contains("c3ludGhldGlj"));
        }
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
}
