use super::trace::*;
use std::{sync::Arc, time::Instant};

fn candidate() -> Candidate {
    Candidate {
        key_id: None,
        provider_id: "fixture".into(),
        provider_name: "Synthetic".into(),
        model: "model-a".into(),
        fixed_effort: None,
        context_window: Some(128000),
        account: None,
        scheduling: Default::default(),
    }
}

#[test]
fn key_pool_running_counts_follow_attempts_completion_and_cancellation(
) -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(Store::default());
    let target = observed_target("synthetic-private-key")?;
    let request = store.begin("codex", "fixture", "Fixture", "ordered", Instant::now());
    let primary_id = store
        .candidate(&target)
        .key_id
        .ok_or("missing observation")?;
    let mut backup = store.candidate(&target);
    backup.key_id = Some("backup".into());
    request.plan(vec![store.candidate(&target), backup], None);
    request.step(Kind::Attempt, Some(0), None);
    assert_eq!(
        store.key_running("synthetic-provider", &primary_id),
        Some(1)
    );
    request.step(Kind::Attempt, Some(1), None);
    assert_eq!(
        store.key_running("synthetic-provider", &primary_id),
        Some(0)
    );
    assert_eq!(store.key_running("synthetic-provider", "backup"), Some(1));
    let encoded = serde_json::to_string(&store.snapshot())?;
    assert!(!encoded.contains("keyId") && !encoded.contains("backup"));
    drop(request);
    assert_eq!(store.key_running("synthetic-provider", "backup"), Some(0));
    Ok(())
}

fn observed_target(key: &str) -> Result<super::Target, Box<dyn std::error::Error>> {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::AUTHORIZATION,
        format!("Bearer {key}").parse()?,
    );
    Ok(super::Target {
        provider_id: "synthetic-provider".into(),
        model: "model-a".into(),
        fixed_effort: None,
        upstream: Some(crate::provider::Provider::with_id(
            "synthetic-provider".into(),
            "Synthetic vendor".into(),
            serde_json::json!({}),
            None,
        )),
        effort: Default::default(),
        url: "http://127.0.0.1:1/responses".parse()?,
        headers,
        official: false,
        credentials: None,
        scheduling: Arc::new(super::scheduling::Scheduling::new(false)),
    })
}

#[test]
fn candidate_labels_only_expose_masked_configured_api_keys(
) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::default();
    for (key, expected) in [
        ("sk-4-synthetic-private-766b", "sk-4…766b"),
        ("short", "••••••••"),
        ("密钥示例synthetic-private结束字符", "密钥示例…结束字符"),
    ] {
        let mut target = observed_target(key)?;
        target
            .upstream
            .as_mut()
            .ok_or("missing provider")?
            .settings_config =
            serde_json::json!({ "auth": { "OPENAI_API_KEY": key }, "routingKeyLabel": expected });
        let value = serde_json::to_value(store.candidate(&target))?;
        assert_eq!(value["account"]["label"], expected);
        assert!(!value.to_string().contains(key));
        target.official = true;
        let official = serde_json::to_value(store.candidate(&target))?;
        assert!(official["account"].get("label").is_none());
        target.official = false;
        target
            .upstream
            .as_mut()
            .ok_or("missing provider")?
            .settings_config["routingSubscription"] = serde_json::json!({"provider":"xai_oauth"});
        assert!(serde_json::to_value(store.candidate(&target))?["account"]
            .get("label")
            .is_none());
    }
    let value =
        serde_json::to_value(store.candidate(&observed_target("synthetic-private-header-token")?))?;
    assert!(value["account"].get("label").is_none());
    let account: Account = serde_json::from_value(value["account"].clone())?;
    assert!(serde_json::to_value(account)?.get("label").is_none());
    let mut single = observed_target("sk-4-synthetic-private-766b")?;
    single
        .upstream
        .as_mut()
        .ok_or("missing provider")?
        .settings_config = serde_json::json!({"auth":{"OPENAI_API_KEY":"fixture"}});
    let value = serde_json::to_value(store.candidate(&single))?;
    assert_eq!(value["account"]["kind"], "provider");
    assert!(value["account"].get("label").is_none());
    Ok(())
}

