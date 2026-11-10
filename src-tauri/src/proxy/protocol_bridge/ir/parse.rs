//! Parse supported wire fields into the common conversation representation.
//! Vendor bookkeeping is ignored at object boundaries, as in Magpie's typed readers.
use super::*;
use crate::proxy::ProxyError;
use serde_json::{json, Map};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) fn request(
    protocol: Protocol,
    body: &Value,
    endpoint: &str,
) -> Result<Request, ProxyError> {
    object(body)?;
    let payload = if protocol == Protocol::Gemini {
        body.get("generateContentRequest")
            .or_else(|| body.get("request").filter(|v| v.get("contents").is_some()))
            .unwrap_or(body)
    } else {
        body
    };
    object(payload)?;
    let model = string(body, "model")?
        .or(string(payload, "model")?)
        .or_else(|| {
            (protocol == Protocol::Gemini).then(|| {
                endpoint
                    .split("/models/")
                    .nth(1)
                    .and_then(|v| v.split(':').next())
                    .unwrap_or_default()
                    .to_owned()
            })
        })
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| invalid("Missing request model"))?;
    let mut request = Request {
        model,
        system: Vec::new(),
        messages: Vec::new(),
        tools: Vec::new(),
        tool_choice: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        stop: Vec::new(),
        stream: boolean(body, "stream")?.unwrap_or(false),
        reasoning: Reasoning::default(),
        parallel: None,
        format: None,
        native: NativeOptions::default(),
    };
    match protocol {
        Protocol::Chat => chat(payload, &mut request)?,
        Protocol::Responses => responses(payload, &mut request)?,
        Protocol::Anthropic => anthropic(payload, &mut request)?,
        Protocol::Gemini => {
            request.stream |= endpoint
                .split('?')
                .next()
                .unwrap_or(endpoint)
                .ends_with(":streamGenerateContent");
            gemini(payload, &mut request)?;
        }
    }
    Ok(request)
}

