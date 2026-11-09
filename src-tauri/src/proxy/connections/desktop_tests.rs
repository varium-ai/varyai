use super::*;
use crate::{
    database::Database,
    proxy::{
        intent::{Menu, MenuModel, Runtime, Session, Target},
        server::ProxyServer,
        ProxyConfig,
    },
    services::{agent_connections::Receipt, intent_routing::RoutingGroup},
};
use axum::{routing::post, Router};
use http::HeaderMap;
use std::{collections::HashMap, sync::Arc, time::Instant};

fn menu(url: &str) -> Result<Menu, Box<dyn std::error::Error>> {
    let mut menu = Menu {
        token: concat!("synthetic", "-menu").into(),
        models: HashMap::new(),
        default_model: Some("vary-provider/chat".into()),
        omitted_entries: 0,
        context_settings: Default::default(),
        groups: HashMap::new(),
    };
    for (order, (id, model, provider)) in [
        ("chat", "synthetic-chat", "primary"),
        ("qwen", "synthetic-qwen", "secondary"),
        ("worker", "synthetic-worker", "secondary"),
    ]
    .into_iter()
    .enumerate()
    {
        let alias = format!("vary-provider/{id}");
        let group: RoutingGroup = serde_json::from_value(json!({
            "id":id,"name":id,"revision":0,"client":"claude-desktop","enabled":true,
            "members":[{"id":"a","providerId":provider,"modelId":model}],
            "defaultMemberId":"a","fallbackMemberIds":[],"intentRecognitionEnabled":false,
            "classifierTarget":null,"classificationBudgetMs":2000,"rules":[],
            "levels":[],"modelAlias":alias
        }))?;
        let mut upstream =
            crate::provider::Provider::with_id(provider.into(), provider.into(), json!({}), None);
        upstream.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("anthropic".into()),
            ..Default::default()
        });
        menu.models.insert(
            alias.clone(),
            MenuModel {
                alias: alias.clone(),
                label: id.into(),
                description: "VaryAI direct provider model",
                order,
            },
        );
        menu.groups.insert(
            alias,
            Arc::new(Session {
                group,
                targets: vec![Target {
                    provider_id: provider.into(),
                    model: model.into(),
                    fixed_effort: None,
                    upstream: Some(upstream),
                    effort: Default::default(),
                    url: url.parse()?,
                    headers: Default::default(),
                    official: false,
                    credentials: None,
                    scheduling: Arc::default(),
                }],
                classifier: None,
                client: reqwest::Client::builder().no_proxy().build()?,
                created: Instant::now(),
                bindings: Arc::default(),
            }),
        );
    }
    Ok(menu)
}

