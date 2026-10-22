use super::*;
use crate::{
    provider::Provider,
    services::pi_routing::catalog::{alias, Catalog},
};
use axum::{routing::post, Router};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
#[serial_test::serial]
async fn pi_gateway_auth_exact_routing_and_lossless_streaming() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |req: axum::extract::Request| {
        let count = count.clone(); async move {
            assert_eq!(req.headers()["authorization"], "Bearer upstream-secret");
            assert!(!req.headers().contains_key("cookie"));
            let bytes = axum::body::to_bytes(req.into_body(), 1024*1024).await.expect("body");
            let text = std::str::from_utf8(&bytes).expect("utf8");
            assert!(text.contains("900719925474099312345"));
            assert!(text.contains("1.2300e-08"));
            let value: Value = serde_json::from_slice(&bytes).expect("json");
            assert_eq!(value["model"], "vendor/model:latest");
            assert_eq!(value["tools"][0]["function"]["name"], "dynamic_tool");
            assert_eq!(value["reasoning_effort"], "high");
            assert_eq!(value["messages"][0]["signature"], "opaque-signature");
            count.fetch_add(1, Ordering::SeqCst);
            Response::builder().header("content-type", "text/event-stream").body(Body::from("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"arguments\":\"{\"}}]}}]}\n\ndata: {\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7}}\n\ndata: [DONE]\n\n")).expect("response")
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("port");
    let addr = listener.local_addr().expect("addr");
    let upstream = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let db = Arc::new(crate::database::Database::memory().expect("db"));
    let runtime = Arc::new(crate::services::pi_routing::Runtime::default());
    let p = Provider::with_id(
        "source".into(),
        "Source".into(),
        json!({"api":"openai-completions","baseUrl":format!("http://{addr}/v1"),"apiKey":"upstream-secret","models":[{"id":"vendor/model:latest"}]}),
        None,
    );
    *runtime.catalog.write().expect("catalog") = Some(Arc::new(
        Catalog::build(
            &[p],
            &["source".into()],
            "http://127.0.0.1:12345",
            "local-token",
        )
        .expect("catalog"),
    ));
    let server = crate::proxy::server::ProxyServer::new(
        crate::proxy::ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db,
        None,
    )
    .with_pi_runtime(runtime.clone());
    let info = server.start().await.expect("gateway");
    let url = format!("http://127.0.0.1:{}/pi/v1/chat/completions", info.port);
    let id = alias("source", "vendor/model:latest");
    let body = format!(
        r#"{{"model":"{id}","messages":[{{"role":"user","content":"hello","signature":"opaque-signature"}}],"tools":[{{"type":"function","function":{{"name":"dynamic_tool","parameters":{{"type":"object"}}}}}}],"reasoning_effort":"high","future":900719925474099312345,"temperature":1.2300e-08,"stream":true}}"#
    );
    let client = reqwest::Client::new();
    for (key, payload, status) in [
        ("wrong", body.clone(), 401),
        ("local-token", r#"{"model":"unknown"}"#.into(), 404),
        (
            "local-token",
            format!(r#"{{"model":"{id}","model":"{id}"}}"#),
            400,
        ),
    ] {
        let res = client
            .post(&url)
            .bearer_auth(key)
            .body(payload)
            .send()
            .await
            .expect("send");
        assert_eq!(res.status().as_u16(), status);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let res = client
        .post(&url)
        .bearer_auth("local-token")
        .header("cookie", "private")
        .body(body)
        .send()
        .await
        .expect("send");
    assert_eq!(res.status(), 200);
    assert!(res.text().await.expect("sse").contains("tool_calls"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    *runtime.catalog.write().expect("disable") = None;
    assert_eq!(
        client
            .post(url)
            .bearer_auth("local-token")
            .body("{}")
            .send()
            .await
            .expect("send")
            .status(),
        503
    );
    server.stop().await.expect("stop");
    upstream.abort();
}

async fn four_protocol_gateway() -> (
    crate::proxy::server::ProxyServer,
    tokio::task::JoinHandle<()>,
    Arc<crate::services::pi_routing::Runtime>,
    String,
) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let app = Router::new().fallback(|req: axum::extract::Request| async move {
        let path = req.uri().path().to_string();
        let headers = req.headers().clone();
        let bytes = axum::body::to_bytes(req.into_body(), 1024*1024).await.expect("body");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        if headers.contains_key("x-pi-native-smoke") {
            let tool_value = value.get("tools").expect("dynamic tools");
            let serialized = tool_value.to_string();
            assert!(serialized.contains("dynamic_tool"));
            if headers.get("x-pi-native-smoke").is_some_and(|v|v == "second") { assert!(serialized.contains("later_tool")); }
            if path.ends_with("/chat/completions") { assert_eq!(value["reasoning_effort"],"high"); }
            else if path.ends_with("/responses") { assert_eq!(value["reasoning"]["effort"],"high"); }
            else if path.ends_with("/messages") {
                assert_eq!(value["thinking"]["type"],"enabled");
                assert!(value["thinking"]["budget_tokens"].as_u64().is_some_and(|n| n > 0));
                if headers.get("x-pi-native-smoke").is_some_and(|v|v == "second") { assert!(value["messages"].to_string().contains("opaque-anthropic-signature")); }
            } else {
                assert_eq!(value["generationConfig"]["thinkingConfig"]["thinkingBudget"],32768);
                if headers.get("x-pi-native-smoke").is_some_and(|v|v == "second") { assert!(value["contents"].to_string().contains("c3ludGhldGljLWdlbWluaS1zaWduYXR1cmU=")); }
            }
        }
        let frame = if path.ends_with("/chat/completions") {
            assert_eq!(headers["authorization"],"Bearer upstream-secret");
            assert_eq!(value["model"],"model-chat");
            "data: {\"id\":\"chat_test\",\"object\":\"chat.completion.chunk\",\"model\":\"model-chat\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"ok\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"chat_test\",\"object\":\"chat.completion.chunk\",\"model\":\"model-chat\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n".to_string()
        } else if path.ends_with("/responses") {
            assert_eq!(headers["authorization"],"Bearer upstream-secret");
            assert_eq!(value["model"],"model-responses");
            let output=json!({"id":"msg_test","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"ok","annotations":[]}]});
            [json!({"type":"response.created","response":{"id":"resp_test","model":"model-responses","status":"in_progress","output":[]}}),
             json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_test","type":"message","role":"assistant","content":[]}}),
             json!({"type":"response.content_part.added","item_id":"msg_test","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
             json!({"type":"response.output_text.delta","item_id":"msg_test","output_index":0,"content_index":0,"delta":"ok"}),
             json!({"type":"response.output_item.done","output_index":0,"item":output}),
             json!({"type":"response.completed","response":{"id":"resp_test","model":"model-responses","status":"completed","output":[output],"usage":{"input_tokens":12,"output_tokens":3,"input_tokens_details":{"cached_tokens":0}}}})].into_iter().map(|v|format!("event: {}\ndata: {v}\n\n",v["type"].as_str().expect("type"))).collect()
        } else if path == "/anthropic/v1/messages" {
            assert_eq!(headers["x-api-key"],"upstream-secret");
            assert_eq!(value["model"],"model-anthropic");
            [json!({"type":"message_start","message":{"id":"msg_test","type":"message","role":"assistant","model":"model-anthropic","content":[],"stop_reason":null,"usage":{"input_tokens":12,"output_tokens":0}}}),
             json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
             json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"fixture thought"}}),
             json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque-anthropic-signature"}}),
             json!({"type":"content_block_stop","index":0}),
             json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
             json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"ok"}}),
             json!({"type":"content_block_stop","index":1}),
             json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}),
             json!({"type":"message_stop"})].into_iter().map(|v|format!("event: {}\ndata: {v}\n\n",v["type"].as_str().expect("type"))).collect()
        } else {
            assert_eq!(path,"/google/v1beta/models/gemini-2.5-pro:streamGenerateContent");
            assert_eq!(headers["x-goog-api-key"],"upstream-secret");
            format!("data: {}\n\n",json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"ok","thoughtSignature":"c3ludGhldGljLWdlbWluaS1zaWduYXR1cmU="}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":3,"totalTokenCount":15},"modelVersion":"model-gemini","responseId":"gem_test"}))
        };
        Response::builder().header("content-type","text/event-stream").body(Body::from(frame)).expect("response")
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let upstream_origin = format!("http://{}", listener.local_addr().expect("addr"));
    let upstream = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let runtime = Arc::new(crate::services::pi_routing::Runtime::default());
    let server = crate::proxy::server::ProxyServer::new(
        crate::proxy::ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(crate::database::Database::memory().expect("db")),
        None,
    )
    .with_pi_runtime(runtime.clone());
    let info = server.start().await.expect("gateway");
    let origin = format!("http://127.0.0.1:{}", info.port);
    let sources=[("chat","openai-completions","/v1"),("responses","openai-responses","/v1"),("anthropic","anthropic-messages","/anthropic"),("gemini","google-generative-ai","/google/v1beta")].map(|(id,api,base)|Provider::with_id(id.into(),id.into(),json!({"api":api,"baseUrl":format!("{upstream_origin}{base}"),"apiKey":"upstream-secret","models":[{"id":if id=="gemini" {"gemini-2.5-pro".to_string()} else {format!("model-{id}")},"reasoning":true,"contextWindow":128000,"maxTokens":8192,"input":["text"],"cost":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0}}]}),None));
    *runtime.catalog.write().expect("catalog") = Some(Arc::new(
        Catalog::build(
            &sources,
            &sources.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            &origin,
            "local-token",
        )
        .expect("catalog"),
    ));
    (server, upstream, runtime, origin)
}

#[tokio::test]
#[serial_test::serial]
async fn pi_all_four_native_protocol_paths_and_credentials() {
    let (server, upstream, _, origin) = four_protocol_gateway().await;
    let client = reqwest::Client::new();
    for (id, path, header, body) in [
        (
            "chat",
            "/pi/v1/chat/completions",
            "authorization",
            json!({"messages":[],"stream":true}),
        ),
        (
            "responses",
            "/pi/v1/responses",
            "authorization",
            json!({"input":[],"stream":true}),
        ),
        (
            "anthropic",
            "/pi/v1/messages",
            "x-api-key",
            json!({"messages":[],"max_tokens":64,"stream":true}),
        ),
        ("gemini", "", "x-goog-api-key", json!({"contents":[]})),
    ] {
        let alias = alias(
            id,
            &if id == "gemini" {
                "gemini-2.5-pro".to_string()
            } else {
                format!("model-{id}")
            },
        );
        let mut body = body;
        let path = if id == "gemini" {
            format!("/pi/v1beta/models/{alias}:streamGenerateContent?alt=sse")
        } else {
            body["model"] = json!(alias);
            path.into()
        };
        let key = if header == "authorization" {
            "Bearer local-token"
        } else {
            "local-token"
        };
        let res = client
            .post(format!("{origin}{path}"))
            .header(header, key)
            .json(&body)
            .send()
            .await
            .expect("request");
        assert_eq!(
            res.status(),
            200,
            "{id}: {}",
            res.text().await.expect("body")
        );
        assert!(res.text().await.expect("body").contains("ok"));
    }
    server.stop().await.expect("stop");
    upstream.abort();
}

#[tokio::test]
#[ignore = "Requires a local Pi installation; set VARY_PI_AGENT_ROOT and run explicitly"]
async fn pi_installed_sdk_consumes_all_four_protocols() {
    let root = std::env::var("VARY_PI_AGENT_ROOT").expect("VARY_PI_AGENT_ROOT");
    let (server, upstream, runtime, _) = four_protocol_gateway().await;
    let dir = tempfile::tempdir().expect("isolated Pi");
    let providers = runtime
        .catalog
        .read()
        .expect("catalog")
        .as_ref()
        .expect("active")
        .providers
        .clone();
    std::fs::write(
        dir.path().join("models.json"),
        json!({"providers":providers}).to_string(),
    )
    .expect("models");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts/tests/pi-provider-smoke.mjs");
    let output = tokio::process::Command::new("node")
        .arg(script)
        .arg(root)
        .arg(dir.path())
        .output()
        .await
        .expect("node");
    server.stop().await.expect("stop");
    upstream.abort();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
#[serial_test::serial]
async fn pi_blocked_requests_use_the_wire_error_envelope_without_echoing_secrets() {
    use crate::proxy::security::{
        runtime::{
            get_security_runtime, set_security_runtime_for_test, CompiledCustomRule,
            SecurityRuntimeConfig,
        },
        types::{AuditAction, RiskLevel},
    };
    struct Restore(SecurityRuntimeConfig);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_security_runtime_for_test(self.0.clone());
        }
    }
    let _restore = Restore((*get_security_runtime()).clone());
    let mut security = SecurityRuntimeConfig::default();
    security.custom_rules.push(CompiledCustomRule {
        id: "pi-test-block".into(),
        name: "Fixture rule".into(),
        rule_type: "keyword".into(),
        pattern: "SYNTHETIC_BLOCKED_CONTENT".into(),
        regex: None,
        risk_level: RiskLevel::High,
        action: AuditAction::Block,
        enabled: true,
    });
    set_security_runtime_for_test(security);
    let (server, upstream, _, origin) = four_protocol_gateway().await;
    for (id, path, key, body) in [
        (
            "chat",
            "/pi/v1/chat/completions",
            "authorization",
            json!({"messages":[{"role":"user","content":"SYNTHETIC_BLOCKED_CONTENT"}]}),
        ),
        (
            "anthropic",
            "/pi/v1/messages",
            "x-api-key",
            json!({"messages":[{"role":"user","content":[{"type":"text","text":"SYNTHETIC_BLOCKED_CONTENT"}]}]}),
        ),
        (
            "gemini",
            "",
            "x-goog-api-key",
            json!({"contents":[{"role":"user","parts":[{"text":"SYNTHETIC_BLOCKED_CONTENT"}]}]}),
        ),
    ] {
        let alias = alias(
            id,
            &if id == "gemini" {
                "gemini-2.5-pro".to_string()
            } else {
                format!("model-{id}")
            },
        );
        let mut body = body;
        let path = if id == "gemini" {
            format!("/pi/v1beta/models/{alias}:generateContent")
        } else {
            body["model"] = json!(alias);
            path.into()
        };
        let res = reqwest::Client::new()
            .post(format!("{origin}{path}"))
            .header(
                key,
                if key == "authorization" {
                    "Bearer local-token"
                } else {
                    "local-token"
                },
            )
            .json(&body)
            .send()
            .await
            .expect("request");
        assert_eq!(res.status(), 400);
        let text = res.text().await.expect("response");
        assert!(!text.contains("SYNTHETIC_BLOCKED_CONTENT"));
        let value: Value = serde_json::from_str(&text).expect("json");
        match id {
            "anthropic" => assert_eq!(value["type"], "error"),
            "gemini" => assert_eq!(value["error"]["code"], 400),
            _ => assert_eq!(value["error"]["code"], "sensitive_data_blocked"),
        };
    }
    server.stop().await.expect("stop");
    upstream.abort();
}

#[tokio::test]
#[serial_test::serial]
async fn pi_tool_argument_fragment_arrives_before_response_completion() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    let wait = Arc::new(tokio::sync::Mutex::new(Some(wait)));
    let app=Router::new().fallback(move||{let wait=wait.clone();async move {
        let wait=wait.lock().await.take().expect("once");
        let stream=async_stream::stream! {
            yield Ok::<_,std::io::Error>(bytes::Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"arguments\":\"{\"}}]}}]}\n\n"));
            let _=wait.await;
            yield Ok(bytes::Bytes::from_static(b"data: [DONE]\n\n"));
        };
        Response::builder().header("content-type","text/event-stream").body(Body::from_stream(stream)).expect("response")
    }});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let addr = listener.local_addr().expect("addr");
    let upstream = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let runtime = Arc::new(crate::services::pi_routing::Runtime::default());
    let p = Provider::with_id(
        "a".into(),
        "A".into(),
        json!({"api":"openai-completions","baseUrl":format!("http://{addr}/v1"),"apiKey":"upstream-secret","models":[{"id":"m"}]}),
        None,
    );
    *runtime.catalog.write().expect("catalog") = Some(Arc::new(
        Catalog::build(&[p], &["a".into()], "http://127.0.0.1:12345", "local-token")
            .expect("catalog"),
    ));
    let server = crate::proxy::server::ProxyServer::new(
        crate::proxy::ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(crate::database::Database::memory().expect("db")),
        None,
    )
    .with_pi_runtime(runtime);
    let info = server.start().await.expect("server");
    let mut res = reqwest::Client::new()
        .post(format!(
            "http://127.0.0.1:{}/pi/v1/chat/completions",
            info.port
        ))
        .bearer_auth("local-token")
        .json(&json!({"model":alias("a","m"),"messages":[],"stream":true}))
        .send()
        .await
        .expect("send");
    let chunk = tokio::time::timeout(Duration::from_secs(2), res.chunk())
        .await
        .expect("first delta arrived before release")
        .expect("chunk")
        .expect("bytes");
    assert!(String::from_utf8_lossy(&chunk).contains("arguments"));
    release.send(()).expect("release");
    let _ = res.bytes().await.expect("finish");
    server.stop().await.expect("stop");
    upstream.abort();
}