#[test]
fn candidate_names_follow_the_enabled_distinct_key_pool() -> Result<(), Box<dyn std::error::Error>>
{
    let store = Store::default();
    for (off, backup, enabled, expected) in [
        (false, "fixture-other-key", true, vec!["Personal", "Backup"]),
        (false, "fixture-other-key", false, vec!["Synthetic vendor"]),
        (true, "fixture-other-key", true, vec!["Synthetic vendor"]),
        (
            false,
            "fixture-primary-key",
            true,
            vec!["Synthetic vendor", "Synthetic vendor"],
        ),
    ] {
        let mut target = observed_target("fixture-primary-key")?;
        let provider = target.upstream.as_mut().ok_or("missing provider")?;
        provider.settings_config = serde_json::json!({
            "auth":{"OPENAI_API_KEY":"fixture-primary-key"},
            "apiFormat":"openai_chat",
            "routingImportedTransport":{"primaryName":"Personal", "primaryOff":off,
                "keys":[{"id":"backup","name":"Backup","key":backup,"enabled":enabled}]}
        });
        let variants = crate::services::routing_upstreams::variants(provider);
        assert_eq!(variants.len(), expected.len());
        for (provider, expected) in variants.into_iter().zip(expected) {
            target.upstream = Some(provider);
            let candidate = store.candidate(&target);
            let account = candidate.account.ok_or("missing account")?;
            assert_eq!(
                account.label.as_deref().unwrap_or(&candidate.provider_name),
                expected
            );
            assert!(
                account.kind
                    == if expected == "Synthetic vendor" {
                        AccountKind::Provider
                    } else {
                        AccountKind::Key
                    }
            );
        }
    }
    Ok(())
}

#[test]
fn account_observations_follow_requests_and_never_enumerate_the_model_catalog(
) -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(Store::default());
    assert!(store.live_accounts(&store.snapshot()).is_empty());
    let target = observed_target("synthetic-private-key")?;
    let trace = store.begin("codex", "fixture", "Fixture", "ordered", Instant::now());
    trace.plan(vec![store.candidate(&target)], None);
    assert_eq!(store.live_accounts(&store.snapshot()).len(), 1);
    let original = store.snapshot()[0].candidates[0]
        .account
        .clone()
        .ok_or("missing observation")?;
    target
        .scheduling
        .failed(&Default::default(), chrono::Utc::now().timestamp());
    let current = store.live_accounts(&store.snapshot());
    assert!(current[0].telemetry.rest_until > original.telemetry.rest_until);
    assert_eq!(
        store.snapshot()[0].candidates[0]
            .account
            .as_ref()
            .ok_or("missing recorded observation")?
            .telemetry
            .rest_until,
        original.telemetry.rest_until
    );
    for _ in 0..CAPACITY {
        drop(store.begin("codex", "fixture", "Fixture", "ordered", Instant::now()));
    }
    assert!(store.live_accounts(&store.snapshot()).is_empty());
    Ok(())
}

#[test]
fn account_observation_identity_survives_model_changes_but_not_key_rotation_or_runtime_changes(
) -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(Store::default());
    let mut target = observed_target("synthetic-private-key")?;
    let original = store.candidate(&target).account.ok_or("missing account")?;
    target.model = "model-b".into();
    assert_eq!(
        store
            .candidate(&target)
            .account
            .ok_or("missing model-b")?
            .id,
        original.id
    );
    let other = observed_target("synthetic-private-other-key")?;
    assert_ne!(
        store.candidate(&other).account.ok_or("missing other")?.id,
        original.id
    );
    assert_ne!(
        Store::default()
            .candidate(&target)
            .account
            .ok_or("missing other runtime")?
            .id,
        original.id
    );
    let serialized = serde_json::to_string(&store.candidate(&target))?;
    assert!(!serialized.contains("synthetic-private"));
    assert!(!serialized.contains("127.0.0.1"));
    assert!(serialized.contains("Synthetic vendor"));
    target.official = true;
    target.upstream = None;
    assert_eq!(store.candidate(&target).provider_name, "Codex");
    Ok(())
}

#[tokio::test]
async fn managed_account_observations_keep_configured_names_and_separate_sign_ins(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
    use crate::services::intent_routing::catalog::freeze_execution;
    let dir = tempfile::tempdir()?;
    let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
    manager
        .add_test_account_with_user_identity("account-a", "synthetic-private-token-a", "user-a")
        .await?;
    manager
        .add_test_account_with_user_identity("account-b", "synthetic-private-token-b", "user-b")
        .await?;
    let mut provider = crate::provider::Provider::with_id(
        "synthetic-uuid".into(),
        "Synthetic subscription".into(),
        serde_json::json!({}),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        provider_type: Some("codex_oauth".into()),
        ..Default::default()
    });
    let mut target = observed_target("synthetic-private-key")?;
    target.official = true;
    target.upstream = None;
    target.credentials = Some(Arc::new(
        freeze_execution(&provider, manager.clone())
            .await
            .ok_or("missing account-a")?,
    ));
    let store = Store::default();
    let first = store.candidate(&target);
    assert_eq!(first.provider_name, "Synthetic subscription");
    manager.set_default_account("account-b").await?;
    target.credentials = Some(Arc::new(
        freeze_execution(&provider, manager)
            .await
            .ok_or("missing account-b")?,
    ));
    let second = store.candidate(&target);
    assert_ne!(
        first.account.as_ref().map(|a| &a.id),
        second.account.as_ref().map(|a| &a.id)
    );
    assert!(!serde_json::to_string(&[first, second])?.contains("synthetic-private"));
    Ok(())
}

