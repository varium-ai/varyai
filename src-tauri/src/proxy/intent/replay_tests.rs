//! Codex Responses replay follows Magpie's upstream validation and affinity.
use super::*;
use crate::{
    database::Database,
    proxy::{server::ProxyServer, ProxyConfig},
};
use axum::{response::IntoResponse, routing::post, Json, Router};
use std::sync::Mutex;

#[tokio::test]
#[serial_test::serial]
async fn codex_new_conversations_forward_optional_and_unobserved_state_intact(
) -> Result<(), Box<dyn std::error::Error>> {
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(move |Json(body): Json<Value>| {
            let captured = captured.clone();
            async move {
                if let Ok(mut seen) = captured.lock() { seen.push(body.clone()); }
                let sealed = body["input"].as_array().is_some_and(|items| items.iter().any(|item| matches!(item["type"].as_str(), Some("reasoning" | "compaction"))));
                if body["synthetic_recovery"] == true && sealed {
                    return (StatusCode::BAD_REQUEST, Json(json!({"error":{"code":"invalid_encrypted_content"}}))).into_response();
                }
                if body["synthetic_unrelated_error"] == true {
                    return (StatusCode::CONFLICT, Json(json!({"error":{"code":"synthetic_conflict"}}))).into_response();
                }
                Json(json!({"id":"synthetic-response","model":"synthetic-model","status":"completed","output":[]})).into_response()
            }
        }))).await;
    });
    let runtime = Arc::new(Runtime::default());
    let client = reqwest::Client::builder().no_proxy().build()?;
    let group: RoutingGroup = serde_json::from_value(json!({
        "id":"synthetic-replay","name":"Synthetic replay","revision":0,"client":"codex","enabled":false,
        "members":[{"id":"one","providerId":"synthetic-provider","modelId":"synthetic-model"}],
        "defaultMemberId":"one","fallbackMemberIds":[],"intentRecognitionEnabled":false,
        "classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
    }))?;
    let session = Session {
        group,
        targets: vec![Target {
            provider_id: "synthetic-provider".into(),
            model: "synthetic-model".into(),
            url: format!("http://{address}/responses").parse()?,
            headers: Default::default(),
            upstream: None,
            official: false,
            credentials: None,
            effort: Default::default(),
            fixed_effort: None,
            scheduling: Arc::default(),
        }],
        classifier: None,
        client: client.clone(),
        created: Instant::now(),
        bindings: Arc::default(),
    };
    let token = runtime
        .install(session, None)
        .map_err(|_| "synthetic session failed")?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await?;
    let url = format!("http://127.0.0.1:{}/intent/v1/responses", info.port);
    let cases = [
        json!({"input":[{"type":"message","role":"user","content":"hello","encrypted_content":null}]}),
        json!({"input":[{"type":"reasoning","id":"synthetic-empty-reasoning","summary":[],"content":null,"encrypted_content":null},{"role":"user","content":"hello"}]}),
        json!({"previous_response_id":"synthetic-unobserved-response","input":"hello"}),
        json!({"input":[{"type":"function_call","id":"synthetic-call-item","call_id":"synthetic-call","name":"synthetic","arguments":"{}"},{"type":"function_call_output","call_id":"synthetic-call","output":"synthetic"},{"role":"user","content":"hello"}]}),
        json!({"input":[{"type":"reasoning","encrypted_content":"synthetic-unobserved-seal","summary":[]},{"type":"item_reference","id":"synthetic-unobserved-item"},{"role":"user","content":"hello"}]}),
    ];
    for (index, mut body) in cases.into_iter().enumerate() {
        body["model"] = json!("vary-auto/synthetic-replay");
        // Exercise both a completely new named session and clients without headers.
        for named in [false, true] {
            let mut request = client.post(&url).bearer_auth(&token).json(&body);
            if named {
                request = request.header("session-id", format!("synthetic-fresh-{index}"));
            }
            let response = request.send().await?;
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "case {index}, named={named}"
            );
            let _: Value = response.json().await?;
            let mut expected = body.clone();
            expected["model"] = json!("synthetic-model");
            assert_eq!(
                seen.lock()
                    .map_err(|_| "synthetic capture poisoned")?
                    .last(),
                Some(&expected)
            );
        }
    }
    let before = seen.lock().map_err(|_| "synthetic capture poisoned")?.len();
    let recovery_body = json!({"model":"vary-auto/synthetic-replay","synthetic_recovery":true,"vendor_extension":{"keep":true},"input":[
        {"type":"reasoning","encrypted_content":"synthetic-reasoning-seal"},
        {"type":"compaction","encrypted_content":"synthetic-compaction-seal"},
        {"type":"message","role":"user","content":"hello","encrypted_content":"synthetic-message-seal"}
    ]});
    let response = client
        .post(&url)
        .bearer_auth(&token)
        .header("session-id", "synthetic-recovery-session")
        .json(&recovery_body)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _: Value = response.json().await?;
    {
        let seen = seen.lock().map_err(|_| "synthetic capture poisoned")?;
        assert_eq!(
            seen.len(),
            before + 3,
            "two bounded retries at the same upstream"
        );
        assert_eq!(seen[before]["input"], recovery_body["input"]);
        assert_eq!(seen[before + 1]["input"].as_array().map(Vec::len), Some(2));
        assert_eq!(seen[before + 1]["input"][0]["type"], "compaction");
        assert_eq!(
            seen[before + 2]["input"],
            json!([{"type":"message","role":"user","content":"hello"}])
        );
        for body in &seen[before..] {
            assert_eq!(body["model"], "synthetic-model");
            assert_eq!(body["vendor_extension"], recovery_body["vendor_extension"]);
        }
    }
    let unrelated = client.post(&url).bearer_auth(&token).json(&json!({"model":"vary-auto/synthetic-replay","synthetic_unrelated_error":true,"input":[{"type":"reasoning","encrypted_content":"synthetic-other-seal"}]})).send().await?;
    assert_eq!(unrelated.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        seen.lock().map_err(|_| "synthetic capture poisoned")?.len(),
        before + 4,
        "unrelated conflicts must not trigger replay"
    );
    let count = seen.lock().map_err(|_| "synthetic capture poisoned")?.len();
    let unauthorized = client
        .post(&url)
        .bearer_auth("synthetic-wrong-token")
        .json(&json!({"model":"vary-auto/synthetic-replay","input":"hello"}))
        .send()
        .await?;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        seen.lock().map_err(|_| "synthetic capture poisoned")?.len(),
        count
    );
    server.stop().await?;
    task.abort();
    Ok(())
}