#[tokio::test]
#[serial_test::serial]
async fn desktop_titles_follow_lead_model_instead_of_first_sonnet_alias(
) -> Result<(), Box<dyn std::error::Error>> {
    struct Restore(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _restore = Restore((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let classifications = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = classifications.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/v1/messages", post(move |Json(body): Json<Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(body).await;
                Json(json!({"id":"synthetic-response","type":"message","role":"assistant","model":"synthetic","content":[{"type":"text","text":"synthetic answer"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}))
            }
        })).route("/v1/chat/completions", post(move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::future::ready(Json(json!({"choices":[{"message":{"content":"{\"intentId\":null}"}}]})))
        }))).await
    });
    let runtime = Arc::new(Runtime::default());
    let mut published = menu(&format!("http://{address}/v1/messages"))?;
    let session = Arc::make_mut(
        published
            .groups
            .get_mut("vary-provider/chat")
            .ok_or("fixture")?,
    );
    session.group.intent_recognition_enabled = true;
    session.group.classifier_target = Some(serde_json::from_value(json!({
        "providerId":"classifier","modelId":"synthetic-classifier"
    }))?);
    session.group.rules = serde_json::from_value(json!([{
        "id":"synthetic","name":"Synthetic","description":"Synthetic routing rule",
        "enabled":true,"targetMemberId":"a"
    }]))?;
    session.classifier = Some(crate::provider::Provider::with_id(
        "classifier".into(),
        "Synthetic classifier".into(),
        json!({
            "apiFormat":"openai_chat", "auth":{"OPENAI_API_KEY":concat!("synthetic", "-classifier-key")},
            "config":format!("model_provider = \"fixture\"\n[model_providers.fixture]\nbase_url = \"http://{address}/v1\"\nwire_api = \"chat\"")
        }),
        None,
    ));
    let mapping = published
        .models
        .keys()
        .map(|alias| {
            (
                crate::claude_desktop_config::shared_model_name(alias),
                alias.clone(),
            )
        })
        .collect();
    runtime.set_global(published, 0)?;
    let receipt = Receipt {
        app: "claude-desktop".into(),
        token: concat!("synthetic", "-local").into(),
        model: "vary-provider/chat".into(),
        model_map: mapping,
        visible_models: None,
        files: vec![],
        suspended: false,
        applied_at_ms: None,
    };
    runtime.connections.install(&receipt)?;
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
    let root = format!(
        "http://127.0.0.1:{}/connections/claude-desktop/v1",
        info.port
    );
    let client = reqwest::Client::builder().no_proxy().build()?;
    let qwen = crate::claude_desktop_config::shared_model_name("vary-provider/qwen");
    let chat = crate::claude_desktop_config::shared_model_name("vary-provider/chat");
    let worker = crate::claude_desktop_config::shared_model_name("vary-provider/worker");
    let title = json!({"model":qwen,"max_tokens":200,"system":"You write short session titles. Return a synthetic title.","messages":[{"role":"user","content":"synthetic"}]});
    let mut lead = json!({"model":chat,"max_tokens":64000,"tools":[{"name":"synthetic_tool","input_schema":{"type":"object"}}],"messages":[{"role":"user","content":"synthetic"}]});
    for (body, cli, subagent, expected) in [
        (title.clone(), false, false, "synthetic-chat"),
        (lead.clone(), true, false, "synthetic-chat"),
        (title.clone(), false, false, "synthetic-chat"),
        (
            json!({"model":qwen,"max_tokens":64000,"tools":[],"system":[{"type":"text","text":"You write short session titles."}],"messages":[{"role":"user","content":"synthetic"}]}),
            true,
            false,
            "synthetic-chat",
        ),
        (
            json!({"model":worker,"max_tokens":256,"system":"Synthetic security classifier","messages":[{"role":"user","content":"synthetic"}]}),
            true,
            false,
            "synthetic-worker",
        ),
        (
            {
                lead["model"] = json!(worker);
                lead.clone()
            },
            true,
            true,
            "synthetic-worker",
        ),
        (title.clone(), false, false, "synthetic-chat"),
        (
            json!({"model":qwen,"max_tokens":32000,"messages":[{"role":"user","content":"synthetic"}]}),
            false,
            false,
            "synthetic-qwen",
        ),
        (
            json!({"model":qwen,"max_tokens":1,"messages":[{"role":"user","content":"."}]}),
            false,
            false,
            "synthetic-chat",
        ),
    ] {
        let mut request = client
            .post(format!("{root}/messages"))
            .bearer_auth(&receipt.token)
            .header(
                "user-agent",
                if cli {
                    "claude-cli/2.1.309 (external, claude-desktop)"
                } else {
                    "Mozilla/5.0 Claude/4.4212.0"
                },
            );
        if subagent {
            request = request.header("x-claude-code-agent-id", "synthetic-worker");
        }
        let before = classifications.load(std::sync::atomic::Ordering::SeqCst);
        let response = request.json(&body).send().await?;
        assert_eq!(response.status(), StatusCode::OK);
        let sent = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await?
            .ok_or("missing synthetic upstream request")?;
        assert_eq!(sent["model"], expected);
        if desktop::probe(&body) {
            assert_eq!(sent["max_tokens"], 16);
            assert_eq!(
                classifications.load(std::sync::atomic::Ordering::SeqCst),
                before
            );
        }
    }
    assert!(classifications.load(std::sync::atomic::Ordering::SeqCst) > 0);
    let traces = runtime.traces.snapshot();
    assert!(traces
        .iter()
        .any(|row| row.kind.as_deref() == Some("title")));
    assert!(traces
        .iter()
        .any(|row| row.kind.as_deref() == Some("provider_test")));
    let mut other_agent = receipt.clone();
    other_agent.app = "claude".into();
    runtime.connections.install(&other_agent)?;
    let response = client
        .post(format!(
            "http://127.0.0.1:{}/connections/claude/v1/messages",
            info.port
        ))
        .bearer_auth(&other_agent.token)
        .json(&title)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let sent = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await?
        .ok_or("missing other agent request")?;
    assert_eq!(sent["model"], "synthetic-qwen");
    server.stop().await?;
    task.abort();
    Ok(())
}

fn connection(menu: &Menu) -> Connection {
    Connection {
        token: concat!("synthetic", "-local").into(),
        model_map: menu
            .models
            .keys()
            .map(|alias| {
                (
                    crate::claude_desktop_config::shared_plain_model_name(alias),
                    alias.clone(),
                )
            })
            .collect(),
        visible_models: Some(vec![
            "vary-provider/chat".into(),
            "vary-provider/qwen".into(),
            "vary-provider/worker".into(),
        ]),
    }
}