#[test]
fn request_purpose_uses_explicit_markers_without_retaining_other_metadata(
) -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(Store::default());
    for (marker, metadata, compact, expected) in [
        (None, None, false, None),
        (
            Some("guardian"),
            Some(r#"{"thread_source":"thread_title"}"#),
            false,
            Some("guardian"),
        ),
        (
            None,
            Some(r#"{"thread_source":"thread_title","session_id":"synthetic-private-session"}"#),
            false,
            Some("thread_title"),
        ),
        (None, Some(r#"{"thread_source":"user"}"#), false, None),
        (None, Some(r#"{"thread_source":"subagent"}"#), false, None),
        (
            None,
            Some(r#"{"thread_source":"thread\u005ftitle"}"#),
            false,
            Some("thread_title"),
        ),
        (None, Some(r#"{"thread_source":42}"#), false, None),
        (None, Some("{"), false, None),
        (
            Some("synthetic-private-unknown"),
            None,
            false,
            Some("unknown"),
        ),
        (Some("thread_title"), None, true, Some("compact")),
    ] {
        let trace = store.begin("codex", "fixture", "Fixture", "ordered", Instant::now());
        let mut headers = http::HeaderMap::new();
        if let Some(marker) = marker {
            headers.insert("x-openai-subagent", marker.parse()?);
        }
        if let Some(metadata) = metadata {
            headers.insert("x-codex-turn-metadata", metadata.parse()?);
        }
        headers.insert(
            http::header::AUTHORIZATION,
            "Bearer synthetic-private-credential".parse()?,
        );
        let original = headers.clone();
        trace.purpose(&headers, compact);
        assert_eq!(headers, original);
        let row = serde_json::to_value(&store.snapshot()[0])?;
        assert_eq!(
            row.get("kind").and_then(serde_json::Value::as_str),
            expected
        );
        assert!(!row.to_string().contains("synthetic-private"));
    }
    for marker in [
        "thread_title_reconsideration",
        "title_generation",
        "memgen",
        "composer_predictions",
        "ambient_suggestion_safety",
        "agent_job",
    ] {
        let trace = store.begin("codex", "fixture", "Fixture", "ordered", Instant::now());
        let mut headers = http::HeaderMap::new();
        headers.insert("x-openai-subagent", marker.parse()?);
        trace.purpose(&headers, false);
        assert_eq!(serde_json::to_value(&store.snapshot()[0])?["kind"], marker);
    }
    Ok(())
}

#[test]
fn request_purpose_observation_is_bounded_and_nonblocking() -> Result<(), Box<dyn std::error::Error>>
{
    let store = Arc::new(Store::default());
    let trace = store.begin("codex", "fixture", "Fixture", "ordered", Instant::now());
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "x-codex-turn-metadata",
        format!(
            r#"{{"thread_source":"thread_title","other":"{}"}}"#,
            "x".repeat(8192)
        )
        .parse()?,
    );
    trace.purpose(&headers, false);
    assert!(serde_json::to_value(&store.snapshot()[0])?
        .get("kind")
        .is_none());
    headers.insert("x-openai-subagent", "thread_title".parse()?);
    let lock = store.rows.write().map_err(|_| "synthetic lock failed")?;
    trace.purpose(&headers, false);
    drop(lock);
    assert!(serde_json::to_value(&store.snapshot()[0])?
        .get("kind")
        .is_none());
    Ok(())
}

#[test]
fn trace_records_real_attempts_and_keeps_only_bounded_safe_metadata() {
    let store = Arc::new(Store::default());
    let trace = store.begin(
        "codex",
        "group/fixture",
        "Fixture",
        "ordered",
        Instant::now(),
    );
    trace.session("synthetic-private-session", "synthetic-private-consumer");
    trace.plan(vec![candidate()], Some(512));
    trace.step(Kind::Attempt, Some(0), None);
    trace.step(Kind::Rejected, Some(0), Some(429));
    trace.finish(503, State::Failed);
    let rows = store.snapshot();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].steps[1].kind, Kind::Attempt);
    assert_eq!(rows[0].steps[2].status, Some(429));
    assert_eq!(rows[0].state, State::Failed);
    let serialized = serde_json::to_string(&rows).expect("synthetic trace");
    assert!(!serialized.contains("synthetic-private"));
    assert!(!serialized.contains("headers"));
    for _ in 0..70 {
        drop(store.begin("codex", "group/fixture", "Fixture", "smart", Instant::now()));
    }
    assert_eq!(store.snapshot().len(), CAPACITY);
}

#[test]
fn dropped_trace_is_cancelled_including_an_unpolled_stream() {
    let store = Arc::new(Store::default());
    let trace = store.begin(
        "claude",
        "group/fixture",
        "Fixture",
        "smart",
        Instant::now(),
    );
    trace.plan(vec![candidate()], None);
    trace.step(Kind::Attempt, Some(0), None);
    drop(trace);
    assert_eq!(store.snapshot()[0].state, State::Cancelled);
    assert_eq!(store.snapshot()[0].status, Some(499));
}

#[test]
fn trace_contention_never_blocks_or_panics_in_forwarding() {
    let store = Arc::new(Store::default());
    let trace = store.begin("codex", "group/fixture", "Fixture", "smart", Instant::now());
    let lock = store.rows.write().expect("test lock");
    trace.step(Kind::Attempt, Some(0), None);
    trace.finish(200, State::Completed);
    drop(trace);
    drop(lock);
}

#[test]
fn expanded_groups_keep_valid_candidate_indexes_and_terminal_trace_state() {
    let store = Arc::new(Store::default());
    let trace = store.begin(
        "codex",
        "group/fixture",
        "Fixture",
        "ordered",
        Instant::now(),
    );
    trace.plan(vec![candidate(); 256], None);
    trace.step(Kind::Attempt, Some(255), None);
    trace.step(Kind::Attempt, Some(256), None);
    for _ in 0..1200 {
        trace.step(Kind::Retry, None, None);
    }
    trace.finish(200, State::Completed);
    let rows = store.snapshot();
    assert_eq!(rows[0].candidates.len(), 256);
    assert_eq!(rows[0].steps.len(), 1024);
    assert_eq!(rows[0].steps[1].candidate, Some(255));
    assert!(rows[0]
        .steps
        .iter()
        .all(|step| step.candidate.is_none_or(|index| index < 256)));
    assert_eq!(
        rows[0].steps.last().expect("terminal").kind,
        Kind::Completed
    );
}

#[test]
fn session_digests_are_scoped_to_the_consumer_and_runtime() {
    let store = Arc::new(Store::default());
    for consumer in ["a", "a", "b"] {
        let trace = store.begin("codex", "group/fixture", "Fixture", "smart", Instant::now());
        trace.session("same-synthetic-session", consumer);
    }
    let rows = store.snapshot();
    assert_eq!(rows[1].session, rows[2].session);
    assert_ne!(rows[0].session, rows[1].session);
    let other = Arc::new(Store::default());
    let trace = other.begin("codex", "group/fixture", "Fixture", "smart", Instant::now());
    trace.session("same-synthetic-session", "a");
    assert_ne!(other.snapshot()[0].session, rows[1].session);
}

#[test]
fn first_content_timing_recognizes_each_wire_protocol_without_treating_headers_as_content() {
    use super::{Bindings, Observer};
    use serde_json::json;
    use std::time::Duration;
    let start = Instant::now();
    for content in [
        json!({"type":"response.output_text.delta","delta":"synthetic"}),
        json!({"choices":[{"delta":{"content":"synthetic"}}]}),
        json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"synthetic"}}),
        json!({"candidates":[{"content":{"parts":[{"text":"synthetic"}]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"function":{"name":"synthetic"}}]}}]}),
    ] {
        let mut observer = Observer::default();
        observer.started = Some(start);
        let mut bindings = Bindings::default();
        observer.observe_at(&json!({"type":"response.created"}), &mut bindings, 0, start);
        assert!(observer.first_content.is_none());
        observer.observe_at(
            &content,
            &mut bindings,
            0,
            start + Duration::from_millis(125),
        );
        assert_eq!(
            observer.first_content,
            Some(start + Duration::from_millis(125))
        );
    }
}

