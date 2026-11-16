//! Explicit client-output protocol and transport selection. Never infer protocol from app identity.
/// Original client identity supplied by the local group router, not request headers.
#[derive(Clone, Copy)]
pub(crate) struct ClientToolObservation(pub &'static str);

/// Client-owned session identity, distinct from internal routing affinity.
#[derive(Clone)]
pub(crate) struct ClientToolSession(pub Option<String>);

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
        session_id: None,
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

// Catalog scanning counts extension tools as well as supported built-ins.
const MAX_CATALOG_TOOLS: usize = 1024;

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
    if tools.len() > MAX_CATALOG_TOOLS {
        return Arc::new(Vec::new());
    }
    let mut candidates = Vec::new();
    for tool in tools {
        let Some(declarations) = tool.get("functionDeclarations").and_then(Value::as_array) else {
            candidates.push(tool);
            continue;
        };
        if candidates.len().saturating_add(declarations.len()) > MAX_CATALOG_TOOLS {
            return Arc::new(Vec::new());
        }
        candidates.extend(declarations.iter());
    }
    if candidates.len() > MAX_CATALOG_TOOLS {
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
            .split_once("async fn handle_codex_chat_to_responses_transform(")
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
    fn catalog_scan_budget_retains_builtin_and_rejects_overflow_and_late_duplicates() {
        let tool = json!({"type":"function","name":"exec_command","parameters":{
            "type":"object","properties":{"cmd":{"type":"string"}},"required":["cmd"]
        }});
        let extension = json!({"type":"function","name":"synthetic_extension"});
        let mut tools = vec![extension.clone(); MAX_CATALOG_TOOLS - 1];
        tools.push(tool.clone());
        assert_eq!(collect_tool_schemas_from_tools(&tools, "codex").len(), 1);
        tools.push(extension);
        assert!(collect_tool_schemas_from_tools(&tools, "codex").is_empty());
        let mut duplicate = tools[..330].to_vec();
        duplicate.push(tool.clone());
        let mut alias = tool;
        alias["name"] = json!(" exec_command ");
        duplicate.push(alias);
        assert!(collect_tool_schemas_from_tools(&duplicate, "codex").is_empty());
        let overflow = json!({"functionDeclarations":tools});
        assert!(collect_tool_schemas_from_tools(&[overflow], "gemini").is_empty());
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
    async fn large_catalog_keeps_permission_risk_events_for_codex_and_pi() {
        struct Reset(ToolRiskRuntime);
        impl Drop for Reset {
            fn drop(&mut self) {
                set_tool_risk_runtime_for_test(self.0.clone());
            }
        }
        let _reset = Reset((*get_tool_risk_runtime()).clone());
        for (client, tool, field) in [("codex", "exec_command", "cmd"), ("pi", "bash", "command")] {
            for transport in [Transport::Json, Transport::Sse] {
                let (_, mut rx) = test_channel().await;
                let mut metadata = enable();
                let mut tools: Vec<Value> = (0..330)
                    .map(|index| {
                        json!({
                            "type":"function","name":format!("synthetic_extension_{index}"),
                            "parameters":{"type":"object","properties":{}}
                        })
                    })
                    .collect();
                tools.push(json!({"type":"function","name":tool,"parameters":{
                    "type":"object","properties":{(field):{"type":"string"}},"required":[field]
                }}));
                metadata.client = client;
                metadata.transport = transport;
                metadata.session_id = Some("synthetic-large-catalog-session".into());
                metadata.definitions = collect_tool_schemas_from_tools(&tools, client);
                let call = json!({"index":0,"id":"synthetic-call","type":"function","function":{
                    "name":tool,"arguments":json!({(field):"chmod 4755 /tmp/synthetic-permission-fixture"}).to_string()
                }});
                let bytes = if transport == Transport::Json {
                    Bytes::from(json!({"choices":[{"index":0,"message":{"role":"assistant","tool_calls":[call]},"finish_reason":"tool_calls"}]}).to_string())
                } else {
                    let delta = json!({"choices":[{"index":0,"delta":{"tool_calls":[call]}}]});
                    let end =
                        json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]});
                    Bytes::from(format!("data: {delta}\n\ndata: {end}\n\n"))
                };
                if transport == Transport::Json {
                    observe_non_streaming(&bytes, metadata);
                } else {
                    let output =
                        observe_stream(futures::stream::iter(vec![Ok(bytes.clone())]), metadata)
                            .collect::<Vec<_>>()
                            .await;
                    assert_eq!(output.len(), 1);
                    assert_eq!(output[0].as_ref().unwrap(), &bytes);
                }
                let piece = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                    .await
                    .unwrap()
                    .unwrap();
                let observations = ResponseProcessor::default().ingest(&piece);
                assert_eq!(observations.len(), 1);
                let event = crate::services::tool_risk::worker::project_event(&observations[0])
                    .expect(
                        "known permission-changing tool must produce an event in a large catalog",
                    );
                assert_eq!(
                    event.verdict.as_deref(),
                    Some("risky"),
                    "client={client}, streaming={}",
                    transport == Transport::Sse
                );
                assert!(event
                    .rule_ids
                    .iter()
                    .any(|id| id == "builtin.privilege_change"));
                assert_eq!(
                    event.session_id.as_deref(),
                    Some("synthetic-large-catalog-session")
                );
            }
        }
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
