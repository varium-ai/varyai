use super::*;

#[tokio::test]
#[serial_test::serial]
async fn withdrawing_gateway_publication_removes_the_group_from_both_gateway_catalogs(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        database::Database,
        proxy::{
            gateway::{KeyPolicy, Period},
            security::{
                get_security_runtime, runtime::set_security_runtime_for_test, SecurityRuntimeConfig,
            },
            server::ProxyServer,
            ProxyConfig,
        },
    };
    use axum::{routing::post, Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RestoreSecurity(SecurityRuntimeConfig);
    impl Drop for RestoreSecurity {
        fn drop(&mut self) {
            set_security_runtime_for_test(self.0.clone());
        }
    }
    let _restore = RestoreSecurity((*get_security_runtime()).clone());
    set_security_runtime_for_test(SecurityRuntimeConfig::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let fixture = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Json(json!({"id":"synthetic-response","object":"response","status":"completed","output":[]})))
        }))).await;
    });
    let db = Arc::new(Database::memory()?);
    let key = db.create_gateway_key(KeyPolicy {
        name: "Synthetic publication".into(),
        models: vec![],
        all_models: true,
        token_limit: None,
        period: Period::Never,
    })?;
    let runtime = Arc::new(Runtime::default());
    let mut menu = global_tests::menu(&format!("http://{address}/responses"));
    let alias = global_tests::group().alias();
    runtime.set_global(menu.clone(), db.routing_generation())?;
    runtime
        .connections
        .install(&crate::services::agent_connections::Receipt {
            app: "grokbuild".into(),
            token: "synthetic-agent-token".into(),
            model: alias.clone(),
            model_map: Default::default(),
            visible_models: None,
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        })?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let origin = format!("http://127.0.0.1:{}", info.port);
    let generation = json!({"model":alias,"input":"Synthetic request"});
    client
        .post(format!("{origin}/gateway/v1/responses"))
        .bearer_auth(&key.secret)
        .json(&generation)
        .send()
        .await?
        .error_for_status()?;
    for (path, key_name) in [
        ("/gateway/v1/models", "data"),
        ("/gateway/v1beta/models", "models"),
    ] {
        let catalog = client
            .get(format!("{origin}{path}"))
            .bearer_auth(&key.secret)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        assert_eq!(catalog[key_name].as_array().map(Vec::len), Some(1));
    }
    let session = menu
        .groups
        .get_mut(&alias)
        .ok_or("synthetic group missing")?;
    Arc::make_mut(session).group.enabled = false;
    db.invalidate_routing_resources();
    runtime.set_global(menu, db.routing_generation())?;
    for (path, key_name) in [
        ("/gateway/v1/models", "data"),
        ("/gateway/v1beta/models", "models"),
    ] {
        let catalog = client
            .get(format!("{origin}{path}"))
            .bearer_auth(&key.secret)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        assert_eq!(catalog[key_name], json!([]));
    }
    for path in [
        "/gateway/v1/responses".into(),
        "/gateway/v1/chat/completions".into(),
        "/gateway/v1/messages".into(),
        format!("/gateway/v1beta/models/{alias}:generateContent"),
    ] {
        let response = client
            .post(format!("{origin}{path}"))
            .bearer_auth(&key.secret)
            .json(&generation)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.json::<Value>().await?["error"]["code"],
            "routing_model_unavailable"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    client
        .post(format!("{origin}/connections/grokbuild/v1/responses"))
        .bearer_auth("synthetic-agent-token")
        .json(&generation)
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    server.stop().await?;
    fixture.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn stale_codex_publications_reject_before_forwarding_and_recover_with_the_current_version(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        database::Database,
        provider::Provider,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{routing::post, Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/v1/responses", post(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Json(json!({"id":"synthetic-response","object":"response","status":"completed","output":[]})))
        }))).await;
    });
    let directory = tempfile::tempdir()?;
    let manager = Arc::new(
        crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(
            directory.path().to_owned(),
        ),
    );
    let provider = Provider::with_id(
        "synthetic".into(),
        "Synthetic".into(),
        json!({
            "auth":{"OPENAI_API_KEY":"synthetic-key"},"apiFormat":"openai_responses",
            "config":format!("model='synthetic-model'\nmodel_provider='synthetic'\n[model_providers.synthetic]\nbase_url='http://{address}/v1'\nwire_api='responses'")
        }),
        None,
    );
    let menu = Arc::new(
        crate::services::intent_routing::menu::prepare(manager, vec![provider], vec![], None)
            .await?,
    );
    let alias = menu.default_model.clone().ok_or("missing model")?;
    let db = Arc::new(Database::memory()?);
    let runtime = Arc::new(Runtime::default());
    runtime
        .native
        .prepare_checkpoints(&directory.path().join("checkpoint.key"))?;
    let publish = |generation| -> Result<(), String> {
        let published = PublishedMenu::new(menu.clone(), generation);
        runtime.set_menu(published.clone())?;
        runtime
            .native
            .publish_menu(
                published,
                json!({"models":[]}),
                std::collections::BTreeMap::from([(
                    "Synthetic readable model".into(),
                    alias.clone(),
                )]),
            )
            .map_err(str::to_owned)
    };
    publish(db.routing_generation())?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let request = |path: &str, token: &str| {
        client
            .post(format!("http://127.0.0.1:{}{path}", info.port))
            .bearer_auth(token)
            .json(&json!({"model":"Synthetic readable model","input":"Synthetic request"}))
    };
    for path in ["/intent/v1/responses", "/backend-api/codex/responses"] {
        assert_eq!(
            request(path, &menu.token).send().await?.status(),
            StatusCode::OK
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    db.invalidate_routing_resources();
    for path in ["/intent/v1/responses", "/backend-api/codex/responses"] {
        let response = request(path, &menu.token).send().await?;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.json::<Value>().await?["error"]["code"],
            "routing_catalog_stale"
        );
    }
    for path in ["/intent/v1/models", "/backend-api/codex/models"] {
        let response = client
            .get(format!("http://127.0.0.1:{}{path}", info.port))
            .bearer_auth(&menu.token)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    assert_eq!(
        request("/intent/v1/responses", "wrong-token")
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "stale resources must not reach the upstream"
    );
    let previous = runtime.set_menu(PublishedMenu::new(menu.clone(), db.routing_generation()))?;
    runtime.restore_menu(previous);
    assert_eq!(
        request("/intent/v1/responses", &menu.token)
            .send()
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    publish(db.routing_generation())?;
    for path in ["/intent/v1/responses", "/backend-api/codex/responses"] {
        assert_eq!(
            request(path, &menu.token).send().await?.status(),
            StatusCode::OK
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    server.stop().await?;
    upstream.abort();
    Ok(())
}

#[tokio::test]
async fn on_demand_refresher_recovers_stale_catalog_without_503(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::Database;
    use std::sync::atomic::Ordering;
    let db = Arc::new(Database::memory()?);
    let runtime = Arc::new(Runtime::default());
    let menu = Arc::new(Menu {
        token: "test-token".into(),
        models: HashMap::new(),
        default_model: None,
        omitted_entries: 0,
        context_settings: crate::services::intent_routing::context::Settings::default(),
        groups: HashMap::new(),
    });
    let (global_calls, menu_calls) = (
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    );
    {
        let db = db.clone();
        let runtime_weak = Arc::downgrade(&runtime);
        let menu = menu.clone();
        let calls = global_calls.clone();
        runtime.set_global_refresher(Arc::new(move || {
            let db = db.clone();
            let runtime_weak = runtime_weak.clone();
            let menu = menu.clone();
            let calls = calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                let rt = runtime_weak.upgrade().ok_or("dropped")?;
                rt.set_global((*menu).clone(), db.routing_generation())
            })
        }));
    }
    {
        let db = db.clone();
        let runtime_weak = Arc::downgrade(&runtime);
        let menu = menu.clone();
        let calls = menu_calls.clone();
        runtime.set_menu_refresher(Some(Arc::new(move || {
            let db = db.clone();
            let runtime_weak = runtime_weak.clone();
            let menu = menu.clone();
            let calls = calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                let rt = runtime_weak.upgrade().ok_or("dropped")?;
                rt.set_menu(PublishedMenu::new(menu, db.routing_generation()))?;
                Ok(())
            })
        })));
    }
    runtime.set_global((*menu).clone(), db.routing_generation())?;
    runtime.set_menu(PublishedMenu::new(menu.clone(), db.routing_generation()))?;
    db.invalidate_routing_resources();
    assert!(!runtime.global_current(db.routing_generation()));
    assert!(runtime.ensure_global_current(&db).await);
    assert!(runtime.ensure_menu_current(&db).await);
    assert_eq!(global_calls.load(Ordering::SeqCst), 1);
    assert_eq!(menu_calls.load(Ordering::SeqCst), 1);
    assert!(runtime.global_current(db.routing_generation()));
    Ok(())
}

#[test]
fn side_request_follows_active_session_model() {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "x-codex-turn-metadata",
        http::HeaderValue::from_static(r#"{"thread_source":"thread_title"}"#),
    );
    assert!(trace::is_side_request(&headers));

    let runtime = Runtime::default();
    runtime.record_session_model("agent:codex", Some("conv-1"), "vary-auto/active-group");
    assert_eq!(
        runtime.side_session_alias("agent:codex", Some("conv-1")).as_deref(),
        Some("vary-auto/active-group")
    );
    // Falls back to the consumer's most recent model when the side request omits session id.
    assert_eq!(
        runtime.side_session_alias("agent:codex", None).as_deref(),
        Some("vary-auto/active-group")
    );
}
