//! Local security inspection and data-loss prevention.
//!
//! Inspect supported request text and tool arguments before forwarding.
//! Policy may audit, redact or block; this is not whole-application traffic coverage.

#![allow(dead_code)]

mod aggregation;
pub mod audit_logger;
pub mod builtin_rules;
pub mod engine;
pub mod extractor;
mod identity;
pub mod pii;
pub mod response;
pub mod runtime;
pub mod types;

#[allow(unused_imports)]
pub use runtime::{get_security_runtime, reload_security_runtime, SecurityRuntimeConfig};

#[allow(unused_imports)]
pub use audit_logger::{emit_audit_event, init_audit_logger};

#[allow(unused_imports)]
pub use response::build_blocked_response;

#[allow(unused_imports)]
pub use engine::SecurityAuditEngine;
#[allow(unused_imports)]
pub use types::*;

#[cfg(test)]
mod aggregation_tests;
#[cfg(test)]
mod tests;

use axum::{http::HeaderMap, response::Response};
use serde_json::Value;

use crate::app_config::AppType;
use crate::proxy::error::ProxyError;

/// Inspect and safely rewrite the body; block unsafe redaction without a partial commit.
pub fn inspect_and_intercept_mut(
    app_type: &AppType,
    body: &mut Value,
) -> Result<Option<Response>, ProxyError> {
    inspect_and_intercept_mut_with_headers(app_type, body, &HeaderMap::new())
}

/// HTTP entry point: capture client session metadata before redaction or blocking.
pub fn inspect_and_intercept_mut_with_headers(
    app_type: &AppType,
    body: &mut Value,
    headers: &HeaderMap,
) -> Result<Option<Response>, ProxyError> {
    inspect_with_context(app_type, app_type, "", None, body, headers)
}

/// Keep client attribution independent from the response wire protocol.
pub(crate) fn inspect_with_context(
    app_type: &AppType,
    wire: &AppType,
    provider_id: &str,
    request_id: Option<&str>,
    body: &mut Value,
    headers: &HeaderMap,
) -> Result<Option<Response>, ProxyError> {
    if body.is_null() || !body.is_object() {
        return Ok(None);
    }
    let session_id = audit_session_id(app_type, headers, body);
    let runtime = get_security_runtime();
    let detected_at = std::time::Instant::now();
    let now = chrono::Utc::now().timestamp_millis();
    let current_turn = extractor::CurrentTurn::new(body);
    let inspection = SecurityAuditEngine::inspect_and_redact_with_runtime(body, &runtime);
    for finding in &inspection.findings {
        if finding.action == AuditAction::Allow
            || (!inspection.is_blocked() && current_turn.is_historical(&finding.location))
        {
            continue;
        }
        emit_audit_event(aggregation::PendingAuditEvent {
            log: crate::database::AuditLogEvent {
                timestamp: now,
                request_id: request_id.map(str::to_owned),
                session_id: session_id.clone(),
                app_type: app_type.as_str().to_string(),
                provider_id: provider_id.into(),
                rule_id: finding.rule_id.clone(),
                rule_name: finding.rule_name.clone(),
                rule_category: finding.category.as_str().to_string(),
                risk_level: finding.risk_level.as_str().to_string(),
                action_taken: if inspection.is_blocked() {
                    "block"
                } else {
                    finding.action.as_str()
                }
                .to_string(),
                matched_snippet_masked: finding.matched_snippet_masked.clone(),
                location: Some(finding.location.clone()),
            },
            identity: if runtime.aggregation_enabled {
                finding.identity
            } else {
                None
            },
            detected_at,
            risk_epoch: runtime.risk_epoch(&finding.rule_id),
            config_revision: runtime.config_revision,
        });
    }
    if inspection.is_blocked() {
        return build_blocked_response(
            wire,
            inspection
                .block_reason
                .as_deref()
                .unwrap_or("检测到敏感信息，无法安全隐藏"),
        )
        .map(Some);
    }
    Ok(None)
}

