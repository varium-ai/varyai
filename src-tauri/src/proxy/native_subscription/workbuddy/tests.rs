use super::*;
use std::io::Read;

#[tokio::test]
async fn workbuddy_chat_wire_preserves_tools_and_model() -> Result<(), Box<dyn std::error::Error>> {
    let input = json!({"model":"custom:example|example","messages":[
        {"role":"developer","content":"fixture policy"},
        {"role":"assistant","content":null,"reasoning_content":"reason","tool_calls":[{"id":"call-1","type":"function","function":{"name":"lookup","arguments":"{}"}}]},
        {"role":"tool","tool_call_id":"call-1","content":"result"}],
        "max_completion_tokens":321,"temperature":0.25,
        "tool_choice":{"type":"function","function":{"name":"lookup"}},
        "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]
    });
    let (result, names) = prepare(input.clone(), Protocol::Chat)?;
    assert!(names.is_empty());
    assert_eq!(result["model"], input["model"]);
    assert_eq!(result["tools"], input["tools"]);
    assert_eq!(result["temperature"], input["temperature"]);
    assert_eq!(result["max_tokens"], 321);
    assert!(result.get("max_completion_tokens").is_none());
    assert_eq!(result["tool_choice"], "lookup");
    assert_eq!(result["messages"][0]["role"], "system");
    assert_eq!(result["messages"][1]["reasoning"], "reason");
    assert_eq!(result["messages"][1]["tool_calls"][0]["id"], "call-1");
    assert_eq!(result["messages"][2]["tool_call_id"], "call-1");
    assert_eq!(result["stream"], true);
    let encoded = encode(result.clone(), Protocol::Chat).await?;
    let mut decoded = String::new();
    flate2::read::GzDecoder::new(encoded.as_ref()).read_to_string(&mut decoded)?;
    assert_eq!(result, serde_json::from_str::<Value>(&decoded)?);
    assert!(prepare(json!({"messages": {}}), Protocol::Chat).is_err());
    assert!(prepare(
        json!({"messages":[],"tool_choice":{"type":"unsupported"}}),
        Protocol::Chat
    )
    .is_err());
    Ok(())
}

#[test]
fn workbuddy_messages_keeps_signed_thinking_and_parameters() -> Result<(), ProxyError> {
    let mut input = json!({"model":"claude-fixture","system":[{"type":"text","text":"fixture instructions"}],
        "messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"foreign"},{"type":"thinking","thinking":"local","signature":"fixture"}]}],
        "max_tokens":4096,"thinking":{"type":"adaptive"},"output_config":{"effort":"max"}});
    let (actual, names) = prepare(input.clone(), Protocol::Anthropic)?;
    assert!(names.is_empty());
    input["stream"] = json!(true);
    input["messages"][0]["content"] =
        json!([{"type":"thinking","thinking":"local","signature":"fixture"}]);
    assert_eq!(input, actual);
    Ok(())
}

#[test]
fn workbuddy_tool_continuation_keeps_the_account_scoped_turn() -> Result<(), ProxyError> {
    let first = json!({"messages":[{"role":"user","content":"fixture question"}]});
    let continued = json!({"messages":[{"role":"user","content":"fixture question"},{"role":"assistant","content":null},{"role":"tool","content":"fixture result"}]});
    let mut original = HeaderMap::new();
    let mut next = HeaderMap::new();
    let mut other_account = HeaderMap::new();
    conversation_headers(&mut original, "one", Some("session"), &first)?;
    conversation_headers(&mut next, "one", Some("session"), &continued)?;
    conversation_headers(&mut other_account, "two", Some("session"), &continued)?;
    assert_eq!(original["x-conversation-id"], next["x-conversation-id"]);
    assert_eq!(
        original["x-conversation-request-id"],
        next["x-conversation-request-id"]
    );
    assert_ne!(original["x-request-id"], next["x-request-id"]);
    assert_ne!(
        original["x-conversation-id"],
        other_account["x-conversation-id"]
    );
    assert_eq!(
        original["x-conversation-id"]
            .to_str()
            .expect("conversation")
            .len(),
        32
    );
    Ok(())
}

