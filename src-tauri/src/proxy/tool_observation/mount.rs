//! Explicit client-output protocol and transport selection. Never infer protocol from app identity.
use super::channel::{
    start_response, OutputQuality, ResponseMetadata, ResponseTap, ToolSchemaRef, Transport,
};
#[cfg(test)]
use super::parsers::{PROTOCOL_OPENAI_CHAT, PROTOCOL_OPENAI_RESPONSES};
use crate::services::tool_risk::adapters::{
    CLAUDE_DESKTOP_TOOLS_SCHEMA, CLAUDE_TOOLS_SCHEMA, CODEX_TOOLS_SCHEMA, GEMINI_TOOLS_SCHEMA,
    OPENAI_TOOLS_SCHEMA, PI_TOOLS_SCHEMA,
};
use bytes::Bytes;
use serde_json::Value;
use std::sync::Arc;

pub fn metadata(
    protocol: &'static str,
    client: &'static str,
    transport: Transport,
    quality: OutputQuality,
    definitions: Arc<Vec<ToolSchemaRef>>,
) -> ResponseMetadata {
    ResponseMetadata {
        protocol,
        client,
        transport,
        quality,
        definitions,
    }
}
pub fn observe_non_streaming(bytes: &Bytes, metadata: ResponseMetadata) {
    if let Some(mut tap) = start_response(metadata) {
        tap.push(bytes);
    }
}
pub fn start_stream(metadata: ResponseMetadata) -> Option<ResponseTap> {
    start_response(metadata)
}

/// Observe immutable upstream bytes before a protocol converter can repair or
/// synthesize events. No parsing, waiting or database access in the forwarding path.
pub fn observe_stream(
    stream: impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    metadata: ResponseMetadata,
) -> impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    use futures::StreamExt;
    async_stream::stream! {
        let mut metadata = Some(metadata);
        let mut tap = None;
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            match &chunk {
                Ok(bytes) => {
                    if let Some(metadata) = metadata.take() { tap = start_response(metadata); }
                    if let Some(tap) = tap.as_mut() { tap.push(bytes); }
                }
                Err(_) => { if let Some(tap) = tap.as_ref() { tap.fail(); } }
            }
            yield chunk;
        }
    }
}

/// Only bounded, recognized request schemas can establish built-in semantics.
/// A schema reference is not authorization; namespaces/custom text remain unknown.
pub fn collect_tool_schemas(body: &Value, client: &str) -> Arc<Vec<ToolSchemaRef>> {
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return Arc::new(Vec::new());
    };
    collect_tool_schemas_from_tools(tools, client)
}

