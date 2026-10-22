//! All sockets use ephemeral ports; no installed agents or paid APIs are involved.
use super::{server::ProxyServer, types::ProxyConfig};
use crate::{
    database::Database,
    provider::{Provider, ProviderMeta},
};
use axum::{
    http::{header, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU16, AtomicUsize, Ordering},
    Arc,
};

struct CancelSignal(Arc<AtomicUsize>);
impl Drop for CancelSignal {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
#[serial_test::serial]
async fn four_protocol_matrix_preserves_text_and_usage_in_json_and_sse() {
    let formats = [
        "anthropic",
        "openai_chat",
        "openai_responses",
        "gemini_native",
    ];
    let captured = Arc::new(tokio::sync::Mutex::new(json!({})));
    let mut evidence = Vec::new();
    let capture = captured.clone();
    let cancelled = Arc::new(AtomicUsize::new(0));
    let cancel_control = cancelled.clone();
    let upstream_status = Arc::new(AtomicU16::new(0));
    let status_control = upstream_status.clone();
    let mock = Router::new().fallback(post(move |request: axum::extract::Request| {
      let capture = capture.clone();
      let status_control = status_control.clone();
      let cancel_control = cancel_control.clone();
      async move {
        let (parts, body) = request.into_parts();
        let body = http_body_util::BodyExt::collect(body).await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let path = parts.uri.path();
        *capture.lock().await = json!({"path":path,"body":body});
        assert!(!parts.headers.values().any(|v|v.to_str().ok().is_some_and(|v|v.contains("client-auth-fixture"))), "client credentials leaked to upstream");
        let credential = if path.ends_with("messages") { parts.headers.get("x-api-key") }
            else if path.contains("generateContent") || path.contains("streamGenerateContent") { parts.headers.get("x-goog-api-key") }
            else { parts.headers.get(header::AUTHORIZATION) };
        assert_eq!(credential.and_then(|h|h.to_str().ok()), Some(if path.ends_with("messages") || path.contains("generateContent") || path.contains("streamGenerateContent") { "fixture-key" } else { "Bearer fixture-key" }), "wrong upstream authentication");
        let payload = body.to_string();
        for marker in ["fixture-system", "fixture-user", "previous-answer", "next-turn"] {
            assert!(payload.contains(marker), "lost conversation content {marker} at {path}");
        }
        assert!(!payload.contains("ghp_111122223333444455556666777788889999"), "DLP was bypassed");
        if path.contains("generateContent") || path.contains("streamGenerateContent") {
            assert!(path.contains("/models/fixture:"), "mapped Gemini model missing: {path}");
        } else { assert_eq!(body["model"], "fixture", "model mapping lost"); }
        let messages = body.get("messages").or_else(||body.get("contents")).or_else(||body.get("input")).and_then(Value::as_array).unwrap();
        let previous = messages.iter().find(|m|m.to_string().contains("previous-answer")).unwrap();
        assert_eq!(previous["role"], if path.contains("generateContent") || path.contains("streamGenerateContent") { "model" } else { "assistant" }, "assistant role changed");
        if payload.contains("fixture-gemini-tool-image") {
            assert!(payload.contains("dG9vbA==") && payload.contains("https://example.test/tool-image.png"), "Gemini tool result images lost");
        }
        if payload.contains("history-call") { assert!(payload.contains("history-result"), "lost tool result"); }

        let status = status_control.load(Ordering::SeqCst);
        if status != 0 && status < 600 {
            return (StatusCode::from_u16(status).unwrap(), [(header::CONTENT_TYPE,"application/json")], json!({"error":{"message":"fixture failure","code":"fixture_failure"}}).to_string()).into_response();
        }
        let stream = body["stream"] == true || path.contains("streamGenerateContent");
        let tools = body["tools"].as_array().is_some_and(|t|!t.is_empty());
        let mut value = if path.ends_with("messages") {
            assert!(body["messages"].is_array());
            json!({"id":"msg_fixture","type":"message","role":"assistant","model":"fixture","content":[{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":7,"output_tokens":3}})
        } else if path.ends_with("chat/completions") {
            assert!(body["messages"].is_array());
            json!({"id":"chat_fixture","object":"chat.completion","model":"fixture","choices":[{"index":0,"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}})
        } else if path.ends_with("responses") {
            assert!(body["input"].is_array());
            json!({"id":"resp_fixture","object":"response","status":"completed","model":"fixture","output":[{"id":"out_fixture","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"hello","annotations":[]}]}],"usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}})
        } else {
            assert!(path.ends_with("generateContent") || path.ends_with("streamGenerateContent"), "unexpected upstream path: {path}");
            assert!(body["contents"].is_array());
            json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"hello"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":3,"totalTokenCount":10}})
        };
        if tools {
            assert!(body.to_string().contains("aGVsbG8="), "image missing from upstream request");
            let calls=[("call-a","甲"),("call-b","乙")];
            if path.ends_with("messages") {
                for (id,path) in calls {value["content"].as_array_mut().unwrap().push(json!({"type":"tool_use","id":id,"name":"read","input":{"path":path}}));}value["stop_reason"]=json!("tool_use");
            } else if path.ends_with("chat/completions") {
                value["choices"][0]["message"]["tool_calls"]=json!(calls.iter().map(|(id,path)|json!({"id":id,"type":"function","function":{"name":"read","arguments":json!({"path":path}).to_string()}})).collect::<Vec<_>>());value["choices"][0]["finish_reason"]=json!("tool_calls");
            } else if path.ends_with("responses") {
                for (id,path) in calls {value["output"].as_array_mut().unwrap().push(json!({"id":format!("fc_{id}"),"type":"function_call","call_id":id,"name":"read","arguments":json!({"path":path}).to_string(),"status":"completed"}));}
            } else {for (id,path) in calls {value["candidates"][0]["content"]["parts"].as_array_mut().unwrap().push(json!({"functionCall":{"id":id,"name":"read","args":{"path":path}}}));}}
        }
        if !stream { return (StatusCode::OK, [(header::CONTENT_TYPE,"application/json")], value.to_string()).into_response(); }
        let frames = if tools && path.ends_with("messages") {
            let mut frames=vec![json!({"type":"message_start","message":{"id":"msg_fixture","type":"message","role":"assistant","model":"fixture","content":[],"usage":{"input_tokens":7,"output_tokens":0}}})];
            for (index,block) in value["content"].as_array().unwrap().iter().enumerate(){frames.push(json!({"type":"content_block_start","index":index,"content_block":block}));frames.push(json!({"type":"content_block_stop","index":index}));}
            frames.push(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}));frames.push(json!({"type":"message_stop"}));frames
        } else if tools && path.ends_with("chat/completions") {
            let mut delta=value["choices"][0]["message"].clone();for (index,call) in delta["tool_calls"].as_array_mut().unwrap().iter_mut().enumerate(){call["index"]=json!(index);}
            vec![json!({"id":"chat_fixture","object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),json!({"id":"chat_fixture","object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":value["usage"]})]
        } else if tools && path.ends_with("responses") {
            let mut frames=vec![json!({"type":"response.created","response":{"id":"resp_fixture","model":"fixture","output":[]}}),json!({"type":"response.output_item.added","output_index":0,"item":{"id":"out_fixture","type":"message","role":"assistant","content":[]}}),json!({"type":"response.content_part.added","output_index":0,"content_index":0,"item_id":"out_fixture","part":{"type":"output_text","text":"","annotations":[]}}),json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"out_fixture","delta":"hello"})];
            for (index,item) in value["output"].as_array().unwrap().iter().enumerate().skip(1){let mut start=item.clone();start["arguments"]=json!("");start["status"]=json!("in_progress");frames.push(json!({"type":"response.output_item.added","output_index":index,"item":start}));frames.push(json!({"type":"response.function_call_arguments.delta","output_index":index,"item_id":item["id"],"delta":item["arguments"]}));frames.push(json!({"type":"response.function_call_arguments.done","output_index":index,"item_id":item["id"],"arguments":item["arguments"]}));frames.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));}
            frames.push(json!({"type":"response.completed","response":value}));frames
        } else if path.ends_with("messages") {
            vec![json!({"type":"message_start","message":{"id":"msg_fixture","type":"message","role":"assistant","model":"fixture","content":[],"usage":{"input_tokens":7,"output_tokens":0}}}),json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}),json!({"type":"content_block_stop","index":0}),json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}),json!({"type":"message_stop"})]
        } else if path.ends_with("chat/completions") {
            vec![json!({"id":"chat_fixture","object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":{"role":"assistant","content":"hello"},"finish_reason":null}]}), json!({"id":"chat_fixture","object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}})]
        } else if path.ends_with("responses") {
            vec![json!({"type":"response.created","response":{"id":"resp_fixture","model":"fixture","output":[]}}),json!({"type":"response.output_item.added","output_index":0,"item":{"id":"out_fixture","type":"message","role":"assistant","content":[]}}),json!({"type":"response.content_part.added","output_index":0,"content_index":0,"item_id":"out_fixture","part":{"type":"output_text","text":"","annotations":[]}}),json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"out_fixture","delta":"hello"}),json!({"type":"response.completed","response":value})]
        } else { vec![value] };
        if status >= 600 {
            let mut partial = frames.into_iter().filter(|event| {
                !matches!(event["type"].as_str(), Some("message_delta" | "message_stop" | "response.completed"))
                    && event["choices"][0]["finish_reason"].as_str().is_none()
            }).collect::<Vec<_>>();
            for event in &mut partial {
                if let Some(candidates) = event.get_mut("candidates").and_then(Value::as_array_mut) {
                    for candidate in candidates { candidate.as_object_mut().unwrap().remove("finishReason"); }
                }
            }
            let prefix = partial.iter().map(|v|format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap_or("message"))).collect::<String>();
            let signal = CancelSignal(cancel_control);
            let stream = async_stream::stream! {
                let _signal = signal;
                yield Ok::<Bytes, std::io::Error>(Bytes::from(prefix));
                if status == 601 { futures::future::pending::<()>().await; }
                else { yield Ok(Bytes::from_static(b"event: error\ndata: {\"type\":\"error\",\"error\":{\"message\":\"fixture midstream failure\"}}\n\n")); }
            };
            return ([(header::CONTENT_TYPE,"text/event-stream")], axum::body::Body::from_stream(stream)).into_response();
        }
        let mut sse=frames.iter().map(|v| format!("event: {}\ndata: {v}\n\n",v["type"].as_str().unwrap_or("message"))).collect::<String>();
        if path.ends_with("chat/completions") { sse.push_str("data: [DONE]\n\n"); }
        (StatusCode::OK, [(header::CONTENT_TYPE,"text/event-stream")], sse).into_response()
    }}));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock).await.unwrap();
    });
    let db = Arc::new(Database::memory().unwrap());
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            max_retries: 0,
            non_streaming_timeout: 10,
            ..Default::default()
        },
        db.clone(),
        None,
    );
    let info = proxy.start().await.unwrap();
    let client = reqwest::Client::new();
    for (input, app, path) in [
        ("anthropic", "claude", "/v1/messages"),
        ("openai_chat", "codex", "/v1/chat/completions"),
        ("openai_responses", "codex", "/v1/responses"),
        (
            "gemini_native",
            "gemini",
            "/v1beta/models/fixture:generateContent",
        ),
    ] {
        for output in formats {
            let mut provider = Provider::with_id(
                format!("matrix-{input}-{output}"),
                "Fixture".into(),
                json!({"base_url":format!("http://{addr}"),"auth":{"OPENAI_API_KEY":"fixture-key"},"env":{"ANTHROPIC_BASE_URL":format!("http://{addr}"),"ANTHROPIC_API_KEY":"fixture-key","ANTHROPIC_MODEL":"fixture","GOOGLE_GEMINI_BASE_URL":format!("http://{addr}"),"GEMINI_API_KEY":"fixture-key"},"config":format!("base_url = \"http://{addr}\"\n")}),
                None,
            );
            provider.meta = Some(ProviderMeta {
                api_format: Some(output.into()),
                api_key_field: Some("ANTHROPIC_API_KEY".into()),
                ..Default::default()
            });
            db.save_provider(app, &provider).unwrap();
            db.set_current_provider(app, &provider.id).unwrap();
            for (stream, tools, error_status) in [
                (false, false, 0),
                (true, false, 0),
                (false, true, 0),
                (true, true, 0),
                (false, false, 400),
                (true, false, 400),
                (false, false, 401),
                (true, false, 401),
                (false, false, 429),
                (true, false, 429),
                (false, false, 503),
                (true, false, 503),
                (true, false, 600),
                (true, false, 601),
            ] {
                upstream_status.store(error_status, Ordering::SeqCst);
                let mut body = match input {
                    "anthropic" => {
                        json!({"model":"fixture","max_tokens":128,"stream":stream,"messages":[{"role":"user","content":"fixture-user ghp_111122223333444455556666777788889999"}]})
                    }
                    "openai_chat" => {
                        json!({"model":"fixture","stream":stream,"messages":[{"role":"user","content":"fixture-user ghp_111122223333444455556666777788889999"}]})
                    }
                    "openai_responses" => {
                        json!({"model":"fixture","stream":stream,"input":[{"role":"user","content":[{"type":"input_text","text":"fixture-user ghp_111122223333444455556666777788889999"}]}]})
                    }
                    _ => {
                        json!({"generationConfig":{"thinkingConfig":{"includeThoughts":true,"thinkingBudget":-1}},"contents":[{"role":"user","parts":[{"text":"fixture-user ghp_111122223333444455556666777788889999"}]}]})
                    }
                };
                if tools {
                    let schema = json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]});
                    let image = "data:image/png;base64,aGVsbG8=";
                    match input {
                        "anthropic" => {
                            body["tools"] = json!([{"name":"read","input_schema":schema}]);
                            body["messages"][0]["content"] = json!([{"type":"text","text":"fixture-user ghp_111122223333444455556666777788889999"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}}]);
                        }
                        "openai_chat" => {
                            body["tools"] = json!([{"type":"function","function":{"name":"read","parameters":schema}}]);
                            body["messages"][0]["content"] = json!([{"type":"text","text":"fixture-user ghp_111122223333444455556666777788889999"},{"type":"image_url","image_url":{"url":image}}]);
                        }
                        "openai_responses" => {
                            body["tools"] =
                                json!([{"type":"function","name":"read","parameters":schema}]);
                            body["input"][0]["content"]
                                .as_array_mut()
                                .unwrap()
                                .push(json!({"type":"input_image","image_url":image}));
                        }
                        _ => {
                            body["tools"] = json!([{"functionDeclarations":[{"name":"read","parameters":schema}]}]);
                            body["contents"][0]["parts"].as_array_mut().unwrap().push(
                                json!({"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}}),
                            );
                        }
                    }
                }
                if input != "gemini_native" {
                    body["model"] = json!("client-model-alias");
                }
                // Verify history and instructions on the actual upstream body in every direction.
                match input {
                    "anthropic" => {
                        body["system"] = json!("fixture-system");
                        body["messages"].as_array_mut().unwrap().extend([
                            json!({"role":"assistant","content":"previous-answer"}),
                            json!({"role":"user","content":"next-turn"}),
                        ]);
                        if tools {
                            body["messages"].as_array_mut().unwrap().extend([
                            json!({"role":"assistant","content":[{"type":"tool_use","id":"history-call","name":"read","input":{"path":"history"}}]}),
                            json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"history-call","content":"history-result"}]}),
                        ]);
                        }
                    }
                    "openai_chat" => {
                        let messages = body["messages"].as_array_mut().unwrap();
                        messages.insert(0, json!({"role":"system","content":"fixture-system"}));
                        messages.extend([
                            json!({"role":"assistant","content":"previous-answer"}),
                            json!({"role":"user","content":"next-turn"}),
                        ]);
                        if tools {
                            messages.extend([
                            json!({"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"history-call","function":{"name":"read","arguments":"{\"path\":\"history\"}"}}]}),
                            json!({"role":"tool","tool_call_id":"history-call","content":"history-result"}),
                        ]);
                        }
                    }
                    "openai_responses" => {
                        body["instructions"] = json!("fixture-system");
                        body["input"].as_array_mut().unwrap().extend([
                            json!({"role":"assistant","content":[{"type":"output_text","text":"previous-answer"}]}),
                            json!({"role":"user","content":[{"type":"input_text","text":"next-turn"}]}),
                        ]);
                        if tools {
                            body["input"].as_array_mut().unwrap().extend([
                            json!({"type":"function_call","call_id":"history-call","name":"read","arguments":"{\"path\":\"history\"}"}),
                            json!({"type":"function_call_output","call_id":"history-call","output":"history-result"}),
                        ]);
                        }
                    }
                    _ => {
                        body["systemInstruction"] = json!({"parts":[{"text":"fixture-system"}]});
                        body["contents"].as_array_mut().unwrap().extend([
                            json!({"role":"model","parts":[{"text":"previous-answer"}]}),
                            json!({"role":"user","parts":[{"text":"next-turn"}]}),
                        ]);
                        if tools {
                            body["contents"].as_array_mut().unwrap().extend([
                            json!({"role":"model","parts":[{"functionCall":{"id":"history-call","name":"read","args":{"path":"history"}}}]}),
                            json!({"role":if output == "gemini_native" {"user"} else {"model"},"parts":[{"functionResponse":{"id":"history-call","name":"read","response":{"result":"history-result fixture-gemini-tool-image"},"parts":[{"inlineData":{"mimeType":"image/png","data":"dG9vbA=="}},{"fileData":{"mimeType":"image/png","fileUri":"https://example.test/tool-image.png"}}]}}]}),
                        ]);
                        }
                    }
                }
                let path = if input == "gemini_native" && stream {
                    "/v1beta/models/fixture:streamGenerateContent?alt=sse"
                } else {
                    path
                };
                let response = client
                    .post(format!("http://127.0.0.1:{}{path}", info.port))
                    .header(header::AUTHORIZATION, "Bearer client-auth-fixture")
                    .header("x-api-key", "client-auth-fixture")
                    .header("x-goog-api-key", "client-auth-fixture")
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                let status = response.status();
                let content = if error_status == 601 {
                    let before = cancelled.load(Ordering::SeqCst);
                    let mut chunks = response.bytes_stream();
                    let first =
                        tokio::time::timeout(std::time::Duration::from_secs(2), chunks.next())
                            .await
                            .unwrap()
                            .unwrap()
                            .unwrap();
                    drop(chunks);
                    tokio::time::timeout(std::time::Duration::from_secs(2), async {
                        while cancelled.load(Ordering::SeqCst) == before {
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                    })
                    .await
                    .expect("client cancellation did not release the upstream connection");
                    String::from_utf8_lossy(&first).into_owned()
                } else if error_status == 600 {
                    let mut chunks = response.bytes_stream();
                    let mut output = String::new();
                    while let Some(chunk) = chunks.next().await {
                        match chunk {
                            Ok(bytes) => output.push_str(&String::from_utf8_lossy(&bytes)),
                            Err(_) => {
                                output.push_str("transport_error");
                                break;
                            }
                        }
                    }
                    output
                } else {
                    response.text().await.unwrap()
                };
                let capture = captured.lock().await.clone();
                let upstream_path = capture["path"].as_str().unwrap();
                evidence.push(json!({"input":input,"output":output,"stream":stream,"tools":tools,"fault":error_status,"upstream":capture,"response_status":status.as_u16(),"response":content}));
                if error_status >= 600 {
                    assert!(
                        !content.contains("message_stop")
                            && !content.contains("response.completed")
                            && !content.contains("[DONE]")
                            && !content.contains("\"finishReason\":\"STOP\""),
                        "{input}->{output}: failure/cancellation fabricated success: {content}"
                    );
                    if error_status == 600 {
                        assert!(
                            content.contains("error"),
                            "{input}->{output}: midstream failure not surfaced: {content}"
                        );
                    }
                    continue;
                }
                if error_status != 0 {
                    assert_eq!(
                        status.as_u16(),
                        error_status,
                        "{input}->{output}: upstream error status changed: {content}"
                    );
                    assert!(
                        content.contains("fixture failure"),
                        "{input}->{output}: upstream error lost"
                    );
                    continue;
                }
                if tools {
                    assert!(
                        capture["body"].to_string().contains("history-result"),
                        "{input}->{output}: tool history lost"
                    );
                }
                let expected = match output {
                    "anthropic" => "/messages",
                    "openai_chat" => "/chat/completions",
                    "openai_responses" => "/responses",
                    _ => {
                        if stream {
                            ":streamGenerateContent"
                        } else {
                            ":generateContent"
                        }
                    }
                };
                assert!(
                    upstream_path.ends_with(expected),
                    "{input}->{output}, stream={stream}: reached {upstream_path}"
                );
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "{input}->{output}, stream={stream}: {content}"
                );
                assert!(
                    content.contains("hello"),
                    "{input}->{output}, stream={stream}: {content}"
                );
                if tools {
                    assert!(
                        content.contains("call-a") && content.contains("call-b"),
                        "{input}->{output}: tool call IDs changed"
                    );
                    assert!(
                        content.contains("甲") && content.contains("乙"),
                        "{input}->{output}, stream={stream}: tool arguments lost: {content}"
                    );
                }
                if stream {
                    assert!(content.contains("data:"), "{input}->{output}: {content}");
                    let events = content
                        .lines()
                        .filter_map(|l| l.strip_prefix("data:"))
                        .filter_map(|s| serde_json::from_str::<Value>(s.trim()).ok())
                        .collect::<Vec<_>>();
                    let terminal = match input {
                        "anthropic" => events.iter().any(|e| e["type"] == "message_stop"),
                        "openai_chat" => content.contains("[DONE]"),
                        "openai_responses" => {
                            events.iter().any(|e| e["type"] == "response.completed")
                        }
                        _ => events
                            .iter()
                            .any(|e| e["candidates"][0]["finishReason"] == "STOP"),
                    };
                    assert!(terminal, "{input}->{output}: missing successful terminal");
                    let usage = match input {
                        "anthropic" => events.iter().any(|e| e["usage"]["output_tokens"] == 3),
                        "openai_chat" => events.iter().any(|e| {
                            e["usage"]["prompt_tokens"] == 7 && e["usage"]["completion_tokens"] == 3
                        }),
                        "openai_responses" => events.iter().any(|e| {
                            e["response"]["usage"]["input_tokens"] == 7
                                && e["response"]["usage"]["output_tokens"] == 3
                        }),
                        _ => events.iter().any(|e| {
                            e["usageMetadata"]["promptTokenCount"] == 7
                                && e["usageMetadata"]["candidatesTokenCount"] == 3
                        }),
                    };
                    assert!(usage, "{input}->{output}: streaming usage lost: {content}");
                } else {
                    let body: Value = serde_json::from_str(&content).unwrap();
                    let usage = if input == "gemini_native" {
                        &body["usageMetadata"]
                    } else {
                        &body["usage"]
                    };
                    let (input_field, output_field) = match input {
                        "gemini_native" => ("promptTokenCount", "candidatesTokenCount"),
                        "openai_chat" => ("prompt_tokens", "completion_tokens"),
                        _ => ("input_tokens", "output_tokens"),
                    };
                    assert_eq!(usage[input_field], 7, "{input}->{output}: input usage");
                    assert_eq!(usage[output_field], 3, "{input}->{output}: output usage");
                    if tools {
                        let calls =
                            match input {
                                "anthropic" => body["content"].as_array().map(|blocks| {
                                    blocks.iter().filter(|b| b["type"] == "tool_use").count()
                                }),
                                "openai_chat" => body["choices"][0]["message"]["tool_calls"]
                                    .as_array()
                                    .map(Vec::len),
                                "openai_responses" => body["output"].as_array().map(|items| {
                                    items
                                        .iter()
                                        .filter(|i| i["type"] == "function_call")
                                        .count()
                                }),
                                _ => body["candidates"][0]["content"]["parts"].as_array().map(
                                    |parts| {
                                        parts
                                            .iter()
                                            .filter(|p| p.get("functionCall").is_some())
                                            .count()
                                    },
                                ),
                            };
                        assert_eq!(calls, Some(2), "{input}->{output}: parallel calls lost");
                    }
                }
            }
        }
    }
    if let Ok(path) = std::env::var("VARYAI_PROTOCOL_MATRIX_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    }
    proxy.stop().await.unwrap();
    server.abort();
}
