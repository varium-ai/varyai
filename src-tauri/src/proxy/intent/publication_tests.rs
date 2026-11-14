use super::*;

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