#[test]
fn desktop_sessions_and_restart_preferences_do_not_store_session_identifiers(
) -> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let path = home.path().join("claude-desktop.model");
    let menu = menu("http://127.0.0.1:1/v1/messages")?;
    let connection = connection(&menu);
    let router = Connections::with_desktop(path.clone(), vec![])?;
    let a = HeaderMap::from_iter([("session_id".parse()?, "synthetic-session-a".parse()?)]);
    let b = HeaderMap::from_iter([("session_id".parse()?, "synthetic-session-b".parse()?)]);
    router.desktop.remember(&a, "vary-provider/chat");
    router.desktop.remember(&b, "vary-provider/qwen");
    let body = json!({"max_tokens":200,"system":"You write short session titles.","messages":[]});
    let asked = crate::claude_desktop_config::shared_model_name("vary-provider/worker");
    assert_eq!(
        router
            .desktop
            .resolve(&connection, &menu, &asked, &body, &a, &[])
            .alias,
        "vary-provider/chat"
    );
    assert_eq!(
        router
            .desktop
            .resolve(&connection, &menu, &asked, &body, &b, &[])
            .alias,
        "vary-provider/qwen"
    );
    let unbound = HeaderMap::from_iter([("session_id".parse()?, "synthetic-session-c".parse()?)]);
    assert_eq!(
        router
            .desktop
            .resolve(&connection, &menu, &asked, &body, &unbound, &[])
            .alias,
        "vary-provider/chat"
    );
    router.desktop.flush()?;
    assert_eq!(std::fs::read_to_string(&path)?, "vary-provider/qwen");
    let reopened = Connections::with_desktop(path, vec![])?;
    assert_eq!(
        reopened
            .desktop
            .resolve(&connection, &menu, &asked, &body, &HeaderMap::new(), &[])
            .alias,
        "vary-provider/qwen"
    );

    let blocked = home.path().join("blocked");
    std::fs::write(&blocked, "synthetic")?;
    let unavailable = Connections::with_desktop(blocked.join("preference"), vec![])?;
    unavailable
        .desktop
        .remember(&HeaderMap::new(), "vary-provider/chat");
    unavailable.desktop.flush()?;
    assert_eq!(
        unavailable
            .desktop
            .resolve(&connection, &menu, &asked, &body, &HeaderMap::new(), &[])
            .alias,
        "vary-provider/chat"
    );
    Ok(())
}

#[test]
fn desktop_probe_uses_native_menu_choice_and_ignores_stale_lead(
) -> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let root = home.path().join("Claude-3p");
    let settings = root.join(
        "local-agent-mode-sessions/synthetic-account/synthetic-org/cowork_account_settings.json",
    );
    std::fs::create_dir_all(settings.parent().ok_or("fixture")?)?;
    let menu = menu("http://127.0.0.1:1/v1/messages")?;
    let mut connection = connection(&menu);
    let qwen = crate::claude_desktop_config::shared_plain_model_name("vary-provider/qwen");
    std::fs::write(
        &settings,
        serde_json::to_vec(
            &json!({"__model_selector_state":{"code":{"model":qwen},"cowork":{"model":"unknown"}}}),
        )?,
    )?;
    let router = Connections::with_desktop(home.path().join("state"), vec![root])?;
    router
        .desktop
        .remember(&HeaderMap::new(), "vary-provider/worker");
    let body = json!({"max_tokens":1,"messages":[{"role":"user","content":"."}]});
    let choices = router.desktop.chosen();
    assert_eq!(
        router
            .desktop
            .resolve(
                &connection,
                &menu,
                &qwen,
                &body,
                &HeaderMap::new(),
                &choices
            )
            .alias,
        "vary-provider/qwen"
    );
    std::fs::write(settings, "synthetic invalid JSON")?;
    assert!(router.desktop.chosen().is_empty());
    assert_eq!(
        router
            .desktop
            .resolve(&connection, &menu, &qwen, &body, &HeaderMap::new(), &[])
            .alias,
        "vary-provider/chat"
    );
    connection.visible_models = Some(vec![]);
    assert_eq!(
        router
            .desktop
            .resolve(&connection, &menu, &qwen, &body, &HeaderMap::new(), &[])
            .alias,
        "vary-provider/qwen"
    );
    Ok(())
}