fn invalid(message: &'static str) -> ProxyError {
    ProxyError::InvalidRequest(message.into())
}
fn object(value: &Value) -> Result<&Map<String, Value>, ProxyError> {
    value
        .as_object()
        .ok_or_else(|| invalid("Invalid protocol object"))
}
fn array(value: &Value) -> Result<&Vec<Value>, ProxyError> {
    value
        .as_array()
        .ok_or_else(|| invalid("Invalid protocol array"))
}
fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.get(name).filter(|v| !v.is_null())
}
fn string(value: &Value, name: &str) -> Result<Option<String>, ProxyError> {
    field(value, name)
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("Invalid protocol string"))
        })
        .transpose()
}
fn required_string(value: &Value, name: &str) -> Result<String, ProxyError> {
    string(value, name)?
        .filter(|v| !v.is_empty())
        .ok_or_else(|| invalid("Missing protocol string"))
}
fn boolean(value: &Value, name: &str) -> Result<Option<bool>, ProxyError> {
    field(value, name)
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| invalid("Invalid protocol boolean"))
        })
        .transpose()
}
fn unsigned(value: &Value, name: &str) -> Result<Option<u64>, ProxyError> {
    field(value, name)
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| invalid("Invalid protocol token limit"))
        })
        .transpose()
}
fn signed(value: &Value, name: &str) -> Result<Option<i64>, ProxyError> {
    field(value, name)
        .map(|v| {
            v.as_i64()
                .ok_or_else(|| invalid("Invalid protocol thinking budget"))
        })
        .transpose()
}
fn number(value: &Value, name: &str) -> Result<Option<Value>, ProxyError> {
    field(value, name)
        .map(|v| {
            if v.is_number() {
                Ok(v.clone())
            } else {
                Err(invalid("Invalid protocol number"))
            }
        })
        .transpose()
}
fn strings(value: &Value) -> Result<Vec<String>, ProxyError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Some(value) = value.as_str() {
        return Ok(vec![value.to_owned()]);
    }
    array(value)?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("Invalid protocol string list"))
        })
        .collect()
}
fn arguments(value: Option<&Value>, encoded: bool) -> Result<Value, ProxyError> {
    let parsed = match value.filter(|v| !v.is_null()) {
        None => json!({}),
        Some(Value::String(text)) if encoded => {
            serde_json::from_str(text).map_err(|_| invalid("Invalid tool arguments JSON"))?
        }
        Some(value) => value.clone(),
    };
    if !parsed.is_object() {
        return Err(invalid("Tool arguments must be an object"));
    }
    Ok(parsed)
}
fn schema(value: Option<&Value>) -> Result<Value, ProxyError> {
    match value.filter(|v| !v.is_null()) {
        None => Ok(json!({"type":"object","properties":{}})),
        Some(v) if v.is_object() || v.is_boolean() => Ok(v.clone()),
        Some(_) => Err(invalid("Invalid tool schema")),
    }
}
fn role(value: &Value, gemini: bool) -> Result<String, ProxyError> {
    let role = string(value, "role")?.unwrap_or_else(|| "user".into());
    match role.as_str() {
        "system" | "developer" | "user" | "assistant" | "tool" => Ok(role),
        "model" if gemini => Ok("assistant".into()),
        _ => Err(invalid("Invalid message role")),
    }
}
fn text(value: &str) -> Part {
    Part::Text {
        text: value.to_owned(),
        signature: None,
    }
}
fn data_uri(value: String) -> (String, Option<String>, Option<String>) {
    if let Some((header, data)) = value.strip_prefix("data:").and_then(|v| v.split_once(',')) {
        if let Some(media_type) = header.strip_suffix(";base64") {
            return (media_type.to_owned(), Some(data.to_owned()), None);
        }
    }
    (String::new(), None, Some(value))
}
fn image_url(value: &Value) -> Result<Part, ProxyError> {
    let (url, detail) = if let Some(url) = value.as_str() {
        (url.to_owned(), None)
    } else {
        object(value)?;
        (required_string(value, "url")?, string(value, "detail")?)
    };
    let (media_type, data, url) = data_uri(url);
    Ok(Part::Image {
        media_type,
        data,
        url,
        detail,
    })
}
fn thinking(
    text: String,
    signature: Option<String>,
    source: Protocol,
    sealed: Option<Value>,
) -> Part {
    Part::Thinking {
        text,
        signature,
        source,
        sealed,
    }
}
fn thinking_details(value: &Value, parts: &mut Vec<Part>) -> Result<(), ProxyError> {
    for detail in array(value)? {
        object(detail)?;
        let kind = string(detail, "type")?.unwrap_or_default();
        let value = string(detail, "text")?
            .or(string(detail, "summary")?)
            .unwrap_or_default();
        let sealed =
            (kind.contains("encrypted") || detail.get("data").is_some()).then(|| detail.clone());
        parts.push(thinking(
            value,
            string(detail, "signature")?,
            Protocol::Chat,
            sealed,
        ));
    }
    Ok(())
}
fn chat_content(value: &Value) -> Result<Vec<Part>, ProxyError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Some(value) = value.as_str() {
        return Ok(vec![text(value)]);
    }
    let mut parts = Vec::new();
    for block in array(value)? {
        object(block)?;
        match block.get("type").and_then(Value::as_str) {
            Some("text" | "input_text" | "output_text") => parts.push(Part::Text {
                text: required_string_allow_empty(block, "text")?,
                signature: string(block, "thoughtSignature")?,
            }),
            Some("image_url") => parts.push(image_url(&block["image_url"])?),
            Some("input_image") => {
                let mut part = image_url(&block["image_url"])?;
                if let Part::Image { detail, .. } = &mut part {
                    *detail = string(block, "detail")?;
                }
                parts.push(part);
            }
            Some("file" | "input_file") => {
                let file = block.get("file").unwrap_or(block);
                object(file)?;
                let source = string(file, "file_data")?;
                let (mut media_type, data, embedded_url) = match source {
                    Some(source) if source.starts_with("data:") => data_uri(source),
                    Some(source) => (String::new(), Some(source), None),
                    None => (String::new(), None, None),
                };
                if media_type.is_empty() {
                    media_type = string(file, "mime_type")?.unwrap_or_default();
                }
                let url = string(file, "file_url")?.or(embedded_url);
                let id = string(file, "file_id")?;
                if data.is_none() && url.is_none() && id.is_none() {
                    return Err(invalid("File source is missing"));
                }
                parts.push(Part::File {
                    media_type,
                    data,
                    url,
                    id,
                    name: string(file, "filename")?,
                });
            }
            Some("refusal") => parts.push(text(&required_string_allow_empty(block, "refusal")?)),
            _ => {}
        }
    }
    Ok(parts)
}
fn required_string_allow_empty(value: &Value, name: &str) -> Result<String, ProxyError> {
    string(value, name)?.ok_or_else(|| invalid("Missing protocol string"))
}
fn common_openai(body: &Value, request: &mut Request) -> Result<(), ProxyError> {
    request.temperature = number(body, "temperature")?;
    request.top_p = number(body, "top_p")?;
    request.stop = strings(&body["stop"])?;
    request.parallel = boolean(body, "parallel_tool_calls")?;
    request.native.metadata = field(body, "metadata").cloned();
    request.native.client_metadata = field(body, "client_metadata").cloned();
    if let Some(include) = field(body, "include") {
        strings(include)?;
        request.native.include = Some(include.clone());
    }
    request.native.prompt_cache_key = string(body, "prompt_cache_key")?;
    request.native.service_tier = string(body, "service_tier")?;
    request.native.user = string(body, "user")?;
    request.native.access_programs = body.get("access_programs").cloned();
    request.native.cache_retention = string(body, "prompt_cache_retention")?;
    request.native.safeguards = body.get("safeguards").cloned();
    Ok(())
}
fn openai_choice(value: &Value) -> Result<ToolChoice, ProxyError> {
    match value.as_str() {
        Some("auto") => Ok(ToolChoice::Auto),
        Some("none") => Ok(ToolChoice::None),
        Some("required") => Ok(ToolChoice::Required),
        Some(_) => Err(invalid("Invalid tool choice")),
        None => {
            object(value)?;
            if value["type"] == "custom" {
                return Ok(ToolChoice::Custom {
                    name: required_string(value, "name")?,
                    namespace: string(value, "namespace")?,
                });
            }
            if value["type"] != "function" {
                return Err(invalid("Invalid tool choice"));
            }
            let function = value.get("function").unwrap_or(value);
            let name = required_string(function, "name")?;
            Ok(
                match string(function, "namespace")?.filter(|v| !v.is_empty()) {
                    Some(namespace) => ToolChoice::Namespaced { namespace, name },
                    None => ToolChoice::Named(name),
                },
            )
        }
    }
}
fn parse_format(value: &Value) -> Result<Option<Format>, ProxyError> {
    object(value)?;
    let kind = required_string(value, "type")?;
    if kind == "text" {
        return Ok(None);
    }
    if !matches!(kind.as_str(), "json_object" | "json_schema") {
        return Err(invalid("Unsupported response format"));
    }
    let definition = value.get("json_schema").unwrap_or(value);
    object(definition)?;
    let schema = if let Some(schema) = field(definition, "schema") {
        Some(self::schema(Some(schema))?)
    } else {
        None
    };
    if kind == "json_schema" && schema.is_none() {
        return Err(invalid("Missing response schema"));
    }
    Ok(Some(Format {
        kind,
        name: string(definition, "name")?,
        schema,
        strict: boolean(definition, "strict")?,
    }))
}
fn function(value: &Value, schema_key: &str) -> Result<Tool, ProxyError> {
    object(value)?;
    Ok(Tool {
        kind: "function".into(),
        native: None,
        name: required_string(value, "name")?,
        description: string(value, "description")?.unwrap_or_default(),
        schema: schema(value.get(schema_key))?,
        strict: boolean(value, "strict")?,
        deferred: boolean(value, "defer_loading")?.unwrap_or(false),
    })
}
fn native_tool(value: &Value, kind: &str) -> Result<Tool, ProxyError> {
    object(value)?;
    let kind = if kind.starts_with("web_search")
        || matches!(
            kind,
            "google_search" | "googleSearch" | "googleSearchRetrieval"
        ) {
        "web_search"
    } else {
        kind
    };
    Ok(Tool {
        kind: kind.to_owned(),
        native: Some(value.clone()),
        name: string(value, "name")?.unwrap_or_default(),
        description: string(value, "description")?.unwrap_or_default(),
        schema: json!({}),
        strict: None,
        deferred: boolean(value, "defer_loading")?.unwrap_or(false),
    })
}
fn chat(body: &Value, request: &mut Request) -> Result<(), ProxyError> {
    common_openai(body, request)?;
    if unsigned(body, "n")?.is_some_and(|n| n != 1) {
        return Err(invalid("Multiple choices are unsupported across protocols"));
    }
    request.max_tokens = unsigned(body, "max_completion_tokens")?.or(unsigned(body, "max_tokens")?);
    request.reasoning.effort = string(body, "reasoning_effort")?;
    request.reasoning.enabled = request.reasoning.effort.as_deref().map(|e| e != "none");
    request.reasoning.summary = string(body, "reasoning_summary")?;
    if let Some(verbosity) = string(body, "verbosity")? {
        request.native.text = Some(json!({"verbosity":verbosity}));
    }
    if let Some(reasoning) = field(body, "reasoning") {
        reasoning_options(reasoning, &mut request.reasoning)?;
    }
    if let Some(value) = field(body, "response_format") {
        request.format = parse_format(value)?;
    }
    if let Some(value) = field(body, "tool_choice") {
        request.tool_choice = Some(openai_choice(value)?);
    }
    if let Some(tools) = field(body, "tools") {
        for tool in array(tools)? {
            object(tool)?;
            let kind = required_string(tool, "type")?;
            if kind == "function" {
                request
                    .tools
                    .push(function(&tool["function"], "parameters")?);
            } else {
                request.tools.push(native_tool(tool, &kind)?);
            }
        }
    }
    for message in array(&body["messages"])? {
        object(message)?;
        let role = role(message, false)?;
        let mut parts = chat_content(&message["content"])?;
        for key in ["reasoning_content", "reasoning"] {
            if let Some(value) = field(message, key) {
                if let Some(value) = value.as_str() {
                    if !value.is_empty() {
                        parts.push(thinking(value.to_owned(), None, Protocol::Chat, None));
                    }
                } else if value.is_array() {
                    thinking_details(value, &mut parts)?;
                } else {
                    return Err(invalid("Invalid assistant reasoning"));
                }
            }
        }
        if let Some(value) = field(message, "reasoning_details") {
            thinking_details(value, &mut parts)?;
        }
        if role == "tool" {
            parts = vec![Part::ToolResult {
                call_id: required_string(message, "tool_call_id")?,
                name: string(message, "name")?,
                parts,
                is_error: false,
            }];
        }
        if let Some(calls) = field(message, "tool_calls") {
            for call in array(calls)? {
                object(call)?;
                if call["type"] != "function" {
                    continue;
                }
                let function = &call["function"];
                object(function)?;
                parts.push(Part::ToolCall {
                    id: required_string(call, "id")?,
                    name: required_string(function, "name")?,
                    arguments: arguments(function.get("arguments"), true)?,
                    signature: call
                        .pointer("/extra_content/google/thought_signature")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
            }
        }
        request.messages.push(Message { role, parts });
    }
    Ok(())
}
fn reasoning_options(value: &Value, reasoning: &mut Reasoning) -> Result<(), ProxyError> {
    object(value)?;
    if let Some(effort) = string(value, "effort")? {
        reasoning.enabled = Some(effort != "none");
        reasoning.effort = Some(effort);
    }
    if let Some(summary) = string(value, "summary")? {
        reasoning.summary = Some(summary);
    }
    if let Some(enabled) = boolean(value, "enabled")? {
        reasoning.enabled = Some(enabled);
    }
    if let Some(budget) = signed(value, "budget_tokens")? {
        reasoning.budget = Some(budget);
    }
    Ok(())
}
fn responses(body: &Value, request: &mut Request) -> Result<(), ProxyError> {
    common_openai(body, request)?;
    request.max_tokens = unsigned(body, "max_output_tokens")?;
    if let Some(instructions) = field(body, "instructions") {
        request.system = vec![text(
            instructions
                .as_str()
                .ok_or_else(|| invalid("Invalid instructions"))?,
        )];
    }
    if let Some(reasoning) = field(body, "reasoning") {
        reasoning_options(reasoning, &mut request.reasoning)?;
    }
    if let Some(value) = field(body, "text") {
        object(value)?;
        string(value, "verbosity")?;
        request.native.text = Some(value.clone());
        if let Some(value) = field(value, "format") {
            request.format = parse_format(value)?;
        }
    }
    if let Some(value) = field(body, "tool_choice") {
        request.tool_choice = Some(openai_choice(value)?);
    }
    if let Some(tools) = field(body, "tools") {
        for tool in array(tools)? {
            object(tool)?;
            let kind = required_string(tool, "type")?;
            if kind == "function" {
                let mut function = function(tool, "parameters")?;
                function.native = Some(tool.clone());
                request.tools.push(function);
            } else {
                request.tools.push(native_tool(tool, &kind)?);
            }
        }
    }
    let input = &body["input"];
    if let Some(value) = input.as_str() {
        request.messages.push(Message {
            role: "user".into(),
            parts: vec![text(value)],
        });
        return Ok(());
    }
    for item in array(input)? {
        object(item)?;
        match item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message")
        {
            "message" => request.messages.push(Message {
                role: role(item, false)?,
                parts: chat_content(&item["content"])?,
            }),
            "function_call" => {
                let id = string(item, "call_id")?
                    .or(string(item, "id")?)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| invalid("Missing tool call ID"))?;
                let name = required_string(item, "name")?;
                let arguments = arguments(item.get("arguments"), true)?;
                let part =
                    if let Some(namespace) = string(item, "namespace")?.filter(|v| !v.is_empty()) {
                        Part::NamespacedToolCall {
                            id,
                            namespace,
                            name,
                            arguments,
                            signature: None,
                        }
                    } else {
                        Part::ToolCall {
                            id,
                            name,
                            arguments,
                            signature: None,
                        }
                    };
                request.messages.push(Message {
                    role: "assistant".into(),
                    parts: vec![part],
                });
            }
            "function_call_output" => request.messages.push(Message {
                role: "tool".into(),
                parts: vec![Part::ToolResult {
                    call_id: required_string(item, "call_id")?,
                    name: None,
                    parts: chat_content(&item["output"])?,
                    is_error: false,
                }],
            }),
            "reasoning" => {
                let mut value = String::new();
                if let Some(summary) = field(item, "summary") {
                    for part in array(summary)? {
                        object(part)?;
                        if let Some(text) = string(part, "text")? {
                            value.push_str(&text);
                        }
                    }
                }
                request.messages.push(Message {
                    role: "assistant".into(),
                    parts: vec![thinking(
                        value,
                        None,
                        Protocol::Responses,
                        Some(item.clone()),
                    )],
                });
            }
            "web_search_call" => {
                let action = item.get("action").unwrap_or(item);
                object(action)?;
                let query = string(action, "query")?.unwrap_or_default();
                let hits = match field(action, "sources") {
                    Some(value) => array(value)?.clone(),
                    None => Vec::new(),
                };
                request.messages.push(Message {
                    role: "assistant".into(),
                    parts: vec![Part::Search {
                        query,
                        hits,
                        gemini: None,
                    }],
                });
            }
            "item_reference" => return Err(invalid("Unresolved response item reference")),
            "custom_tool_call" => request.messages.push(Message {
                role: "assistant".into(),
                parts: vec![Part::CustomToolCall {
                    id: string(item, "call_id")?
                        .or(string(item, "id")?)
                        .filter(|v| !v.is_empty())
                        .ok_or_else(|| invalid("Missing tool call ID"))?,
                    name: required_string(item, "name")?,
                    input: required_string_allow_empty(item, "input")?,
                    namespace: string(item, "namespace")?,
                }],
            }),
            "custom_tool_call_output" => request.messages.push(Message {
                role: "tool".into(),
                parts: vec![Part::CustomToolResult {
                    call_id: required_string(item, "call_id")?,
                    parts: chat_content(&item["output"])?,
                }],
            }),
            "tool_search_call" | "tool_search_output" => {
                return Err(invalid("Unsupported native tool item across protocols"))
            }
            _ => {}
        }
    }
    Ok(())
}
fn anthropic_parts(value: &Value) -> Result<Vec<Part>, ProxyError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Some(value) = value.as_str() {
        return Ok(vec![text(value)]);
    }
    let mut parts = Vec::new();
    for block in array(value)? {
        object(block)?;
        match block.get("type").and_then(Value::as_str) {
            Some("text") => parts.push(text(&required_string_allow_empty(block, "text")?)),
            Some("thinking") => parts.push(thinking(
                required_string_allow_empty(block, "thinking")?,
                string(block, "signature")?,
                Protocol::Anthropic,
                None,
            )),
            Some("redacted_thinking") => parts.push(thinking(
                String::new(),
                None,
                Protocol::Anthropic,
                Some(block.clone()),
            )),
            Some("image" | "document") => {
                let source = &block["source"];
                object(source)?;
                let kind = required_string(source, "type")?;
                let media_type = string(source, "media_type")?.unwrap_or_default();
                let (data, url) = match kind.as_str() {
                    "base64" => (Some(required_string(source, "data")?), None),
                    "url" => (None, Some(required_string(source, "url")?)),
                    "text" if block["type"] == "document" => {
                        parts.push(text(&required_string_allow_empty(source, "data")?));
                        continue;
                    }
                    _ => return Err(invalid("Unsupported attachment source")),
                };
                if block["type"] == "image" {
                    parts.push(Part::Image {
                        media_type,
                        data,
                        url,
                        detail: None,
                    });
                } else {
                    parts.push(Part::File {
                        media_type,
                        data,
                        url,
                        id: None,
                        name: string(block, "title")?,
                    });
                }
            }
            Some("tool_use" | "server_tool_use") => parts.push(Part::ToolCall {
                id: required_string(block, "id")?,
                name: required_string(block, "name")?,
                arguments: arguments(block.get("input"), false)?,
                signature: None,
            }),
            Some("tool_result") => parts.push(Part::ToolResult {
                call_id: required_string(block, "tool_use_id")?,
                name: None,
                parts: anthropic_parts(&block["content"])?,
                is_error: boolean(block, "is_error")?.unwrap_or(false),
            }),
            Some("tool_reference") => parts.push(Part::ToolReference {
                name: required_string(block, "tool_name")?,
            }),
            Some("web_search_tool_result") => {
                let hits = match field(block, "content") {
                    Some(Value::Array(hits)) => hits.clone(),
                    Some(value) => vec![value.clone()],
                    None => Vec::new(),
                };
                parts.push(Part::Search {
                    query: String::new(),
                    hits,
                    gemini: None,
                });
            }
            _ => {}
        }
    }
    Ok(parts)
}
fn anthropic(body: &Value, request: &mut Request) -> Result<(), ProxyError> {
    request.max_tokens = unsigned(body, "max_tokens")?;
    request.temperature = number(body, "temperature")?;
    request.top_p = number(body, "top_p")?;
    request.native.top_k = number(body, "top_k")?;
    request.stop = strings(&body["stop_sequences"])?;
    request.native.metadata = field(body, "metadata").cloned();
    request.native.safeguards = body.get("safeguards").cloned();
    if let Some(system) = field(body, "system") {
        request.system = anthropic_parts(system)?;
    }
    if let Some(value) = field(body, "thinking") {
        object(value)?;
        let kind = required_string(value, "type")?;
        if !matches!(kind.as_str(), "enabled" | "disabled" | "adaptive") {
            return Err(invalid("Invalid thinking type"));
        }
        request.reasoning.enabled = Some(kind != "disabled");
        request.reasoning.visible = kind != "disabled";
        request.reasoning.budget = signed(value, "budget_tokens")?;
    }
    if let Some(output) = field(body, "output_config") {
        object(output)?;
        request.reasoning.effort = string(output, "effort")?;
        if let Some(value) = field(output, "format") {
            request.format = parse_format(value)?;
        }
    }
    if let Some(value) = field(body, "output_format") {
        request.format = parse_format(value)?;
    }
    if let Some(tools) = field(body, "tools") {
        for tool in array(tools)? {
            object(tool)?;
            if let Some(kind) = string(tool, "type")? {
                if kind != "custom" {
                    request.tools.push(native_tool(tool, &kind)?);
                    continue;
                }
            }
            request.tools.push(function(tool, "input_schema")?);
        }
    }
    if let Some(choice) = field(body, "tool_choice") {
        object(choice)?;
        request.tool_choice = Some(match required_string(choice, "type")?.as_str() {
            "auto" => ToolChoice::Auto,
            "none" => ToolChoice::None,
            "any" => ToolChoice::Required,
            "tool" => ToolChoice::Named(required_string(choice, "name")?),
            _ => return Err(invalid("Invalid tool choice")),
        });
        request.parallel = boolean(choice, "disable_parallel_tool_use")?.map(|value| !value);
    }
    for message in array(&body["messages"])? {
        object(message)?;
        request.messages.push(Message {
            role: role(message, false)?,
            parts: anthropic_parts(&message["content"])?,
        });
    }
    Ok(())
}

