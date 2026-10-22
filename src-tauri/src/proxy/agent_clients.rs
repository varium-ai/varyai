//! Transparent model requests for native agents; credentials remain client-owned.
use super::{qoder_usage::Job, server::ProxyState, ProxyError};
use crate::{
    app_config::AppType,
    services::agent_clients::{self as native, Backup},
};
use axum::{
    body::to_bytes,
    extract::{Path, Request, State},
    http::{header, HeaderMap},
    response::Response,
};
use serde_json::Value;
use std::str::FromStr;

fn invalid() -> ProxyError {
    ProxyError::InvalidRequest("Invalid or inactive agent route; nothing was forwarded".into())
}
pub(crate) async fn handle(
    State(state): State<ProxyState>,
    Path((app, id, path)): Path<(String, String, String)>,
    request: Request,
) -> Result<Response, ProxyError> {
    if !native::supported(&app) && app != "trae" {
        return Err(invalid());
    }
    let app_type = AppType::from_str(&app).map_err(|_| invalid())?;
    let db = state.db.clone();
    let lookup = app.clone();
    let backup = tokio::task::spawn_blocking(move || {
        if !db.get_bool_flag(&native::enabled_key(&lookup))? {
            return Err(crate::error::AppError::InvalidInput(
                "Agent route disabled".into(),
            ));
        }
        futures::executor::block_on(db.get_live_backup(&lookup))
    })
    .await
    .map_err(|_| invalid())?
    .map_err(|_| invalid())?
    .ok_or_else(invalid)?;
    let backup: Backup = serde_json::from_str(&backup.original_config).map_err(|_| invalid())?;
    let route = backup
        .routes
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(invalid)?;
    let local = url::Url::parse(&route.local).map_err(|_| invalid())?;
    let prefix = format!("/agents/{app}/providers/{id}");
    let base_path = local
        .path()
        .strip_prefix(&prefix)
        .ok_or_else(invalid)?
        .trim_matches('/');
    let suffix = if base_path.is_empty() {
        path.as_str()
    } else {
        path.strip_prefix(base_path)
            .and_then(|p| p.strip_prefix('/'))
            .ok_or_else(invalid)?
    };
    let is_model = matches!(
        suffix,
        "chat/completions" | "responses" | "messages" | "beta/messages" | "v1/messages"
    ) && request.method() == http::Method::POST;
    if !is_model && !(suffix == "models" && request.method() == http::Method::GET) {
        return Err(invalid());
    }
    #[cfg(test)]
    if let Ok(dir) = std::env::var("VARYAI_LIVE_TEST_DIR") {
        let _ = std::fs::write(std::path::Path::new(&dir).join("endpoint-suffix"), suffix);
    }
    let (mut parts, body) = request.into_parts();
    let bytes = to_bytes(body, 100 * 1024 * 1024)
        .await
        .map_err(|_| invalid())?;
    let bytes = super::handlers::decode_codex_request_body(&mut parts.headers, bytes)?;
    let mut payload = if is_model {
        serde_json::from_slice::<Value>(&bytes).map_err(|_| invalid())?
    } else {
        Value::Null
    };
    let model = payload["model"].as_str().unwrap_or_default().to_string();
    if is_model {
        if !route.models.is_empty() && !route.models.contains(&model) {
            return Err(invalid());
        }
        #[cfg(test)]
        let before = payload.to_string();
        let blocked = super::security::inspect_and_intercept_mut_with_headers(
            &app_type,
            &mut payload,
            &parts.headers,
        )?;
        #[cfg(test)]
        if let Ok(dir) = std::env::var("VARYAI_LIVE_TEST_DIR") {
            let after = payload.to_string();
            let flags = serde_json::json!({"phone_before":before.contains("13800138000"),"phone_after":after.contains("13800138000"),"private_marker_before":before.contains("-----BEGIN RSA PRIVATE KEY-----"),"private_marker_after":after.contains("-----BEGIN RSA PRIVATE KEY-----"),"blocked":blocked.is_some()});
            let _ = crate::config::atomic_write_private(
                &std::path::Path::new(&dir).join("dlp-outcome.json"),
                flags.to_string().as_bytes(),
            );
        }
        if let Some(blocked) = blocked {
            return Ok(blocked);
        }
    }
    let session = super::security::audit_session_id(&app_type, &parts.headers, &payload);
    let mut target = url::Url::parse(&route.upstream).map_err(|_| invalid())?;
    target.set_path(&format!("{}/{suffix}", target.path().trim_end_matches('/')));
    target.set_query(parts.uri.query());
    let mut headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        if matches!(
            name.as_str(),
            "host"
                | "content-length"
                | "content-encoding"
                | "connection"
                | "proxy-authorization"
                | "proxy-connection"
                | "transfer-encoding"
                | "cookie"
                | "accept-encoding"
        ) {
            continue;
        }
        headers.append(name.clone(), value.clone());
    }
    headers.insert(
        header::ACCEPT_ENCODING,
        http::HeaderValue::from_static("identity"),
    );
    parts.headers = headers;
    let job = is_model.then(|| Job {
        app_type: app,
        db: state.db.clone(),
        provider_id: route.provider_id.clone(),
        request_model: model,
        session_id: session,
        usage: Default::default(),
        latency_ms: 0,
        status: 0,
        streaming: payload["stream"].as_bool().unwrap_or(false),
    });
    let bytes = if is_model {
        serde_json::to_vec(&payload).map_err(|_| invalid())?
    } else {
        bytes.to_vec()
    };
    super::qoder::send(target.into(), parts, bytes, job).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Router};
    use serde_json::json;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };
    fn authorized_test_gateway() -> Value {
        let path = dirs::home_dir()
            .expect("home")
            .join(".cc-switch/cc-switch.db");
        let conn =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("read configured gateway");
        let raw: String = conn
            .query_row(
                "SELECT settings_config FROM providers WHERE app_type='qoder' AND name='varyai'",
                [],
                |row| row.get(0),
            )
            .expect("existing authorized gateway");
        let saved: Value = serde_json::from_str(&raw).expect("config");
        let models = saved["models"].as_array().expect("models");
        json!({"baseUrl":saved["baseUrl"],"apiKey":saved["apiKey"],"protocol":"openai","models":[{"model":models[0]["model"]}]})
    }

    #[tokio::test]
    #[ignore = "Manual desktop acceptance; explicitly enable with VARYAI_LIVE_DESKTOP_TEST=1, restore with a stop control file"]
    async fn live_desktop_acceptance_with_config_restore() {
        assert_eq!(
            std::env::var("VARYAI_LIVE_DESKTOP_TEST").as_deref(),
            Ok("1"),
            "explicit live desktop opt-in required"
        );
        let _ = rustls::crypto::ring::default_provider().install_default();
        let dir = std::path::PathBuf::from(
            std::env::var("VARYAI_LIVE_TEST_DIR").expect("explicit control directory"),
        );
        assert!(dir.is_dir(), "control directory exists");
        let app = std::env::var("VARYAI_LIVE_DESKTOP_APP").unwrap_or_else(|_| "zcode".into());
        assert!(matches!(app.as_str(), "kimi" | "zcode"));
        let app = app.as_str();
        let path = native::config_path(app).expect("desktop config");
        let mut doc = native::Document::read(app, &path).expect("valid native config");
        let selection = doc.selection(app);
        let custom =
            if app == "kimi" || std::env::var("VARYAI_LIVE_DESKTOP_CUSTOM").as_deref() == Ok("1") {
                Some(authorized_test_gateway())
            } else {
                None
            };
        if let Some(config) = &custom {
            let exists = if app == "kimi" {
                doc.value["providers"].get("varyai-desktop-test").is_some()
            } else {
                doc.value
                    .pointer("/config/providerConfigRules/providerRules")
                    .and_then(Value::as_array)
                    .is_some_and(|rules| {
                        rules
                            .iter()
                            .any(|rule| rule["providerId"] == "varyai-desktop-test")
                    })
            };
            assert!(!exists, "test provider must not exist");
            let metadata = json!({"app":app,"provider_id":"desktop-test","baseUrl":config["baseUrl"],"selection":selection});
            crate::config::atomic_write_private(
                &dir.join("cleanup.json"),
                metadata.to_string().as_bytes(),
            )
            .expect("recovery metadata");
            doc.apply(app, "desktop-test", "VaryAI Desktop Test", config)
                .expect("test provider");
        }
        let added = if app == "zcode" {
            doc.prepare_zcode_official(
                &native::zcode_builtin().expect("public provider definitions"),
            )
            .expect("official endpoints")
        } else {
            vec![]
        };
        let db = Arc::new(crate::database::Database::memory().expect("db"));
        let gateway = super::super::server::ProxyServer::new(
            crate::proxy::ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            db.clone(),
            None,
        );
        let info = gateway.start().await.expect("start gateway");
        let mut routes = doc
            .route(app, &format!("http://127.0.0.1:{}", info.port))
            .expect("route");
        for route in &mut routes {
            if let Some(rule) = route
                .pointer
                .strip_suffix("/config/api/baseUrl")
                .and_then(|p| doc.value.pointer(p))
            {
                route.created_provider = added
                    .iter()
                    .find(|created| created["providerId"] == rule["providerId"])
                    .cloned();
            }
        }
        let backup = Backup {
            path: path.clone(),
            routes,
        };
        let encoded = serde_json::to_string(&backup).expect("encode");
        crate::config::atomic_write_private(&dir.join("routing-backup.json"), encoded.as_bytes())
            .expect("durable recovery metadata");
        db.save_live_backup(app, &encoded).await.expect("backup");
        db.set_setting(&native::enabled_key(app), "true")
            .expect("enabled");
        doc.write(&path).expect("project desktop routes");
        std::fs::write(dir.join("ready"), info.port.to_string()).expect("ready");
        let mut previous = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
        while tokio::time::Instant::now() < deadline && !dir.join("stop").exists() {
            let policy =
                std::fs::read_to_string(dir.join("policy")).unwrap_or_else(|_| "redact".into());
            if policy != previous {
                use super::super::security::{types::AuditAction, SecurityRuntimeConfig};
                let action = match policy.trim() {
                    "block" => AuditAction::Block,
                    "audit" => AuditAction::Audit,
                    _ => AuditAction::Redact,
                };
                let mut runtime = SecurityRuntimeConfig::default();
                for id in ["rsa_ssh_private_key", "chinese_phone_number"] {
                    runtime.builtin_action_overrides.insert(id.into(), action);
                }
                super::super::security::runtime::set_security_runtime_for_test(runtime);
                previous = policy;
            }
            if let Ok(summary) = db.get_usage_summary(None, None, Some(app), None, None) {
                let _ = std::fs::write(dir.join("usage-count"), summary.total_requests.to_string());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let restored = native::restore(&db, app).await;
        super::super::security::runtime::set_security_runtime_for_test(Default::default());
        gateway.stop().await.expect("stop gateway");
        restored.expect("restore desktop config");
        if let Some(config) = &custom {
            let mut restored = native::Document::read(app, &path).expect("restored doc");
            if restored.selection_owned(app, Some("desktop-test")) {
                restored
                    .set_selection(app, &selection)
                    .expect("original selection");
            }
            restored
                .remove(app, "desktop-test", config)
                .expect("remove temporary provider");
            restored.write(&path).expect("restore provider list");
        }
        std::fs::write(dir.join("restored"), "yes").expect("restoration marker");
    }

    #[tokio::test]
    #[ignore = "Requires the signed-in native Kimi CLI and explicit live opt-in"]
    async fn signed_in_kimi_official_request_reaches_gateway_and_restores() {
        assert_eq!(
            std::env::var("VARYAI_KIMI_OFFICIAL_TEST").as_deref(),
            Ok("1")
        );
        let _ = rustls::crypto::ring::default_provider().install_default();
        let path = native::config_path("kimi").expect("config path");
        let mut doc = native::Document::read("kimi", &path).expect("native config");
        let db = Arc::new(crate::database::Database::memory().expect("database"));
        let gateway = super::super::server::ProxyServer::new(
            crate::proxy::ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            db.clone(),
            None,
        );
        let info = gateway.start().await.expect("gateway");
        let routes = doc
            .route("kimi", &format!("http://127.0.0.1:{}", info.port))
            .expect("routes");
        let encoded = serde_json::to_string(&Backup {
            path: path.clone(),
            routes,
        })
        .expect("backup");
        let recovery =
            std::path::PathBuf::from("/private/tmp/varyai-kimi-official-routing-backup.json");
        crate::config::atomic_write_private(&recovery, encoded.as_bytes()).expect("durable backup");
        db.save_live_backup("kimi", &encoded).await.expect("save");
        db.set_setting(&native::enabled_key("kimi"), "true")
            .expect("enable");
        doc.write(&path).expect("project");
        let dir = tempfile::tempdir().expect("working dir");
        let mut command =
            tokio::process::Command::new(std::env::var("VARYAI_KIMI_TEST_BIN").expect("binary"));
        command
            .current_dir(dir.path())
            .arg("-p")
            .arg("Do not use tools. Reply only OK.")
            .kill_on_drop(true);
        let result = tokio::time::timeout(Duration::from_secs(120), command.output()).await;
        let restored = native::restore(&db, "kimi").await;
        let summary = db
            .get_usage_summary(None, None, Some("kimi"), None, None)
            .expect("usage");
        gateway.stop().await.expect("stop");
        restored.expect("restore configuration");
        std::fs::write("/private/tmp/varyai-kimi-official-restored", "yes").expect("marker");
        let output = result
            .expect("request timed out after restoring routing")
            .expect("run CLI");
        assert!(
            output.status.success(),
            "Official Kimi request failed; credentials and response withheld"
        );
        assert!(
            summary.total_requests > 0,
            "No successful official request was recorded"
        );
    }

    #[tokio::test]
    #[ignore = "Requires installed native client binaries"]
    async fn installed_cli_reads_projected_configuration_and_reaches_the_gateway() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        use super::super::security::{types::AuditAction, SecurityRuntimeConfig};
        let protocol =
            std::env::var("VARYAI_NATIVE_TEST_PROTOCOL").unwrap_or_else(|_| "openai".into());
        let only_app = std::env::var("VARYAI_NATIVE_TEST_APP").ok();
        let real = std::env::var("VARYAI_NATIVE_REAL_UPSTREAM").as_deref() == Ok("1");
        let config = if real {
            authorized_test_gateway()
        } else {
            Value::Null
        };
        let actions = if real {
            vec![AuditAction::Audit]
        } else {
            vec![AuditAction::Block, AuditAction::Redact, AuditAction::Audit]
        };
        for action in actions {
            let mut runtime = SecurityRuntimeConfig::default();
            for id in ["rsa_ssh_private_key", "chinese_phone_number"] {
                runtime.builtin_action_overrides.insert(id.into(), action);
            }
            super::super::security::runtime::set_security_runtime_for_test(runtime);
            for (app, var, home_env) in [
                ("kimi", "VARYAI_KIMI_TEST_BIN", "KIMI_CODE_HOME"),
                ("qwen", "VARYAI_QWEN_TEST_BIN", "QWEN_HOME"),
                ("zcode", "VARYAI_ZCODE_TEST_BIN", "ZCODE_DATA_BASE_DIR"),
            ] {
                if only_app.as_deref().is_some_and(|selected| selected != app) {
                    continue;
                }
                let binary = std::env::var(var).expect("explicit installed client binary");
                let calls = Arc::new(AtomicUsize::new(0));
                let count = calls.clone();
                let endpoint = match protocol.as_str() {
                    "openai-responses" => "/v1/responses",
                    "anthropic" => "/v1/messages",
                    _ => "/v1/chat/completions",
                };
                let response = match protocol.as_str() {
                    "openai-responses" => concat!(
                        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_test\",\"object\":\"response\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
                        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_test\",\"type\":\"message\",\"status\":\"in_progress\",\"role\":\"assistant\",\"content\":[]}}\n\n",
                        "event: response.content_part.added\ndata: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_test\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}\n\n",
                        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_test\",\"output_index\":0,\"content_index\":0,\"delta\":\"NATIVE_CLIENT_OK\"}\n\n",
                        "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_test\",\"output_index\":0,\"content_index\":0,\"text\":\"NATIVE_CLIENT_OK\"}\n\n",
                        "event: response.content_part.done\ndata: {\"type\":\"response.content_part.done\",\"item_id\":\"msg_test\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"NATIVE_CLIENT_OK\",\"annotations\":[]}}\n\n",
                        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"msg_test\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"NATIVE_CLIENT_OK\",\"annotations\":[]}]}}\n\n",
                        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_test\",\"object\":\"response\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_test\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"NATIVE_CLIENT_OK\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":11,\"output_tokens\":4,\"total_tokens\":15}}}\n\n"
                    ),
                    "anthropic" => concat!(
                        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"alpha\",\"content\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":0}}}\n\n",
                        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"NATIVE_CLIENT_OK\"}}\n\n",
                        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":4}}\n\n",
                        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
                    ),
                    _ => concat!(
                        "data: {\"id\":\"chatcmpl-synthetic\",\"object\":\"chat.completion.chunk\",\"model\":\"alpha\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"NATIVE_CLIENT_OK\"},\"finish_reason\":null}]}\n\n",
                        "data: {\"id\":\"chatcmpl-synthetic\",\"object\":\"chat.completion.chunk\",\"model\":\"alpha\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":4}}\n\n",
                        "data: [DONE]\n\n"
                    )
                };
                let upstream = Router::new()
                    .route(
                        endpoint,
                        post(move |request: Request| {
                            let count = count.clone();
                            async move {
                                let body = to_bytes(request.into_body(), 1024 * 1024)
                                    .await
                                    .expect("body");
                                let body = String::from_utf8_lossy(&body);
                                assert_eq!(
                                    body.contains("13800138000"),
                                    action == AuditAction::Audit
                                );
                                assert_eq!(
                                    body.contains("-----BEGIN RSA PRIVATE KEY-----"),
                                    action == AuditAction::Audit
                                );
                                count.fetch_add(1, Ordering::SeqCst);
                                axum::response::Response::builder()
                                    .header("content-type", "text/event-stream")
                                    .body(axum::body::Body::from(response))
                                    .expect("response")
                            }
                        }),
                    )
                    .fallback(|request: Request| async move {
                        eprintln!(
                            "Native test unexpected upstream path: {}",
                            request.uri().path()
                        );
                        axum::http::StatusCode::NOT_FOUND
                    });
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("port");
                let addr = listener.local_addr().expect("address");
                let task = tokio::spawn(async move {
                    axum::serve(listener, upstream).await.expect("serve");
                });
                let db = Arc::new(crate::database::Database::memory().expect("db"));
                let gateway = super::super::server::ProxyServer::new(
                    crate::proxy::ProxyConfig {
                        listen_port: 0,
                        ..Default::default()
                    },
                    db.clone(),
                    None,
                );
                let info = gateway.start().await.expect("gateway");
                let dir = tempfile::tempdir().expect("isolated client directory");
                let path = dir.path().join(if app == "kimi" {
                    "config.toml"
                } else if app == "zcode" {
                    "provider_config.json"
                } else {
                    "settings.json"
                });
                let mut doc = native::Document::read(app, &path).expect("native document");
                let selected = if real {
                    config.clone()
                } else {
                    json!({"baseUrl":format!("http://{addr}{}", if protocol == "anthropic" { "" } else { "/v1" }),"apiKey":"synthetic-native-key","protocol":protocol,"models":[{"model":"alpha"}]})
                };
                doc.apply(app, "test", "Test", &selected).expect("project");
                let routes = doc
                    .route(app, &format!("http://127.0.0.1:{}", info.port))
                    .expect("route");
                doc.write(&path).expect("write");
                db.save_live_backup(
                    app,
                    &serde_json::to_string(&Backup { path, routes }).expect("encode"),
                )
                .await
                .expect("persist");
                db.set_setting(&native::enabled_key(app), "true")
                    .expect("enabled");
                let mut command = tokio::process::Command::new(binary);
                command
                .current_dir(dir.path())
                .env(home_env, dir.path())
                .env("NO_COLOR", "1")
                .kill_on_drop(true)
                .arg("-p")
                .arg(if real {"Do not use tools. Reply only OK."} else {"Do not use tools. Phone 13800138000. -----BEGIN RSA PRIVATE KEY----- SYNTHETIC. Reply OK."});
                if app == "zcode" {
                    command.env("ZCODE_PERSONAL_PROVIDER_CONFIG_FILE",dir.path().join("provider_config.json")).env("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE","/Applications/ZCode.app/Contents/Resources/config/provider/zcode-builtin.json").env("HOME",dir.path());
                }
                let result = tokio::time::timeout(
                    Duration::from_secs(if real { 120 } else { 35 }),
                    command.output(),
                )
                .await
                .expect("CLI completed")
                .expect("execute CLI");
                let has_marker =
                    String::from_utf8_lossy(&result.stdout).contains("NATIVE_CLIENT_OK");
                if real {
                    assert!(
                        result.status.success(),
                        "{app} real upstream CLI failed; details withheld to protect credentials"
                    );
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
                    loop {
                        let summary = db
                            .get_usage_summary(None, None, Some(app), None, None)
                            .expect("usage");
                        if summary.total_requests > 0 {
                            break;
                        }
                        assert!(
                            tokio::time::Instant::now() < deadline,
                            "{app} real request did not record successful forwarding"
                        );
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                } else if action == AuditAction::Block {
                    assert!(!has_marker, "{app} blocked request reached upstream");
                    assert_eq!(calls.load(Ordering::SeqCst), 0);
                } else {
                    assert!(
                        result.status.success(),
                        "{app} CLI failed for {action:?}: {}",
                        String::from_utf8_lossy(&result.stderr)
                    );
                    assert!(
                        has_marker,
                        "{app} did not receive the gateway response for {action:?}"
                    );
                    assert!(
                        calls.load(Ordering::SeqCst) > 0,
                        "{app} never reached upstream"
                    );
                }
                eprintln!("Native acceptance {app} {action:?} real={real}: passed");
                gateway.stop().await.expect("stop");
                task.abort();
            }
        }
        super::super::security::runtime::set_security_runtime_for_test(Default::default());
    }
    #[tokio::test]
    #[ignore = "Manual TRAE desktop test with a synthetic local model; enable explicitly"]
    async fn live_trae_custom_desktop_gateway() {
        assert_eq!(
            std::env::var("VARYAI_TRAE_DESKTOP_TEST").as_deref(),
            Ok("1")
        );
        let _ = rustls::crypto::ring::default_provider().install_default();
        let dir =
            std::path::PathBuf::from(std::env::var("VARYAI_LIVE_TEST_DIR").expect("control dir"));
        let upstream = std::env::var("VARYAI_TRAE_TEST_UPSTREAM").expect("synthetic upstream");
        assert!(upstream.starts_with("http://127.0.0.1:"));
        let db = Arc::new(crate::database::Database::memory().expect("database"));
        let gateway = super::super::server::ProxyServer::new(
            crate::proxy::ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            db.clone(),
            None,
        );
        let info = gateway.start().await.expect("gateway");
        let local = format!(
            "http://127.0.0.1:{}/agents/trae/providers/desktop-test/v1",
            info.port
        );
        let backup = Backup {
            path: "unused-synthetic-trae-config".into(),
            routes: vec![native::Route {
                id: "desktop-test".into(),
                pointer: "synthetic-desktop-test".into(),
                upstream,
                local: local.clone(),
                models: vec!["varyai-local-test".into()],
                provider_id: "synthetic-desktop-test".into(),
                created_provider: None,
            }],
        };
        db.save_live_backup("trae", &serde_json::to_string(&backup).expect("backup"))
            .await
            .expect("save");
        db.set_setting(&native::enabled_key("trae"), "true")
            .expect("enabled");
        std::fs::write(dir.join("ready"), local).expect("ready");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(900);
        while tokio::time::Instant::now() < deadline && !dir.join("stop").exists() {
            use super::super::security::{types::AuditAction, SecurityRuntimeConfig};
            let action = match std::fs::read_to_string(dir.join("policy"))
                .unwrap_or_default()
                .trim()
            {
                "block" => AuditAction::Block,
                "audit" => AuditAction::Audit,
                _ => AuditAction::Redact,
            };
            let mut runtime = SecurityRuntimeConfig::default();
            for rule in ["rsa_ssh_private_key", "chinese_phone_number"] {
                runtime.builtin_action_overrides.insert(rule.into(), action);
            }
            super::super::security::runtime::set_security_runtime_for_test(runtime);
            let usage = db
                .get_usage_summary(None, None, Some("trae"), None, None)
                .expect("usage");
            std::fs::write(dir.join("usage.json"),json!({"requests":usage.total_requests,"input":usage.total_input_tokens,"output":usage.total_output_tokens}).to_string()).expect("stats");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        gateway.stop().await.expect("stop");
        super::super::security::runtime::set_security_runtime_for_test(Default::default());
    }
    #[tokio::test]
    async fn native_agents_redact_before_forwarding_preserve_parameters_and_record_independent_usage(
    ) {
        let _ = rustls::crypto::ring::default_provider().install_default();
        for app in ["kimi", "qwen", "zcode", "trae"] {
            let calls = Arc::new(AtomicUsize::new(0));
            let counted = calls.clone();
            let upstream=Router::new().route("/v1/chat/completions",post(move |request:Request| {let counted=counted.clone();async move {
                assert_eq!(request.headers()["authorization"],"Bearer synthetic-native-key");
                assert!(!request.headers().contains_key("cookie"));
                let value:Value=serde_json::from_slice(&to_bytes(request.into_body(),1024*1024).await.expect("body")).expect("json");
                assert_eq!(value["messages"][0]["content"],"[REDACTED_PRIVATE_KEY]");
                assert_eq!(value["future"],json!({"preserve":true}));
                counted.fetch_add(1,Ordering::SeqCst);
                axum::Json(json!({"model":"alpha","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":4}}))
            }}));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("port");
            let addr = listener.local_addr().expect("address");
            let task = tokio::spawn(async move {
                axum::serve(listener, upstream).await.expect("serve");
            });
            let db = Arc::new(crate::database::Database::memory().expect("database"));
            let gateway = super::super::server::ProxyServer::new(
                crate::proxy::ProxyConfig {
                    listen_port: 0,
                    ..Default::default()
                },
                db.clone(),
                None,
            );
            let info = gateway.start().await.expect("gateway");
            let origin = format!("http://127.0.0.1:{}", info.port);
            let local = format!("{origin}/agents/{app}/providers/test/v1");
            let backup = Backup {
                path: std::path::PathBuf::from("unused-test-config"),
                routes: vec![native::Route {
                    id: "test".into(),
                    pointer: "/unused".into(),
                    upstream: format!("http://{addr}/v1"),
                    local: local.clone(),
                    models: vec!["alpha".into()],
                    provider_id: "native-test".into(),
                    created_provider: None,
                }],
            };
            db.save_live_backup(app, &serde_json::to_string(&backup).expect("backup"))
                .await
                .expect("persist");
            db.set_setting(&native::enabled_key(app), "true")
                .expect("enabled");
            let client = reqwest::Client::new();
            let content = format!("{} RSA PRIVATE KEY{} SYNTHETIC_ONLY", "-----BEGIN", "-----");
            let response=client.post(format!("{local}/chat/completions")).bearer_auth("synthetic-native-key").header("cookie","synthetic-cookie").json(&json!({"model":"alpha","messages":[{"role":"user","content":content}],"future":{"preserve":true}})).send().await.expect("request");
            assert_eq!(response.status(), 200);
            response.bytes().await.expect("read response");
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let summary = db
                        .get_usage_summary(None, None, Some(app), None, None)
                        .expect("summary");
                    if summary.total_requests == 1 {
                        assert_eq!(summary.total_input_tokens, 11);
                        assert_eq!(summary.total_output_tokens, 4);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("asynchronous usage");
            db.set_setting(&native::enabled_key(app), "false")
                .expect("disable");
            assert!(!client
                .post(format!("{local}/chat/completions"))
                .json(&json!({"model":"alpha"}))
                .send()
                .await
                .expect("disabled response")
                .status()
                .is_success());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            gateway.stop().await.expect("stop");
            task.abort();
        }
    }
}
