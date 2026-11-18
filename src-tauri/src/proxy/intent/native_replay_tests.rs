//! Synthetic same-conversation model-switch regressions; no native app is launched.
use super::{native_gateway, Runtime};
use crate::{
    database::Database,
    proxy::{security, server::ProxyServer, ProxyConfig},
};
use axum::{extract::Request, response::IntoResponse, Json, Router};
use http::{header, HeaderMap, StatusCode};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct RestorePolicy(security::SecurityRuntimeConfig);
impl Drop for RestorePolicy {
    fn drop(&mut self) {
        security::runtime::set_security_runtime_for_test(self.0.clone());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn native_responses_model_switch_recovers_all_transparent_agent_adapters(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::services::agent_clients::{self, Backup, Route};
    let _policy = RestorePolicy((*security::get_security_runtime()).clone());
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = Router::new().fallback(move |Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            if let Ok(mut seen) = captured.lock() { seen.push(body.clone()); }
            if body["input"].as_array().into_iter().flatten().any(|item| item["id"] == "rs_native_missing") {
                return (StatusCode::NOT_FOUND, Json(json!({"error":{"message":"Referenced reasoning item 'rs_native_missing' was not found or has expired."}}))).into_response();
            }
            let reply = json!({"id":"resp_synthetic","status":"completed","output":[],"usage":{"input_tokens":1,"output_tokens":1}});
            if body["stream"] == true {
                ([("content-type", "text/event-stream")], format!("event: response.completed\ndata: {reply}\n\n")).into_response()
            } else { Json(reply).into_response() }
        }
    });
    let upstream = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let db = Arc::new(Database::memory()?);
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    );
    let info = proxy.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for agent in agent_clients::APPS.iter().copied().chain(["trae", "qoder"]) {
        let local = if agent == "qoder" {
            db.set_setting(crate::services::qoder_proxy::ENABLED_KEY, "true")?;
            db.save_provider("qoder", &crate::provider::Provider::with_id("synthetic".into(), "Synthetic".into(), json!({
                "baseUrl":format!("http://{address}/v1"), "apiKey":"synthetic-native-key", "protocol":"openai-responses", "model":"synthetic-model", "models":[{"model":"synthetic-model"}]
            }), None))?;
            format!(
                "http://127.0.0.1:{}/qoder/providers/synthetic/v1",
                info.port
            )
        } else {
            let local = format!(
                "http://127.0.0.1:{}/agents/{agent}/providers/synthetic/v1",
                info.port
            );
            let backup = Backup {
                path: "unused-synthetic-config".into(),
                routes: vec![Route {
                    id: "synthetic".into(),
                    pointer: "/unused".into(),
                    upstream: format!("http://{address}/v1"),
                    local: local.clone(),
                    models: vec!["synthetic-model".into()],
                    provider_id: "synthetic-provider".into(),
                    created_provider: None,
                }],
            };
            db.save_live_backup(agent, &serde_json::to_string(&backup)?)
                .await?;
            db.set_setting(&agent_clients::enabled_key(agent), "true")?;
            local
        };
        for enabled in [false, true] {
            security::runtime::set_security_runtime_for_test(security::SecurityRuntimeConfig {
                enabled,
                custom_rules: vec![security::runtime::CompiledCustomRule {
                    id: "synthetic-native".into(),
                    name: "Synthetic native rule".into(),
                    rule_type: "regex".into(),
                    pattern: "synthetic-native-secret".into(),
                    regex: regex::Regex::new("synthetic-native-secret").ok(),
                    risk_level: security::RiskLevel::High,
                    action: security::AuditAction::Redact,
                    enabled: true,
                }],
                ..Default::default()
            });
            for stream in [false, true] {
                let before = seen.lock().map_err(|_| "synthetic lock poisoned")?.len();
                let reply = client.post(format!("{local}/responses")).bearer_auth(if agent == "qoder" { "varyai-local" } else { "synthetic-native-key" }).json(&json!({
                    "model":"synthetic-model", "stream":stream, "store":false, "input":[
                        {"type":"function_call","id":"fc_synthetic","call_id":"call_synthetic","name":"synthetic_tool","arguments":"{}"},
                        {"type":"function_call_output","call_id":"call_synthetic","output":"Synthetic result"},
                        {"type":"reasoning","id":"rs_native_missing","summary":[],"encrypted_content":"synthetic-foreign"},
                        {"role":"user","content":"Synthetic continue synthetic-native-secret"}
                    ], "synthetic_extension":{"keep":true}
                })).send().await?;
                assert_eq!(
                    reply.status(),
                    StatusCode::OK,
                    "agent={agent} enabled={enabled} stream={stream}"
                );
                let text = reply.text().await?;
                assert!(text.contains("resp_synthetic"));
                assert_eq!(text.starts_with("event:"), stream);
                let seen = seen.lock().map_err(|_| "synthetic lock poisoned")?;
                assert_eq!(seen.len() - before, 2);
                let mut expected = seen[before].clone();
                expected["input"]
                    .as_array_mut()
                    .ok_or("synthetic input")?
                    .remove(2);
                let last = &seen[before + 1];
                assert_eq!(last, &expected);
                assert_eq!(
                    last["input"][2]["content"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("synthetic-native-secret"),
                    !enabled
                );
            }
        }
    }
    proxy.stop().await?;
    upstream.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn native_model_switch_filters_bare_reasoning_and_recovers_named_404(
) -> Result<(), Box<dyn std::error::Error>> {
    let _policy = RestorePolicy((*security::get_security_runtime()).clone());
    let seen = Arc::new(Mutex::new(Vec::<(HeaderMap, Value)>::new()));
    let captured = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = Router::new().fallback(move |request: Request| {
        let captured = captured.clone();
        async move {
            let (parts, body) = request.into_parts();
            let Ok(raw) = axum::body::to_bytes(body, 1024 * 1024).await else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            let Ok(body) = serde_json::from_slice::<Value>(&raw) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if let Ok(mut seen) = captured.lock() { seen.push((parts.headers, body.clone())); }
            let bad = body["input"].as_array().into_iter().flatten().find(|item| {
                item["id"] == "rs_0" || item["id"] == "rs_foreign"
            });
            if let Some(item) = bad {
                return (StatusCode::NOT_FOUND, Json(json!({"error":{"message":format!(
                    "Item with id '{}' not found. Items are not persisted when `store` is set to false.",
                    item["id"].as_str().unwrap_or_default())}}))).into_response();
            }
            ([("x-synthetic-reply", "preserved")], Json(json!({
                "id":"resp_synthetic", "model":"synthetic-native", "status":"completed",
                "output":[{"type":"message","id":"msg_synthetic","role":"assistant","status":"completed",
                    "content":[{"type":"output_text","text":body["input"].as_array().and_then(|items| items.last()).and_then(|item| item.get("content")).cloned().unwrap_or(json!("Synthetic reply"))}]}]
            }))).into_response()
        }
    });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let _origin = native_gateway::set_test_origin(format!("http://{address}/backend-api/codex"));
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(Arc::new(Runtime::default()));
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for (enabled, restore) in [(false, false), (true, false), (true, true)] {
        security::runtime::set_security_runtime_for_test(security::SecurityRuntimeConfig {
            enabled,
            custom_rules: if restore {
                vec![security::runtime::CompiledCustomRule {
                    id: "synthetic-native-replay".into(),
                    name: "Synthetic native replay".into(),
                    rule_type: "regex".into(),
                    pattern: "synthetic-replay-secret".into(),
                    regex: regex::Regex::new("synthetic-replay-secret").ok(),
                    risk_level: security::RiskLevel::High,
                    action: security::AuditAction::RedactRestore,
                    enabled: true,
                }]
            } else {
                vec![]
            },
            ..Default::default()
        });
        for encrypted in [false, true] {
            let before = seen.lock().map_err(|_| "synthetic lock poisoned")?.len();
            let reasoning = if encrypted {
                json!({"type":"reasoning","id":"rs_foreign","summary":[],"encrypted_content":"synthetic-foreign-state"})
            } else {
                json!({"type":"reasoning","id":"rs_0","summary":[{"type":"summary_text","text":"Synthetic prior thought"}],"encrypted_content":null})
            };
            let input = json!([
                reasoning,
                {"type":"reasoning","id":"rs_valid","summary":[],"encrypted_content":"synthetic-valid-state"},
                {"type":"message","role":"assistant","content":[{"type":"output_text","text":"Synthetic prior answer"}]},
                {"type":"function_call","id":"fc_synthetic","call_id":"call_synthetic","name":"synthetic_tool","arguments":"{}"},
                {"type":"function_call_output","call_id":"call_synthetic","output":"Synthetic result"},
                {"role":"user","content":"Synthetic continue synthetic-replay-secret"}
            ]);
            let response = client.post(format!("http://127.0.0.1:{}/backend-api/codex/responses?synthetic=1", info.port))
                .bearer_auth("synthetic-native-token")
                .header("chatgpt-account-id", "synthetic-workspace")
                .header("session_id", "synthetic-same-conversation")
                .json(&json!({"model":"synthetic-native","store":false,"stream":false,"input":input,"synthetic_extension":{"keep":true}}))
                .send().await?;
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "enabled={enabled}, encrypted={encrypted}"
            );
            assert_eq!(response.headers()["x-synthetic-reply"], "preserved");
            assert_eq!(
                response.json::<Value>().await?["output"][0]["content"][0]["text"],
                "Synthetic continue synthetic-replay-secret"
            );
            let seen = seen.lock().map_err(|_| "synthetic lock poisoned")?;
            assert_eq!(seen.len() - before, if encrypted { 2 } else { 1 });
            let (headers, last) = seen.last().ok_or("missing synthetic request")?;
            assert_eq!(
                headers[header::AUTHORIZATION],
                "Bearer synthetic-native-token"
            );
            assert_eq!(headers["chatgpt-account-id"], "synthetic-workspace");
            assert_eq!(headers["session_id"], "synthetic-same-conversation");
            if restore {
                assert!(!last["input"][4]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("synthetic-replay-secret"));
                assert_eq!(last["input"][1], input[2]);
                assert_eq!(last["input"][2], input[3]);
                assert_eq!(last["input"][3], input[4]);
            } else {
                assert_eq!(
                    last["input"],
                    Value::Array(input.as_array().ok_or("synthetic input")?[1..].to_vec())
                );
            }
            assert_eq!(last["synthetic_extension"], json!({"keep":true}));
        }
    }
    server.stop().await?;
    task.abort();
    Ok(())
}