/// Borrow actual outbound definitions so size limits precede any payload copy.
pub(crate) fn collect_tool_schemas_from_tools(
    tools: &[Value],
    client: &str,
) -> Arc<Vec<ToolSchemaRef>> {
    let schema = match client {
        "claude" => CLAUDE_TOOLS_SCHEMA,
        "claude-desktop" => CLAUDE_DESKTOP_TOOLS_SCHEMA,
        "codex" => CODEX_TOOLS_SCHEMA,
        "gemini" => GEMINI_TOOLS_SCHEMA,
        "openai" => OPENAI_TOOLS_SCHEMA,
        "pi" => PI_TOOLS_SCHEMA,
        _ => return Arc::new(Vec::new()),
    };
    if tools.len() > 64 {
        return Arc::new(Vec::new());
    }
    let mut candidates = Vec::new();
    for tool in tools {
        let Some(declarations) = tool.get("functionDeclarations").and_then(Value::as_array) else {
            candidates.push(tool);
            continue;
        };
        if declarations.len() > 64 {
            return Arc::new(Vec::new());
        }
        candidates.extend(declarations.iter());
    }
    if candidates.len() > 64 {
        return Arc::new(Vec::new());
    }
    let mut names = std::collections::HashMap::new();
    for tool in &candidates {
        let name = if client == "codex" {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .or_else(|| tool.get("name"))
        } else {
            tool.get("function").unwrap_or(tool).get("name")
        };
        if let Some(name) = name.and_then(Value::as_str) {
            // Count trimmed aliases before size/schema validation, matching the
            // bridge's first-wins name normalization without allocating names.
            let name = if client == "codex" { name.trim() } else { name };
            if name.len() <= 256 {
                *names.entry(name).or_insert(0usize) += 1;
            }
        }
    }
    let mut definitions = Vec::new();
    for outer in candidates {
        if outer
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| t != "function")
        {
            continue;
        }
        let tool = outer.get("function").unwrap_or(outer);
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        if names.get(name) != Some(&1)
            || tool.get("namespace").is_some()
            || outer.get("namespace").is_some()
        {
            continue;
        }
        let field = match (client, name) {
            ("pi", "bash") => "command",
            ("pi", "read" | "write" | "edit") => "path",
            ("claude" | "claude-desktop", "Bash") => "command",
            ("claude" | "claude-desktop", "Read" | "Write" | "Edit") => "file_path",
            ("claude" | "claude-desktop", "WebFetch") => "url",
            ("codex", "exec_command" | "shell_command") => {
                if name == "exec_command" {
                    "cmd"
                } else {
                    "command"
                }
            }
            ("codex", "read_file" | "write_file") => "path",
            ("gemini", "run_shell_command") => "command",
            ("gemini", "read_file" | "write_file") => "file_path",
            ("gemini", "web_fetch") => "url",
            // OpenAI has no built-in client-side tool semantics.
            _ => continue,
        };
        let input = if let Some(schema) = tool.get("parametersJsonSchema") {
            if tool.get("parameters").is_some() || tool.get("input_schema").is_some() {
                continue;
            }
            Some(schema)
        } else if client == "codex" {
            tool.get("parameters")
        } else {
            tool.get("input_schema").or_else(|| tool.get("parameters"))
        };
        let Some(input) = input else {
            continue;
        };
        if crate::proxy::tool_observation::types::bounded_input_size(input).is_err() {
            continue;
        }
        if input.get("type").and_then(Value::as_str) != Some("object") {
            continue;
        }
        if input
            .get("properties")
            .and_then(|p| p.get(field))
            .and_then(|p| p.get("type"))
            .and_then(Value::as_str)
            != Some("string")
        {
            continue;
        }
        if !input
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|r| r.iter().any(|v| v.as_str() == Some(field)))
        {
            continue;
        }
        definitions.push(ToolSchemaRef {
            name: name.into(),
            namespace: None,
            schema,
        });
    }
    Arc::new(definitions)
}