#[test]
fn desktop_classifier_native_tiers_stay_beside_the_selected_claude_provider(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut menu = menu("http://127.0.0.1:1/v1/messages")?;
    Arc::make_mut(menu.groups.get_mut("vary-provider/chat").ok_or("fixture")?).targets[0].model =
        "claude-opus-5-5".into();
    Arc::make_mut(
        menu.groups
            .get_mut("vary-provider/worker")
            .ok_or("fixture")?,
    )
    .targets[0]
        .model = "claude-sonnet-5".into();
    let mut sibling = (*menu.groups["vary-provider/worker"]).clone();
    sibling.targets[0].provider_id = "primary".into();
    menu.groups
        .insert("vary-provider/sibling".into(), Arc::new(sibling.clone()));
    menu.models.insert(
        "vary-provider/sibling".into(),
        MenuModel {
            alias: "vary-provider/sibling".into(),
            label: "Sibling".into(),
            description: "VaryAI direct provider model",
            order: 3,
        },
    );
    sibling.targets[0].model = "claude-sonnet-6".into();
    menu.groups
        .insert("vary-provider/newer".into(), Arc::new(sibling));
    menu.models.insert(
        "vary-provider/newer".into(),
        MenuModel {
            alias: "vary-provider/newer".into(),
            label: "Newer".into(),
            description: "VaryAI direct provider model",
            order: 4,
        },
    );
    let connection = connection(&menu);
    let router = desktop::Desktop::default();
    router.remember(&HeaderMap::new(), "vary-provider/chat");
    let headers = HeaderMap::from_iter([(
        "user-agent".parse()?,
        "claude-cli/2.1.309 (external, claude-desktop)".parse()?,
    )]);
    let body = json!({"max_tokens":256,"system":"Synthetic classifier","messages":[]});
    for explicit in [
        "synthetic-unknown-model",
        "claude-sonnet-5-unsupported",
        "provider/claude-sonnet-5",
    ] {
        assert_eq!(
            router
                .resolve(&connection, &menu, explicit, &body, &headers, &[])
                .alias,
            explicit
        );
    }
    assert_eq!(
        router
            .resolve(&connection, &menu, "claude-sonnet-5", &body, &headers, &[])
            .alias,
        "vary-provider/sibling"
    );
    assert_eq!(
        router
            .resolve(
                &connection,
                &menu,
                "claude-sonnet-5-20251001",
                &body,
                &headers,
                &[]
            )
            .alias,
        "vary-provider/sibling"
    );
    let explicit = crate::claude_desktop_config::shared_plain_model_name("vary-provider/worker");
    assert_eq!(
        router
            .resolve(&connection, &menu, &explicit, &body, &headers, &[])
            .alias,
        "vary-provider/worker"
    );
    Arc::make_mut(menu.groups.get_mut("vary-provider/chat").ok_or("fixture")?).targets[0].model =
        "synthetic-non-claude".into();
    assert_eq!(
        router
            .resolve(&connection, &menu, "claude-sonnet-5", &body, &headers, &[])
            .alias,
        "vary-provider/chat"
    );
    Ok(())
}

