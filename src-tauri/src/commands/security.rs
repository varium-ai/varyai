use serde::{Deserialize, Serialize};
use tauri::State;

use crate::database::{
    AuditLogFilter, AuditLogPageResult, AuditLogRecord, CustomSecurityRule, Database,
    SecurityAuditSettings,
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
    update_builtin_security_rule_action_db(&state.db, &id, action.as_deref())
}

fn update_builtin_security_rule_action_db(
    db: &Database,
    id: &str,
    action: Option<&str>,
) -> Result<(), String> {
    if !matches!(id, "chinese_id_card" | "chinese_phone_number")
        && !BUILTIN_RULES.iter().any(|rule| rule.id == id)
    {
        return Err(crate::error::AppError::Database("无效的内置规则 ID".into()).to_string());
    }
    if let Some(action) = action {
        if id == "catalog:generic-sk-key-notice"
            && !matches!(action.to_ascii_lowercase().as_str(), "audit" | "allow")
        {
            return Err(crate::error::AppError::Database(
                "通用 sk- 提醒规则仅支持 audit 或 allow 动作".into(),
            )
            .to_string());
        }
        validate_rule_action_update(action)?;
    }
    crate::proxy::security::runtime::update_security_runtime(db, |db| {
        db.update_builtin_security_rule_action(id, action)
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_custom_security_rule_action(
    state: State<'_, AppState>,
    id: String,
    action: String,
) -> Result<(), String> {
    update_custom_security_rule_action_db(&state.db, &id, &action)
}

fn update_custom_security_rule_action_db(
    db: &Database,
    id: &str,
    action: &str,
) -> Result<(), String> {
    validate_rule_action_update(action)?;
    let updated = crate::proxy::security::runtime::update_security_runtime(db, |db| {
        db.update_custom_security_rule_action(id, action)
    })
    .map_err(|e| e.to_string())?;
    if !updated {
        return Err("自定义规则不存在".to_string());
    }
    Ok(())
}

// Match DAO acceptance, including the legacy block-to-redact normalization.
fn validate_rule_action_update(action: &str) -> Result<(), String> {
    if matches!(
        action.to_ascii_lowercase().as_str(),
        "block" | "redact" | "audit" | "allow"
    ) {
        Ok(())
    } else {
        Err(
            crate::error::AppError::Database("无效的处理动作，仅支持 redact、audit、allow".into())
                .to_string(),
        )
    }
}

#[tauri::command]
pub async fn save_custom_security_rule(
    state: State<'_, AppState>,
    rule: CustomSecurityRule,
) -> Result<(), String> {
    save_custom_security_rule_db(&state.db, rule)
}

fn save_custom_security_rule_db(db: &Database, mut rule: CustomSecurityRule) -> Result<(), String> {
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
        regex::Regex::new(&rule.pattern).map_err(|_| "无效的正则表达式".to_string())?;
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

    crate::proxy::security::runtime::update_security_runtime(db, |db| {
        db.save_custom_security_rule(&rule)
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn toggle_custom_security_rule(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<bool, String> {
    toggle_custom_security_rule_db(&state.db, &id, enabled)
}

fn toggle_custom_security_rule_db(db: &Database, id: &str, enabled: bool) -> Result<bool, String> {
    crate::proxy::security::runtime::update_security_runtime(db, |db| {
        db.toggle_custom_security_rule(id, enabled)
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_custom_security_rule(
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    delete_custom_security_rule_db(&state.db, &id)
}

fn delete_custom_security_rule_db(db: &Database, id: &str) -> Result<bool, String> {
    crate::proxy::security::runtime::update_security_runtime(db, |db| {
        db.delete_custom_security_rule(id)
    })
    .map_err(|e| e.to_string())
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

#[cfg(test)]
mod rule_write_tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::proxy::security::{
        get_security_runtime, inspect_and_intercept_mut, runtime::set_security_runtime_for_test,
        types::AuditAction, SecurityRuntimeConfig,
    };
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        atomic::{AtomicU16, Ordering},
        Arc,
    };

    const CUSTOM_ID: &str = "synthetic-command-rule";
    const CUSTOM_TEXT: &str = "synthetic-command-policy-marker";
    const BUILTIN_ID: &str = "catalog:generic-sk-key-notice";
    const BUILTIN_TEXT: &str = "sk-syntheticcommandfixture";

    struct RestoreRuntime(SecurityRuntimeConfig);

    impl RestoreRuntime {
        fn capture() -> Self {
            Self((*get_security_runtime()).clone())
        }
    }

    impl Drop for RestoreRuntime {
        fn drop(&mut self) {
            set_security_runtime_for_test(self.0.clone());
        }
    }

    fn rule() -> CustomSecurityRule {
        CustomSecurityRule {
            id: CUSTOM_ID.into(),
            name: "Synthetic command rule".into(),
            rule_type: "keyword".into(),
            pattern: CUSTOM_TEXT.into(),
            risk_level: "HIGH".into(),
            action: "audit".into(),
            enabled: true,
            description: None,
            updated_at: 1,
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Write {
        BuiltinSet,
        BuiltinReset,
        CustomAction,
        Save,
        Toggle,
        Delete,
    }

    impl Write {
        fn is_builtin(self) -> bool {
            matches!(self, Self::BuiltinSet | Self::BuiltinReset)
        }

        fn fixture(self) -> Database {
            let db = Database::memory().unwrap();
            db.save_custom_security_rule(&rule()).unwrap();
            if matches!(self, Self::BuiltinReset) {
                db.update_builtin_security_rule_action(BUILTIN_ID, Some("audit"))
                    .unwrap();
            }
            crate::proxy::security::reload_security_runtime(&db).unwrap();
            db
        }

        fn apply(self, db: &Database) -> Result<(), String> {
            match self {
                Self::BuiltinSet => {
                    update_builtin_security_rule_action_db(db, BUILTIN_ID, Some("audit"))
                }
                Self::BuiltinReset => update_builtin_security_rule_action_db(db, BUILTIN_ID, None),
                Self::CustomAction => update_custom_security_rule_action_db(db, CUSTOM_ID, "allow"),
                Self::Save => {
                    let mut updated = rule();
                    updated.action = "redact".into();
                    save_custom_security_rule_db(db, updated)
                }
                Self::Toggle => {
                    toggle_custom_security_rule_db(db, CUSTOM_ID, false).map(|updated| {
                        assert!(updated);
                    })
                }
                Self::Delete => delete_custom_security_rule_db(db, CUSTOM_ID).map(|deleted| {
                    assert!(deleted);
                }),
            }
        }

        fn action(self, runtime: &SecurityRuntimeConfig) -> Option<AuditAction> {
            let (id, text) = if self.is_builtin() {
                (BUILTIN_ID, BUILTIN_TEXT)
            } else {
                (CUSTOM_ID, CUSTOM_TEXT)
            };
            SecurityAuditEngine::inspect_text_with_runtime(text, "synthetic_input", runtime)
                .into_iter()
                .find(|finding| finding.rule_id == id)
                .map(|finding| finding.action)
        }

        fn expected_action(self) -> Option<AuditAction> {
            match self {
                Self::BuiltinSet => Some(AuditAction::Audit),
                Self::BuiltinReset | Self::CustomAction => Some(AuditAction::Allow),
                Self::Save => Some(AuditAction::Redact),
                Self::Toggle | Self::Delete => None,
            }
        }

        fn assert_persisted(self, db: &Database) {
            if self.is_builtin() {
                let actions = db.get_builtin_security_rule_actions().unwrap();
                assert_eq!(
                    actions.get(BUILTIN_ID).map(String::as_str),
                    if matches!(self, Self::BuiltinSet) {
                        Some("audit")
                    } else {
                        None
                    }
                );
            } else {
                let rules = db.get_custom_security_rules().unwrap();
                let saved = rules.iter().find(|rule| rule.id == CUSTOM_ID);
                match self {
                    Self::CustomAction => assert_eq!(saved.unwrap().action, "allow"),
                    Self::Save => assert_eq!(saved.unwrap().action, "redact"),
                    Self::Toggle => assert!(!saved.unwrap().enabled),
                    Self::Delete => assert!(saved.is_none()),
                    _ => unreachable!(),
                }
            }
        }

        fn table_and_operation(self) -> (&'static str, &'static str) {
            match self {
                Self::BuiltinSet => ("security_settings", "INSERT"),
                Self::BuiltinReset => ("security_settings", "DELETE"),
                Self::CustomAction | Self::Toggle => ("security_custom_rules", "UPDATE"),
                Self::Save => ("security_custom_rules", "INSERT"),
                Self::Delete => ("security_custom_rules", "DELETE"),
            }
        }
    }

    fn http_status() -> u16 {
        let mut body = serde_json::json!({ "input": "synthetic harmless fixture" });
        inspect_and_intercept_mut(&AppType::Codex, &mut body)
            .unwrap()
            .map(|response| response.status().as_u16())
            .unwrap_or(200)
    }

    async fn assert_http_closed() {
        for _ in 0..2 {
            let mut body = serde_json::json!({ "input": "synthetic harmless fixture" });
            let response = inspect_and_intercept_mut(&AppType::Codex, &mut body)
                .unwrap()
                .expect("uncertain policy must reject HTTP admission");
            assert_eq!(
                response.status(),
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            );
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(error["error"]["code"], "dlp_unavailable");
            assert_eq!(error["error"]["message"], "dlp_unavailable");
        }
    }

    fn publishes_after_closing(write: Write) {
        let _restore = RestoreRuntime::capture();
        let db = write.fixture();
        let old = get_security_runtime();
        let old_action = write.action(&old);
        assert_ne!(old_action, write.expected_action());
        assert_eq!(http_status(), 200);

        // Observe HTTP admission while SQLite prepares the actual mutation.
        let observed = Arc::new(AtomicU16::new(0));
        let captured = observed.clone();
        let (target, _) = write.table_and_operation();
        db.extension_connection().lock().unwrap().authorizer(Some(
            move |context: AuthContext<'_>| {
                let table = match context.action {
                    AuthAction::Insert { table_name }
                    | AuthAction::Delete { table_name }
                    | AuthAction::Update { table_name, .. } => Some(table_name),
                    _ => None,
                };
                if table == Some(target) {
                    captured.store(http_status(), Ordering::SeqCst);
                }
                Authorization::Allow
            },
        ));
        write.apply(&db).unwrap();
        db.extension_connection()
            .lock()
            .unwrap()
            .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
        write.assert_persisted(&db);
        let current = get_security_runtime();
        assert_eq!(write.action(&current), write.expected_action());
        assert_eq!(
            write.action(&old),
            old_action,
            "captured request snapshot changed"
        );
        assert!(!Arc::ptr_eq(&old, &current));
        assert!(current.config_revision > old.config_revision);
        assert_eq!(http_status(), 200);
        assert_eq!(
            observed.load(Ordering::SeqCst),
            503,
            "{write:?} admitted HTTP before saving"
        );
    }

    async fn source_failure_stays_closed(write: Write) {
        let _restore = RestoreRuntime::capture();
        let db = write.fixture();
        let old = get_security_runtime();
        let old_action = write.action(&old);
        let missing_source = if write.is_builtin() {
            "security_custom_rules"
        } else {
            "security_settings"
        };
        db.extension_connection()
            .lock()
            .unwrap()
            .execute_batch(&format!("DROP TABLE {missing_source}"))
            .unwrap();
        let result = write.apply(&db);
        write.assert_persisted(&db);
        assert_eq!(result, Err("dlp_unavailable".into()), "{write:?}");
        assert_http_closed().await;
        assert_eq!(write.action(&old), old_action);
        assert!(Arc::ptr_eq(&old, &get_security_runtime()));
    }

    macro_rules! write_path_tests {
        ($success:ident, $failure:ident, $write:ident) => {
            #[test]
            #[serial_test::serial]
            fn $success() {
                publishes_after_closing(Write::$write);
            }

            #[tokio::test]
            #[serial_test::serial]
            async fn $failure() {
                source_failure_stays_closed(Write::$write).await;
            }
        };
    }

    write_path_tests!(
        builtin_set_closes_then_publishes,
        builtin_set_source_failure,
        BuiltinSet
    );
    write_path_tests!(
        builtin_reset_closes_then_publishes,
        builtin_reset_source_failure,
        BuiltinReset
    );
    write_path_tests!(
        custom_action_closes_then_publishes,
        custom_action_source_failure,
        CustomAction
    );
    write_path_tests!(save_closes_then_publishes, save_source_failure, Save);
    write_path_tests!(toggle_closes_then_publishes, toggle_source_failure, Toggle);
    write_path_tests!(delete_closes_then_publishes, delete_source_failure, Delete);

    #[tokio::test]
    #[serial_test::serial]
    async fn mutation_failures_are_sanitized_and_keep_http_closed() {
        let _restore = RestoreRuntime::capture();
        for write in [
            Write::BuiltinSet,
            Write::BuiltinReset,
            Write::CustomAction,
            Write::Save,
            Write::Toggle,
            Write::Delete,
        ] {
            let db = write.fixture();
            let (table, operation) = write.table_and_operation();
            db.extension_connection()
                .lock()
                .unwrap()
                .execute_batch(&format!(
                    "CREATE TRIGGER synthetic_failure BEFORE {operation} ON {table}
                 BEGIN SELECT RAISE(ABORT, 'synthetic private persistence detail'); END;"
                ))
                .unwrap();
            assert_eq!(write.apply(&db), Err("dlp_unavailable".into()), "{write:?}");
            assert_http_closed().await;
        }
    }

    #[test]
    #[serial_test::serial]
    fn form_validation_precedes_policy_transaction() {
        let _restore = RestoreRuntime::capture();
        let db = Write::Save.fixture();
        let old = get_security_runtime();
        let mut invalid = Vec::new();
        for field in ["name", "pattern", "type", "regex", "risk", "action"] {
            let mut value = rule();
            match field {
                "name" => value.name.clear(),
                "pattern" => value.pattern.clear(),
                "type" => value.rule_type = "unsupported".into(),
                "regex" => {
                    value.rule_type = "regex".into();
                    value.pattern = "[".into();
                }
                "risk" => value.risk_level = "unsupported".into(),
                "action" => value.action = "redact_restore".into(),
                _ => unreachable!(),
            }
            invalid.push(value);
        }
        for value in invalid {
            assert!(save_custom_security_rule_db(&db, value).is_err());
        }
        for (id, action) in [
            ("synthetic-unknown", Some("audit")),
            (BUILTIN_ID, Some("redact")),
            (BUILTIN_ID, Some("block")),
            ("chinese_phone_number", Some("redact_restore")),
        ] {
            assert!(update_builtin_security_rule_action_db(&db, id, action).is_err());
        }
        for action in ["unsupported", "redact_restore"] {
            assert!(update_custom_security_rule_action_db(&db, CUSTOM_ID, action).is_err());
        }
        assert_eq!(db.get_custom_security_rules().unwrap(), vec![rule()]);
        assert!(Arc::ptr_eq(&old, &get_security_runtime()));
        assert_eq!(http_status(), 200);
    }

    #[test]
    #[serial_test::serial]
    fn existing_action_ranges_normalization_and_defaults_are_preserved() {
        let _restore = RestoreRuntime::capture();
        let db = Write::Save.fixture();
        for action in ["BLOCK", "ReDaCt", "AUDIT", "allow"] {
            let mut value = rule();
            value.action = action.into();
            value.rule_type = "KEYWORD".into();
            value.risk_level = "high".into();
            value.updated_at = 0;
            save_custom_security_rule_db(&db, value).unwrap();
            let saved = db.get_custom_security_rules().unwrap().remove(0);
            let expected = if action == "BLOCK" {
                "redact".into()
            } else {
                action.to_ascii_lowercase()
            };
            assert_eq!(saved.action, expected);
            assert_eq!(saved.rule_type, "keyword");
            assert_eq!(saved.risk_level, "HIGH");
            assert!(saved.updated_at > 0);
            update_custom_security_rule_action_db(&db, CUSTOM_ID, action).unwrap();
            assert_eq!(db.get_custom_security_rules().unwrap()[0].action, expected);
            update_builtin_security_rule_action_db(&db, "chinese_phone_number", Some(action))
                .unwrap();
            assert_eq!(
                db.get_builtin_security_rule_actions().unwrap()["chinese_phone_number"],
                expected
            );
        }
        update_builtin_security_rule_action_db(&db, "chinese_phone_number", None).unwrap();
        assert!(!get_security_runtime()
            .builtin_action_overrides
            .contains_key("chinese_phone_number"));
        let mut value = rule();
        value.id.clear();
        save_custom_security_rule_db(&db, value).unwrap();
        assert_eq!(db.get_custom_security_rules().unwrap().len(), 2);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn missing_rules_publish_before_preserving_existing_results() {
        let _restore = RestoreRuntime::capture();
        let db = Write::Save.fixture();
        let before = get_security_runtime().config_revision;
        assert_eq!(
            update_custom_security_rule_action_db(&db, "synthetic-missing", "audit"),
            Err("自定义规则不存在".into())
        );
        assert!(get_security_runtime().config_revision > before);
        assert!(!toggle_custom_security_rule_db(&db, "synthetic-missing", false).unwrap());
        assert!(!delete_custom_security_rule_db(&db, "synthetic-missing").unwrap());
        assert_eq!(http_status(), 200);
        db.extension_connection()
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE security_settings")
            .unwrap();
        assert_eq!(
            update_custom_security_rule_action_db(&db, "synthetic-missing", "audit"),
            Err("dlp_unavailable".into())
        );
        assert_http_closed().await;
    }
}

#[cfg(test)]
mod regex_diagnostic_tests {
    use super::*;

    #[test]
    fn invalid_regex_validation_does_not_echo_private_rule_fields(
    ) -> Result<(), crate::error::AppError> {
        let db = Database::memory()?;
        let result = save_custom_security_rule_db(
            &db,
            CustomSecurityRule {
                id: "synthetic-regex-validation".into(),
                name: "SYNTHETIC_PRIVATE_RULE_NAME".into(),
                rule_type: "regex".into(),
                pattern: "SYNTHETIC_PRIVATE_PATTERN(".into(),
                risk_level: "HIGH".into(),
                action: "redact".into(),
                enabled: true,
                description: None,
                updated_at: 1,
            },
        );
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(
                !error.contains("SYNTHETIC_PRIVATE"),
                "validation must use a fixed error"
            );
            assert_eq!(error, "无效的正则表达式");
        }
        assert!(db.get_custom_security_rules()?.is_empty());
        Ok(())
    }
}