#[cfg(test)]
mod protocol_regression_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn client_app_cannot_select_response_protocol() {
        assert_eq!(
            metadata(
                PROTOCOL_OPENAI_CHAT,
                "codex",
                Transport::Json,
                OutputQuality::Native,
                Arc::new(Vec::new())
            )
            .protocol,
            PROTOCOL_OPENAI_CHAT
        );
        assert_eq!(
            metadata(
                PROTOCOL_OPENAI_RESPONSES,
                "codex",
                Transport::Sse,
                OutputQuality::Native,
                Arc::new(Vec::new())
            )
            .protocol,
            PROTOCOL_OPENAI_RESPONSES
        );
    }
    #[test]
    fn codex_chat_metadata_borrows_tools_before_bounded_collection() {
        // A resource-shape regression: runtime results cannot reveal an eager
        // clone of the full descriptions/schemas before the collector's limits.
        let handlers = include_str!("../handlers.rs");
        let helper = handlers
            .split_once("fn codex_chat_observation_metadata(")
            .unwrap()
            .1
            .split_once("\nasync fn handle_codex_chat_to_responses_transform(")
            .unwrap()
            .0;
        assert!(
            !helper.contains("json!("),
            "metadata must not serialize/clone all outbound tools before checking limits"
        );
        let compact: String = helper.chars().filter(|c| !c.is_whitespace()).collect();
        let compact = compact.replace(",)", ")");
        assert!(compact.contains("mount::collect_tool_schemas_from_tools(tool_context.chat_tools(),ctx.app_type_str)"),
            "metadata must pass the borrowed outbound tool slice to the bounded collector");
    }
    #[test]
    fn borrowed_tool_slice_preserves_schema_mapping_without_touching_descriptions() {
        let body = json!({"tools":[{"type":"function","function":{"name":"exec_command",
            "description":"x".repeat(super::super::types::MAX_INPUT_BYTES * 2),
            "parameters":{"type":"object","properties":{"cmd":{"type":"string"}},"required":["cmd"]}}}]});
        let tools = body["tools"].as_array().unwrap();
        let description = tools[0]["function"]["description"].as_str().unwrap();
        let borrowed = collect_tool_schemas_from_tools(tools, "codex");
        let wrapped = collect_tool_schemas(&body, "codex");
        assert_eq!(borrowed.len(), 1);
        assert_eq!(borrowed[0].name, "exec_command");
        assert_eq!(borrowed[0].schema, CODEX_TOOLS_SCHEMA);
        assert_eq!(wrapped[0].name, borrowed[0].name);
        assert_eq!(wrapped[0].schema, borrowed[0].schema);
        assert_eq!(
            description.as_ptr(),
            tools[0]["function"]["description"]
                .as_str()
                .unwrap()
                .as_ptr()
        );
        assert!(collect_tool_schemas_from_tools(tools, "unknown").is_empty());
    }
    #[test]
    fn borrowed_tool_slice_keeps_ambiguity_namespace_and_size_limits() {
        let tool = json!({"type":"function","function":{"name":"exec_command",
            "parameters":{"type":"object","properties":{"cmd":{"type":"string"}},"required":["cmd"]}}});
        assert!(collect_tool_schemas_from_tools(&vec![tool.clone(); 65], "codex").is_empty());
        let mut alias = tool.clone();
        alias["function"]["name"] = json!(" exec_command ");
        assert!(collect_tool_schemas_from_tools(&[tool.clone(), alias], "codex").is_empty());
        let mut namespaced = tool.clone();
        namespaced["namespace"] = json!("custom");
        assert!(collect_tool_schemas_from_tools(&[namespaced], "codex").is_empty());
        let mut oversized = tool;
        oversized["function"]["parameters"]["description"] =
            json!("x".repeat(super::super::types::MAX_INPUT_BYTES));
        assert!(collect_tool_schemas_from_tools(&[oversized], "codex").is_empty());
    }
    #[test]
    fn verified_request_schema_is_required_for_semantic_mapping() {
        let body = json!({"tools":[{"name":"Bash","input_schema":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}}]});
        let defs = collect_tool_schemas(&body, "claude");
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "Bash");
        let wrong = json!({"tools":[{"name":"Bash","input_schema":{"type":"object","properties":{"command":{"type":"number"}}}}]});
        assert!(collect_tool_schemas(&wrong, "claude").is_empty());
        assert!(collect_tool_schemas(&body, "unknown").is_empty());
    }
    #[test]
    fn duplicate_or_oversized_schema_does_not_establish_identity() {
        let tool = json!({"name":"Read","input_schema":{"type":"object","properties":{"file_path":{"type":"string"}},"required":["file_path"]}});
        assert!(collect_tool_schemas(&json!({"tools":[tool.clone(),tool]}), "claude").is_empty());
        assert!(collect_tool_schemas(&json!({"tools":[]}), "claude").is_empty());
    }
}

#[cfg(test)]
mod upstream_stream_tests {
    use super::super::channel::*;
    use super::super::types::*;
    use super::*;
    use crate::services::tool_risk::{response_parser::ResponseProcessor, runtime::*};
    use futures::StreamExt;
    use serde_json::json;
    use std::{collections::HashMap, time::Duration};