#[derive(Default)]
struct Calls {
    pending: HashMap<String, VecDeque<String>>,
    used: HashSet<String>,
    sequence: usize,
}
impl Calls {
    fn allocate(&mut self) -> String {
        loop {
            let id = format!("call_gemini_{}", self.sequence);
            self.sequence += 1;
            if self.used.insert(id.clone()) {
                return id;
            }
        }
    }
    fn call(&mut self, name: &str, id: Option<String>) -> String {
        let id = id.unwrap_or_else(|| self.allocate());
        self.used.insert(id.clone());
        self.pending
            .entry(name.into())
            .or_default()
            .push_back(id.clone());
        id
    }
    fn result(&mut self, name: &str, id: Option<String>) -> String {
        if let Some(id) = id {
            if let Some(queue) = self.pending.get_mut(name) {
                queue.retain(|value| value != &id);
            }
            return id;
        }
        self.pending
            .get_mut(name)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| {
                // A result may be the first retained item of a trimmed native history.
                self.allocate()
            })
    }
}
fn gemini_parts(value: &Value, calls: &mut Calls) -> Result<Vec<Part>, ProxyError> {
    let mut parts = Vec::new();
    for block in array(value)? {
        object(block)?;
        let signature = string(block, "thoughtSignature")?;
        if let Some(value) = string(block, "text")? {
            if boolean(block, "thought")?.unwrap_or(false) {
                parts.push(thinking(value, signature.clone(), Protocol::Gemini, None));
            } else {
                parts.push(Part::Text {
                    text: value,
                    signature: signature.clone(),
                });
            }
        }
        if let Some(source) = field(block, "inlineData").or_else(|| field(block, "inline_data")) {
            object(source)?;
            let media_type = string(source, "mimeType")?
                .or(string(source, "mime_type")?)
                .unwrap_or_default();
            let data = Some(required_string(source, "data")?);
            if media_type.starts_with("image/") {
                parts.push(Part::Image {
                    media_type,
                    data,
                    url: None,
                    detail: None,
                });
            } else {
                parts.push(Part::File {
                    media_type,
                    data,
                    url: None,
                    id: None,
                    name: None,
                });
            }
        }
        if let Some(source) = field(block, "fileData").or_else(|| field(block, "file_data")) {
            object(source)?;
            let media_type = string(source, "mimeType")?
                .or(string(source, "mime_type")?)
                .unwrap_or_default();
            let url = string(source, "fileUri")?
                .or(string(source, "file_uri")?)
                .ok_or_else(|| invalid("Missing file URL"))?;
            if media_type.starts_with("image/") {
                parts.push(Part::Image {
                    media_type,
                    data: None,
                    url: Some(url),
                    detail: None,
                });
            } else {
                parts.push(Part::File {
                    media_type,
                    data: None,
                    url: Some(url),
                    id: None,
                    name: None,
                });
            }
        }
        if let Some(call) = field(block, "functionCall") {
            object(call)?;
            let name = required_string(call, "name")?;
            let id = calls.call(&name, string(call, "id")?);
            parts.push(Part::ToolCall {
                id,
                name,
                arguments: arguments(call.get("args"), false)?,
                signature: signature.clone(),
            });
        }
        if let Some(result) = field(block, "functionResponse") {
            object(result)?;
            let name = required_string(result, "name")?;
            let call_id = calls.result(&name, string(result, "id")?);
            let response = result.get("response").unwrap_or(&Value::Null);
            object(response)?;
            let mut output = vec![text(&response.to_string())];
            if let Some(images) = field(result, "parts") {
                output.extend(gemini_parts(images, calls)?);
            }
            parts.push(Part::ToolResult {
                call_id,
                name: Some(name),
                parts: output,
                is_error: response.get("error").is_some(),
            });
        }
    }
    Ok(parts)
}
fn gemini(body: &Value, request: &mut Request) -> Result<(), ProxyError> {
    let mut calls = Calls::default();
    if let Some(system) =
        field(body, "systemInstruction").or_else(|| field(body, "system_instruction"))
    {
        object(system)?;
        request.system = gemini_parts(&system["parts"], &mut calls)?;
    }
    if let Some(config) =
        field(body, "generationConfig").or_else(|| field(body, "generation_config"))
    {
        object(config)?;
        request.max_tokens =
            unsigned(config, "maxOutputTokens")?.or(unsigned(config, "max_output_tokens")?);
        request.temperature = number(config, "temperature")?;
        request.top_p = number(config, "topP")?.or(number(config, "top_p")?);
        request.native.top_k = number(config, "topK")?.or(number(config, "top_k")?);
        request.stop = strings(
            config
                .get("stopSequences")
                .or_else(|| config.get("stop_sequences"))
                .unwrap_or(&Value::Null),
        )?;
        request.native.image_config = field(config, "imageConfig").cloned();
        request.native.modalities = field(config, "responseModalities")
            .or_else(|| field(config, "response_modalities"))
            .cloned();
        if let Some(value) = &request.native.modalities {
            strings(value)?;
        }
        let mime = string(config, "responseMimeType")?;
        if mime.as_deref() == Some("application/json") {
            let schema = field(config, "responseJsonSchema")
                .or_else(|| field(config, "responseSchema"))
                .map(|v| self::schema(Some(v)))
                .transpose()?;
            request.format = Some(Format {
                kind: if schema.is_some() {
                    "json_schema"
                } else {
                    "json_object"
                }
                .into(),
                name: None,
                schema,
                strict: None,
            });
        }
        if let Some(thinking) =
            field(config, "thinkingConfig").or_else(|| field(config, "thinking_config"))
        {
            object(thinking)?;
            request.reasoning.effort = string(thinking, "thinkingLevel")?
                .or(string(thinking, "thinking_level")?)
                .map(|v| v.to_ascii_lowercase());
            request.reasoning.budget =
                signed(thinking, "thinkingBudget")?.or(signed(thinking, "thinking_budget")?);
            request.reasoning.enabled = request
                .reasoning
                .budget
                .map(|v| v != 0)
                .or_else(|| request.reasoning.effort.as_deref().map(|v| v != "none"));
            request.reasoning.visible = boolean(thinking, "includeThoughts")?
                .or(boolean(thinking, "include_thoughts")?)
                .unwrap_or(false);
        }
    }
    request.native.safety_settings = field(body, "safetySettings")
        .or_else(|| field(body, "safety_settings"))
        .cloned();
    if let Some(tools) = field(body, "tools") {
        for tool in array(tools)? {
            object(tool)?;
            if let Some(declarations) =
                field(tool, "functionDeclarations").or_else(|| field(tool, "function_declarations"))
            {
                for definition in array(declarations)? {
                    let key = if definition.get("parametersJsonSchema").is_some() {
                        "parametersJsonSchema"
                    } else {
                        "parameters"
                    };
                    request.tools.push(function(definition, key)?);
                }
            }
            for kind in [
                "googleSearch",
                "googleSearchRetrieval",
                "google_search",
                "codeExecution",
                "urlContext",
            ] {
                if field(tool, kind).is_some() {
                    request
                        .tools
                        .push(native_tool(&json!({kind:tool[kind]}), kind)?);
                }
            }
        }
    }
    if let Some(config) = field(body, "toolConfig").or_else(|| field(body, "tool_config")) {
        object(config)?;
        if let Some(function) = field(config, "functionCallingConfig")
            .or_else(|| field(config, "function_calling_config"))
        {
            object(function)?;
            let names = strings(
                function
                    .get("allowedFunctionNames")
                    .or_else(|| function.get("allowed_function_names"))
                    .unwrap_or(&Value::Null),
            )?;
            request.tool_choice = match string(function, "mode")?.as_deref() {
                None | Some("AUTO" | "VALIDATED") => Some(ToolChoice::Auto),
                Some("NONE") => Some(ToolChoice::None),
                Some("ANY") if names.len() == 1 => Some(ToolChoice::Named(names[0].clone())),
                Some("ANY") => Some(ToolChoice::Required),
                _ => return Err(invalid("Invalid function calling mode")),
            };
        }
    }
    for message in array(&body["contents"])? {
        object(message)?;
        request.messages.push(Message {
            role: role(message, true)?,
            parts: gemini_parts(&message["parts"], &mut calls)?,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_history_annotations_and_host_bookkeeping_do_not_enter_ir() {
        let parsed = request(Protocol::Chat,&json!({"model":"synthetic","messages":[
            {"role":"assistant","agent":"synthetic","conversationRequestId":"synthetic",
             "messageId":"synthetic","model":"synthetic","rawUsage":{},"requestModelId":"synthetic",
             "requestModelName":"synthetic","stepSeq":1,"traceId":"synthetic","usage":{},
             "content":[{"type":"text","text":"Synthetic answer","annotations":[{"synthetic":true}]}]}
        ]}),"/v1/chat/completions").unwrap();
        assert_eq!(
            parsed.messages,
            vec![Message {
                role: "assistant".into(),
                parts: vec![text("Synthetic answer")]
            }]
        );
    }

    #[test]
    fn media_files_detail_and_native_tool_output_images_survive_parsing() {
        let chat = request(Protocol::Chat,&json!({"model":"synthetic","messages":[{"role":"user","content":[
            {"type":"image_url","image_url":{"url":"data:image/png;base64,c3ludGhldGlj","detail":"high"}},
            {"type":"image_url","image_url":{"url":"https://synthetic.invalid/image.png","detail":"low"}},
            {"type":"file","file":{"file_data":"c3ludGhldGlj","mime_type":"application/pdf","filename":"synthetic.pdf"}},
            {"type":"file","file":{"file_id":"synthetic-file"}}
        ]}]}),"/v1/chat/completions").unwrap();
        assert!(
            matches!(&chat.messages[0].parts[0],Part::Image {media_type,data:Some(data),detail:Some(detail),..} if media_type=="image/png" && data=="c3ludGhldGlj" && detail=="high")
        );
        assert!(
            matches!(&chat.messages[0].parts[1],Part::Image {url:Some(url),detail:Some(detail),..} if url=="https://synthetic.invalid/image.png" && detail=="low")
        );
        assert!(
            matches!(&chat.messages[0].parts[2],Part::File {media_type,data:Some(_),name:Some(name),..} if media_type=="application/pdf" && name=="synthetic.pdf")
        );
        assert!(
            matches!(&chat.messages[0].parts[3],Part::File {id:Some(id),..} if id=="synthetic-file")
        );
        let anthropic = request(Protocol::Anthropic,&json!({"model":"synthetic","messages":[{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"synthetic-call","is_error":true,"content":[
                {"type":"text","text":"Synthetic output"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"c3ludGhldGlj"}}
            ]}
        ]}]}),"/v1/messages").unwrap();
        assert!(
            matches!(&anthropic.messages[0].parts[0],Part::ToolResult{is_error:true,parts,..} if matches!(parts[1],Part::Image{..}))
        );
        let gemini = request(Protocol::Gemini,&json!({"contents":[{"role":"user","parts":[
            {"fileData":{"mimeType":"application/pdf","fileUri":"https://synthetic.invalid/file.pdf"}},
            {"functionResponse":{"name":"lookup","id":"synthetic-call","response":{"output":"Synthetic output"},"parts":[
                {"inlineData":{"mimeType":"image/png","data":"c3ludGhldGlj"}}
            ]}}
        ]}]}),"/v1beta/models/synthetic:generateContent").unwrap();
        assert!(
            matches!(&gemini.messages[0].parts[0],Part::File{media_type,url:Some(_),..} if media_type=="application/pdf")
        );
        assert!(
            matches!(&gemini.messages[0].parts[1],Part::ToolResult{parts,..} if matches!(parts[1],Part::Image{..}))
        );
    }

    #[test]
    fn thinking_signatures_and_sealed_content_keep_their_origin() {
        let sealed = json!({"type":"reasoning","id":"synthetic-reasoning","summary":[{"type":"summary_text","text":"Synthetic summary"}],"encrypted_content":"synthetic-seal"});
        let response = request(
            Protocol::Responses,
            &json!({"model":"synthetic","input":[sealed]}),
            "/v1/responses",
        )
        .unwrap();
        assert!(
            matches!(&response.messages[0].parts[0],Part::Thinking{source:Protocol::Responses,sealed:Some(value),text,..} if value==&sealed && text=="Synthetic summary")
        );
        let anthro = request(Protocol::Anthropic,&json!({"model":"synthetic","messages":[{"role":"assistant","content":[
            {"type":"thinking","thinking":"Synthetic thought","signature":"synthetic-signature"},
            {"type":"redacted_thinking","data":"synthetic-seal"}
        ]}]}),"/v1/messages").unwrap();
        assert!(
            matches!(&anthro.messages[0].parts[0],Part::Thinking{source:Protocol::Anthropic,signature:Some(signature),..} if signature=="synthetic-signature")
        );
        assert!(
            matches!(&anthro.messages[0].parts[1],Part::Thinking{source:Protocol::Anthropic,sealed:Some(value),..} if value["data"]=="synthetic-seal")
        );
        let gemini = request(Protocol::Gemini,&json!({"contents":[{"role":"model","parts":[
            {"text":"Synthetic thought","thought":true,"thoughtSignature":"synthetic-thought-signature"},
            {"functionCall":{"name":"lookup","args":{}},"thoughtSignature":"synthetic-call-signature"},
            {"text":"Synthetic answer","thoughtSignature":"synthetic-text-signature"}
        ]}]}),"/v1beta/models/synthetic:generateContent").unwrap();
        assert!(
            matches!(&gemini.messages[0].parts[0],Part::Thinking{source:Protocol::Gemini,signature:Some(signature),..} if signature=="synthetic-thought-signature")
        );
        assert!(
            matches!(&gemini.messages[0].parts[1],Part::ToolCall{signature:Some(signature),..} if signature=="synthetic-call-signature")
        );
        assert!(
            matches!(&gemini.messages[0].parts[2],Part::Text{signature:Some(signature),..} if signature=="synthetic-text-signature")
        );
    }

    #[test]
    fn formats_native_options_tool_choices_and_strict_false_are_preserved() {
        let shape = json!({"type":"object","properties":{"agent":{"type":"string"}},"required":["agent"],"additionalProperties":false});
        let body = json!({"model":"synthetic","input":"Synthetic prompt","instructions":"Synthetic system",
            "reasoning":{"effort":"none","summary":"auto"},"parallel_tool_calls":false,
            "text":{"verbosity":"low","format":{"type":"json_schema","name":"synthetic","schema":shape,"strict":true}},
            "metadata":{"synthetic":"metadata"},"client_metadata":{"synthetic":"client"},"include":["reasoning.encrypted_content"],
            "access_programs":null,"prompt_cache_key":"synthetic-cache","prompt_cache_retention":"24h","service_tier":"priority",
            "tools":[{"type":"function","name":"lookup","strict":false,"parameters":shape,"defer_loading":true}],
            "tool_choice":{"type":"function","name":"lookup"}});
        let parsed = request(Protocol::Responses, &body, "/v1/responses").unwrap();
        assert_eq!(parsed.reasoning.enabled, Some(false));
        assert_eq!(parsed.reasoning.summary.as_deref(), Some("auto"));
        assert_eq!(parsed.parallel, Some(false));
        assert_eq!(
            parsed.format.as_ref().unwrap().schema.as_ref(),
            Some(&shape)
        );
        assert_eq!(parsed.native.text, Some(body["text"].clone()));
        assert_eq!(parsed.native.access_programs, Some(Value::Null));
        assert_eq!(parsed.native.cache_retention.as_deref(), Some("24h"));
        assert_eq!(parsed.native.service_tier.as_deref(), Some("priority"));
        assert_eq!(parsed.tools[0].strict, Some(false));
        assert!(parsed.tools[0].deferred);
        assert_eq!(parsed.tool_choice, Some(ToolChoice::Named("lookup".into())));
        let gemini_body = json!({"contents":[],"generationConfig":{"responseMimeType":"application/json","responseJsonSchema":shape,
            "thinkingConfig":{"thinkingBudget":-1,"includeThoughts":true},"imageConfig":{"aspectRatio":"16:9"},"responseModalities":["TEXT","IMAGE"],"topK":40},
            "safetySettings":[{"category":"synthetic","threshold":"synthetic"}]});
        let parsed = request(
            Protocol::Gemini,
            &gemini_body,
            "/v1beta/models/synthetic:generateContent",
        )
        .unwrap();
        assert_eq!(parsed.reasoning.budget, Some(-1));
        assert!(parsed.reasoning.visible);
        assert_eq!(parsed.native.top_k, Some(json!(40)));
        assert_eq!(parsed.native.modalities, Some(json!(["TEXT", "IMAGE"])));
        assert_eq!(
            parsed.native.image_config,
            Some(json!({"aspectRatio":"16:9"}))
        );
        assert_eq!(
            parsed.native.safety_settings,
            Some(gemini_body["safetySettings"].clone())
        );
    }

    #[test]
    fn native_tools_and_deferred_references_are_retained_for_target_validation() {
        let native = json!({"type":"web_search","external_web_access":false});
        let parsed = request(
            Protocol::Responses,
            &json!({"model":"synthetic","input":[],"tools":[native]}),
            "/v1/responses",
        )
        .unwrap();
        assert_eq!(parsed.tools[0].kind, "web_search");
        assert_eq!(parsed.tools[0].native, Some(native));
        let parsed = request(Protocol::Anthropic,&json!({"model":"synthetic","messages":[{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"synthetic","content":[{"type":"tool_reference","tool_name":"lookup"}]}
        ]}],"tools":[{"type":"web_search_20250305","name":"web_search","max_uses":3}]}),"/v1/messages").unwrap();
        assert_eq!(parsed.tools[0].kind, "web_search");
        assert!(
            matches!(&parsed.messages[0].parts[0],Part::ToolResult{parts,..} if parts==&vec![Part::ToolReference{name:"lookup".into()}])
        );
    }

    #[test]
    fn workbuddy_full_request_ignores_host_fields_and_preserves_nested_tool_schema() {
        let tools: Vec<_> = (0..23)
            .map(|i| {
                json!({"type":"function","function":{
                    "name":format!("synthetic_tool_{i}"),"description":"Synthetic tool",
                    "strict":false,"parameters":{"type":"object","properties":{
                        "agent":{"type":"string"},"conversationRequestId":{"type":"string"}
                    }},"vendor_bookkeeping":true
                }})
            })
            .collect();
        let body = json!({"model":"gemini-3.6-flash","messages":[
            {"role":"system","content":[{"type":"text","text":"Synthetic system"}]},
            {"role":"user","content":[{"type":"text","text":"Synthetic prompt"}],
             "agent":"synthetic-agent","conversationRequestId":"synthetic-request",
             "rootRequestId":"synthetic-root","startsNewUserRequest":true},
            {"role":"assistant","content":null,"reasoning_content":null},
            {"role":"assistant","content":"Synthetic answer","reasoning_content":"Synthetic thought"}
        ],"tools":tools,"stream":true,"stream_options":{"include_usage":true},
        "reasoning_effort":"high","vendor_request_id":"synthetic"});
        let parsed = request(Protocol::Chat, &body, "/v1/chat/completions").unwrap();
        assert_eq!(parsed.tools.len(), 23);
        assert_eq!(parsed.tools[0].strict, Some(false));
        assert_eq!(
            parsed.tools[0].schema,
            body["tools"][0]["function"]["parameters"]
        );
        assert_eq!(parsed.reasoning.effort.as_deref(), Some("high"));
        assert!(parsed.stream);
        assert!(parsed
            .messages
            .last()
            .unwrap()
            .parts
            .iter()
            .any(|p| matches!(p,
            Part::Thinking { text, .. } if text == "Synthetic thought")));
    }

    #[test]
    fn four_protocols_parse_equivalent_text_tools_and_controls() {
        let fixtures = [
            (
                Protocol::Chat,
                json!({"model":"synthetic-model","messages":[{"role":"user","content":"Synthetic prompt"}],
                "max_completion_tokens":512,"temperature":0.3,"top_p":0.8,"stop":["END"],
                "reasoning_effort":"high","tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"},"strict":true}}]}),
            ),
            (
                Protocol::Responses,
                json!({"model":"synthetic-model","input":[{"role":"user","content":[{"type":"input_text","text":"Synthetic prompt"}]}],
                "max_output_tokens":512,"temperature":0.3,"top_p":0.8,"stop":["END"],
                "reasoning":{"effort":"high"},"tools":[{"type":"function","name":"lookup","parameters":{"type":"object"},"strict":true}]}),
            ),
            (
                Protocol::Anthropic,
                json!({"model":"synthetic-model","messages":[{"role":"user","content":[{"type":"text","text":"Synthetic prompt"}]}],
                "max_tokens":512,"temperature":0.3,"top_p":0.8,"stop_sequences":["END"],
                "output_config":{"effort":"high"},"tools":[{"name":"lookup","input_schema":{"type":"object"},"strict":true}]}),
            ),
            (
                Protocol::Gemini,
                json!({"model":"synthetic-model","contents":[{"role":"user","parts":[{"text":"Synthetic prompt"}]}],
                "generationConfig":{"maxOutputTokens":512,"temperature":0.3,"topP":0.8,"stopSequences":["END"],"thinkingConfig":{"thinkingLevel":"HIGH"}},
                "tools":[{"functionDeclarations":[{"name":"lookup","parameters":{"type":"object"}}]}]}),
            ),
        ];
        for (protocol, body) in fixtures {
            let parsed = request(protocol, &body, "/v1/chat/completions").unwrap();
            assert_eq!(parsed.model, "synthetic-model");
            assert_eq!(
                parsed.messages[0].parts,
                vec![Part::Text {
                    text: "Synthetic prompt".into(),
                    signature: None
                }]
            );
            assert_eq!(parsed.max_tokens, Some(512));
            assert_eq!(parsed.temperature, Some(json!(0.3)));
            assert_eq!(parsed.top_p, Some(json!(0.8)));
            assert_eq!(parsed.stop, ["END"]);
            assert_eq!(parsed.reasoning.effort.as_deref(), Some("high"));
            assert_eq!(parsed.tools[0].name, "lookup");
            assert_eq!(parsed.tools[0].schema, json!({"type":"object"}));
        }
    }

    #[test]
    fn gemini_name_only_parallel_results_match_call_ids_in_order() {
        let parsed = request(Protocol::Gemini, &json!({"contents":[
            {"role":"model","parts":[{"functionCall":{"name":"lookup","args":{"n":1}}},{"functionCall":{"name":"lookup","args":{"n":2}}}]},
            {"role":"user","parts":[{"functionResponse":{"name":"lookup","response":{"text":"First"}}},{"functionResponse":{"name":"lookup","response":{"text":"Second"}}}]}
        ]}), "/v1beta/models/synthetic-model:streamGenerateContent?alt=sse").unwrap();
        assert_eq!(parsed.model, "synthetic-model");
        assert!(parsed.stream);
        let calls: Vec<_> = parsed.messages[0]
            .parts
            .iter()
            .map(|p| match p {
                Part::ToolCall { id, .. } => id.clone(),
                _ => panic!("expected call"),
            })
            .collect();
        let results: Vec<_> = parsed.messages[1]
            .parts
            .iter()
            .map(|p| match p {
                Part::ToolResult { call_id, .. } => call_id.clone(),
                _ => panic!("expected result"),
            })
            .collect();
        assert_eq!(calls, results);
        assert_ne!(calls[0], calls[1]);
    }

    #[test]
    fn malformed_known_fields_return_static_errors_without_payload_contents() {
        for (protocol, body) in [
            (
                Protocol::Chat,
                json!({"model":"synthetic","messages":"synthetic-private-value"}),
            ),
            (
                Protocol::Chat,
                json!({"model":"synthetic","messages":[],"reasoning_effort":123}),
            ),
            (
                Protocol::Responses,
                json!({"model":"synthetic","input":[],"max_output_tokens":-1}),
            ),
            (
                Protocol::Anthropic,
                json!({"model":"synthetic","messages":[{"role":"user","content":[{"type":"image","source":true}]}]}),
            ),
            (
                Protocol::Gemini,
                json!({"contents":[{"role":"model","parts":[{"functionCall":{"name":"lookup","args":"synthetic-private-value"}}]}]}),
            ),
        ] {
            let err =
                request(protocol, &body, "/v1beta/models/synthetic:generateContent").unwrap_err();
            assert!(!format!("{err:?}").contains("synthetic-private-value"));
        }
    }
}
