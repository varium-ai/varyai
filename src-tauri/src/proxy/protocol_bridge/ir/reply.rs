//! Replies enter and leave the shared IR directly; no vendor wire is used as an intermediate.
use super::{Part, Protocol, Reply, Usage};
use crate::proxy::providers::transform_codex_chat::{
    flatten_namespace_tool_name, response_tool_call_item_from_chat_name, CodexToolContext,
};
use crate::proxy::ProxyError;
use serde_json::{json, Value};

fn invalid() -> ProxyError {
    ProxyError::TransformError("Invalid upstream protocol reply".into())
}
fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}
fn optional(value: &Value) -> Option<String> {
    value.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}
fn required(value: &Value) -> Result<String, ProxyError> {
    value.as_str().map(str::to_owned).ok_or_else(invalid)
}
fn tool_name(value: &Value) -> Result<String, ProxyError> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(invalid)
}
fn call_id(value: &Value) -> String {
    optional(value).unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple()))
}
fn count(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .unwrap_or(0)
}
pub(super) fn prompt(usage: &Usage) -> u64 {
    usage
        .input
        .saturating_add(usage.cache_read)
        .saturating_add(usage.cache_write)
}
pub(super) fn usage(protocol: Protocol, value: &Value) -> Usage {
    let (whole, output, read, write, reasoning) = match protocol {
        Protocol::Anthropic => (
            count(&value["input_tokens"]),
            count(&value["output_tokens"]),
            count(&value["cache_read_input_tokens"]),
            count(&value["cache_creation_input_tokens"]),
            count(&value["reasoning_tokens"]),
        ),
        Protocol::Chat => (
            count(&value["prompt_tokens"]),
            count(&value["completion_tokens"]),
            count(&value["prompt_tokens_details"]["cached_tokens"])
                .max(count(&value["prompt_cache_hit_tokens"]))
                .max(count(&value["cached_tokens"]))
                .max(count(&value["cache_read_input_tokens"])),
            count(&value["prompt_tokens_details"]["cache_write_tokens"])
                .max(count(&value["cache_creation_input_tokens"])),
            count(&value["completion_tokens_details"]["reasoning_tokens"]),
        ),
        Protocol::Responses => (
            count(&value["input_tokens"]),
            count(&value["output_tokens"]),
            count(&value["input_tokens_details"]["cached_tokens"]),
            count(&value["input_tokens_details"]["cache_write_tokens"]),
            count(&value["output_tokens_details"]["reasoning_tokens"]),
        ),
        Protocol::Gemini => (
            count(&value["promptTokenCount"]),
            count(&value["candidatesTokenCount"])
                .saturating_add(count(&value["thoughtsTokenCount"])),
            count(&value["cachedContentTokenCount"]),
            0,
            count(&value["thoughtsTokenCount"]),
        ),
    };
    Usage {
        input: if protocol == Protocol::Anthropic {
            whole
        } else {
            whole.saturating_sub(read).saturating_sub(write)
        },
        output,
        cache_read: read,
        cache_write: write,
        reasoning,
    }
}
pub(super) fn render_usage(protocol: Protocol, u: &Usage) -> Value {
    let p = prompt(u);
    match protocol {
        Protocol::Anthropic => {
            json!({"input_tokens":u.input,"output_tokens":u.output,"cache_read_input_tokens":u.cache_read,"cache_creation_input_tokens":u.cache_write})
        }
        Protocol::Chat => {
            json!({"prompt_tokens":p,"completion_tokens":u.output,"total_tokens":p.saturating_add(u.output),"prompt_tokens_details":{"cached_tokens":u.cache_read,"cache_write_tokens":u.cache_write},"completion_tokens_details":{"reasoning_tokens":u.reasoning}})
        }
        Protocol::Responses => {
            json!({"input_tokens":p,"output_tokens":u.output,"total_tokens":p.saturating_add(u.output),"input_tokens_details":{"cached_tokens":u.cache_read,"cache_write_tokens":u.cache_write},"output_tokens_details":{"reasoning_tokens":u.reasoning}})
        }
        Protocol::Gemini => {
            json!({"promptTokenCount":p,"candidatesTokenCount":u.output.saturating_sub(u.reasoning),"thoughtsTokenCount":u.reasoning,"totalTokenCount":p.saturating_add(u.output),"cachedContentTokenCount":u.cache_read})
        }
    }
}
pub(super) fn stop(protocol: Protocol, value: &str) -> String {
    match protocol {
        Protocol::Chat => match value {
            "tool_calls" | "function_call" => "tool",
            "length" => "length",
            "content_filter" => "filter",
            _ => "stop",
        },
        Protocol::Anthropic => match value {
            "tool_use" => "tool",
            "max_tokens" | "model_context_window_exceeded" => "length",
            "refusal" => "filter",
            _ => "stop",
        },
        Protocol::Responses => match value {
            "max_output_tokens" => "length",
            "content_filter" => "filter",
            _ => "stop",
        },
        Protocol::Gemini => match value {
            "MAX_TOKENS" => "length",
            "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" => "filter",
            _ => "stop",
        },
    }
    .into()
}
pub(super) fn render_stop(protocol: Protocol, value: &str) -> &'static str {
    match (protocol, value) {
        (Protocol::Anthropic, "tool") => "tool_use",
        (Protocol::Anthropic, "length") => "max_tokens",
        (Protocol::Anthropic, "filter") => "refusal",
        (Protocol::Anthropic, _) => "end_turn",
        (Protocol::Chat, "tool") => "tool_calls",
        (Protocol::Chat, "length") => "length",
        (Protocol::Chat, "filter") => "content_filter",
        (Protocol::Chat, _) => "stop",
        (Protocol::Gemini, "length") => "MAX_TOKENS",
        (Protocol::Gemini, "filter") => "SAFETY",
        (Protocol::Gemini, _) => "STOP",
        (Protocol::Responses, "length") => "max_output_tokens",
        (Protocol::Responses, "filter") => "content_filter",
        (Protocol::Responses, _) => "completed",
    }
}
pub(super) fn arguments(value: &Value) -> Result<Value, ProxyError> {
    let value = if let Some(s) = value.as_str() {
        if s.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(s).map_err(|_| invalid())?
        }
    } else {
        value.clone()
    };
    if value.is_object() {
        Ok(value)
    } else {
        Err(invalid())
    }
}
pub(super) fn parse_part(protocol: Protocol, part: &Value) -> Result<Option<Part>, ProxyError> {
    let kind = part["type"].as_str().unwrap_or_default();
    Ok(match protocol {
        Protocol::Anthropic => match kind {
            "text" => Some(Part::Text {
                text: required(&part["text"])?,
                signature: None,
            }),
            "thinking" | "redacted_thinking" => Some(Part::Thinking {
                text: text(&part["thinking"]),
                signature: optional(&part["signature"]),
                source: protocol,
                sealed: (kind == "redacted_thinking").then(|| part.clone()),
            }),
            "tool_use" => Some(Part::ToolCall {
                id: call_id(&part["id"]),
                name: tool_name(&part["name"])?,
                arguments: arguments(&part["input"])?,
                signature: None,
            }),
            "image" => Some(Part::Image {
                media_type: text(&part["source"]["media_type"]),
                data: optional(&part["source"]["data"]),
                url: optional(&part["source"]["url"]),
                detail: None,
            }),
            "server_tool_use" if part["name"] == "web_search" => Some(Part::Search {
                query: text(&part["input"]["query"]),
                hits: Vec::new(),
                gemini: None,
            }),
            "web_search_tool_result" => Some(Part::Search {
                query: String::new(),
                hits: part["content"].as_array().cloned().ok_or_else(invalid)?,
                gemini: None,
            }),
            _ => return Err(invalid()),
        },
        Protocol::Responses => match kind {
            "output_text" | "text" | "refusal" => Some(Part::Text {
                text: required(part.get("text").unwrap_or(&part["refusal"]))?,
                signature: None,
            }),
            "function_call" if optional(&part["namespace"]).is_some() => {
                Some(Part::NamespacedToolCall {
                    id: call_id(part.get("call_id").unwrap_or(&part["id"])),
                    namespace: text(&part["namespace"]),
                    name: tool_name(&part["name"])?,
                    arguments: arguments(&part["arguments"])?,
                    signature: None,
                })
            }
            "custom_tool_call" => Some(Part::CustomToolCall {
                id: call_id(part.get("call_id").unwrap_or(&part["id"])),
                name: tool_name(&part["name"])?,
                input: required(&part["input"])?,
                namespace: optional(&part["namespace"]),
            }),
            "tool_search_call" => Some(Part::ToolCall {
                id: text(part.get("call_id").unwrap_or(&part["id"])),
                name: "tool_search".into(),
                arguments: arguments(&part["arguments"])?,
                signature: None,
            }),
            "function_call" => Some(Part::ToolCall {
                id: call_id(part.get("call_id").unwrap_or(&part["id"])),
                name: tool_name(&part["name"])?,
                arguments: if kind == "custom_tool_call" {
                    json!({"input":part["input"]})
                } else {
                    arguments(&part["arguments"])?
                },
                signature: None,
            }),
            "reasoning" => Some(Part::Thinking {
                text: part["summary"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|p| p["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default(),
                signature: None,
                source: protocol,
                sealed: Some(part.clone()),
            }),
            "image_generation_call" => optional(&part["result"]).map(|data| Part::Image {
                media_type: "image/png".into(),
                data: Some(data),
                url: None,
                detail: None,
            }),
            "web_search_call" => Some(Part::Search {
                query: text(&part["action"]["query"]),
                hits: part["action"]["sources"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
                gemini: None,
            }),
            _ => return Err(invalid()),
        },
        Protocol::Gemini => {
            let signature = optional(&part["thoughtSignature"]);
            if let Some(tool) = part.get("toolCall").or_else(|| part.get("toolResponse")) {
                if !matches!(
                    tool["toolType"].as_str(),
                    Some("GOOGLE_SEARCH_WEB" | "GOOGLE_SEARCH_IMAGE")
                ) {
                    return Err(invalid());
                }
                let query = match part.pointer("/toolCall/args/queries") {
                    None => String::new(),
                    Some(value) => value
                        .as_array()
                        .ok_or_else(invalid)?
                        .iter()
                        .map(required)
                        .collect::<Result<Vec<_>, _>>()?
                        .join("\n"),
                };
                Some(Part::Search {
                    query,
                    hits: Vec::new(),
                    gemini: Some(part.clone()),
                })
            } else if let Some(call) = part.get("functionCall") {
                Some(Part::ToolCall {
                    id: optional(&call["id"])
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple())),
                    name: tool_name(&call["name"])?,
                    arguments: arguments(call.get("args").unwrap_or(&json!({})))?,
                    signature,
                })
            } else if part.get("text").is_some() {
                if part["thought"] == true {
                    Some(Part::Thinking {
                        text: required(&part["text"])?,
                        signature,
                        source: protocol,
                        sealed: None,
                    })
                } else {
                    Some(Part::Text {
                        text: required(&part["text"])?,
                        signature,
                    })
                }
            } else if let Some(data) = part.get("inlineData") {
                Some(Part::Image {
                    media_type: text(&data["mimeType"]),
                    data: optional(&data["data"]),
                    url: None,
                    detail: None,
                })
            } else if let Some(file) = part.get("fileData") {
                Some(Part::File {
                    media_type: text(&file["mimeType"]),
                    data: None,
                    url: optional(&file["fileUri"]),
                    id: None,
                    name: None,
                })
            } else {
                return Err(invalid());
            }
        }
        Protocol::Chat => None,
    })
}
pub(crate) fn parse(protocol: Protocol, body: &Value) -> Result<Reply, ProxyError> {
    if !body.is_object() || body.get("error").is_some_and(|v| !v.is_null()) {
        return Err(invalid());
    }
    let body = if protocol == Protocol::Gemini {
        body.get("response").unwrap_or(body)
    } else {
        body
    };
    let mut result = Reply {
        id: text(&body["id"]),
        model: text(&body["model"]),
        parts: Vec::new(),
        stop: "stop".into(),
        usage: Usage::default(),
    };
    match protocol {
        Protocol::Chat => {
            if body["choices"].as_array().is_some_and(|a| a.len() != 1) {
                return Err(invalid());
            }
            let choice = body["choices"]
                .as_array()
                .and_then(|a| a.first())
                .ok_or_else(invalid)?;
            let msg = &choice["message"];
            if !msg.is_object()
                || msg
                    .get("content")
                    .is_some_and(|v| !v.is_null() && !v.is_string() && !v.is_array())
            {
                return Err(invalid());
            }
            if let Some(s) = msg["content"].as_str() {
                result.parts.push(Part::Text {
                    text: s.into(),
                    signature: None,
                });
            } else if let Some(parts) = msg["content"].as_array() {
                for part in parts {
                    if matches!(part["type"].as_str(), Some("text" | "output_text")) {
                        let s = part["text"].as_str().ok_or_else(invalid)?;
                        result.parts.push(Part::Text {
                            text: s.into(),
                            signature: None,
                        });
                    } else if part["type"] == "image_url" {
                        let url = part["image_url"]["url"].as_str().ok_or_else(invalid)?;
                        let mut image = Part::Image {
                            media_type: "image/*".into(),
                            data: None,
                            url: Some(url.into()),
                            detail: optional(&part["image_url"]["detail"]),
                        };
                        if let Some((head, data)) = url
                            .strip_prefix("data:")
                            .and_then(|s| s.split_once(";base64,"))
                        {
                            image = Part::Image {
                                media_type: head.into(),
                                data: Some(data.into()),
                                url: None,
                                detail: optional(&part["image_url"]["detail"]),
                            };
                        }
                        result.parts.push(image);
                    } else {
                        return Err(invalid());
                    }
                }
            }
            for key in ["reasoning_content", "reasoning"] {
                if let Some(s) = msg[key].as_str().filter(|s| !s.is_empty()) {
                    result.parts.insert(
                        0,
                        Part::Thinking {
                            text: s.into(),
                            signature: None,
                            source: protocol,
                            sealed: None,
                        },
                    );
                    break;
                }
            }
            let refused = msg["refusal"].as_str().filter(|s| !s.is_empty());
            if let Some(text) = refused {
                result.parts.push(Part::Text {
                    text: text.into(),
                    signature: None,
                });
            }
            if let Some(calls) = msg["tool_calls"].as_array() {
                for call in calls {
                    if call.get("type").is_some_and(|v| v != "function") {
                        return Err(invalid());
                    }
                    result.parts.push(Part::ToolCall {
                        id: call_id(&call["id"]),
                        name: tool_name(&call["function"]["name"])?,
                        arguments: arguments(&call["function"]["arguments"])?,
                        signature: optional(&call["extra_content"]["google"]["thought_signature"]),
                    });
                }
            } else if let Some(call) = msg.get("function_call").filter(|v| !v.is_null()) {
                result.parts.push(Part::ToolCall {
                    id: call_id(&call["id"]),
                    name: tool_name(&call["name"])?,
                    arguments: arguments(&call["arguments"])?,
                    signature: None,
                });
            }
            result.stop = stop(
                protocol,
                choice["finish_reason"].as_str().unwrap_or_default(),
            );
            if refused.is_some() {
                result.stop = "filter".into();
            }
            result.usage = usage(protocol, &body["usage"]);
        }
        Protocol::Anthropic => {
            for part in body["content"].as_array().ok_or_else(invalid)? {
                if let Some(part) = parse_part(protocol, part)? {
                    result.parts.push(part);
                }
            }
            result.stop = stop(protocol, body["stop_reason"].as_str().unwrap_or_default());
            result.usage = usage(protocol, &body["usage"]);
        }
        Protocol::Responses => {
            for item in body["output"].as_array().ok_or_else(invalid)? {
                if item["type"] == "message" {
                    for part in item["content"].as_array().ok_or_else(invalid)? {
                        if let Some(part) = parse_part(protocol, part)? {
                            result.parts.push(part);
                        }
                    }
                } else if let Some(part) = parse_part(protocol, item)? {
                    result.parts.push(part);
                }
            }
            result.stop = stop(
                protocol,
                body["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or_default(),
            );
            result.usage = usage(protocol, &body["usage"]);
        }
        Protocol::Gemini => {
            if body["candidates"].as_array().is_some_and(|a| a.len() > 1) {
                return Err(invalid());
            }
            let candidate = body["candidates"].as_array().and_then(|a| a.first());
            if let Some(candidate) = candidate {
                if let Some(parts) = candidate["content"]["parts"].as_array() {
                    for part in parts {
                        if let Some(part) = parse_part(protocol, part)? {
                            result.parts.push(part);
                        }
                    }
                }
                result.stop = stop(
                    protocol,
                    candidate["finishReason"].as_str().unwrap_or_default(),
                );
            } else if body["promptFeedback"]["blockReason"].is_string() {
                result.stop = "filter".into();
            } else {
                return Err(invalid());
            }
            result.id = text(&body["responseId"]);
            result.model = text(&body["modelVersion"]);
            result.usage = usage(protocol, &body["usageMetadata"]);
        }
    }
    if result.parts.iter().any(|p| {
        matches!(
            p,
            Part::ToolCall { .. } | Part::NamespacedToolCall { .. } | Part::CustomToolCall { .. }
        )
    }) && result.stop == "stop"
    {
        result.stop = "tool".into();
    }
    Ok(result)
}
pub(super) fn part(protocol: Protocol, p: &Part, index: usize) -> Option<Value> {
    if let Part::Search {
        query,
        hits,
        gemini,
    } = p
    {
        if protocol == Protocol::Gemini {
            if let Some(native) = gemini {
                return Some(native.clone());
            }
        }
        if protocol == Protocol::Responses {
            return Some(
                json!({"type":"web_search_call","id":format!("ws_{index}"),"status":"completed","action":{"type":"search","query":query,"sources":hits}}),
            );
        }
        let sources = hits
            .iter()
            .map(|h| {
                format!(
                    "{}: {}",
                    h["title"].as_str().unwrap_or_default(),
                    h["url"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return part(
            protocol,
            &Part::Text {
                text: format!("[Web search: {query}]\n{sources}"),
                signature: None,
            },
            index,
        );
    }
    if let Part::File {
        media_type, url, ..
    } = p
    {
        let text = if let Some(url) = url {
            format!("[attachment {media_type}: {url}]")
        } else {
            format!("[attachment {media_type}]")
        };
        return part(
            protocol,
            &Part::Text {
                text,
                signature: None,
            },
            index,
        );
    }
    if let Part::NamespacedToolCall {
        id,
        namespace,
        name,
        arguments,
        signature,
    } = p
    {
        if protocol == Protocol::Responses {
            return Some(
                json!({"id":format!("fc_{index}"),"type":"function_call","status":"completed","call_id":id,"name":name,"namespace":namespace,"arguments":arguments.to_string()}),
            );
        }
        return part(
            protocol,
            &Part::ToolCall {
                id: id.clone(),
                name: flatten_namespace_tool_name(namespace, name),
                arguments: arguments.clone(),
                signature: signature.clone(),
            },
            index,
        );
    }
    if let Part::CustomToolCall {
        id,
        name,
        input,
        namespace,
    } = p
    {
        if protocol == Protocol::Responses {
            let mut item = json!({"id":format!("ctc_{index}"),"type":"custom_tool_call","status":"completed","call_id":id,"name":name,"input":input});
            if let Some(namespace) = namespace {
                item["namespace"] = json!(namespace);
            }
            return Some(item);
        }
        return part(
            protocol,
            &Part::ToolCall {
                id: id.clone(),
                name: namespace
                    .as_ref()
                    .map(|ns| flatten_namespace_tool_name(ns, name))
                    .unwrap_or_else(|| name.clone()),
                arguments: json!({"input":input}),
                signature: None,
            },
            index,
        );
    }
    match (protocol, p) {
        (Protocol::Anthropic, Part::Text { text, .. }) => Some(json!({"type":"text","text":text})),
        (
            Protocol::Anthropic,
            Part::Thinking {
                text,
                signature,
                source,
                sealed,
            },
        ) => {
            if *source == protocol && sealed.is_some() {
                return sealed.clone();
            }
            if text.is_empty() && (*source != protocol || signature.is_none()) {
                return None;
            }
            Some(
                json!({"type":"thinking","thinking":text,"signature":if *source==protocol {signature.as_deref().unwrap_or_default()}else{""}}),
            )
        }
        (
            Protocol::Anthropic,
            Part::ToolCall {
                id,
                name,
                arguments,
                ..
            },
        ) => Some(json!({"type":"tool_use","id":id,"name":name,"input":arguments})),
        (
            Protocol::Anthropic,
            Part::Image {
                media_type,
                data,
                url,
                ..
            },
        ) => Some(
            json!({"type":"image","source":if let Some(data)=data{json!({"type":"base64","media_type":media_type,"data":data})}else{json!({"type":"url","url":url})}}),
        ),
        (Protocol::Responses, Part::Text { text, .. }) => Some(
            json!({"id":format!("msg_{index}"),"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]}),
        ),
        (
            Protocol::Responses,
            Part::Thinking {
                text,
                source,
                sealed,
                ..
            },
        ) => {
            if *source == protocol && sealed.is_some() {
                return sealed.clone();
            }
            if text.is_empty() {
                return None;
            }
            Some(
                json!({"id":format!("rs_{index}"),"type":"reasoning","status":"completed","summary":[{"type":"summary_text","text":text}]}),
            )
        }
        (
            Protocol::Responses,
            Part::ToolCall {
                id,
                name,
                arguments,
                ..
            },
        ) => Some(
            json!({"id":format!("fc_{index}"),"type":"function_call","status":"completed","call_id":id,"name":name,"arguments":arguments.to_string()}),
        ),
        (
            Protocol::Responses,
            Part::Image {
                data: Some(data), ..
            },
        ) => Some(
            json!({"id":format!("ig_{index}"),"type":"image_generation_call","status":"completed","result":data}),
        ),
        (Protocol::Gemini, Part::Text { text, signature }) => {
            let mut p = json!({"text":text});
            if let Some(s) = signature {
                p["thoughtSignature"] = json!(s);
            }
            Some(p)
        }
        (
            Protocol::Gemini,
            Part::Thinking {
                text,
                signature,
                source,
                ..
            },
        ) => {
            if text.is_empty() && (*source != protocol || signature.is_none()) {
                return None;
            }
            let mut p = json!({"text":text,"thought":true});
            if *source == protocol {
                if let Some(s) = signature {
                    p["thoughtSignature"] = json!(s);
                }
            }
            Some(p)
        }
        (
            Protocol::Gemini,
            Part::ToolCall {
                id,
                name,
                arguments,
                signature,
            },
        ) => {
            let mut p = json!({"functionCall":{"id":id,"name":name,"args":arguments}});
            if let Some(s) = signature {
                p["thoughtSignature"] = json!(s);
            }
            Some(p)
        }
        (
            Protocol::Gemini,
            Part::Image {
                media_type,
                data: Some(data),
                ..
            },
        ) => Some(json!({"inlineData":{"mimeType":media_type,"data":data}})),
        (
            Protocol::Gemini,
            Part::Image {
                media_type,
                url: Some(url),
                ..
            },
        ) => Some(json!({"fileData":{"mimeType":media_type,"fileUri":url}})),
        _ => None,
    }
}

pub(crate) fn render_with_context(
    protocol: Protocol,
    reply: &Reply,
    context: &CodexToolContext,
) -> Result<Value, ProxyError> {
    let mut rendered = render(protocol, reply)?;
    if protocol == Protocol::Responses {
        let mut position = 0;
        for (index, p) in reply.parts.iter().enumerate() {
            if part(protocol, p, index).is_none() {
                continue;
            }
            if let Part::ToolCall {
                id,
                name,
                arguments,
                ..
            } = p
            {
                rendered["output"][position] = response_tool_call_item_from_chat_name(
                    &format!("fc_{index}"),
                    "completed",
                    id,
                    name,
                    &arguments.to_string(),
                    None,
                    context,
                );
            }
            position += 1;
        }
    }
    Ok(rendered)
}
pub(crate) fn render(protocol: Protocol, reply: &Reply) -> Result<Value, ProxyError> {
    if reply.parts.iter().any(|part| {
        matches!(
            part,
            Part::ToolResult { .. } | Part::CustomToolResult { .. } | Part::ToolReference { .. }
        ) || (protocol == Protocol::Chat && matches!(part, Part::Image { .. }))
    }) {
        return Err(invalid());
    }
    let usage = render_usage(protocol, &reply.usage);
    let blocks: Vec<_> = reply
        .parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| part(protocol, p, i))
        .collect();
    Ok(match protocol {
        Protocol::Anthropic => {
            json!({"id":reply.id,"type":"message","role":"assistant","model":reply.model,"content":blocks,"stop_reason":render_stop(protocol,&reply.stop),"stop_sequence":null,"usage":usage})
        }
        Protocol::Responses => {
            let incomplete = matches!(reply.stop.as_str(), "length" | "filter");
            json!({"id":reply.id,"object":"response","created_at":chrono::Utc::now().timestamp(),"model":reply.model,"status":if incomplete{"incomplete"}else{"completed"},"output":blocks,"usage":usage,"incomplete_details":if incomplete{json!({"reason":render_stop(protocol,&reply.stop)})}else{Value::Null},"error":null,"parallel_tool_calls":true,"tools":[],"tool_choice":"auto"})
        }
        Protocol::Gemini => {
            json!({"responseId":reply.id,"modelVersion":reply.model,"candidates":[{"index":0,"content":{"role":"model","parts":blocks},"finishReason":render_stop(protocol,&reply.stop)}],"usageMetadata":usage})
        }
        Protocol::Chat => {
            let mut content = String::new();
            let mut reasoning = String::new();
            let mut calls = Vec::new();
            for p in &reply.parts {
                match p{
                Part::Text{text,..}=>content.push_str(text),Part::Thinking{text,..}=>reasoning.push_str(text),
                Part::Search{query,hits,..}=>{content.push_str(&format!("[Web search: {query}]\n"));for hit in hits{content.push_str(&format!("{}: {}\n",hit["title"].as_str().unwrap_or_default(),hit["url"].as_str().unwrap_or_default()));}},
                Part::File{media_type,url,..}=>content.push_str(&if let Some(url)=url{format!("[attachment {media_type}: {url}]")}else{format!("[attachment {media_type}]")}),
                Part::NamespacedToolCall{id,namespace,name,arguments,..}=>calls.push(json!({"id":id,"type":"function","function":{"name":flatten_namespace_tool_name(namespace,name),"arguments":arguments.to_string()}})),
                Part::CustomToolCall{id,name,input,namespace}=>calls.push(json!({"id":id,"type":"function","function":{"name":namespace.as_ref().map(|ns|flatten_namespace_tool_name(ns,name)).unwrap_or_else(||name.clone()),"arguments":json!({"input":input}).to_string()}})),
                Part::ToolCall{id,name,arguments,..}=>calls.push(json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}})),_=>{}
            }
            }
            let mut message = json!({"role":"assistant","content":if content.is_empty(){Value::Null}else{json!(content)}});
            if !calls.is_empty() {
                message["tool_calls"] = json!(calls);
            }
            if !reasoning.is_empty() {
                message["reasoning_content"] = json!(reasoning);
            }
            json!({"id":reply.id,"object":"chat.completion","created":chrono::Utc::now().timestamp(),"model":reply.model,"choices":[{"index":0,"message":message,"finish_reason":render_stop(protocol,&reply.stop)}],"usage":usage})
        }
    })
}

#[cfg(test)]
pub(super) fn gemini_search_fixture() -> Value {
    json!({"responseId":"synthetic-search-response","modelVersion":"gemini-3.6-flash","candidates":[{"content":{"role":"model","parts":[
        {"toolCall":{"toolType":"GOOGLE_SEARCH_WEB","id":"synthetic-search","args":{"queries":["synthetic query"]}},"thoughtSignature":"synthetic-call-signature"},
        {"toolResponse":{"toolType":"GOOGLE_SEARCH_WEB","id":"synthetic-search","response":{"search_suggestions":"Synthetic result"}},"thoughtSignature":"synthetic-result-signature"},
        {"text":"Synthetic answer","thoughtSignature":"synthetic-answer-signature"}
    ]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":2}})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Reply {
        Reply {
            id: "synthetic-reply".into(),
            model: "synthetic-model".into(),
            parts: vec![
                Part::Thinking {
                    text: "Synthetic reasoning".into(),
                    signature: None,
                    source: Protocol::Chat,
                    sealed: None,
                },
                Part::Text {
                    text: "Synthetic answer".into(),
                    signature: None,
                },
                Part::ToolCall {
                    id: "call-synthetic".into(),
                    name: "inspect".into(),
                    arguments: json!({"value":"synthetic"}),
                    signature: None,
                },
            ],
            stop: "tool".into(),
            usage: Usage {
                input: 7,
                output: 5,
                cache_read: 3,
                cache_write: 2,
                reasoning: 1,
            },
        }
    }
    #[test]
    fn all_reply_protocol_edges_preserve_text_tools_reasoning_usage_and_finish() {
        let protocols = [
            Protocol::Chat,
            Protocol::Responses,
            Protocol::Anthropic,
            Protocol::Gemini,
        ];
        for source in protocols {
            let wire = render(source, &fixture()).unwrap();
            let parsed = parse(source, &wire).unwrap();
            for target in protocols {
                if source == target {
                    continue;
                }
                let output = render(target, &parsed).unwrap();
                let actual = parse(target, &output).unwrap();
                assert_eq!(actual.id, parsed.id);
                assert_eq!(actual.model, parsed.model);
                assert_eq!(actual.stop, "tool");
                assert_eq!(
                    actual.usage.input,
                    if source == Protocol::Gemini || target == Protocol::Gemini {
                        9
                    } else {
                        7
                    }
                );
                assert_eq!(actual.usage.cache_read, 3);
                assert_eq!(
                    actual.usage.cache_write,
                    if source == Protocol::Gemini || target == Protocol::Gemini {
                        0
                    } else {
                        2
                    }
                );
                assert!(actual
                    .parts
                    .iter()
                    .any(|p| matches!(p,Part::Text{text,..} if text=="Synthetic answer")));
                assert!(actual
                    .parts
                    .iter()
                    .any(|p| matches!(p,Part::Thinking{text,..} if text=="Synthetic reasoning")));
                assert!(actual.parts.iter().any(|p|matches!(p,Part::ToolCall{id,name,arguments,..} if id=="call-synthetic"&&name=="inspect"&&arguments["value"]=="synthetic")));
            }
        }
    }
    #[test]
    fn opaque_reasoning_never_crosses_vendor_boundaries() {
        let body = json!({"id":"synthetic","model":"synthetic","content":[{"type":"redacted_thinking","data":"opaque-synthetic"},{"type":"thinking","thinking":"visible","signature":"signed-synthetic"}],"stop_reason":"end_turn","usage":{}});
        let parsed = parse(Protocol::Anthropic, &body).unwrap();
        assert_eq!(
            render(Protocol::Anthropic, &parsed).unwrap()["content"],
            body["content"]
        );
        for target in [Protocol::Chat, Protocol::Responses, Protocol::Gemini] {
            let wire = render(target, &parsed).unwrap().to_string();
            assert!(!wire.contains("opaque-synthetic"));
            assert!(!wire.contains("signed-synthetic"));
            assert!(wire.contains("visible"));
        }
    }
    #[test]
    fn error_details_are_not_echoed() {
        let error = parse(
            Protocol::Chat,
            &json!({"error":{"message":"synthetic-private-value"}}),
        )
        .unwrap_err()
        .to_string();
        assert!(!error.contains("synthetic-private-value"));
    }
    #[test]
    fn multiple_choices_and_unknown_semantic_parts_are_rejected_without_echoing() {
        for (protocol, body) in [
            (
                Protocol::Chat,
                json!({"choices":[{"message":{"content":"a"}},{"message":{"content":"b"}}]}),
            ),
            (
                Protocol::Gemini,
                json!({"candidates":[{"content":{"parts":[{"text":"a"}]}},{"content":{"parts":[{"text":"b"}]}}]}),
            ),
            (
                Protocol::Anthropic,
                json!({"content":[{"type":"unsupported_semantic","text":"synthetic-private-detail"}]}),
            ),
            (
                Protocol::Responses,
                json!({"output":[{"type":"unsupported_semantic","text":"synthetic-private-detail"}]}),
            ),
        ] {
            let error = parse(protocol, &body).unwrap_err().to_string();
            assert!(!error.contains("synthetic-private-detail"));
        }
    }
    #[test]
    fn gemini_combined_server_search_is_not_a_client_function_call() {
        let input = gemini_search_fixture();
        let parsed = parse(Protocol::Gemini, &input).unwrap();
        assert_eq!(parsed.stop, "stop");
        let native = render(Protocol::Gemini, &parsed).unwrap();
        assert_eq!(
            native["candidates"][0]["content"]["parts"],
            input["candidates"][0]["content"]["parts"]
        );
        let client = render(Protocol::Responses, &parsed).unwrap();
        assert_eq!(client["output"][0]["type"], "web_search_call");
        assert_eq!(client["output"][0]["action"]["query"], "synthetic query");
        assert_eq!(client["output"][1]["type"], "web_search_call");
        assert!(!client.to_string().contains("synthetic-call-signature"));
        assert!(!client["output"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "function_call"));
        let mut invalid = input;
        invalid["candidates"][0]["content"]["parts"][0]["toolCall"]["toolType"] =
            json!("UNSUPPORTED_TOOL");
        assert!(parse(Protocol::Gemini, &invalid).is_err());
    }

    #[test]
    fn native_gemini_parallel_calls_without_ids_get_unique_ids_for_every_client() {
        let input = json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"first","args":{"value":1}}},{"functionCall":{"name":"second","args":{"value":2}}}]},"finishReason":"STOP"}]});
        let parsed = parse(Protocol::Gemini, &input).unwrap();
        let ids: Vec<_> = parsed
            .parts
            .iter()
            .filter_map(|p| {
                if let Part::ToolCall { id, .. } = p {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().all(|id| !id.is_empty()));
        assert_ne!(ids[0], ids[1]);
        for client in [Protocol::Chat, Protocol::Responses, Protocol::Anthropic] {
            let rendered = render(client, &parsed).unwrap();
            let reparsed = parse(client, &rendered).unwrap();
            let actual: Vec<_> = reparsed
                .parts
                .iter()
                .filter_map(|p| {
                    if let Part::ToolCall { id, .. } = p {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(actual, ids);
        }
    }
    #[test]
    fn responses_native_namespace_and_custom_tool_replies_keep_their_identity() {
        let input = json!({"id":"synthetic","model":"fixture","output":[{"type":"function_call","id":"fc1","call_id":"call-one","name":"read","namespace":"files","arguments":"{\"path\":\"synthetic\"}"},{"type":"custom_tool_call","id":"ctc1","call_id":"call-two","name":"apply_patch","input":"synthetic input\nsecond line"}],"usage":{}});
        let parsed = parse(Protocol::Responses, &input).unwrap();
        assert_eq!(parsed.stop, "tool");
        let rendered = render(Protocol::Responses, &parsed).unwrap();
        assert_eq!(rendered["output"][0]["namespace"], "files");
        assert_eq!(rendered["output"][1]["type"], "custom_tool_call");
        assert_eq!(
            rendered["output"][1]["input"],
            "synthetic input\nsecond line"
        );
        for target in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
            let output = render(target, &parsed).unwrap();
            let result = parse(target, &output).unwrap();
            assert!(result.parts.iter().any(|p|matches!(p,Part::ToolCall{name,arguments,..}if name=="files__read"&&arguments["path"]=="synthetic")));
            assert!(result.parts.iter().any(|p|matches!(p,Part::ToolCall{name,arguments,..}if name=="apply_patch"&&arguments["input"]=="synthetic input\nsecond line")));
        }
    }
    #[test]
    fn chat_refusal_with_null_content_keeps_visible_text_and_filter_finish() {
        let input = json!({"id":"synthetic","model":"fixture","choices":[{"message":{"role":"assistant","content":null,"refusal":"Synthetic refusal"},"finish_reason":"stop"}],"usage":{}});
        let parsed = parse(Protocol::Chat, &input).unwrap();
        assert_eq!(parsed.stop, "filter");
        for target in [Protocol::Anthropic, Protocol::Responses, Protocol::Gemini] {
            let output = render(target, &parsed).unwrap();
            let actual = parse(target, &output).unwrap();
            assert_eq!(actual.stop, "filter");
            assert!(actual
                .parts
                .iter()
                .any(|p| matches!(p,Part::Text{text,..}if text=="Synthetic refusal")));
        }
    }
}
