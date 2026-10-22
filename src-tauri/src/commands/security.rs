use serde::{Deserialize, Serialize};
use tauri::State;

use crate::database::{
    AuditLogFilter, AuditLogPageResult, AuditLogRecord, CustomSecurityRule, SecurityAuditSettings,
};
use crate::proxy::security::builtin_rules::BUILTIN_RULES;
use crate::proxy::security::engine::SecurityAuditEngine;
use crate::store::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityRuleItem {
    pub id: String,
    pub name: String,
    pub category: String,
    pub rule_type: Option<String>,
    pub risk_level: String,
    pub action: String,
    pub enabled: bool,
    pub is_builtin: bool,
    pub description: Option<String>,
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleMatchResult {
    pub rule_id: String,
    pub rule_name: String,
    pub category: String,
    pub risk_level: String,
    pub action: String,
    pub matched_snippet_masked: String,
    pub location: String,
}

#[tauri::command]
pub async fn get_security_audit_settings(
    state: State<'_, AppState>,
) -> Result<SecurityAuditSettings, String> {
    state.db.get_security_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_security_audit_settings(
    state: State<'_, AppState>,
    settings: SecurityAuditSettings,
) -> Result<(), String> {
    state
        .proxy_service
        .update_security_audit_settings(&settings)
        .await
}

#[tauri::command]
pub async fn get_security_audit_logs(
    state: State<'_, AppState>,
    filter: AuditLogFilter,
    page: u32,
    page_size: u32,
) -> Result<AuditLogPageResult, String> {
    state
        .db
        .get_security_audit_logs(&filter, page, page_size)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_security_audit_logs(
    state: State<'_, AppState>,
    before_timestamp: Option<i64>,
) -> Result<u64, String> {
    state
        .db
        .clear_security_audit_logs(before_timestamp)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_security_rules(
    state: State<'_, AppState>,
) -> Result<Vec<SecurityRuleItem>, String> {
    let mut items = Vec::new();
    let runtime = crate::proxy::security::get_security_runtime();

    for rule in BUILTIN_RULES.iter() {
        items.push(SecurityRuleItem {
            id: rule.id.to_string(),
            name: rule.name.to_string(),
            category: rule.category.as_str().to_string(),
            rule_type: None,
            risk_level: rule.risk_level.as_str().to_string(),
            action: runtime
                .builtin_action_overrides
                .get(rule.id)
                .copied()
                .unwrap_or(rule.default_action)
                .as_str()
                .to_string(),
            enabled: true,
            is_builtin: true,
            description: Some("系统内置敏感信息检测规则".to_string()),
            pattern: None,
        });
    }

    items.push(SecurityRuleItem {
        id: "chinese_id_card".to_string(),
        name: "中国大陆居民身份证号".to_string(),
        category: "pii".to_string(),
        rule_type: None,
        risk_level: "MEDIUM".to_string(),
        action: runtime
            .builtin_action_overrides
            .get("chinese_id_card")
            .copied()
            .unwrap_or(crate::proxy::security::types::AuditAction::Redact)
            .as_str()
            .to_string(),
        enabled: true,
        is_builtin: true,
        description: Some("基于 GB 11643 标准模 11 校验和算法".to_string()),
        pattern: None,
    });
    items.push(SecurityRuleItem {
        id: "chinese_phone_number".to_string(),
        name: "中国大陆手机号码".to_string(),
        category: "pii".to_string(),
        rule_type: None,
        risk_level: "MEDIUM".to_string(),
        action: runtime
            .builtin_action_overrides
            .get("chinese_phone_number")
            .copied()
            .unwrap_or(crate::proxy::security::types::AuditAction::Redact)
            .as_str()
            .to_string(),
        enabled: true,
        is_builtin: true,
        description: Some("11 位国内手机号码规则".to_string()),
        pattern: None,
    });

    let custom_rules = state
        .db
        .get_custom_security_rules()
        .map_err(|e| e.to_string())?;

    for rule in custom_rules {
        items.push(SecurityRuleItem {
            id: rule.id,
            name: rule.name,
            category: "custom".to_string(),
            rule_type: Some(rule.rule_type),
            risk_level: rule.risk_level,
            action: rule.action,
            enabled: rule.enabled,
            is_builtin: false,
            description: rule.description,
            pattern: Some(rule.pattern),
        });
    }

    Ok(items)
}

#[tauri::command]
pub async fn update_builtin_security_rule_action(
    state: State<'_, AppState>,
    id: String,
    action: Option<String>,
) -> Result<(), String> {
    state
        .db
        .update_builtin_security_rule_action(&id, action.as_deref())
        .map_err(|e| e.to_string())?;
    crate::proxy::security::reload_security_runtime(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_custom_security_rule_action(
    state: State<'_, AppState>,
    id: String,
    action: String,
) -> Result<(), String> {
    let updated = state
        .db
        .update_custom_security_rule_action(&id, &action)
        .map_err(|e| e.to_string())?;
    if !updated {
        return Err("自定义规则不存在".to_string());
    }
    crate::proxy::security::reload_security_runtime(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_custom_security_rule(
    state: State<'_, AppState>,
    mut rule: CustomSecurityRule,
) -> Result<(), String> {
    if rule.id.trim().is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    if rule.name.trim().is_empty() {
        return Err("规则名称不能为空".to_string());
    }
    if rule.pattern.trim().is_empty() {
        return Err("规则匹配模式不能为空".to_string());
    }

    let normalized_type = rule.rule_type.to_ascii_lowercase();
    if normalized_type != "keyword" && normalized_type != "regex" {
        return Err("无效的规则类型，仅支持 'keyword' 或 'regex'".to_string());
    }
    rule.rule_type = normalized_type;

    if rule.rule_type == "regex" {
        regex::Regex::new(&rule.pattern).map_err(|e| format!("无效的正则表达式: {e}"))?;
    }

    let normalized_risk = rule.risk_level.to_ascii_uppercase();
    if !["CRITICAL", "HIGH", "MEDIUM", "LOW"].contains(&normalized_risk.as_str()) {
        return Err("无效的风险等级，仅支持 CRITICAL, HIGH, MEDIUM, LOW".to_string());
    }
    rule.risk_level = normalized_risk;

    let normalized_action = rule.action.to_ascii_lowercase();
    if !["block", "audit", "redact", "allow"].contains(&normalized_action.as_str()) {
        return Err("无效的处理动作，仅支持 block、audit、redact 或 allow".to_string());
    }
    rule.action = normalized_action;

    if rule.updated_at <= 0 {
        rule.updated_at = chrono::Utc::now().timestamp_millis();
    }

    state
        .db
        .save_custom_security_rule(&rule)
        .map_err(|e| e.to_string())?;

    if let Err(e) = crate::proxy::security::reload_security_runtime(&state.db) {
        log::warn!("[SecurityAudit] 热更新安全运行时配置失败: {e}");
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_custom_security_rule(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<bool, String> {
    let res = state
        .db
        .toggle_custom_security_rule(&id, enabled)
        .map_err(|e| e.to_string())?;

    if let Err(e) = crate::proxy::security::reload_security_runtime(&state.db) {
        log::warn!("[SecurityAudit] 热更新安全运行时配置失败: {e}");
    }
    Ok(res)
}

#[tauri::command]
pub async fn delete_custom_security_rule(
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    let res = state
        .db
        .delete_custom_security_rule(&id)
        .map_err(|e| e.to_string())?;

    if let Err(e) = crate::proxy::security::reload_security_runtime(&state.db) {
        log::warn!("[SecurityAudit] 热更新安全运行时配置失败: {e}");
    }
    Ok(res)
}

#[tauri::command]
pub async fn test_security_rule_match(text: String) -> Result<Vec<RuleMatchResult>, String> {
    let findings = SecurityAuditEngine::inspect_text(&text, "sandbox_input");
    let results = findings
        .into_iter()
        .map(|f| RuleMatchResult {
            rule_id: f.rule_id,
            rule_name: f.rule_name,
            category: f.category.as_str().to_string(),
            risk_level: f.risk_level.as_str().to_string(),
            action: f.action.as_str().to_string(),
            matched_snippet_masked: f.matched_snippet_masked,
            location: f.location,
        })
        .collect();

    Ok(results)
}

#[tauri::command]
pub async fn get_security_audit_alerts(
    state: State<'_, AppState>,
    since_timestamp: Option<i64>,
) -> Result<Vec<AuditLogRecord>, String> {
    let since = since_timestamp.unwrap_or(0);
    state
        .db
        .get_recent_high_risk_alerts(since)
        .map_err(|e| e.to_string())
}

/// Aggregate committed security audit events off the async runtime thread.
#[tauri::command]
pub async fn get_security_event_count(
    state: State<'_, AppState>,
    app_type: String,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<u64, String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.get_security_event_count(&app_type, start_timestamp, end_timestamp)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn validate_notification_query(since_id: i64, mounted_at: i64, limit: u32) -> Result<(), String> {
    if since_id < 0 || mounted_at < 0 || !(1..=100).contains(&limit) {
        return Err("invalid security audit notification query parameters".into());
    }
    Ok(())
}

async fn query_notification_batch(
    db: std::sync::Arc<crate::database::Database>,
    since_id: i64,
    mounted_at: i64,
    limit: u32,
) -> Result<Vec<AuditLogRecord>, String> {
    validate_notification_query(since_id, mounted_at, limit)?;
    match tokio::task::spawn_blocking(move || {
        db.get_security_audit_notification_batch(since_id, mounted_at, limit)
    })
    .await
    {
        Ok(Ok(rows)) => Ok(rows),
        Ok(Err(_)) => {
            log::error!("[SecurityAudit] stage=notification_query category=database since_id={since_id} mounted_at={mounted_at} limit={limit}");
            Err("security audit notification query failed".into())
        }
        Err(_) => {
            log::error!("[SecurityAudit] stage=notification_query category=join since_id={since_id} mounted_at={mounted_at} limit={limit}");
            Err("security audit notification query failed".into())
        }
    }
}

#[tauri::command]
pub async fn get_security_audit_notification_batch(
    state: State<'_, AppState>,
    since_id: i64,
    mounted_at: i64,
    limit: u32,
) -> Result<Vec<AuditLogRecord>, String> {
    validate_notification_query(since_id, mounted_at, limit)?;
    query_notification_batch(state.db.clone(), since_id, mounted_at, limit).await
}

#[cfg(test)]
mod notification_query_tests {
    use super::*;

    #[test]
    fn rejects_invalid_cursor_time_and_page_size() {
        for (id, time, size) in [(-1, 0, 100), (0, -1, 100), (0, 0, 0), (0, 0, 101)] {
            assert!(validate_notification_query(id, time, size).is_err());
        }
        assert!(validate_notification_query(0, 0, 1).is_ok());
        assert!(validate_notification_query(i64::MAX, i64::MAX, 100).is_ok());
    }

    #[tokio::test]
    async fn ipc_query_propagates_database_failure_without_secrets() {
        let db = std::sync::Arc::new(crate::database::Database::memory().unwrap());
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE security_audit_logs")
            .unwrap();
        let result = query_notification_batch(db, 0, 0, 100).await;
        assert_eq!(
            result.unwrap_err(),
            "security audit notification query failed"
        );
    }
}