    fn enable() -> ResponseMetadata {
        set_tool_risk_runtime_for_test(ToolRiskRuntime::build(true, 33, vec![], &HashMap::new()));
        metadata(
            "openai_chat",
            "codex",
            Transport::Sse,
            OutputQuality::Native,
            collect_tool_schemas(
                &json!({"tools":[{"type":"function","name":"exec_command","parameters":{
                    "type":"object","properties":{"cmd":{"type":"string"}},"required":["cmd"]
                }}]}),
                "codex",
            ),
        )
    }
    #[tokio::test]
    #[serial_test::serial]
    async fn error_and_client_drop_preserve_bytes_but_never_complete_pending_call() {
        for fail in [false, true] {
            let (_, mut rx) = test_channel().await;
            let metadata = enable();
            let bytes = Bytes::from_static(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"exec_command\",\"arguments\":\"{\\\"cmd\\\":\\\"ls\\\"}\"}}]}}]}\n\n");
            let chunks = vec![
                Ok(bytes.clone()),
                Err(std::io::Error::other("upstream interrupted")),
            ];
            let mut output = Box::pin(observe_stream(futures::stream::iter(chunks), metadata));
            let delivered = output.next().await.unwrap().unwrap();
            assert_eq!(delivered, bytes);
            assert_eq!(delivered.as_ptr(), bytes.as_ptr());
            let piece = rx.recv().await.unwrap();
            let state = piece.state.clone();
            let mut parser = ResponseProcessor::default();
            assert!(parser.ingest(&piece).is_empty());
            if fail {
                let error = output.next().await.unwrap().unwrap_err();
                assert_eq!(error.to_string(), "upstream interrupted");
                assert!(state.faulted.load(std::sync::atomic::Ordering::Acquire));
            }
            drop(output);
            let observations = parser.sweep();
            assert_eq!(observations.len(), 1);
            assert_eq!(observations[0].input.status, InputStatus::Incomplete);
            assert_eq!(
                observations[0].tool.definition_ref.as_deref(),
                Some("builtin:codex:tools:v1")
            );
            let decision = crate::services::tool_risk::engine::evaluate(
                &crate::services::tool_risk::engine::EngineInput {
                    observation: &observations[0],
                    rules: &crate::services::tool_risk::builtin::builtin_rules(),
                    lists: &[],
                },
            );
            assert_eq!(
                decision.verdict,
                Some(crate::services::tool_risk::engine::Verdict::Unchecked)
            );
            set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
        }
    }
    #[tokio::test]
    #[serial_test::serial]
    async fn no_upstream_reservation_before_first_chunk_and_budget_failure_is_passthrough() {
        let (sender, mut rx) = test_channel().await;
        let metadata = enable();
        let mut pending = Box::pin(observe_stream(futures::stream::pending(), metadata.clone()));
        assert!(
            tokio::time::timeout(Duration::from_millis(2), pending.next())
                .await
                .is_err()
        );
        let taps: Vec<_> = (0..MAX_ACTIVE_BYTES / RESPONSE_RESERVATION)
            .filter_map(|_| sender.start(metadata.clone(), 33))
            .collect();
        assert_eq!(taps.len(), MAX_ACTIVE_BYTES / RESPONSE_RESERVATION);
        let bytes = Bytes::from_static(b"data: [DONE]\n\n");
        let mut output = Box::pin(observe_stream(
            futures::stream::iter(vec![Ok(bytes.clone())]),
            metadata,
        ));
        assert_eq!(output.next().await.unwrap().unwrap(), bytes);
        assert!(output.next().await.is_none());
        assert!(tokio::time::timeout(Duration::from_millis(2), rx.recv())
            .await
            .is_err());
        drop(taps);
        set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
    }
}

#[cfg(test)]
mod pi_tests {
    use super::*;
    #[test]
    fn pi_tool_schemas_are_validated_independently_of_wire_protocol() {
        for tool in [
            serde_json::json!({"type":"function","function":{"name":"bash","parameters":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}}}),
            serde_json::json!({"name":"bash","input_schema":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}}),
        ] {
            let schemas = collect_tool_schemas(&serde_json::json!({"tools":[tool]}), "pi");
            assert_eq!(schemas.len(), 1);
            assert_eq!(schemas[0].name, "bash");
        }
        assert!(collect_tool_schemas(
            &serde_json::json!({"tools":[{"name":"bash","parameters":{"type":"object"}}]}),
            "pi"
        )
        .is_empty());
    }
}