#[tokio::test]
#[serial_test::serial]
async fn manual_effort_seats_forward_distinct_levels_without_disabling_the_other_seat(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{Runtime, Session, Target};
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{routing::post, Json, Router};
    use serde_json::{json, Value};
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            Router::new().route(
                "/responses",
                post(move |Json(body): Json<Value>| {
                    let tx = tx.clone();
                    async move {
                        let _ = tx.send(body).await;
                        Json(json!({"id":"synthetic", "output":[], "status":"completed"}))
                    }
                }),
            ),
        )
        .await;
    });
    let runtime = Arc::new(Runtime::default());
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
    let client = reqwest::Client::builder().no_proxy().build()?;
    for (picked, expected, off) in [("low", "low", vec!["high"]), ("high", "high", vec!["low"])] {
        let group = serde_json::from_value(json!({
            "id":"seats", "name":"Synthetic effort seats", "revision":0, "enabled":true,
            "members":[{"id":"low","providerId":"same","modelId":"same-model","effort":"low"}, {"id":"high","providerId":"same","modelId":"same-model","effort":"max"}],
            "defaultMemberId":"low", "fallbackMemberIds":[], "intentRecognitionEnabled":false, "classifierTarget":null, "classificationBudgetMs":2000, "rules":[],
            "routing":"manual", "manualMemberId":picked, "off":off
        }))?;
        let mut session = Session {
            group,
            targets: Vec::new(),
            classifier: None,
            client: client.clone(),
            created: Instant::now(),
            bindings: Arc::default(),
        };
        for member in &session.group.members {
            session.targets.push(Target {
                provider_id: member.provider_id.clone(),
                model: member.model_id.clone(),
                fixed_effort: member.effort.clone(),
                upstream: None,
                effort: crate::services::intent_routing::effort::Capability::parse(
                    &json!({"reasoningLevels":["low","high"]}),
                ),
                url: format!("http://{address}/responses").parse()?,
                headers: Default::default(),
                official: false,
                credentials: None,
                scheduling: Arc::default(),
            });
        }
        assert_ne!(
            session.targets[0].continuation_destination(),
            session.targets[1].continuation_destination()
        );
        assert!(!session.targets[0].same_destination(&session.targets[1]));
        assert_eq!(
            session.offered_levels(),
            [if picked == "high" { "max" } else { "low" }]
        );
        let token = runtime.install(session, None)?;
        let response = client.post(format!("http://127.0.0.1:{}/intent/v1/responses",info.port)).bearer_auth(token).json(&json!({"model":"vary-auto/seats", "input":"synthetic", "reasoning":{"effort":"medium", "summary":"auto"}})).send().await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        let _: Value = response.json().await?;
        let sent = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await?
            .ok_or("missing request")?;
        assert_eq!(sent["model"], "same-model");
        assert_eq!(sent["reasoning"]["effort"], expected);
        assert_eq!(sent["reasoning"]["summary"], "auto");
        assert_eq!(runtime.traces.snapshot()[0].candidates.len(), 1);
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn folder_pauses_exclude_members_on_every_request_without_exposing_the_folder(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{Runtime, Session, Target};
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{routing::post, Json, Router};
    use serde_json::{json, Value};
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            Router::new().route(
                "/responses",
                post(move |Json(body): Json<Value>| {
                    let tx = tx.clone();
                    async move {
                        let _ = tx.send(body).await;
                        Json(json!({"id":"synthetic-pause", "status":"completed", "output":[]}))
                    }
                }),
            ),
        )
        .await;
    });
    let runtime = Arc::new(Runtime::default());
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
    let client = reqwest::Client::builder().no_proxy().build()?;
    for all in [false, true] {
        let mut group: super::RoutingGroup = serde_json::from_value(json!({
            "id":"pauses", "name":"Synthetic pauses", "revision":0, "enabled":true,
            "members":[{"id":"a","providerId":"fixture","modelId":"a"},{"id":"b","providerId":"fixture","modelId":"b"}],
            "defaultMemberId":"a", "fallbackMemberIds":["b"], "routing":"ordered",
            "intentRecognitionEnabled":false, "classifierTarget":null, "classificationBudgetMs":2000,
            "rules":[{"id":"reserve","name":"Reserve", "description":"", "enabled":true,"targetMemberId":"a", "pause":true,
                "conditions":{"agents":["codex"],"exceptFolders":["/synthetic/protected"]}}]
        }))?;
        if all {
            let mut rule = group.rules[0].clone();
            rule.id = "reserve-b".into();
            rule.target_member_id = "b".into();
            group.rules.push(rule);
        }
        let targets = group
            .members
            .iter()
            .map(|member| {
                Ok(Target {
                    fixed_effort: None,
                    provider_id: member.provider_id.clone(),
                    model: member.model_id.clone(),
                    upstream: None,
                    effort: Default::default(),
                    url: format!("http://{address}/responses").parse()?,
                    headers: Default::default(),
                    official: false,
                    credentials: None,
                    scheduling: Arc::default(),
                })
            })
            .collect::<Result<Vec<_>, url::ParseError>>()?;
        let token = runtime.install(
            Session {
                group,
                targets,
                classifier: None,
                client: client.clone(),
                created: Instant::now(),
                bindings: Arc::default(),
            },
            None,
        )?;
        for folder in ["/synthetic/protected/task", "/synthetic/other", ""] {
            let input = format!("<environment_context><cwd>{folder}</cwd></environment_context>");
            let response = client
                .post(format!(
                    "http://127.0.0.1:{}/intent/v1/responses",
                    info.port
                ))
                .bearer_auth(&token)
                .json(
                    &json!({"model":"vary-auto/pauses", "input":[{"role":"user","content":input}]}),
                )
                .send()
                .await?;
            if all && !folder.starts_with("/synthetic/protected") {
                assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
                let body = response.text().await?;
                assert!(!body.contains("/synthetic"));
                assert!(rx.try_recv().is_err());
            } else {
                assert_eq!(response.status(), http::StatusCode::OK);
                let _: Value = response.json().await?;
                let sent = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
                    .await?
                    .ok_or("missing synthetic request")?;
                assert_eq!(
                    sent["model"],
                    if folder.starts_with("/synthetic/protected") {
                        "a"
                    } else {
                        "b"
                    }
                );
            }
            let snapshot = runtime.traces.snapshot();
            assert!(!serde_json::to_string(&snapshot)?.contains("/synthetic"));
            assert_eq!(
                snapshot[0]
                    .steps
                    .iter()
                    .any(|step| step.kind == Kind::Paused),
                !folder.starts_with("/synthetic/protected")
            );
        }
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn deterministic_conditions_route_and_fail_over_in_rule_order_with_fitted_output_effort(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{Runtime, Session, Target};
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{response::IntoResponse, routing::post, Json, Router};
    use serde_json::{json, Value};
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            Router::new().route(
                "/responses",
                post(move |Json(body): Json<Value>| {
                    let tx = tx.clone();
                    async move {
                        let rejected = body["model"] == "b";
                        let _ = tx.send(body).await;
                        if rejected {
                            (http::StatusCode::SERVICE_UNAVAILABLE, "synthetic").into_response()
                        } else {
                            Json(json!({"id":"synthetic", "status":"completed", "output":[]}))
                                .into_response()
                        }
                    }
                }),
            ),
        )
        .await;
    });
    let group: super::RoutingGroup = serde_json::from_value(json!({
        "id":"conditions", "name":"Synthetic rules", "revision":0, "enabled":true,
        "members":[{"id":"a","providerId":"fixture","modelId":"a"},{"id":"b","providerId":"fixture","modelId":"b"},{"id":"c","providerId":"fixture","modelId":"c"}],
        "defaultMemberId":"a","fallbackMemberIds":[],"intentRecognitionEnabled":false,"classifierTarget":null,"classificationBudgetMs":2000,"patience":-1,
        "rules":[{"id":"vision","name":"Vision", "description":"", "enabled":true,"targetMemberId":"b", "fallbackMemberIds":["c"],"useEffort":"max", "conditions":{"tokens":100,"images":true,"effort":"high","agents":["codex"]}}]
    }))?;
    let targets = group
        .members
        .iter()
        .map(|member| {
            Ok(Target {
                provider_id: member.provider_id.clone(),
                model: member.model_id.clone(),
                fixed_effort: None,
                upstream: None,
                effort: crate::services::intent_routing::effort::Capability::parse(
                    &json!({"reasoningLevels":["low","high"],"input_modalities":["text","image"]}),
                ),
                url: format!("http://{address}/responses").parse()?,
                headers: Default::default(),
                official: false,
                credentials: None,
                scheduling: Arc::default(),
            })
        })
        .collect::<Result<Vec<_>, url::ParseError>>()?;
    let runtime = Arc::new(Runtime::default());
    let client = reqwest::Client::builder().no_proxy().build()?;
    let token = runtime.install(
        Session {
            group,
            targets,
            classifier: None,
            client: client.clone(),
            created: Instant::now(),
            bindings: Arc::default(),
        },
        None,
    )?;
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
    for (asked, expected) in [("high", vec!["b", "c"]), ("low", vec!["a"])] {
        let response=client.post(format!("http://127.0.0.1:{}/intent/v1/responses",info.port)).bearer_auth(&token).header("session-id", format!("synthetic-conditions-{asked}")).json(&json!({
            "model":"vary-auto/conditions", "reasoning":{"effort":asked},"input":[{"role":"user","content":[{"type":"input_text","text":"x".repeat(800)},{"type":"input_image","image_url":"data:image/png;base64,c3ludGhldGlj"}]}]
        })).send().await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        let _: Value = response.json().await?;
        for model in expected {
            let sent = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
                .await?
                .ok_or("missing fixture request")?;
            assert_eq!(sent["model"], model);
            assert_eq!(
                sent["reasoning"]["effort"],
                if model == "a" { "low" } else { "high" }
            );
        }
        let trace = runtime.traces.snapshot();
        assert_eq!(
            trace[0].steps.iter().any(|step| step.kind == Kind::Rule),
            asked == "high"
        );
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn first_content_timeout_withholds_the_slow_reply_and_patience_retries_only_transient_failures(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{scheduling, RoutingGroup, Runtime, Session, Target};
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{body::Body, response::IntoResponse, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    let transient = Arc::new(AtomicUsize::new(0));
    let slow = Arc::new(AtomicUsize::new(0));
    let observed = transient.clone();
    let slow_observed = slow.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(move |Json(body): Json<Value>| {
            let transient = observed.clone();
            let slow = slow_observed.clone();
            async move {
                if body["input"] == "retry" && transient.fetch_add(1, Ordering::SeqCst) < 2 {
                    return (http::StatusCode::SERVICE_UNAVAILABLE, [(http::header::RETRY_AFTER, "1")], "synthetic").into_response();
                }
                if body["model"] == "model-a" {
                    slow.fetch_add(1, Ordering::SeqCst);
                    if body["input"] == "headers" {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                    let stream = async_stream::stream! {
                        yield Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"slow\"}}\n\n"));
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        yield Ok(bytes::Bytes::from_static(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"slow\"}\n\n"));
                    };
                    return ([(http::header::CONTENT_TYPE, "text/event-stream")], Body::from_stream(stream)).into_response();
                }
                ([(http::header::CONTENT_TYPE, "text/event-stream")], "data: {\"type\":\"response.output_text.delta\",\"delta\":\"synthetic-fast\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n").into_response()
            }
        }))).await;
    });
    let runtime = Arc::new(Runtime::default());
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
    let client = reqwest::Client::builder().no_proxy().build()?;
    for (input, off, expected_attempts) in [
        ("timeout", vec![], 2),
        ("headers", vec![], 2),
        ("off", vec!["a"], 1),
        ("retry", vec![], 4),
    ] {
        let group: RoutingGroup = serde_json::from_value(json!({
            "id":"controls","name":"Synthetic","revision":0,"client":"global","enabled":true,
            "members":[{"id":"a","providerId":"a","modelId":"model-a"},{"id":"b","providerId":"b","modelId":"model-b"}],
            "defaultMemberId":"a","fallbackMemberIds":["b"],"intentRecognitionEnabled":false,
            "classifierTarget":null,"classificationBudgetMs":2000,"rules":[],"routing":"ordered",
            "firstToken":1,"patience":3,"off":off
        }))?;
        let targets = group
            .members
            .iter()
            .map(|member| {
                Ok(Target {
                    fixed_effort: None,
                    provider_id: member.provider_id.clone(),
                    model: member.model_id.clone(),
                    upstream: None,
                    effort: Default::default(),
                    url: format!("http://{address}/responses").parse()?,
                    headers: Default::default(),
                    official: false,
                    credentials: None,
                    scheduling: Arc::new(scheduling::Scheduling::new(false)),
                })
            })
            .collect::<Result<Vec<_>, url::ParseError>>()?;
        let token = runtime.install(
            Session {
                group,
                targets,
                classifier: None,
                client: client.clone(),
                created: Instant::now(),
                bindings: Arc::default(),
            },
            None,
        )?;
        let response = tokio::time::timeout(
            Duration::from_secs(8),
            client
                .post(format!(
                    "http://127.0.0.1:{}/intent/v1/responses",
                    info.port
                ))
                .bearer_auth(token)
                .json(&json!({"model":"vary-auto/controls","input":input,"stream":true}))
                .send(),
        )
        .await??;
        assert_eq!(response.status(), http::StatusCode::OK);
        let bytes = response.bytes().await?;
        let text = std::str::from_utf8(&bytes)?;
        assert!(text.contains("synthetic-fast"));
        assert!(!text.contains("slow"));
        let row = &runtime.traces.snapshot()[0];
        assert_eq!(
            row.steps
                .iter()
                .filter(|step| step.kind == Kind::Attempt)
                .count(),
            expected_attempts
        );
        assert_eq!(row.state, State::Completed);
    }
    assert_eq!(slow.load(Ordering::SeqCst), 3);
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn forwarding_trace_records_fallback_and_usage_without_changing_the_reply(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{scheduling, RoutingGroup, Runtime, Session, Target};
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{response::IntoResponse, routing::post, Json, Router};
    use http::StatusCode;
    use serde_json::{json, Value};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(|headers: http::HeaderMap, Json(body): Json<Value>| async move {
            if body["model"] == "model-a" {
                (StatusCode::TOO_MANY_REQUESTS, "synthetic-private-upstream-error").into_response()
            } else if body["stream"] == true {
                let terminal = if body["input"] == "synthetic-complete" {
                    "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
                } else { "" };
                ([(http::header::CONTENT_TYPE, "text/event-stream")], format!("data: {{\"type\":\"response.output_text.delta\",\"delta\":\"synthetic\"}}\n\n{terminal}")).into_response()
            } else {
                // Observation must not expand the existing forwarded-header allowlist.
                assert!(headers.get("x-openai-subagent").is_none());
                assert_eq!(body["reasoning"]["effort"], "high");
                Json(json!({"model":"model-b","usage":{"input_tokens":9,"output_tokens":3},"extension":{"retained":true}})).into_response()
            }
        }))).await;
    });
    let group: RoutingGroup = serde_json::from_value(json!({
        "id":"trace-fixture","name":"Trace fixture","revision":0,"client":"global","enabled":true,
        "members":[{"id":"a","providerId":"a","modelId":"model-a"},{"id":"b","providerId":"b","modelId":"model-b"}],
        "defaultMemberId":"a","fallbackMemberIds":["b"],"intentRecognitionEnabled":false,
        "classifierTarget":null,"classificationBudgetMs":2000,"rules":[],"routing":"ordered",
        "sink":true,"levels":["low","max"]
    }))?;
    let targets = group
        .members
        .iter()
        .map(|member| {
            Ok(Target {
                fixed_effort: None,
                provider_id: member.provider_id.clone(),
                model: member.model_id.clone(),
                upstream: None,
                effort: crate::services::intent_routing::effort::Capability::parse(
                    &json!({"reasoningLevels":["low","high"]}),
                ),
                url: format!("http://{address}/responses").parse()?,
                headers: Default::default(),
                official: false,
                credentials: None,
                scheduling: Arc::new(scheduling::Scheduling::new(false)),
            })
        })
        .collect::<Result<Vec<_>, url::ParseError>>()?;
    let runtime = Arc::new(Runtime::default());
    let primary_scheduling = targets[0].scheduling.clone();
    let token = runtime.install(
        Session {
            group,
            targets,
            classifier: None,
            client: reqwest::Client::builder().no_proxy().build()?,
            created: Instant::now(),
            bindings: Arc::default(),
        },
        None,
    )?;
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
    let client = reqwest::Client::builder().no_proxy().build()?;
    let reply: Value = client
        .post(format!(
            "http://127.0.0.1:{}/intent/v1/responses",
            info.port
        ))
        .bearer_auth(&token)
        .header("x-openai-subagent", "title_generation")
        .json(&json!({"model":"vary-auto/trace-fixture","input":"synthetic-private-input","reasoning":{"effort":"max"}}))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(reply["extension"]["retained"], true);
    let rows = runtime.traces.snapshot();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, State::Completed);
    assert_eq!(serde_json::to_value(&rows[0])?["kind"], "title_generation");
    assert_eq!(rows[0].input_tokens, Some(9));
    assert_eq!(rows[0].output_tokens, Some(3));
    assert_eq!(
        rows[0]
            .steps
            .iter()
            .filter(|step| step.kind == Kind::Attempt)
            .count(),
        2
    );
    assert!(rows[0]
        .steps
        .iter()
        .any(|step| step.kind == Kind::Rejected && step.status == Some(429)));
    assert!(!serde_json::to_string(&rows)?.contains("synthetic-private"));
    primary_scheduling.succeeded();
    for (input, expected) in [
        ("synthetic-complete", State::Completed),
        ("synthetic-truncated", State::Failed),
    ] {
        let response = client
            .post(format!(
                "http://127.0.0.1:{}/intent/v1/responses",
                info.port
            ))
            .bearer_auth(&token)
            .json(&json!({"model":"vary-auto/trace-fixture","input":input,"stream":true}))
            .send()
            .await?;
        let _ = response.bytes().await?;
        let rows = runtime.traces.snapshot();
        assert_eq!(rows[0].state, expected);
        assert_eq!(rows[0].candidates[0].provider_id, "b");
        assert_eq!(
            rows[0]
                .steps
                .iter()
                .filter(|step| step.kind == Kind::Attempt)
                .count(),
            1
        );
    }
    server.stop().await?;
    upstream.abort();
    Ok(())
}

