//! Authenticated local bridge for native TRAE user-message text, before internal encryption.
use super::server::ProxyState;
use crate::{app_config::AppType, services::trae_bridge};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};

pub(crate) async fn audit(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    Json(mut value): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if value["health"] == true {
        return Ok(Json(json!({"ready":true})));
    }
    let session = value["sessionId"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() < 200 && !s.chars().any(char::is_whitespace))
        .map(|s| format!("trae_{s}"));
    if let Some(usage) = value.get("usage") {
        let number = |key: &str| {
            usage[key]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0)
        };
        let db = state.db.clone();
        let provider_id =
            tokio::task::spawn_blocking(move || db.get_current_provider("trae").ok().flatten())
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "trae-official".into());
        super::qoder_usage::emit(super::qoder_usage::Job {
            app_type: "trae".into(),
            db: state.db,
            provider_id,
            request_model: value["model"]
                .as_str()
                .filter(|s| s.len() < 256)
                .unwrap_or("auto")
                .into(),
            session_id: session,
            usage: super::usage::parser::TokenUsage {
                input_tokens: number("input_tokens").saturating_sub(number("cache_read_tokens")),
                output_tokens: number("output_tokens"),
                cache_read_tokens: number("cache_read_tokens"),
                cache_creation_tokens: number("cache_creation_tokens"),
                ..Default::default()
            },
            latency_ms: value["latencyMs"].as_u64().unwrap_or(0).min(86_400_000),
            status: 200,
            streaming: true,
        });
        return Ok(Json(json!({"recorded":true})));
    }
    let body = value
        .get_mut("body")
        .filter(|v| v.is_object())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if let Some(session) = session {
        body["metadata"] = json!({"session_id":session});
    }
    match super::security::inspect_and_intercept_mut_with_headers(
        &AppType::Trae,
        body,
        &HeaderMap::new(),
    ) {
        Ok(blocked) => Ok(Json(
            json!({"blocked":blocked.is_some(),"body":if blocked.is_some(){Value::Null}else{body.clone()}}),
        )),
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(trae_bridge::authorized)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unauthenticated_bridge_never_accepts_message_data() {
        assert!(!authorized(&HeaderMap::new()));
    }
}

#[cfg(test)]
mod desktop_acceptance {
    use super::*;
    use std::{sync::Arc, time::Duration};
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "Explicit TRAE desktop acceptance; binds an ephemeral port and restores the native hook"]
    async fn live_native_bridge() {
        let directory = std::path::PathBuf::from(
            std::env::var("VARYAI_TRAE_LIVE_DIR").expect("explicit test directory"),
        );
        assert!(directory.is_dir());
        let db = Arc::new(crate::database::Database::memory().expect("isolated database"));
        super::super::security::init_audit_logger(db.clone());
        let gateway = super::super::server::ProxyServer::new(
            crate::proxy::ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            db.clone(),
            None,
        );
        let info = gateway.start().await.expect("ephemeral gateway");
        let origin = format!("http://127.0.0.1:{}", info.port);
        let enable_origin = origin.clone();
        tokio::task::spawn_blocking(move || trae_bridge::enable(&enable_origin))
            .await
            .expect("join")
            .expect("native hook");
        std::fs::write(directory.join("ready"), info.port.to_string()).expect("ready");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1200);
        while tokio::time::Instant::now() < deadline && !directory.join("stop").exists() {
            use super::super::security::{types::AuditAction, SecurityRuntimeConfig};
            let policy = std::fs::read_to_string(directory.join("policy")).unwrap_or_default();
            let action = match policy.trim() {
                "audit" => AuditAction::Audit,
                "allow" => AuditAction::Allow,
                "block" => AuditAction::Block,
                _ => AuditAction::Redact,
            };
            let mut runtime = SecurityRuntimeConfig::default();
            for rule in ["rsa_ssh_private_key", "chinese_phone_number"] {
                runtime.builtin_action_overrides.insert(rule.into(), action);
            }
            super::super::security::runtime::set_security_runtime_for_test(runtime);
            // A dev rebuild may release an old process's hook during live acceptance.
            // Reconnect using this isolated gateway's token, never the user's gateway.
            let reconnect_origin = origin.clone();
            let connected =
                tokio::task::spawn_blocking(move || trae_bridge::reconnect(&reconnect_origin))
                    .await
                    .expect("reconnect task");
            let usage = db
                .get_usage_summary(None, None, Some("trae"), None, None)
                .expect("usage");
            let logs = db
                .get_security_audit_logs(
                    &crate::database::AuditLogFilter {
                        app_type: Some("trae".into()),
                        ..Default::default()
                    },
                    1,
                    100,
                )
                .expect("audit events");
            let proof = json!({"connected":connected,"policy":policy.trim(),"requests":usage.total_requests,"input":usage.total_input_tokens,"output":usage.total_output_tokens,"auditEvents":logs.items.iter().map(|v|json!({"sessionId":v.session_id,"action":v.action_taken,"rule":v.rule_id})).collect::<Vec<_>>()});
            crate::config::atomic_write_private(
                &directory.join("proof.json"),
                proof.to_string().as_bytes(),
            )
            .expect("proof");
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        tokio::task::spawn_blocking(trae_bridge::disable)
            .await
            .expect("join")
            .expect("restore");
        gateway.stop().await.expect("stop");
        super::super::security::runtime::set_security_runtime_for_test(Default::default());
    }
}
