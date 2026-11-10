use super::*;
use crate::{
    database::Database,
    proxy::{server::ProxyServer, ProxyConfig},
};
use axum::{response::IntoResponse, routing::post, Router};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
#[serial_test::serial]
async fn parity_rejected_and_precontent_failed_attempts_use_backup_without_replaying_content(
) -> Result<(), Box<dyn std::error::Error>> {
    struct Security(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for Security {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _security = Security((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    for (status,primary,expected_backup) in [
        (401,"synthetic rejected",true),(402,"synthetic rejected",true),(500,"synthetic rejected",true),
        (200,"event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"server_error\"}}}\n\n",true),
        (200,"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"synthetic partial\"}\n\nevent: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\"}}\n\n",false),
        (200,"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n",false),
    ] {
        let hits = Arc::new(AtomicUsize::new(0));
        let observed = hits.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener,Router::new()
                .route("/primary",post(move || async move {(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),[("content-type","text/event-stream")],primary).into_response()}))
                .route("/backup",post(move || {let hits=observed.clone();async move {hits.fetch_add(1,Ordering::SeqCst);(StatusCode::OK,[("content-type","text/event-stream")],"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n")}}))).await;
        });
        let published = global_tests::menu(&format!("http://{address}/primary"));
        let mut session = published.groups.values().next().ok_or("fixture")?.as_ref().clone();
        session.group.enabled=false;
        session.group.patience=-1;
        session.group.members.push(crate::services::intent_routing::Member { id:"backup".into(),provider_id:"backup".into(),model_id:"exact-model".into(),weight:1,free:None,effort:None });
        session.group.fallback_member_ids.push("backup".into());
        let mut backup = session.targets[0].clone();
        backup.provider_id="backup".into();
        backup.url=format!("http://{address}/backup").parse()?;
        session.targets.push(backup);
        let runtime=Arc::new(Runtime::default());
        let alias=session.group.alias();
        let token=runtime.install(session,None)?;
        let server=ProxyServer::new(ProxyConfig {listen_port:0,..Default::default()},Arc::new(Database::memory()?),None).with_intent_runtime(runtime);
        let info=server.start().await?;
        let response=reqwest::Client::builder().no_proxy().build()?.post(format!("http://127.0.0.1:{}/intent/v1/responses",info.port)).bearer_auth(token).json(&json!({"model":alias,"input":"synthetic","stream":true})).send().await?;
        let actual_status=response.status();
        let body = response.text().await?;
        assert_eq!(actual_status,StatusCode::OK,"status case {status}: {body}");
        assert_eq!(hits.load(Ordering::SeqCst),usize::from(expected_backup),"status case {status}");
        server.stop().await?;
        task.abort();
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn parity_official_account_rotation_and_unavailable_credentials_keep_approved_fallback(
) -> Result<(), Box<dyn std::error::Error>> {
    struct Security(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for Security {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _security = Security((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let dir = tempfile::tempdir()?;
    let manager = Arc::new(
        crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(dir.path().to_owned()),
    );
    for account in ["one", "two"] {
        manager
            .add_test_account_with_user_identity(account, &format!("synthetic-{account}"), account)
            .await?;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let task = tokio::spawn(async move {
        let _=axum::serve(listener,Router::new().route("/responses",post(move |headers:http::HeaderMap| {let tx=tx.clone(); async move {
            let account=headers.get("x-synthetic-account").and_then(|v|v.to_str().ok()).unwrap_or_default().to_owned();
            let _=tx.send(account).await;
            ([("content-type","text/event-stream")],"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n")
        }}))).await;
    });
    let menu = global_tests::menu(&format!("http://{address}/responses"));
    let mut session = menu
        .groups
        .values()
        .next()
        .ok_or("fixture")?
        .as_ref()
        .clone();
    session.group.enabled = false;
    session.group.routing = crate::services::intent_routing::RoutingStrategy::RoundRobin;
    session.group.patience = -1;
    let mut targets = vec![];
    for account in ["one", "two"] {
        let mut provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({}),
            None,
        );
        provider.category = Some("official".into());
        crate::services::codex_provider::bind(&mut provider, account);
        let credentials =
            crate::services::intent_routing::catalog::freeze_execution(&provider, manager.clone())
                .await
                .ok_or("fixture credentials")?;
        let mut target = session.targets[0].clone();
        target.official = true;
        target.credentials = Some(Arc::new(credentials));
        target
            .headers
            .insert("x-synthetic-account", http::HeaderValue::from_str(account)?);
        targets.push(target);
    }
    session.targets = targets;
    let runtime = Arc::new(Runtime::default());
    let alias = session.group.alias();
    let token = "synthetic-account-menu".to_owned();
    runtime.set_menu(Arc::new(Menu {
        token: token.clone(),
        models: Default::default(),
        default_model: Some(alias.clone()),
        omitted_entries: 0,
        context_settings: Default::default(),
        groups: HashMap::from([(alias.clone(), Arc::new(session))]),
    }))?;
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
    let client = reqwest::Client::builder().no_proxy().build()?;
    for (index, expected) in ["one", "two", "one", "two"].into_iter().enumerate() {
        let response = client
            .post(format!(
                "http://127.0.0.1:{}/intent/v1/responses",
                info.port
            ))
            .bearer_auth(&token)
            // These are new conversations, not repeated turns of one opening.
            .header("session-id", format!("synthetic-rotation-{index}"))
            .json(&json!({"model":alias,"input":"synthetic","stream":true,"store":false}))
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), rx.recv())
                .await?
                .ok_or("upstream")?,
            expected
        );
    }
    manager.remove_account("one").await?;
    let response = client
        .post(format!(
            "http://127.0.0.1:{}/intent/v1/responses",
            info.port
        ))
        .bearer_auth(&token)
        .json(&json!({"model":alias,"input":"synthetic","stream":true,"store":false}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await?;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await?
            .ok_or("fallback")?,
        "two"
    );
    server.stop().await?;
    task.abort();
    Ok(())
}