#[test]
fn completed_gateway_history_survives_a_new_store_with_decisions_and_safe_metadata(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let store = Arc::new(Store::with_history(dir.path().join("history"))?);
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    let target = observed_target("synthetic-private-key")?;
    let request = store.begin(
        "codex",
        "group/research",
        "Research",
        "ordered",
        Instant::now(),
    );
    request.purpose(&http::HeaderMap::new(), true);
    request.session("synthetic-private-session", "codex");
    request.plan(vec![store.candidate(&target)], Some(100));
    request.step(Kind::Attempt, Some(0), None);
    assert!(store.history(Some(&day))?.requests.is_empty());
    request.finish(200, State::Completed);
    request.finish(502, State::Failed);
    store.flush_history()?;
    let reopened = Store::with_history(dir.path().join("history"))?;
    assert_eq!(reopened.history_days()?, vec![day.clone()]);
    let history = reopened.history(Some(&day))?;
    assert!(!history.cut);
    assert_eq!(history.requests.len(), 1);
    let row = &history.requests[0];
    assert_eq!(row.state, State::Completed);
    assert_eq!(row.kind.as_deref(), Some("compact"));
    assert_eq!(row.steps.last().map(|s| s.kind), Some(Kind::Completed));
    assert_eq!(row.candidates[0].model, "model-a");
    let json = serde_json::to_string(&history)?;
    for private in [
        "synthetic-private-key",
        "synthetic-private-session",
        "127.0.0.1",
        "authorization",
    ] {
        assert!(!json.contains(private));
    }
    assert!(row.candidates[0].scheduling.upgrade().is_none());
    assert!(reopened.history(Some("../private")).is_err());
    assert!(reopened.history(Some("2026-02-30")).is_err());
    Ok(())
}

#[test]
fn history_failure_never_changes_a_forwarding_observation() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let blocked = dir.path().join("file");
    std::fs::write(&blocked, "synthetic")?;
    let store = Arc::new(Store::with_history(blocked.join("history"))?);
    let request = store.begin("codex", "fixture", "Fixture", "ordered", Instant::now());
    request.finish(200, State::Completed);
    store.flush_history()?;
    assert_eq!(store.snapshot()[0].state, State::Completed);
    assert!(store.history(None).is_err());
    Ok(())
}
