use super::types::{
    bounded_input_size, Correlation, ExecutionScope, IdentityQuality, InputEncoding, InputFidelity,
    InputStatus, ObservationContext, ObservationQuality, ObservationSource, ObservationStage,
    QualityOrigin, SourceTermination, ToolIdentity, ToolIdentityQuality, ToolInput,
    ToolObservation, MAX_ID_BYTES, MAX_INPUT_BYTES, MAX_NAME_BYTES,
};
use serde_json::Value;

pub const PROTOCOL_ANTHROPIC: &str = "anthropic_messages";
pub const PROTOCOL_OPENAI_CHAT: &str = "openai_chat";
pub const PROTOCOL_OPENAI_RESPONSES: &str = "openai_responses";
pub const PROTOCOL_GEMINI: &str = "gemini_generate_content";

#[derive(Debug, Clone, PartialEq)]
pub struct ParseOutcome {
    pub observations: Vec<ToolObservation>,
    pub recognized: bool,
}

#[derive(Clone)]
pub struct ParseContext {
    pub scope_id: String,
    pub client: Option<String>,
    pub execution_scope: ExecutionScope,
}

impl std::fmt::Debug for ParseContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ParseContext { <redacted> }")
    }
}

pub(crate) fn bounded_string(value: Option<&Value>, limit: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= limit)
        .map(str::to_string)
}

pub(crate) fn source_index(value: &Value, fallback: usize) -> Option<u64> {
    match value.get("index") {
        Some(index) => index.as_u64(),
        None => u64::try_from(fallback).ok(),
    }
}

pub(crate) fn termination(reason: Option<&Value>, normal: &[&str]) -> SourceTermination {
    match reason {
        None | Some(Value::Null) => SourceTermination::Unknown,
        Some(Value::String(reason)) if normal.contains(&reason.as_str()) => {
            SourceTermination::Normal
        }
        _ => SourceTermination::Abnormal,
    }
}

fn input(value: Option<&Value>, json_string: bool) -> ToolInput {
    let Some(value) = value else {
        return ToolInput {
            encoding: InputEncoding::Json,
            value: None,
            status: InputStatus::Invalid,
        };
    };

    if value.as_str().is_some_and(|s| s.len() > MAX_INPUT_BYTES)
        || (!json_string && bounded_input_size(value).is_err())
    {
        return ToolInput {
            encoding: InputEncoding::Json,
            value: None,
            status: InputStatus::Incomplete,
        };
    }
    if json_string {
        if let Some(raw) = value.as_str() {
            return match serde_json::from_str::<Value>(raw) {
                Ok(parsed) if bounded_input_size(&parsed).is_ok() => ToolInput {
                    encoding: InputEncoding::Json,
                    value: Some(parsed),
                    status: InputStatus::Complete,
                },
                Ok(_) => ToolInput {
                    encoding: InputEncoding::Json,
                    value: None,
                    status: InputStatus::Incomplete,
                },
                Err(_) => ToolInput {
                    encoding: InputEncoding::Text,
                    value: Some(value.clone()),
                    status: InputStatus::Invalid,
                },
            };
        }
        if bounded_input_size(value).is_err() {
            return ToolInput {
                encoding: InputEncoding::Json,
                value: None,
                status: InputStatus::Incomplete,
            };
        }
    }
    ToolInput {
        encoding: if value.is_string() {
            InputEncoding::Text
        } else {
            InputEncoding::Json
        },
        value: Some(value.clone()),
        status: InputStatus::Complete,
    }
}