#[tokio::test]
async fn regional_http_transport_delivers_structured_streamed_tool_calls(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::services::routing_upstreams::SubscriptionProvider;
    use axum::{extract::Request, routing::post, Router};

    let chat = concat!(
        "data: {\"id\":\"fixture\",\"model\":\"claude-fixture\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_fixture\",\"function\":{\"name\":\"repo_lookup\",\"arguments\":\"{\\\"q\\\":1}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let messages = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"fixture\",\"model\":\"claude-fixture\",\"content\":[]}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_fixture\",\"name\":\"repo_lookup\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":1}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let (sender, mut seen) = tokio::sync::mpsc::unbounded_channel();
    let handler = move |request: Request| {
        let sender = sender.clone();
        async move {
            let (parts, body) = request.into_parts();
            let bytes = axum::body::to_bytes(body, 16384).await.ok();
            let reply = if parts.uri.path() == "/v1/messages" {
                messages
            } else {
                chat
            };
            let _ = sender.send((parts, bytes));
            ([(header::CONTENT_TYPE, "text/event-stream")], reply)
        }
    };
    let app = Router::new()
        .route("/v1/messages", post(handler.clone()))
        .route("/v2/chat/completions", post(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let _server = Server(tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    }));
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    for (provider, model, path) in [
        (
            SubscriptionProvider::Workbuddy,
            "claude-fixture",
            "/v1/messages",
        ),
        (
            SubscriptionProvider::Workbuddy,
            "glm-fixture",
            "/v2/chat/completions",
        ),
        (
            SubscriptionProvider::WorkbuddyAi,
            "glm-fixture",
            "/v2/chat/completions",
        ),
    ] {
        let (protocol, tool, name_pointer) = if provider.workbuddy_messages(model) {
            (
                Protocol::Anthropic,
                json!({"name":"repo.lookup","input_schema":{"type":"object"}}),
                "/tools/0/name",
            )
        } else {
            (
                Protocol::Chat,
                json!({"type":"function","function":{"name":"repo.lookup","parameters":{"type":"object"}}}),
                "/tools/0/function/name",
            )
        };
        let account: accounts::Account = serde_json::from_value(json!({
            "id":"fixture", "provider":provider, "login":"Fixture", "access":"synthetic",
            "workbuddy":{"uid":"fixture-user","domain":"fixture.example","enterprise":"fixture-team","refreshExpires":0}
        }))?;
        let mut input = json!({"model":model,"tools":[tool],"messages":[{"role":"user","content":"fixture prompt"}]});
        if protocol == Protocol::Chat {
            input["messages"] = json!([
                {"role":"system","content":"You are powered by a service. You are an assistant powered by a service."},
                {"role":"user","content":"fixture prompt"}
            ]);
        }
        let (body, names) = prepare(input, protocol)?;
        if protocol == Protocol::Chat {
            assert_eq!(
                body["messages"][0]["content"],
                "You are powered by a service. You are an assistant, powered by a service."
            );
        }
        assert_eq!(
            body.pointer(name_pointer).and_then(Value::as_str),
            Some("repo_lookup")
        );
        let bytes = encode(body.clone(), protocol).await?;
        let response = request(
            &client,
            &account,
            &base,
            protocol,
            &HeaderMap::new(),
            &bytes,
            protocol == Protocol::Anthropic,
        )?
        .send()
        .await?;
        assert!(response.status().is_success());
        let (parts, payload) = seen.recv().await.ok_or("Missing captured request")?;
        assert_eq!(parts.uri.path(), path);
        assert_eq!(parts.headers["x-enterprise-id"], "fixture-team");
        assert_eq!(parts.headers["x-tenant-id"], "fixture-team");
        assert!(parts.headers.get(header::AUTHORIZATION).is_some());
        assert_eq!(
            parts.headers.contains_key(header::CONTENT_ENCODING),
            protocol == Protocol::Chat
        );
        if protocol == Protocol::Anthropic {
            assert_eq!(parts.uri.query(), Some("beta=true"));
            assert_eq!(parts.headers["anthropic-version"], "2023-06-01");
        }
        assert_eq!(payload.ok_or("Missing captured request body")?, bytes);

        let source: ByteStream = Box::pin(
            response
                .bytes_stream()
                .map(|chunk| chunk.map_err(std::io::Error::other)),
        );
        let reply =
            ir::stream::collect(restore_tool_names(source, names, protocol), protocol).await?;
        for destination in [
            Protocol::Chat,
            Protocol::Anthropic,
            Protocol::Responses,
            Protocol::Gemini,
        ] {
            let rendered =
                ir::reply::render_with_context(destination, &reply, &Default::default())?;
            let rendered = rendered.to_string();
            assert!(rendered.contains("repo.lookup"));
            assert!(rendered.contains("call_fixture"));
        }
    }
    Ok(())
}

#[test]
fn workbuddy_chat_sanitizes_illegal_tool_names_consistently() -> Result<(), ProxyError> {
    let input = json!({"model":"glm-fixture","messages":[
        {"role":"user","content":"fixture"},
        {"role":"assistant","content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":"github.create","arguments":"{}"}}]},
        {"role":"tool","name":"github.create","tool_call_id":"call-1","content":"result"}],
        "tool_choice":{"type":"function","function":{"name":"github.create"}},
        "tools":[
            {"type":"function","function":{"name":"github.create","parameters":{"properties":{"q":{"type":"string"}}}}},
            {"type":"function","function":{"name":"legal_name","parameters":{"type":"object"}}}
        ]
    });
    let (result, names) = prepare(input, Protocol::Chat)?;
    assert_eq!(result["tools"][0]["function"]["name"], "github_create");
    assert_eq!(result["tools"][1]["function"]["name"], "legal_name");
    assert_eq!(result["tool_choice"], "github_create");
    assert_eq!(
        result["messages"][2]["tool_calls"][0]["function"]["name"],
        "github_create"
    );
    assert_eq!(
        names.get("github.create").map(String::as_str),
        Some("github_create")
    );
    assert_eq!(result["messages"][3]["name"], "github_create");
    assert!(!names.contains_key("legal_name"));
    Ok(())
}

