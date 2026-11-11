use super::*;
use crate::proxy::{intent::Menu, server::ProxyServer, ProxyConfig};
use axum::{routing::post, Json, Router};
use std::sync::atomic::{AtomicUsize, Ordering};

async fn menu(origin: &str) -> Result<Menu, Box<dyn std::error::Error>> {
    let provider = crate::provider::Provider::with_id(
        "synthetic-provider".into(),
        "Synthetic provider".into(),
        json!({
            "auth":{"OPENAI_API_KEY":"synthetic-upstream-key"},
            "apiFormat":"openai_responses",
            "config":format!("model='synthetic-model'\nmodel_provider='fixture'\n[model_providers.fixture]\nbase_url='{origin}/v1'\nwire_api='responses'\n"),
            "modelCatalog":{"models":[{"model":"synthetic-model"}]}
        }),
        None,
    );
    let mut menu = super::super::menu::prepare(
        Arc::new(
            crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(
                tempfile::tempdir()?.path().to_owned(),
            ),
        ),
        vec![provider],
        vec![],
        None,
    )
    .await?;
    for session in menu.groups.values_mut() {
        Arc::make_mut(session).client = reqwest::Client::builder().no_proxy().build()?;
    }
    Ok(menu)
}

#[tokio::test]
#[serial_test::serial]
async fn codex_restart_accepts_cached_credentials_and_old_conversation_model(
) -> Result<(), Box<dyn std::error::Error>> {
    struct SecurityRestore(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for SecurityRestore {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _security = SecurityRestore((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", upstream.local_addr()?);
    let upstream_task = tokio::spawn(async move {
        axum::serve(
            upstream,
            Router::new().route("/v1/responses", post(move |Json(body): Json<Value>| {
                let seen = seen.clone();
                async move {
                    assert_eq!(body["model"], "synthetic-model");
                    seen.fetch_add(1, Ordering::Relaxed);
                    Json(json!({"id":"resp-synthetic","object":"response","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Synthetic reply"}]}],"usage":{"input_tokens":1,"output_tokens":1}}))
                }
            })),
        )
        .await
    });
    let (dir, first) = tests::setup();
    fs::write(&first.config, "model='original'\n")?;
    let db = Arc::new(crate::database::Database::memory()?);
    let gateway = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    )
    .with_intent_runtime(first.runtime.clone());
    let info = gateway.start().await?;
    let base = format!("http://127.0.0.1:{}/intent/v1", info.port);
    let prepared = menu(&origin).await?;
    let mut renamed = prepared.clone();
    renamed.token = "synthetic-new-process-token".into();
    for model in renamed.models.values_mut() {
        model.label = "Renamed synthetic model".into();
    }
    first.activate_menu(Arc::new(prepared), &base)?;
    let installed: toml::Value = fs::read_to_string(&first.config)?.parse()?;
    let cached_token = installed["model_providers"][PROVIDER]["experimental_bearer_token"]
        .as_str()
        .ok_or("missing synthetic credential")?
        .to_owned();
    let cached_model = installed["model"]
        .as_str()
        .ok_or("missing synthetic model")?
        .to_owned();
    let cached_catalog = PathBuf::from(
        installed["model_catalog_json"]
            .as_str()
            .ok_or("missing catalog")?,
    );
    // A stopped process closes its sockets; keep the simulated client credentials and model.
    let client = reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .build()?;
    let send = |model: String| {
        client.post(format!("{base}/responses"))
        .bearer_auth(&cached_token)
        .json(&json!({"model":model,"input":[{"role":"user","content":"Synthetic follow-up"}],"stream":false}))
    };
    let response = send(cached_model.clone()).send().await?;
    let status = response.status();
    let reply: Value = response.json().await?;
    assert_eq!(
        status, 200,
        "synthetic setup error code: {}",
        reply["error"]["code"]
    );
    first.restore()?;
    gateway.stop().await?;
    drop(first);

    let restarted = Manager::new(
        dir.path().join("config.toml"),
        dir.path().join("receipt.json"),
        Arc::default(),
    );
    // Startup recovery must preserve connection identity without activating any route.
    restarted.restore()?;
    assert!(!restarted.runtime.menu_active());
    let gateway = ProxyServer::new(
        ProxyConfig {
            listen_port: info.port,
            ..Default::default()
        },
        db,
        None,
    )
    .with_intent_runtime(restarted.runtime.clone());
    gateway.start().await?;
    restarted.activate_menu_with_native(Arc::new(renamed), &base, None, true)?;
    let restored: toml::Value = fs::read_to_string(&restarted.config)?.parse()?;
    assert!(
        restored["model_providers"][PROVIDER]["experimental_bearer_token"].as_str()
            == Some(cached_token.as_str()),
        "restarting the gateway must retain the existing client's credential"
    );
    assert!(
        cached_catalog.exists(),
        "a running client can still reference its original catalog"
    );
    let response = send(cached_model.clone()).send().await?;
    assert_eq!(
        response.status(),
        200,
        "an existing conversation must survive a gateway restart"
    );
    let current: toml::Value = fs::read_to_string(&restarted.config)?.parse()?;
    assert_eq!(
        send(
            current["model"]
                .as_str()
                .ok_or("missing current model")?
                .into()
        )
        .send()
        .await?
        .status(),
        200
    );
    assert_eq!(calls.load(Ordering::Relaxed), 3);

    restarted.disconnect()?;
    assert_eq!(
        client
            .get(format!("{base}/models"))
            .bearer_auth(&cached_token)
            .send()
            .await?
            .status(),
        401
    );
    let fresh = menu(&origin).await?;
    restarted.activate_menu_with_native(Arc::new(fresh), &base, None, true)?;
    assert_eq!(
        client
            .get(format!("{base}/models"))
            .bearer_auth(&cached_token)
            .send()
            .await?
            .status(),
        200,
        "explicitly re-enabling routing must restore the existing client's identity"
    );
    assert_eq!(send(cached_model).send().await?.status(), 200);
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    restarted.disconnect()?;
    gateway.stop().await?;
    upstream_task.abort();
    Ok(())
}

#[tokio::test]
async fn saved_codex_identity_is_private_and_rejects_a_different_config(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    let base = "http://127.0.0.1:1234/intent/v1";
    let prepared = Arc::new(menu("https://synthetic.invalid").await?);
    manager.activate_menu(prepared.clone(), base)?;
    manager.restore()?;
    let identity_path = manager.receipt.with_extension("identity.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&identity_path)?.permissions().mode() & 0o777,
            0o600
        );
    }
    let other = dir.path().join("other.toml");
    fs::write(&other, "model='synthetic-original'\n")?;
    let unrelated = Manager::new(other.clone(), manager.receipt.clone(), Arc::default());
    assert!(
        matches!(unrelated.activate_menu(prepared.clone(), base), Err(error) if error == CONFLICT)
    );
    assert!(!unrelated.runtime.menu_active());
    assert_eq!(fs::read_to_string(other)?, "model='synthetic-original'\n");
    fs::write(identity_path, "invalid synthetic identity")?;
    assert!(matches!(manager.activate_menu(prepared, base), Err(error) if error == FAILED));
    assert!(!manager.runtime.menu_active());
    Ok(())
}