pub fn parse_response(protocol: &str, body: &Value, ctx: &ParseContext) -> ParseOutcome {
    let carrier = match protocol {
        PROTOCOL_ANTHROPIC => "content",
        PROTOCOL_OPENAI_CHAT => "choices",
        PROTOCOL_OPENAI_RESPONSES => "output",
        PROTOCOL_GEMINI => "candidates",
        _ => {
            return ParseOutcome {
                observations: Vec::new(),
                recognized: false,
            }
        }
    };
    let Some(items) = body.get(carrier).and_then(Value::as_array) else {
        return ParseOutcome {
            observations: Vec::new(),
            recognized: false,
        };
    };
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut degraded = false;
    for (i, item) in items.iter().enumerate() {
        let (calls, candidate, term): (Vec<&Value>, Option<u64>, SourceTermination) = match protocol
        {
            PROTOCOL_ANTHROPIC => (
                vec![item],
                None,
                termination(
                    body.get("stop_reason"),
                    &["tool_use", "end_turn", "stop_sequence"],
                ),
            ),
            PROTOCOL_OPENAI_RESPONSES => (
                vec![item],
                None,
                termination(body.get("status"), &["completed"]),
            ),
            PROTOCOL_OPENAI_CHAT => (
                item.get("message")
                    .map(|message| {
                        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                            calls.iter().collect()
                        } else {
                            message.get("function_call").into_iter().collect()
                        }
                    })
                    .unwrap_or_default(),
                source_index(item, i),
                termination(
                    item.get("finish_reason"),
                    &["tool_calls", "stop", "function_call"],
                ),
            ),
            _ => (
                item.get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(Value::as_array)
                    .map(|v| v.iter().collect())
                    .unwrap_or_default(),
                source_index(item, i),
                termination(item.get("finishReason"), &["STOP"]),
            ),
        };
        for (ti, call) in calls.into_iter().enumerate() {
            let (tool, value, parse_json) = match protocol {
                PROTOCOL_ANTHROPIC
                    if call.get("type").and_then(Value::as_str) == Some("tool_use") =>
                {
                    (call, call.get("input"), false)
                }
                PROTOCOL_OPENAI_CHAT => {
                    let legacy = item.get("message").is_some_and(|message| {
                        message.get("tool_calls").is_none()
                            && message.get("function_call").is_some()
                    });
                    let function = if legacy {
                        call
                    } else {
                        let Some(function) = call.get("function") else {
                            continue;
                        };
                        function
                    };
                    (function, function.get("arguments"), true)
                }
                PROTOCOL_OPENAI_RESPONSES => match call.get("type").and_then(Value::as_str) {
                    Some("function_call") => (call, call.get("arguments"), true),
                    Some("custom_tool_call") => (call, call.get("input"), false),
                    _ => continue,
                },
                PROTOCOL_GEMINI => {
                    let Some(function) = call.get("functionCall") else {
                        continue;
                    };
                    (function, function.get("args"), false)
                }
                _ => continue,
            };
            if out.len() >= super::stream::MAX_PENDING_CALLS {
                degraded = true;
                break;
            }
            let name = bounded_string(tool.get("name"), MAX_NAME_BYTES);
            let invalid_namespace = tool
                .get("namespace")
                .is_some_and(|v| !v.is_null() && bounded_string(Some(v), MAX_NAME_BYTES).is_none());
            let invalid_chat_type = protocol == PROTOCOL_OPENAI_CHAT
                && call.get("type").is_some_and(|value| value != "function");
            let tool_identity = if !invalid_namespace
                && !invalid_chat_type
                && name.as_deref().is_some_and(|s| s != "unknown_tool")
            {
                ToolIdentityQuality::Source
            } else {
                ToolIdentityQuality::Unknown
            };
            let call_id = bounded_string(
                if protocol == PROTOCOL_OPENAI_RESPONSES {
                    call.get("call_id")
                } else if protocol == PROTOCOL_GEMINI {
                    tool.get("id")
                } else {
                    call.get("id")
                },
                MAX_ID_BYTES,
            );
            let identity_quality = if call_id.is_some() {
                IdentityQuality::SourceId
            } else {
                IdentityQuality::Synthetic
            };
            let mut input = input(value, parse_json);
            if invalid_namespace {
                input.status = InputStatus::Invalid;
                input.value = None;
            }
            degraded |= input.status == InputStatus::Incomplete;
            if let Some(value) = &input.value {
                total =
                    total.saturating_add(bounded_input_size(value).unwrap_or(MAX_INPUT_BYTES + 1));
            }
            degraded |= total > super::stream::MAX_RESPONSE_INPUT_BYTES;
            let term = if body.get("error").is_some_and(|v| !v.is_null())
                || item.get("error").is_some_and(|v| !v.is_null())
                || (protocol == PROTOCOL_OPENAI_RESPONSES
                    && call.get("status").is_some_and(|v| v != "completed"))
            {
                SourceTermination::Abnormal
            } else {
                term
            };
            if term != SourceTermination::Normal && input.status == InputStatus::Complete {
                input.status = InputStatus::Incomplete;
            }
            let tool_index =
                if protocol == PROTOCOL_ANTHROPIC || protocol == PROTOCOL_OPENAI_RESPONSES {
                    i as u64
                } else {
                    source_index(call, ti).unwrap_or(u64::MAX)
                };
            out.push(ToolObservation {
                schema_version: 1,
                observation_id: format!("{protocol}:{}:{}", ctx.scope_id, out.len()),
                source: ObservationSource::ModelResponse,
                stage: ObservationStage::Proposed,
                execution_scope: ctx.execution_scope,
                protocol: protocol.into(),
                scope_id: ctx.scope_id.clone(),
                correlation: Correlation {
                    call_id,
                    item_id: if protocol == PROTOCOL_OPENAI_RESPONSES {
                        bounded_string(call.get("id"), MAX_ID_BYTES)
                    } else {
                        None
                    },
                    candidate_index: candidate.and_then(|v| u32::try_from(v).ok()),
                    tool_index: u32::try_from(tool_index).ok(),
                    identity_quality: Some(identity_quality),
                    ..Default::default()
                },
                tool: ToolIdentity {
                    client: ctx.client.clone(),
                    name: name.unwrap_or_else(|| "unknown_tool".into()),
                    namespace: bounded_string(tool.get("namespace"), MAX_NAME_BYTES),
                    server_id: None,
                    definition_ref: None,
                },
                input,
                quality: ObservationQuality {
                    origin: QualityOrigin::Native,
                    source_termination: term,
                    tool_identity,
                    input_fidelity: InputFidelity::Preserved,
                },
                context: ObservationContext::default(),
            });
        }
    }
    if degraded {
        for observation in &mut out {
            observation.input.value = None;
            observation.input.status = InputStatus::Incomplete;
            observation.quality.input_fidelity = InputFidelity::RepairedOrDropped;
        }
    }
    ParseOutcome {
        observations: out,
        recognized: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn legacy_support_does_not_trust_flat_tool_calls() {
        let body = json!({"choices":[{"message":{"tool_calls":[{
            "name":"exec_command","arguments":"{\"cmd\":\"rm -rf /\"}"
        }]},"finish_reason":"tool_calls"}]});
        assert!(parse_response(PROTOCOL_OPENAI_CHAT, &body, &ctx())
            .observations
            .is_empty());
    }

    fn ctx() -> ParseContext {
        ParseContext {
            scope_id: "resp-1".into(),
            client: Some("claude".into()),
            execution_scope: ExecutionScope::Client,
        }
    }

    #[test]
    fn anthropic_single_tool_use() {
        let body = json!({
            "id": "msg_1",
            "stop_reason": "tool_use",
            "content": [
                {"type": "text", "text": "let me check"},
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}}
            ]
        });
        let out = parse_response(PROTOCOL_ANTHROPIC, &body, &ctx());
        assert!(out.recognized);
        assert_eq!(out.observations.len(), 1);
        let o = &out.observations[0];
        assert_eq!(o.tool.name, "Bash");
        assert_eq!(o.correlation.call_id.as_deref(), Some("toolu_1"));
        assert_eq!(
            o.correlation.identity_quality,
            Some(IdentityQuality::SourceId)
        );
        assert_eq!(o.input.encoding, InputEncoding::Json);
        assert_eq!(o.input.value, Some(json!({"command": "ls"})));
        assert_eq!(o.input.status, InputStatus::Complete);
        assert!(o.validate().is_ok());
    }

    #[test]
    fn anthropic_no_tool_use_recognized_empty() {
        let body = json!({"content": [{"type": "text", "text": "hi"}]});
        let out = parse_response(PROTOCOL_ANTHROPIC, &body, &ctx());
        assert!(out.recognized);
        assert!(out.observations.is_empty());
    }

    #[test]
    fn anthropic_missing_content_unrecognized() {
        let body = json!({"foo": "bar"});
        let out = parse_response(PROTOCOL_ANTHROPIC, &body, &ctx());
        assert!(!out.recognized);
    }

    #[test]
    fn anthropic_missing_id_is_synthetic() {
        let body = json!({
            "content": [{"type": "tool_use", "name": "Read", "input": {"file_path": "a.txt"}}]
        });
        let out = parse_response(PROTOCOL_ANTHROPIC, &body, &ctx());
        assert_eq!(out.observations.len(), 1);
        assert_eq!(
            out.observations[0].correlation.identity_quality,
            Some(IdentityQuality::Synthetic)
        );
    }

    #[test]
    fn openai_chat_tool_calls() {
        let body = json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "read_file", "arguments": "{\"path\":\"a.txt\"}"}
                    }]
                }
            }]
        });
        let out = parse_response(PROTOCOL_OPENAI_CHAT, &body, &ctx());
        assert!(out.recognized);
        assert_eq!(out.observations.len(), 1);
        let o = &out.observations[0];
        assert_eq!(o.tool.name, "read_file");
        assert_eq!(o.input.value, Some(json!({"path": "a.txt"})));
        assert_eq!(o.input.status, InputStatus::Complete);
    }

    #[test]
    fn openai_chat_invalid_arguments_marked_invalid() {
        let body = json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {"name": "read_file", "arguments": "{not json"}
                    }]
                }
            }]
        });
        let out = parse_response(PROTOCOL_OPENAI_CHAT, &body, &ctx());
        assert_eq!(out.observations.len(), 1);
        assert_eq!(out.observations[0].input.status, InputStatus::Invalid);
        assert_eq!(out.observations[0].input.encoding, InputEncoding::Text);
    }

    #[test]
    fn openai_chat_multiple_choices_and_calls() {
        let body = json!({
            "choices": [
                {"message": {"tool_calls": [
                    {"id": "c1", "function": {"name": "A", "arguments": "{}"}},
                    {"id": "c2", "function": {"name": "B", "arguments": "{}"}}
                ]}},
                {"message": {"tool_calls": [
                    {"id": "c3", "function": {"name": "C", "arguments": "{}"}}
                ]}}
            ]
        });
        let out = parse_response(PROTOCOL_OPENAI_CHAT, &body, &ctx());
        assert_eq!(out.observations.len(), 3);
        assert_eq!(out.observations[0].correlation.candidate_index, Some(0));
        assert_eq!(out.observations[0].correlation.tool_index, Some(0));
        assert_eq!(out.observations[2].correlation.candidate_index, Some(1));
    }

    #[test]
    fn openai_responses_function_call() {
        let body = json!({
            "status": "completed",
            "output": [
                {"type": "message", "content": []},
                {"type": "function_call", "call_id": "call_1", "name": "shell", "arguments": "{\"cmd\":\"ls\"}"}
            ]
        });
        let out = parse_response(PROTOCOL_OPENAI_RESPONSES, &body, &ctx());
        assert!(out.recognized);
        assert_eq!(out.observations.len(), 1);
        let o = &out.observations[0];
        assert_eq!(o.tool.name, "shell");
        assert_eq!(o.correlation.call_id.as_deref(), Some("call_1"));
        assert_eq!(o.input.value, Some(json!({"cmd": "ls"})));
    }

    #[test]
    fn openai_responses_custom_tool_call_text() {
        let body = json!({
            "status": "completed",
            "output": [
                {"type": "custom_tool_call", "call_id": "call_2", "name": "apply_patch", "input": "*** Begin Patch\n..."}
            ]
        });
        let out = parse_response(PROTOCOL_OPENAI_RESPONSES, &body, &ctx());
        assert_eq!(out.observations.len(), 1);
        let o = &out.observations[0];
        assert_eq!(o.input.encoding, InputEncoding::Text);
        assert_eq!(o.input.status, InputStatus::Complete);
    }

    #[test]
    fn openai_responses_namespace_preserved() {
        let body = json!({
            "status": "completed",
            "output": [
                {"type": "function_call", "call_id": "c", "name": "run", "namespace": "mcp__srv", "arguments": "{}"}
            ]
        });
        let out = parse_response(PROTOCOL_OPENAI_RESPONSES, &body, &ctx());
        assert_eq!(
            out.observations[0].tool.namespace.as_deref(),
            Some("mcp__srv")
        );
    }

    #[test]
    fn gemini_function_call() {
        let body = json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [
                        {"text": "checking"},
                        {"functionCall": {"name": "run_shell", "args": {"command": "ls"}}}
                    ]
                }
            }]
        });
        let out = parse_response(PROTOCOL_GEMINI, &body, &ctx());
        assert!(out.recognized);
        assert_eq!(out.observations.len(), 1);
        let o = &out.observations[0];
        assert_eq!(o.tool.name, "run_shell");
        assert_eq!(o.input.value, Some(json!({"command": "ls"})));
    }

    #[test]
    fn gemini_multiple_candidates() {
        let body = json!({
            "candidates": [
                {"content": {"parts": [{"functionCall": {"name": "A", "args": {}}}]}},
                {"content": {"parts": [{"functionCall": {"name": "B", "args": {}}}]}}
            ]
        });
        let out = parse_response(PROTOCOL_GEMINI, &body, &ctx());
        assert_eq!(out.observations.len(), 2);
        assert_eq!(out.observations[1].correlation.candidate_index, Some(1));
    }

    #[test]
    fn unknown_protocol_unrecognized() {
        let out = parse_response("nope", &json!({}), &ctx());
        assert!(!out.recognized);
    }

    #[test]
    fn all_observations_validate() {
        let bodies = [
            (
                PROTOCOL_ANTHROPIC,
                json!({"content": [{"type": "tool_use", "id": "t", "name": "Bash", "input": {"command": "ls"}}]}),
            ),
            (
                PROTOCOL_OPENAI_CHAT,
                json!({"choices": [{"message": {"tool_calls": [{"id": "c", "function": {"name": "Bash", "arguments": "{\"command\":\"ls\"}"}}]}}]}),
            ),
            (
                PROTOCOL_OPENAI_RESPONSES,
                json!({"status": "completed",
            "output": [{"type": "function_call", "call_id": "c", "name": "Bash", "arguments": "{\"command\":\"ls\"}"}]}),
            ),
            (
                PROTOCOL_GEMINI,
                json!({"candidates": [{"content": {"parts": [{"functionCall": {"name": "Bash", "args": {"command": "ls"}}}]}}]}),
            ),
        ];
        for (proto, body) in bodies {
            let out = parse_response(proto, &body, &ctx());
            assert_eq!(out.observations.len(), 1, "protocol {proto}");
            assert!(out.observations[0].validate().is_ok(), "protocol {proto}");
        }
    }

    fn protocol_fixtures() -> Vec<(&'static str, Value)> {
        vec![
            (
                PROTOCOL_ANTHROPIC,
                json!({"content":[{"type":"tool_use","name":"Bash","input":{}}]}),
            ),
            (
                PROTOCOL_OPENAI_CHAT,
                json!({"choices":[{"index":7,"message":{"tool_calls":[{"function":{"name":"Bash","arguments":"{}"}}]}}]}),
            ),
            (
                PROTOCOL_OPENAI_RESPONSES,
                json!({"output":[{"type":"function_call","name":"Bash","arguments":"{}"}]}),
            ),
            (
                PROTOCOL_GEMINI,
                json!({"candidates":[{"index":9,"content":{"parts":[{"functionCall":{"name":"Bash","args":{}}}]}}]}),
            ),
        ]
    }

    #[test]
    fn regression_nonstream_requires_normal_terminal_evidence() {
        for (protocol, body) in protocol_fixtures() {
            let out = parse_response(protocol, &body, &ctx());
            assert_eq!(out.observations.len(), 1, "{protocol}");
            assert_eq!(
                out.observations[0].input.status,
                InputStatus::Incomplete,
                "{protocol}"
            );
            assert_ne!(
                out.observations[0].quality.source_termination,
                SourceTermination::Normal
            );
        }
    }

    #[test]
    fn regression_nonstream_abnormal_and_normal_terminal_reasons() {
        for (protocol, mut body) in protocol_fixtures() {
            let (bad, good) = match protocol {
                PROTOCOL_ANTHROPIC => ("max_tokens", "tool_use"),
                PROTOCOL_OPENAI_CHAT => ("length", "tool_calls"),
                PROTOCOL_OPENAI_RESPONSES => ("incomplete", "completed"),
                _ => ("MAX_TOKENS", "STOP"),
            };
            for (reason, expected) in [
                (bad, InputStatus::Incomplete),
                (good, InputStatus::Complete),
            ] {
                match protocol {
                    PROTOCOL_ANTHROPIC => body["stop_reason"] = json!(reason),
                    PROTOCOL_OPENAI_CHAT => body["choices"][0]["finish_reason"] = json!(reason),
                    PROTOCOL_OPENAI_RESPONSES => body["status"] = json!(reason),
                    _ => body["candidates"][0]["finishReason"] = json!(reason),
                }
                let out = parse_response(protocol, &body, &ctx());
                assert_eq!(
                    out.observations[0].input.status, expected,
                    "{protocol}: {reason}"
                );
            }
            body["error"] = json!({"message":"fixture-error"});
            assert_ne!(
                parse_response(protocol, &body, &ctx()).observations[0]
                    .input
                    .status,
                InputStatus::Complete
            );
        }
    }

    #[test]
    fn regression_nonstream_uses_source_candidate_indices() {
        for (protocol, body) in protocol_fixtures() {
            if protocol == PROTOCOL_OPENAI_CHAT || protocol == PROTOCOL_GEMINI {
                let expected = if protocol == PROTOCOL_OPENAI_CHAT {
                    7
                } else {
                    9
                };
                let out = parse_response(protocol, &body, &ctx());
                assert_eq!(
                    out.observations[0].correlation.candidate_index,
                    Some(expected)
                );
            }
        }
    }

    #[test]
    fn regression_missing_name_is_unknown_not_silently_dropped() {
        for (protocol, mut body) in protocol_fixtures() {
            let tool = match protocol {
                PROTOCOL_ANTHROPIC => &mut body["content"][0],
                PROTOCOL_OPENAI_CHAT => {
                    &mut body["choices"][0]["message"]["tool_calls"][0]["function"]
                }
                PROTOCOL_OPENAI_RESPONSES => &mut body["output"][0],
                _ => &mut body["candidates"][0]["content"]["parts"][0]["functionCall"],
            };
            tool.as_object_mut().unwrap().remove("name");
            let out = parse_response(protocol, &body, &ctx());
            assert_eq!(out.observations.len(), 1, "{protocol}");
            assert_eq!(
                out.observations[0].quality.tool_identity,
                ToolIdentityQuality::Unknown
            );
        }
    }

    #[test]
    fn regression_nonstream_input_budget_releases_oversized_values() {
        let body = json!({"status":"completed","output":[{"type":"custom_tool_call","name":"apply_patch","input":"x".repeat(256 * 1024 + 1)}]});
        let out = parse_response(PROTOCOL_OPENAI_RESPONSES, &body, &ctx());
        assert_eq!(out.observations[0].input.value, None);
        assert_ne!(out.observations[0].input.status, InputStatus::Complete);
        assert_eq!(
            out.observations[0].quality.input_fidelity,
            InputFidelity::RepairedOrDropped
        );
    }
}
