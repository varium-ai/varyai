//! Historical Magpie selectors through the real Codex projection and ingress.
use super::super::{mode_tests, tests, Manager};
use crate::{
    database::Database,
    proxy::{intent::MenuModel, server::ProxyServer, ProxyConfig},
};
use axum::{routing::post, Json, Router};
use http::{header, HeaderMap, HeaderValue, StatusCode};
use serde_json::{json, Value};
use std::{fs, sync::Arc};

struct RestoreSecurity(crate::proxy::security::SecurityRuntimeConfig);
impl Drop for RestoreSecurity {
    fn drop(&mut self) {
        crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn magpie_codex_alias_survives_takeover_restart_and_revokes_with_its_source(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = RestoreSecurity((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let upstream = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/responses", post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send((headers, body)).await;
                ([(header::CONTENT_TYPE, "text/event-stream")], "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"synthetic-response\",\"status\":\"completed\",\"output\":[]}}\n\n")
            }
        }))).await
    });
    let (dir, initial) = tests::setup();
    fs::write(
        dir.path().join("config.toml"),
        "model_provider='magpie'\nmodel='codex/synthetic-model'\n",
    )?;
    for paginated in [false, true] {
        mode_tests::thread_model_fixture(
            dir.path(),
            if paginated { "paged" } else { "legacy" },
            "magpie",
            paginated,
            Some("codex/synthetic-model"),
            Some("codex/synthetic-model"),
        )?;
    }
    let mut menu = super::tests::source_menu(&[
        ("subscription", "Codex", true),
        ("relay", "Same model API", false),
    ]);
    for (alias, session) in &mut menu.groups {
        let session = Arc::make_mut(session);
        let target = &mut session.targets[0];
        target.url = format!("http://{address}/responses").parse()?;
        target.upstream = None;
        target.headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static(if target.official {
                "Bearer synthetic-subscription"
            } else {
                "Bearer synthetic-relay"
            }),
        );
        session.client = reqwest::Client::builder().no_proxy().build()?;
        session.group.id = target.provider_id.clone();
        session.group.members.truncate(1);
        session.group.members[0].provider_id = target.provider_id.clone();
        session.group.members[0].model_id = target.model.clone();
        session.group.fallback_member_ids.clear();
        session.group.rules.clear();
        menu.models.insert(
            session.group.id.clone(),
            MenuModel {
                alias: alias.clone(),
                label: target.provider_id.clone(),
                description: "VaryAI direct provider model",
                order: 0,
            },
        );
    }
    let runtime = initial.runtime.clone();
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
    let base = format!("http://127.0.0.1:{}/intent/v1", info.port);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let original_menu = menu.clone();
    for restart in [false, true] {
        let manager = Manager::new(
            dir.path().join("config.toml"),
            dir.path().join("receipt.json"),
            runtime.clone(),
        );
        runtime.set_menu(Arc::new(menu.clone()))?;
        let groups = menu
            .groups
            .values()
            .map(|s| s.group.clone())
            .collect::<Vec<_>>();
        manager.write_projection(&groups, &menu.token, &base)?;
        manager.sync_threads()?;
        assert_eq!(
            manager.selected_model()?.as_deref(),
            Some("vary-provider/subscription")
        );
        let receipt = manager.load()?.ok_or("missing synthetic receipt")?;
        assert_eq!(
            receipt.model_map["codex/synthetic-model"],
            "vary-provider/subscription"
        );
        let db = rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?;
        let preserved: i64 = db.query_row(
            "SELECT count(*) FROM threads WHERE model_provider='varyai' AND model='codex/synthetic-model'", [], |row| row.get(0),
        )?;
        assert_eq!(preserved, 2);
        let response = client.post(format!("{base}/responses"))
            .bearer_auth(&menu.token)
            .json(&json!({"model":"codex/synthetic-model","input":"Synthetic history","stream":true,"store":false,"metadata":{"synthetic_integer":9007199254740993_u64}}))
            .send().await?;
        assert_eq!(response.status(), StatusCode::OK, "restart={restart}");
        assert!(response.text().await?.contains("response.completed"));
        let (headers, body) = rx.try_recv()?;
        assert_eq!(
            headers[header::AUTHORIZATION],
            "Bearer synthetic-subscription"
        );
        assert_eq!(body["model"], "synthetic-model");
        assert_eq!(
            body["metadata"]["synthetic_integer"],
            json!(9007199254740993_u64)
        );
        runtime.clear_menu();
    }
    menu.groups.remove("vary-provider/subscription");
    menu.models.remove("subscription");
    runtime.set_menu(Arc::new(menu.clone()))?;
    let groups = menu
        .groups
        .values()
        .map(|s| s.group.clone())
        .collect::<Vec<_>>();
    initial.write_projection(&groups, &menu.token, &base)?;
    for (token, model, expected) in [
        (
            "wrong-token",
            "codex/synthetic-model",
            StatusCode::UNAUTHORIZED,
        ),
        (
            menu.token.as_str(),
            "codex/synthetic-model",
            StatusCode::NOT_FOUND,
        ),
        (
            menu.token.as_str(),
            "codex/unknown-model",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let response = client.post(format!("{base}/responses")).bearer_auth(token)
            .json(&json!({"model":model,"input":"Synthetic private history","stream":true,"store":false})).send().await?;
        assert_eq!(response.status(), expected);
        let body = response.text().await?;
        assert!(!body.contains(model));
        assert!(!body.contains("Synthetic private history"));
        assert!(rx.try_recv().is_err());
    }
    // Re-adding the original source restores the same retained alias owner.
    runtime.set_menu(Arc::new(original_menu.clone()))?;
    let groups = original_menu
        .groups
        .values()
        .map(|s| s.group.clone())
        .collect::<Vec<_>>();
    initial.write_projection(&groups, &original_menu.token, &base)?;
    assert_eq!(
        initial.load()?.ok_or("missing receipt")?.model_map["codex/synthetic-model"],
        "vary-provider/subscription"
    );
    server.stop().await?;
    upstream.abort();
    Ok(())
}