#[test]
fn workbuddy_chat_sanitized_names_never_collide_with_legal_tools() -> Result<(), ProxyError> {
    let input = json!({"model":"glm-fixture","messages":[{"role":"user","content":"fixture"}],
        "tools":[
            {"type":"function","function":{"name":"github.create","parameters":{"type":"object"}}},
            {"type":"function","function":{"name":"github_create","parameters":{"type":"object"}}}
        ]
    });
    let (result, names) = prepare(input, Protocol::Chat)?;
    let sanitized = result["tools"][0]["function"]["name"]
        .as_str()
        .expect("sanitized name")
        .to_string();
    assert!(sanitized.starts_with("github_create_"));
    assert_ne!(sanitized, "github_create");
    assert_eq!(result["tools"][1]["function"]["name"], "github_create");
    assert_eq!(
        names.get("github.create").map(String::as_str),
        Some(sanitized.as_str())
    );
    Ok(())
}

#[test]
fn workbuddy_anthropic_sanitizes_tool_names_and_named_choices() -> Result<(), ProxyError> {
    let input = json!({"model":"claude-fixture","system":[{"type":"text","text":"fixture instructions"}],
        "messages":[
            {"role":"user","content":[{"type":"text","text":"fixture"}]},
            {"role":"assistant","content":[{"type":"tool_use","id":"call-1","name":"github.create","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"call-1","content":"ok"}]}
        ],
        "tool_choice":{"type":"tool","name":"github.create"},
        "tools":[{"name":"github.create","description":"fixture","input_schema":{"properties":{"q":{"type":"string"}}}}]
    });
    let (result, names) = prepare(input, Protocol::Anthropic)?;
    assert_eq!(result["tools"][0]["name"], "github_create");
    assert_eq!(result["tool_choice"]["name"], "github_create");
    assert_eq!(result["messages"][1]["content"][0]["name"], "github_create");
    assert_eq!(
        names.get("github.create").map(String::as_str),
        Some("github_create")
    );
    Ok(())
}

#[test]
fn workbuddy_chat_normalizes_tool_parameter_schemas() {
    let mut root = json!({"properties":{"q":{"type":"string"}},"required":["q"]});
    normalize_schema(&mut root);
    assert_eq!(root["type"], "object");

    let mut combinators = json!({"anyOf":[
        {"type":"object","properties":{"kind":{"const":"x"},"a":{"type":"string"}},"required":["kind","a"]},
        {"type":"object","properties":{"kind":{"const":"y"},"b":{"type":"number"}},"required":["kind","b"]}
    ]});
    normalize_schema(&mut combinators);
    assert!(combinators.get("anyOf").is_none());
    assert_eq!(
        combinators["properties"]["kind"],
        json!({"anyOf":[{"const":"x"},{"const":"y"}]})
    );
    assert_eq!(combinators["properties"]["a"]["type"], "string");
    assert_eq!(combinators["properties"]["b"]["type"], "number");
    assert_eq!(combinators["required"], json!(["kind"]));

    let mut tuple = json!({"type":"array","items":[{"type":"number"},{"type":"string"}],"additionalItems":false});
    normalize_schema(&mut tuple);
    assert_eq!(
        tuple,
        json!({"type":"array","items":{"anyOf":[{"type":"number"},{"type":"string"}]}})
    );

    let mut params = json!({"type":"object","examples":[{"enum":["x",1]}],"properties":{
        "pattern":{"pattern":r"\0|\\0|\01"},
        "null":{"enum":[true,null]},
        "mixed":{"enum":["tag",1,null]},
        "literal":{"pattern":"a\u{0}b"},
        "deny":false
    }});
    normalize_schema(&mut params);
    assert_eq!(
        params["properties"]["pattern"]["pattern"],
        r"\u0000|\\0|\01"
    );
    assert_eq!(params["properties"]["literal"]["pattern"], r"a\u0000b");
    assert_eq!(
        params["properties"]["null"]["type"],
        json!(["boolean", "null"])
    );
    assert_eq!(
        params["properties"]["mixed"],
        json!({"anyOf":[{"type":"string","enum":["tag"]},{"type":"integer","enum":[1]},{"type":"null"}]})
    );
    assert_eq!(params["properties"]["deny"], json!({"not":{}}));
    assert_eq!(params["examples"], json!([{"enum":["x",1]}]));

    let mut refs = json!({"$defs":{"A":{"type":"object","properties":{"a":{"type":"string"}},"required":["a"]}},
        "allOf":[{"$ref":"#/$defs/A"},{"type":"object","properties":{"b":{"type":"string"}}}]});
    normalize_schema(&mut refs);
    assert!(refs.get("allOf").is_none());
    assert_eq!(
        refs["properties"],
        json!({"a":{"type":"string"},"b":{"type":"string"}})
    );
    assert_eq!(refs["required"], json!(["a"]));
    for mut empty in [Value::Null, json!({})] {
        normalize_schema(&mut empty);
        assert_eq!(empty, json!({"type":"object","properties":{}}));
    }

    // Ambiguous combinators and reference branches stay untouched.
    let mut foreign = json!({"anyOf":[{"$ref":"#/$defs/Fixture"},{"type":"null"}]});
    normalize_schema(&mut foreign);
    assert_eq!(foreign["anyOf"][0]["$ref"], "#/$defs/Fixture");

    let mut nested =
        json!({"type":"object","properties":{"rows":{"type":"array","items":{"enum":["a","b"]}}}});
    normalize_schema(&mut nested);
    assert_eq!(nested["properties"]["rows"]["items"]["type"], "string");
}

#[test]
fn workbuddy_nested_object_unions_follow_local_refs_and_preserve_data() {
    let mut schema = json!({"type":"object","$defs":{
        "a":{"type":"object"}, "alias":{"$ref":"#/$defs/a"},
        "cycle":{"$ref":"#/$defs/cycle"}
    }, "properties":{
        "valid":{"oneOf":[{"$ref":"#/$defs/alias"},{"type":"object"}]},
        "recursive":{"oneOf":[{"$ref":"#/$defs/cycle"}]},
        "external":{"oneOf":[{"$ref":"example.json#/A"}]},
        "mixed":{"oneOf":[{"type":"object"},{"type":"number"}]},
        "literal":{"examples":[{"oneOf":[{"type":"object"}]}]},
        "tags":{"type":["string","integer","null"],"enum":["x",2,null]},
        "required":{"type":"String"},
        "surrogates":{"type":"string","pattern":r"^[^\uD800-\uDFFF]*$"}
    },"additionalProperties":false});
    let original = schema.clone();
    normalize_schema(&mut schema);
    assert_eq!(schema["properties"]["valid"]["type"], "object");
    for unchanged in ["recursive", "external", "mixed", "literal"] {
        assert_eq!(
            schema["properties"][unchanged],
            original["properties"][unchanged]
        );
    }
    assert_eq!(
        schema["properties"]["tags"],
        json!({"anyOf":[{"type":"string","enum":["x"]},{"type":"integer","enum":[2]},{"type":"null"}]})
    );
    assert_eq!(schema["properties"]["required"]["type"], "string");
    assert_eq!(schema["properties"]["surrogates"], json!({"type":"string"}));
    assert_eq!(schema["additionalProperties"], false);
    let normalized = schema.clone();
    normalize_schema(&mut schema);
    assert_eq!(schema, normalized);
}

#[test]
fn workbuddy_rejection_codes_classify_fixed_numeric_metadata_only() {
    for body in [
        json!({"code":11128,"msg":"synthetic-refusal-detail"}),
        json!({"error":{"code":"11128","message":"synthetic-refusal-detail"}}),
        json!({"error":{"code":11128}}),
    ] {
        assert_eq!(
            rejection_code(body.to_string().as_bytes()),
            Some("routing_upstream_channel_restricted")
        );
    }
    for body in [
        json!({"code":11133,"msg":"synthetic-parameter-detail"}),
        json!({"error":{"code":"11133"}}),
    ] {
        assert_eq!(
            rejection_code(body.to_string().as_bytes()),
            Some("routing_upstream_invalid_parameters_rejected")
        );
    }
    for body in [
        json!({"code":"1302","msg":"synthetic"}),
        json!({"error":{"code":9999}}),
        json!({"message":"synthetic"}),
        json!("synthetic"),
    ] {
        assert_eq!(rejection_code(body.to_string().as_bytes()), None);
    }
    assert_eq!(rejection_code(b"synthetic"), None);
    assert_eq!(rejection_code(b""), None);
}

#[test]
fn workbuddy_status_codes_apply_only_to_bound_workbuddy_bad_requests() {
    let body = br#"{"code":11128}"#;
    for service in ["workbuddy", "workbuddy-ai", "github_copilot"] {
        let provider = Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({"routingSubscription":{"provider":service,"accountId":"fixture"}}),
            None,
        );
        assert_eq!(
            super::super::rejection_code(&provider, 400, body).is_some(),
            service != "github_copilot"
        );
        assert_eq!(super::super::rejection_code(&provider, 503, body), None);
    }
}

#[tokio::test]
async fn workbuddy_rejection_carrier_is_fixed_and_never_echoes_upstream_text(
) -> Result<(), Box<dyn std::error::Error>> {
    use axum::{routing::post, Router};
    let app = Router::new().route(
        "/",
        post(|| async {
            (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "application/json")],
                "{\"code\":11128,\"msg\":\"synthetic-refusal-detail synthetic-secret\"}",
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = format!("http://{}", listener.local_addr()?);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let response = reqwest::Client::new().post(&address).send().await?;
    assert_eq!(response.status(), 400);
    let error = rejection(400, response).await;
    let ProxyError::UpstreamError { status, body } = error else {
        return Err("unexpected error variant".into());
    };
    assert_eq!(status, 400);
    let body = body.ok_or("missing classified carrier")?;
    assert_eq!(
        rejection_code(body.as_bytes()),
        Some("routing_upstream_channel_restricted")
    );
    assert!(body.contains("11128"));
    assert!(!body.contains("synthetic-refusal-detail"));
    assert!(!body.contains("synthetic-secret"));
    task.abort();
    Ok(())
}

#[tokio::test]
async fn workbuddy_chat_stream_restores_sanitized_tool_names(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, names) = prepare(
        json!({"messages":[{"role":"user","content":"fixture"}],"tools":[{"type":"function","function":{"name":"github.create","parameters":{"type":"object"}}}]}),
        Protocol::Chat,
    )?;
    let frames = [
        "data: {\"id\":\"fixture\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_fixture\",\"function\":{\"name\":\"github_create\",\"arguments\":\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"q\\\":1}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"github_create stays literal\"}}]}\n\n",
        "data: [DONE]\n\n",
    ];
    let source: ByteStream = Box::pin(futures::stream::iter(
        frames
            .into_iter()
            .map(|frame| Ok(Bytes::from(frame.to_string()))),
    ));
    let text = drain_frames(restore_tool_names(source, names, Protocol::Chat)).await?;
    assert!(text.contains("\"github.create\""));
    assert!(text.contains("github_create stays literal"));
    assert!(text.contains("[DONE]"));
    Ok(())
}

#[tokio::test]
async fn workbuddy_anthropic_stream_restores_sanitized_tool_names(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, names) = prepare(
        json!({"messages":[{"role":"user","content":"fixture"}],"tools":[{"name":"github.create","input_schema":{"type":"object"}}]}),
        Protocol::Anthropic,
    )?;
    let frames = [
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"fixture\",\"content\":[]}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_fixture\",\"name\":\"github_create\",\"input\":{}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    ];
    let source: ByteStream = Box::pin(futures::stream::iter(
        frames
            .into_iter()
            .map(|frame| Ok(Bytes::from(frame.to_string()))),
    ));
    let text = drain_frames(restore_tool_names(source, names, Protocol::Anthropic)).await?;
    assert!(text.contains("\"name\":\"github.create\""));
    assert!(text.contains("event: content_block_start"));
    assert!(text.contains("event: message_stop"));
    Ok(())
}

#[tokio::test]
async fn workbuddy_stream_leaves_unmapped_names_and_framing_unchanged(
) -> Result<(), Box<dyn std::error::Error>> {
    let names = ToolNames::new();
    let frame = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"other_tool\",\"arguments\":\"{}\"}}]}}]}\n\n";
    let source: ByteStream = Box::pin(futures::stream::iter([Ok(Bytes::from(frame.to_string()))]));
    let text = drain_frames(restore_tool_names(source, names, Protocol::Chat)).await?;
    assert_eq!(text, frame);
    Ok(())
}

async fn drain_frames(mut stream: ByteStream) -> Result<String, Box<dyn std::error::Error>> {
    let mut text = String::new();
    while let Some(chunk) = stream.next().await {
        text.push_str(&String::from_utf8(chunk?.to_vec())?);
    }
    Ok(text)
}

#[test]
fn workbuddy_chat_rewrites_known_channel_fingerprints_without_changing_user_text(
) -> Result<(), ProxyError> {
    let input = json!({
        "model": "deepseek-v4.1-flash",
        "messages": [
            {"role": "system", "content": "You are an AI agent powered by DeepSeek Harness."},
            {"role": "assistant", "content": [{"type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude"}]},
            {"role": "assistant", "content": "web gui whose URL the runtime context names. When the user refers to this page"},
            {"role": "user", "content": "You are an AI agent powered by DeepSeek Harness."},
            {"role": "tool", "content": "x-anthropic-billing-header: fixture\nkept"}
        ]
    });
    let (result, names) = prepare(input, Protocol::Chat)?;
    assert!(names.is_empty());
    assert_eq!(
        result["messages"][0]["content"],
        "You are an AI agent, powered by DeepSeek Harness."
    );
    assert_eq!(
        result["messages"][1]["content"][0]["text"],
        "You are Claude Code, Anthropic's CLI for Claude"
    );
    assert_eq!(
        result["messages"][2]["content"],
        "Web GUI whose URL the runtime context names. The interface, when the user refers to this page"
    );
    assert_eq!(
        result["messages"][3]["content"],
        "You are an AI agent powered by DeepSeek Harness."
    );
    assert_eq!(
        result["messages"][4]["content"],
        "x-anthropic-billing-header: fixture\nkept"
    );
    Ok(())
}

#[test]
fn workbuddy_chat_preserves_claude_utf8_at_opener_boundary() -> Result<(), ProxyError> {
    let text = format!("You are {}中. You are here to help.", "x".repeat(111));
    let input = json!({
        "model": "glm-fixture",
        "max_tokens": 128,
        "system": text,
        "messages": [{"role": "user", "content": "fixture prompt"}]
    });
    let request = ir::parse::request(Protocol::Anthropic, &input, "/v1/messages")?;
    let chat = ir::build::request(&request, Protocol::Chat)?;
    let (result, _) = prepare(chat, Protocol::Chat)?;
    assert_eq!(result["messages"][0]["content"], input["system"]);
    assert_eq!(result["messages"][1], input["messages"][0]);
    Ok(())
}

#[test]
fn workbuddy_chat_fills_kimi_call_only_turns_and_repairs_tool_then_user() -> Result<(), ProxyError>
{
    let input = json!({
        "model": "kimi-k3.1-pro-highspeed",
        "messages": [
            {"role": "user", "content": "fixture"},
            {"role": "assistant", "content": null, "tool_calls": [{"id": "call-1", "type": "function", "function": {"name": "lookup", "arguments": "{}"}}]},
            {"role": "tool", "tool_call_id": "call-1", "content": "result"},
            {"role": "user", "content": "continue"}
        ]
    });
    let (result, _) = prepare(input, Protocol::Chat)?;
    assert_eq!(result["messages"][0]["role"], "system");
    assert_eq!(result["messages"][2]["content"], " ");
    assert_eq!(result["messages"][4]["role"], "assistant");
    assert_eq!(result["messages"][4]["content"], "");
    assert_eq!(result["messages"][5]["content"], "continue");

    let other = json!({
        "model": "glm-5.3",
        "messages": [
            {"role": "assistant", "content": "", "tool_calls": [{"id": "call-1", "type": "function", "function": {"name": "lookup", "arguments": "{}"}}]}
        ]
    });
    let (other, _) = prepare(other, Protocol::Chat)?;
    assert_eq!(other["messages"][1]["content"], "");
    Ok(())
}

#[test]
fn workbuddy_anthropic_hides_harness_prose_but_keeps_paths_and_tool_results(
) -> Result<(), ProxyError> {
    let input = json!({
        "model": "claude-fixture",
        "system": "You are an AI agent powered by DeepSeek Harness.",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "Current DeepSeek Harness implementation checkout: /root/DSH/Work"}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call-1", "content": "DeepSeek Harness"}]}
        ],
        "tools": [{"name": "lookup", "description": "Use DeepSeek Harness", "input_schema": {"type": "object"}}]
    });
    let (result, _) = prepare(input, Protocol::Anthropic)?;
    assert_eq!(result["system"], "You are an AI agent.");
    assert!(result["messages"][0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("/root/DSH/Work"));
    assert_eq!(
        result["messages"][1]["content"][0]["content"],
        "DeepSeek Harness"
    );
    assert_eq!(result["tools"][0]["description"], "Use the agent harness");
    Ok(())
}

#[tokio::test]
async fn workbuddy_ai_shares_chat_rewrites_and_sends_its_own_client_identity(
) -> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "model": "gpt-5.5",
        "messages": [
            {"role": "system", "content": "You are an AI agent powered by DeepSeek Harness."},
            {"role": "user", "content": "keep this exact"}
        ]
    });
    let (body, _) = prepare(input, Protocol::Chat)?;
    assert_eq!(
        body["messages"][0]["content"],
        "You are an AI agent, powered by DeepSeek Harness."
    );
    assert_eq!(body["messages"][1]["content"], "keep this exact");

    let account: accounts::Account = serde_json::from_value(json!({
        "id": "fixture",
        "provider": SubscriptionProvider::WorkbuddyAi,
        "login": "Fixture",
        "access": "synthetic",
        "workbuddy": {
            "uid": "fixture-user",
            "domain": "www.workbuddy.ai",
            "enterprise": "fixture-team",
            "refreshExpires": 0
        }
    }))?;
    let _ = body;
    let built = request(
        &reqwest::Client::new(),
        &account,
        "https://www.workbuddy.ai/v2",
        Protocol::Chat,
        &HeaderMap::new(),
        &Bytes::new(),
        false,
    )?
    .build()?;
    let headers = built.headers();
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(user_agent.contains("workbuddy-ai/"));
    assert!(!user_agent.contains("WorkBuddy/5.9.3 WorkBuddy/"));
    assert_eq!(headers["x-stainless-lang"], "js");
    assert_eq!(headers["x-stainless-package-version"], "6.25.0");
    assert_eq!(headers["x-stainless-runtime"], "node");
    assert_eq!(headers["x-stainless-runtime-version"], "v22.21.1");
    assert_eq!(headers["x-agent-type"], "main");
    assert_eq!(headers["x-agent-runtime"], "local-agent");
    assert_eq!(
        headers.get(header::CONTENT_ENCODING).map(|v| v.as_bytes()),
        Some(&b"gzip"[..])
    );
    assert_eq!(built.url().host_str(), Some("www.workbuddy.ai"));
    let mut outbound = body.clone();
    let mut conversation = HeaderMap::new();
    conversation_headers(&mut conversation, &account.id, Some("session"), &outbound)?;
    attach_growth_event(
        outbound.as_object_mut().ok_or("Missing chat object")?,
        Some("gpt-5.5"),
        conversation
            .get("x-conversation-id")
            .and_then(|value| value.to_str().ok()),
    );
    let events: Value = serde_json::from_str(
        outbound["extra_vars"]["growthEvent"]
            .as_str()
            .ok_or("Missing WorkBuddy AI growth event")?,
    )?;
    assert_eq!(events[0]["eventCode"], "chat_request_send");
    assert_eq!(events[0]["id"], conversation["x-conversation-id"].to_str()?);
    assert_eq!(events[0]["extra"]["requestModelId"], "gpt-5.5");
    assert_eq!(
        events[0]["extra"]["inputLength"],
        "keep this exact".chars().count()
    );
    assert_eq!(outbound["extra_vars"]["initiator"], "user");

    let china: accounts::Account = serde_json::from_value(json!({
        "id": "fixture",
        "provider": SubscriptionProvider::Workbuddy,
        "login": "Fixture",
        "access": "synthetic",
        "workbuddy": {
            "uid": "fixture-user",
            "domain": "copilot.tencent.com",
            "enterprise": "fixture-team",
            "refreshExpires": 0
        }
    }))?;
    let china_body = prepare(
        json!({"model":"glm-5.3","messages":[{"role":"user","content":"keep this exact"}]}),
        Protocol::Chat,
    )?
    .0;
    assert!(china_body.get("extra_vars").is_none());
    let user_agent = vendor::headers(&china)?
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(user_agent.contains("WorkBuddy/5.9.3 WorkBuddy/"));
    assert!(!user_agent.contains("workbuddy-ai/"));
    Ok(())
}

