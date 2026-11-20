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
    let result = prepare(input.clone(), Protocol::Chat)?;
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
    let actual = prepare(input.clone(), Protocol::Anthropic)?;
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
    Ok(())
}

#[tokio::test]
async fn regional_http_transport_delivers_structured_streamed_tool_calls(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::services::routing_upstreams::SubscriptionProvider;
    use axum::{extract::Request, routing::post, Router};

    let chat = concat!(
        "data: {\"id\":\"fixture\",\"model\":\"claude-fixture\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_fixture\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"q\\\":1}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let messages = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"fixture\",\"model\":\"claude-fixture\",\"content\":[]}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_fixture\",\"name\":\"lookup\",\"input\":{}}}\n\n",
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

    for (provider, protocol, path) in [
        (
            SubscriptionProvider::Workbuddy,
            Protocol::Anthropic,
            "/v1/messages",
        ),
        (
            SubscriptionProvider::WorkbuddyAi,
            Protocol::Chat,
            "/v2/chat/completions",
        ),
    ] {
        let account: accounts::Account = serde_json::from_value(json!({
            "id":"fixture", "provider":provider, "login":"Fixture", "access":"synthetic",
            "workbuddy":{"uid":"fixture-user","domain":"fixture.example","enterprise":"fixture-team","refreshExpires":0}
        }))?;
        let body = prepare(
            json!({"model":"claude-fixture","messages":[{"role":"user","content":"fixture prompt"}]}),
            protocol,
        )?;
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
        let reply = ir::stream::collect(source, protocol).await?;
        for destination in [
            Protocol::Chat,
            Protocol::Anthropic,
            Protocol::Responses,
            Protocol::Gemini,
        ] {
            let rendered =
                ir::reply::render_with_context(destination, &reply, &Default::default())?;
            let rendered = rendered.to_string();
            assert!(rendered.contains("lookup"));
            assert!(rendered.contains("call_fixture"));
        }
    }
    Ok(())
}
