//! Concurrent forwarding contracts from Magpie's affinity.go and gateway.go.
use super::*;
use crate::{
    database::Database,
    proxy::{server::ProxyServer, ProxyConfig},
};
use axum::{routing::post, Json, Router};
use std::sync::atomic::{AtomicUsize, Ordering};

struct RestoreSecurity(crate::proxy::security::SecurityRuntimeConfig);
impl Drop for RestoreSecurity {
    fn drop(&mut self) {
        crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn magpie_concurrent_responses_do_not_hold_a_conversation_lock(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = RestoreSecurity((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let sends = Arc::new(AtomicUsize::new(0));
    let seen = sends.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(move |Json(_body): Json<Value>| {
            let seen = seen.clone();
            async move {
                let index = seen.fetch_add(1, Ordering::SeqCst);
                let stream = async_stream::stream! {
                    let event = json!({"type":"response.created","response":{"id":format!("synthetic-{index}"),"status":"in_progress","output":[]}});
                    yield Ok::<_, std::io::Error>(bytes::Bytes::from(format!("event: response.created\ndata: {event}\n\n")));
                    std::future::pending::<()>().await;
                };
                let mut response = Response::new(Body::from_stream(stream));
                response.headers_mut().insert(header::CONTENT_TYPE, http::HeaderValue::from_static("text/event-stream"));
                response
            }
        }))).await;
    });
    let client = reqwest::Client::builder().no_proxy().build()?;
    let runtime = Arc::new(Runtime::default());
    let token = runtime.install(Session {
        group: serde_json::from_value(json!({
            "id":"synthetic-concurrency","name":"Synthetic concurrency","revision":0,"client":"codex","enabled":false,
            "members":[{"id":"one","providerId":"synthetic-provider","modelId":"synthetic-model"}],
            "defaultMemberId":"one","fallbackMemberIds":[],"intentRecognitionEnabled":false,
            "classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
        }))?,
        targets: vec![Target {
            provider_id: "synthetic-provider".into(), model: "synthetic-model".into(),
            url: format!("http://{address}/responses").parse()?, headers: Default::default(),
            upstream: None, official: false, credentials: None, effort: Default::default(),
            fixed_effort: None, scheduling: Arc::default(),
        }],
        classifier: None, client: client.clone(), created: Instant::now(), bindings: Arc::default(),
    }, None).map_err(|_| "synthetic install failed")?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await?;
    let url = format!("http://127.0.0.1:{}/intent/v1/responses", info.port);
    // Keep every response open: later requests must reach the upstream before
    // earlier streams end, including retries/side requests in the same session.
    let mut responses = Vec::new();
    for (index, session) in [
        Some("synthetic-a"),
        Some("synthetic-a"),
        Some("synthetic-b"),
        None,
        None,
    ]
    .into_iter()
    .enumerate()
    {
        let mut body = json!({
            "model":"vary-auto/synthetic-concurrency", "stream":true,
            "input":[{"type":"reasoning","summary":[]},
                {"role":"user","content":"<environment_context>synthetic cwd</environment_context>"},
                {"role":"user","content":format!("synthetic question {index}")}]
        });
        if session.is_some() {
            body["prompt_cache_key"] = json!("synthetic-shared-cache");
        }
        let mut request = client.post(&url).bearer_auth(&token).json(&body);
        if let Some(session) = session {
            request = request.header("session_id", session);
        }
        let mut response = tokio::time::timeout(Duration::from_secs(5), request.send()).await??;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "concurrent request {index}"
        );
        let chunk = tokio::time::timeout(Duration::from_secs(5), response.chunk()).await??;
        assert!(chunk.is_some(), "initial stream event {index}");
        responses.push(response);
    }
    assert_eq!(sends.load(Ordering::SeqCst), 5);
    let lane = runtime
        .sessions
        .read()
        .map_err(|_| "synthetic sessions poisoned")?
        .get(&token)
        .map(|entry| entry.snapshot.bindings.clone())
        .ok_or("missing synthetic session")?;
    let scope =
        runtime
            .conversations
            .acquire("local-codex", &lane, Some("synthetic-a"), &json!({}))?;
    assert!(
        scope.bindings.try_lock().is_ok(),
        "an open stream retains no mutex guard"
    );
    scope.update(|bindings| {
        for index in [0, 1] {
            assert_eq!(
                bindings.responses.get(&continuation::hash(
                    "response",
                    &format!("synthetic-{index}")
                )),
                Some(&0),
                "overlapping streams preserve both observed owners"
            );
        }
    })?;
    let other =
        runtime
            .conversations
            .acquire("local-codex", &lane, Some("synthetic-b"), &json!({}))?;
    assert!(
        !Arc::ptr_eq(&scope, &other),
        "shared cache keys do not merge native sessions"
    );
    drop(responses);
    // Cancellation also leaves no exclusive conversation lease behind.
    let response = tokio::time::timeout(Duration::from_secs(5), client.post(&url)
        .bearer_auth(&token).header("session_id", "synthetic-a")
        .json(&json!({"model":"vary-auto/synthetic-concurrency","input":"synthetic after cancellation","stream":true}))
        .send()).await??;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(sends.load(Ordering::SeqCst), 6);
    drop(response);
    tokio::time::timeout(Duration::from_secs(5), server.stop()).await??;
    task.abort();
    Ok(())
}