#[tokio::test]
async fn workbuddy_chat_strips_content_filter_text_for_both_editions(
) -> Result<(), Box<dyn std::error::Error>> {
    let frames = [
        "data: {\"choices\":[{\"delta\":{\"content\":\"Sorry, I can't\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" respond to this question.\"},\"finish_reason\":\"content_filter\"}]}\n\n",
        "data: [DONE]\n\n",
    ];
    let source: ByteStream = Box::pin(futures::stream::iter(
        frames
            .into_iter()
            .map(|frame| Ok(Bytes::from(frame.to_string()))),
    ));
    let text = drain_frames(restore_tool_names(source, ToolNames::new(), Protocol::Chat)).await?;
    assert!(!text.contains("Sorry, I can't respond"));
    assert!(
        text.contains("\"finish_reason\":\"content_filter\"")
            || text.contains("\"finish_reason\": \"content_filter\"")
    );
    assert!(text.contains("data: [DONE]"));

    let kept = "data: {\"choices\":[{\"delta\":{\"content\":\"ordinary fixture reply\"}}]}\n\n";
    let source: ByteStream = Box::pin(futures::stream::iter([Ok(Bytes::from(kept.to_string()))]));
    let text = drain_frames(restore_tool_names(source, ToolNames::new(), Protocol::Chat)).await?;
    assert_eq!(text, kept);
    Ok(())
}