/// Read-only inspection returns a protocol-compatible 400 response when blocked.
pub fn inspect_and_intercept(
    app_type: &AppType,
    body: &Value,
) -> Result<Option<Response>, ProxyError> {
    if body.is_null() || !body.is_object() {
        return Ok(None);
    }

    // A read-only caller cannot redact: conservatively block content requiring redaction.
    let session_id = audit_session_id(app_type, &HeaderMap::new(), body);
    let runtime = get_security_runtime();
    let detected_at = std::time::Instant::now();
    let now = chrono::Utc::now().timestamp_millis();
    let inspection = SecurityAuditEngine::inspect_request_with_runtime(body, &runtime);
    if inspection.findings.is_empty() {
        return Ok(None);
    }

    // Allow-policy hits remain internal and produce no audit event or notification.
    if inspection
        .findings
        .iter()
        .all(|finding| finding.action == AuditAction::Allow)
    {
        return Ok(None);
    }

    let blocked = inspection.is_blocked()
        || inspection
            .findings
            .iter()
            .any(|f| f.action == AuditAction::Redact);

    // Enqueue audit events asynchronously.
    let current_turn = extractor::CurrentTurn::new(body);
    for finding in &inspection.findings {
        if finding.action == AuditAction::Allow
            || (!blocked && current_turn.is_historical(&finding.location))
        {
            continue;
        }
        emit_audit_event(aggregation::PendingAuditEvent {
            log: crate::database::AuditLogEvent {
                timestamp: now,
                request_id: None,
                session_id: session_id.clone(),
                app_type: app_type.as_str().to_string(),
                provider_id: String::new(),
                rule_id: finding.rule_id.clone(),
                rule_name: finding.rule_name.clone(),
                rule_category: finding.category.as_str().to_string(),
                risk_level: finding.risk_level.as_str().to_string(),
                action_taken: if blocked {
                    "block"
                } else {
                    finding.action.as_str()
                }
                .to_string(),
                matched_snippet_masked: finding.matched_snippet_masked.clone(),
                location: Some(finding.location.clone()),
            },
            identity: if runtime.aggregation_enabled {
                finding.identity
            } else {
                None
            },
            detected_at,
            risk_epoch: runtime.risk_epoch(&finding.rule_id),
            config_revision: runtime.config_revision,
        });
    }

    if blocked {
        let reason = inspection
            .block_reason
            .as_deref()
            .unwrap_or("检测到敏感信息；当前请求无法安全隐藏，已拦截");

        log::warn!(
            "[SecurityAudit] [{}] 请求已被安全审计拦截: {}",
            app_type.as_str(),
            reason
        );

        let resp = build_blocked_response(app_type, reason)?;
        return Ok(Some(resp));
    }

    log::info!(
        "[SecurityAudit] [{}] 已记录安全事件: 检测到 {} 项敏感信息（请求已继续发送）",
        app_type.as_str(),
        inspection.findings.len()
    );

    Ok(None)
}

/// Only client-provided identities can correlate requests. Generated UUIDs are request-scoped.
pub(crate) fn audit_session_id(
    app_type: &AppType,
    headers: &HeaderMap,
    body: &Value,
) -> Option<String> {
    let format = if matches!(app_type, AppType::ClaudeDesktop) {
        "claude"
    } else {
        app_type.as_str()
    };
    let session = crate::proxy::session::extract_client_session_id(headers, body, format)?;
    let id = session.session_id.trim();
    // Reject invalid/oversized IDs instead of truncating and merging distinct sessions.
    if session.client_provided
        && !id.is_empty()
        && id.len() <= 256
        && !id.chars().any(char::is_whitespace)
        && !id.chars().any(char::is_control)
    {
        let id = if matches!(
            app_type,
            AppType::WorkBuddy | AppType::Kimi | AppType::Qwen | AppType::ZCode
        ) {
            format!("{}_{id}", app_type.as_str())
        } else {
            id.to_string()
        };
        (id.len() <= 256).then_some(id)
    } else {
        None
    }
}