#[test]
fn desktop_auxiliary_patch_preserves_opaque_protocol_values(
) -> Result<(), Box<dyn std::error::Error>> {
    let raw = br#"{"model":"synthetic","max_tokens":1,"messages":[{"role":"user","content":"."}],"opaque":{"n":18446744073709551616,"p":0.12345678901234567890123456789},"tools_extra":[{"input":"synthetic"}]}"#;
    let patched = desktop::patch(raw, "vary-provider/chat", true)?;
    let text = std::str::from_utf8(&patched)?;
    assert!(text.contains(r#""n":18446744073709551616"#));
    assert!(text.contains(r#""p":0.12345678901234567890123456789"#));
    assert!(text.contains(r#""tools_extra":[{"input":"synthetic"}]"#));
    Ok(())
}

#[test]
fn ordinary_desktop_aliases_do_not_enter_native_small_fast_selection(
) -> Result<(), Box<dyn std::error::Error>> {
    let menu = menu("http://127.0.0.1:1/v1/messages")?;
    for model in crate::services::agent_connections::models(&menu) {
        let wire = crate::claude_desktop_config::shared_model_wire(&model);
        assert!(crate::claude_desktop_config::is_claude_safe_model_id(&wire));
        assert!(!["haiku", "sonnet", "opus"]
            .iter()
            .any(|tier| wire.contains(tier)));
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn desktop_electron_probe_keeps_authentication_and_real_upstream_failures(
) -> Result<(), Box<dyn std::error::Error>> {
    struct Restore(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _restore = Restore((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let rejected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let upstream_rejected = rejected.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/v1/messages", post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let tx = tx.clone();
            let rejected = upstream_rejected.load(std::sync::atomic::Ordering::SeqCst);
            async move {
                let _ = tx.send((headers, body)).await;
                if rejected {
                    return (StatusCode::FORBIDDEN, Json(json!({"error":{"type":"permission_error","message":"Synthetic upstream rejection"}})));
                }
                (StatusCode::OK, Json(json!({"id":"synthetic-response","type":"message","role":"assistant","model":"synthetic","content":[{"type":"text","text":"synthetic"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}})))
            }
        }))).await
    });
    let published = menu(&format!("http://{address}/v1/messages"))?;
    let connection = connection(&published);
    let runtime = Arc::new(Runtime::default());
    runtime.set_global(published, 0)?;
    let receipt = Receipt {
        app: "claude-desktop".into(),
        token: connection.token,
        model: "vary-provider/chat".into(),
        model_map: connection.model_map,
        visible_models: None,
        files: vec![],
        suspended: false,
        applied_at_ms: None,
    };
    runtime.connections.install(&receipt)?;
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
    let root = format!(
        "http://127.0.0.1:{}/connections/claude-desktop/v1",
        info.port
    );
    let client = reqwest::Client::builder().no_proxy().build()?;
    let probe = json!({"model":crate::claude_desktop_config::shared_plain_model_name("vary-provider/chat"),"max_tokens":1,"messages":[{"role":"user","content":"."}]});
    for origin in [None, Some("null"), Some("app://localhost")] {
        for auth_header in ["authorization", "x-api-key"] {
            let credential = if auth_header == "authorization" {
                format!("Bearer {}", receipt.token)
            } else {
                receipt.token.clone()
            };
            for (suffix, method) in [
                ("models", http::Method::GET),
                ("messages", http::Method::POST),
            ] {
                let mut request = client
                    .request(method.clone(), format!("{root}/{suffix}"))
                    .header(auth_header, &credential)
                    .header("sec-fetch-site", "none")
                    .header("sec-fetch-mode", "no-cors")
                    .header("sec-fetch-dest", "empty");
                if let Some(origin) = origin {
                    request = request.header("origin", origin);
                }
                if method == http::Method::POST {
                    request = request.json(&probe);
                }
                let response = request.send().await?;
                assert_eq!(response.status(), StatusCode::OK);
                assert!(!response
                    .headers()
                    .contains_key("access-control-allow-origin"));
                if method == http::Method::POST {
                    let (headers, sent) =
                        tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
                            .await?
                            .ok_or("missing fixture request")?;
                    assert_eq!(sent["max_tokens"], 16);
                    assert_eq!(sent["model"], "synthetic-chat");
                    assert!(!headers.contains_key("authorization"));
                    assert!(!headers.contains_key("x-api-key"));
                }
            }
        }
    }
    for token in [None, Some("synthetic-wrong-key")] {
        for (suffix, method) in [
            ("models", http::Method::GET),
            ("messages", http::Method::POST),
        ] {
            let mut request = client
                .request(method.clone(), format!("{root}/{suffix}"))
                .header("origin", "null")
                .header("sec-fetch-site", "none");
            if method == http::Method::POST {
                request = request.json(&probe);
            }
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            let response = request.send().await?;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                response.json::<Value>().await?["error"]["code"],
                "agent_connection_unavailable"
            );
            assert!(rx.try_recv().is_err());
        }
    }
    let response = client
        .post(format!("{root}/messages"))
        .bearer_auth(&receipt.token)
        .header("origin", "https://fixture.invalid")
        .header("sec-fetch-site", "cross-site")
        .json(&probe)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(rx.try_recv().is_err());
    rejected.store(true, std::sync::atomic::Ordering::SeqCst);
    let response = client
        .post(format!("{root}/messages"))
        .bearer_auth(&receipt.token)
        .header("origin", "null")
        .header("sec-fetch-site", "none")
        .json(&probe)
        .send()
        .await?;
    // The shared router sanitizes upstream rejection into its existing 502 response.
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        response.json::<Value>().await?["error"]["code"],
        "routing_candidates_unavailable"
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await?
            .is_some()
    );
    runtime.connections.revoke("claude-desktop");
    let response = client
        .post(format!("{root}/messages"))
        .bearer_auth(&receipt.token)
        .header("origin", "null")
        .header("sec-fetch-site", "none")
        .json(&probe)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(rx.try_recv().is_err());
    server.stop().await?;
    task.abort();
    Ok(())
}