#[test]
fn codex_replay_scope_discovers_known_owner_among_unobserved_items(
) -> Result<(), Box<dyn std::error::Error>> {
    let scopes = scopes::Conversations::default();
    let session = replay_scope_session()?;
    let initial = json!({"input":[{"type":"reasoning","summary":[]}]});
    let first = scopes.acquire_durable_with_policy(
        "local-codex",
        &session,
        None,
        &initial,
        None,
        scopes::HistoryPolicy::ResponsesReplay,
    )?;
    {
        let mut bindings = first.bindings.try_lock()?;
        bindings.observe(&json!({"id":"synthetic-owned-response","output":[{"type":"message","id":"synthetic-owned-message","encrypted_content":"synthetic-owned-seal"}]}), 1);
    }
    let replay = json!({"previous_response_id":"synthetic-owned-response","input":[{"type":"function_call_output","call_id":"synthetic-new-call","output":"synthetic"}]});
    let restored = scopes.acquire_durable_with_policy(
        "local-codex",
        &session,
        None,
        &replay,
        None,
        scopes::HistoryPolicy::ResponsesReplay,
    )?;
    assert!(Arc::ptr_eq(&first, &restored));
    assert_eq!(
        restored.bindings.try_lock()?.resolve_replay(&replay, None),
        Ok(Some(1))
    );
    let ordinary = json!({"input":[{"type":"message","id":"synthetic-owned-message","encrypted_content":"synthetic-owned-seal","role":"assistant","content":"synthetic"}]});
    let ordinary_scope = scopes.acquire_durable_with_policy(
        "local-codex",
        &session,
        None,
        &ordinary,
        None,
        scopes::HistoryPolicy::ResponsesReplay,
    )?;
    assert!(Arc::ptr_eq(&first, &ordinary_scope));
    assert_eq!(
        ordinary_scope
            .bindings
            .try_lock()?
            .resolve_replay(&ordinary, None),
        Ok(Some(1))
    );
    Ok(())
}

#[tokio::test]
async fn codex_replay_restores_remaining_affinity_after_destination_set_changes(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let store =
        continuation::Store::load(dir.path().join("state.json"), dir.path().join("state.key"))
            .await?;
    let session = replay_scope_session()?;
    let scopes = scopes::Conversations::default();
    let first = scopes.acquire_durable_with_policy(
        "local-codex",
        &session,
        Some("synthetic-thread"),
        &json!({"input":"hello"}),
        Some(store.clone()),
        scopes::HistoryPolicy::ResponsesReplay,
    )?;
    {
        let mut bindings = first.bindings.lock().await;
        bindings.bind_session("synthetic-thread", 0);
        bindings.observe(&json!({"id":"synthetic-owned-response"}), 0);
    }
    store.flush().await?;
    let mut next = replay_scope_session()?;
    let mut added = next.targets[0].clone();
    added.provider_id = "synthetic-added-provider".into();
    next.targets.insert(0, added);
    let body = json!({"previous_response_id":"synthetic-owned-response","input":[{"type":"reasoning","summary":[]}]});
    let restored = scopes::Conversations::default().acquire_durable_with_policy(
        "local-codex",
        &next,
        Some("synthetic-thread"),
        &body,
        Some(store.clone()),
        scopes::HistoryPolicy::ResponsesReplay,
    )?;
    assert_eq!(
        restored
            .bindings
            .lock()
            .await
            .resolve_replay(&body, Some("synthetic-thread")),
        Ok(Some(1))
    );
    next.targets.pop();
    let removed = scopes::Conversations::default().acquire_durable_with_policy(
        "local-codex",
        &next,
        Some("synthetic-thread"),
        &body,
        Some(store),
        scopes::HistoryPolicy::ResponsesReplay,
    )?;
    assert_eq!(
        removed
            .bindings
            .lock()
            .await
            .resolve_replay(&body, Some("synthetic-thread")),
        Ok(None)
    );
    Ok(())
}

fn replay_scope_session() -> Result<Session, Box<dyn std::error::Error>> {
    Ok(Session {
        group: serde_json::from_value(json!({
            "id":"synthetic-replay","name":"Synthetic replay","revision":0,"client":"codex","enabled":false,
            "members":[],"defaultMemberId":"one","fallbackMemberIds":[],"intentRecognitionEnabled":false,
            "classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
        }))?,
        targets: vec![Target {
            provider_id: "synthetic-provider".into(),
            model: "synthetic-model".into(),
            url: "https://synthetic.invalid/responses".parse()?,
            headers: Default::default(),
            upstream: None,
            official: false,
            credentials: None,
            effort: Default::default(),
            fixed_effort: None,
            scheduling: Arc::default(),
        }],
        classifier: None,
        client: reqwest::Client::new(),
        created: Instant::now(),
        bindings: Arc::default(),
    })
}