#[test]
fn workbuddy_ai_activity_reports_follow_the_client_batches() {
    let account: accounts::Account = serde_json::from_value(json!({
        "id": "fixture",
        "provider": SubscriptionProvider::WorkbuddyAi,
        "login": "Fixture",
        "access": "synthetic",
        "workbuddy": {"uid": "fixture-user", "domain": "www.workbuddy.ai", "enterprise": "", "refreshExpires": 0}
    }))
    .expect("fixture account");
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-conversation-id",
        HeaderValue::from_static("conversation"),
    );
    headers.insert(
        "x-conversation-request-id",
        HeaderValue::from_static("turn"),
    );
    headers.insert(
        "x-conversation-message-id",
        HeaderValue::from_static("message"),
    );
    let body = json!({"model":"gpt-5.5","messages":[
        {"role":"user","content":"keep this exact"},
        {"role":"assistant","content":"","tool_calls":[{"id":"call"}]},
        {"role":"tool","content":"result"},
        {"role":"user","content":"next"}
    ]});
    let step = activity_step(&body, &headers, Some("GPT fixture"));
    let facts = reply_facts(
        br#"data: {"finish_reason":"stop","usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":9,"cached_tokens":1}}"#,
    );
    let batches = activity_batches(&account, &step, &facts, 10, 25);
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0][0]["eventCode"], "chat_message_send");
    assert_eq!(batches[0][0]["requestModelId"], "gpt-5.5");
    assert_eq!(batches[0][0]["requestModelName"], "GPT fixture");
    assert!(
        batches[0][0]["cpuModel"]
            .as_str()
            .is_some_and(|model| !model.is_empty())
    );
    assert!(batches[0][0]["memorySize"].as_u64().unwrap_or(0) > 0);
    assert_eq!(batches[1][0]["eventCode"], "chat_request_send");
    assert_eq!(batches[1][0]["inputLength"], "next".chars().count());
    assert!(batches[1][3].get("traceId").is_none());
    assert_eq!(batches[2][0]["eventCode"], "agent_task_completed");
    assert_eq!(batches[2][0]["duration_ms"], 15);
    assert_eq!(batches[2][1]["eventCode"], "chat_response_complete");
    let serialized = serde_json::to_string(&batches).expect("batches");
    assert!(!serialized.contains("synthetic"));
    let machine = batches[0][0]["machineId"].as_str().unwrap_or_default();
    assert_eq!(machine.len(), 64);
    assert_eq!(machine, report_machine_id());
    assert!(!batches[0][0]["sessionId"]
        .as_str()
        .unwrap_or_default()
        .is_empty());

    let tool_facts = reply_facts(br#"data: {"finish_reason":"tool_calls"}"#);
    let tool_batches = activity_batches(&account, &step, &tool_facts, 10, 25);
    assert_eq!(tool_batches.len(), 2);
    assert!(tool_batches[1]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| event["eventCode"] != "chat_response_complete"));
}