#[tokio::test]
#[serial_test::serial]
async fn pi_original_provider_scope_routes_all_protocols_and_rejects_other_models() {
    let (server, upstream, _, origin) = four_protocol_gateway().await;
    for (provider, suffix, model, key, mut body) in [
        (
            "chat",
            "/v1/chat/completions",
            "model-chat",
            "authorization",
            json!({"messages":[{"role":"user","content":"hello"}],"stream":true}),
        ),
        (
            "responses",
            "/v1/responses",
            "model-responses",
            "authorization",
            json!({"input":"hello","stream":true}),
        ),
        (
            "anthropic",
            "/v1/messages",
            "model-anthropic",
            "x-api-key",
            json!({"messages":[{"role":"user","content":"hello"}],"stream":true}),
        ),
        (
            "gemini",
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent",
            "gemini-2.5-pro",
            "x-goog-api-key",
            json!({"contents":[{"role":"user","parts":[{"text":"hello"}]}]}),
        ),
    ] {
        if provider != "gemini" {
            body["model"] = json!(model);
        }
        let url = format!("{origin}/pi/p/{}{suffix}", alias(provider, ""));
        let response = reqwest::Client::new()
            .post(&url)
            .header(
                key,
                if key == "authorization" {
                    "Bearer local-token"
                } else {
                    "local-token"
                },
            )
            .json(&body)
            .send()
            .await
            .expect("request");
        assert_eq!(response.status(), 200, "{provider}");
        assert!(response.text().await.expect("body").contains("ok"));
        if provider != "gemini" {
            body["model"] = json!("model-from-other-provider");
            assert_eq!(
                reqwest::Client::new()
                    .post(&url)
                    .header(
                        key,
                        if key == "authorization" {
                            "Bearer local-token"
                        } else {
                            "local-token"
                        }
                    )
                    .json(&body)
                    .send()
                    .await
                    .expect("unknown")
                    .status(),
                404
            );
        }
    }
    server.stop().await.expect("stop");
    upstream.abort();
}

#[tokio::test]
#[serial_test::serial]
async fn pi_original_provider_phone_is_inspected_before_forwarding() {
    use crate::proxy::security::{
        runtime::{get_security_runtime, set_security_runtime_for_test, SecurityRuntimeConfig},
        types::AuditAction,
    };
    struct Restore(SecurityRuntimeConfig);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_security_runtime_for_test(self.0.clone());
        }
    }
    let _restore = Restore((*get_security_runtime()).clone());
    let mut security = SecurityRuntimeConfig::default();
    security
        .builtin_action_overrides
        .insert("chinese_phone_number".into(), AuditAction::Block);
    set_security_runtime_for_test(security);
    let (server, upstream, _, origin) = four_protocol_gateway().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{origin}/pi/p/{}/v1/chat/completions",
            alias("chat", "")
        ))
        .bearer_auth("local-token")
        .json(&json!({"model":"model-chat","messages":[{"role":"user","content":"13800138000"}]}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 400);
    let text = response.text().await.expect("body");
    assert!(!text.contains("13800138000"));
    assert!(text.contains("sensitive_data_blocked"));
    server.stop().await.expect("stop");
    upstream.abort();
}