#[test]
fn workbuddy_remembers_a_system_repair_without_rewriting_user_text() {
    let body = json!({"model":"glm-5.3","messages":[
        {"role":"system","content":"You are an agent powered by Fixture."},
        {"role":"user","content":"You are an agent powered by Fixture."}
    ]});
    let key = system_key(&body);
    remember_repair(RepairKind::Comma, key);
    let mut next = body.clone();
    apply_known_repairs(&mut next);
    assert_eq!(
        next["messages"][0]["content"],
        "You are an agent, powered by Fixture."
    );
    assert_eq!(
        next["messages"][1]["content"],
        body["messages"][1]["content"]
    );
}

#[test]
fn workbuddy_classifies_restricted_and_exhausted_codes_without_echoing_text() {
    for (bytes, code) in [
        (
            &b"{\"code\":11140,\"msg\":\"secret restricted\"}"[..],
            "11140",
        ),
        (
            &b"{\"error\":{\"code\":\"14018\",\"message\":\"secret exhausted\"}}"[..],
            "14018",
        ),
        (
            &b"{\"code\":11102,\"error\":\"11158:secret entitlement\"}"[..],
            "11158",
        ),
    ] {
        match rejection_from(403, bytes) {
            ProxyError::UpstreamError { body, .. } => {
                let body = body.expect("carrier");
                assert!(body.contains(code));
                assert!(!body.contains("secret"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn workbuddy_keeps_a_named_nested_reasoning_effort() {
    let named = json!({"reasoning":{"effort":"low"}});
    let unnamed = json!({"max_tokens":2048});
    let small = json!({"max_tokens":64});
    assert!(names_effort(&named));
    assert!(!names_effort(&unnamed));
    assert!(small["max_tokens"].as_u64().is_some_and(|max| max <= 1024));
}

#[test]
fn workbuddy_moves_a_refused_system_into_tagged_user_instructions() {
    let body = json!({"messages":[
        {"role":"system","content":"Fixture policy"},
        {"role":"system","content":[{"type":"text","text":"second line"}]},
        {"role":"user","content":"keep this exact"}
    ]});
    let moved = move_refused_system(&body).expect("moved");
    assert_eq!(
        moved["messages"][0]["content"],
        "You are a helpful assistant."
    );
    assert_eq!(moved["messages"][1]["role"], "user");
    assert_eq!(
        moved["messages"][1]["content"],
        "<instructions>\nFixture policy\n\nsecond line\n</instructions>\n\nkeep this exact"
    );
}

#[test]
fn workbuddy_drops_rejected_thinking_signatures_without_echoing_upstream_text() {
    let body = json!({"messages":[{"role":"assistant","content":[
        {"type":"thinking","thinking":"foreign","signature":"fixture"},
        {"type":"text","text":"keep this exact"}
    ]}]});
    let dropped = drop_signed_thinking(&body).expect("dropped");
    assert_eq!(
        dropped["messages"][0]["content"],
        json!([{"type":"text","text":"keep this exact"}])
    );
    let bytes = br#"{"error":{"code":11159,"message":"secret upstream text"}}"#;
    assert!(bad_signature(bytes));
    match rejection_from(400, bytes) {
        ProxyError::UpstreamError { body, .. } => {
            let body = body.expect("carrier");
            assert!(body.contains("11159"));
            assert!(!body.contains("secret upstream text"));
        }
        other => panic!("unexpected {other:?}"),
    }
}
