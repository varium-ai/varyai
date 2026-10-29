use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::database::{lock_conn, Database};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogRecord {
    pub id: i64,
    pub timestamp: i64,
    pub trigger_count: i64,
    pub last_seen_at: i64,
    pub request_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub app_type: String,
    pub provider_id: String,
    pub rule_id: String,
    pub rule_name: String,
    pub rule_category: String,
    pub risk_level: String,
    pub action_taken: String,
    pub matched_snippet_masked: String,
    pub location: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogEvent {
    pub timestamp: i64,
    pub request_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub app_type: String,
    pub provider_id: String,
    pub rule_id: String,
    pub rule_name: String,
    pub rule_category: String,
    pub risk_level: String,
    pub action_taken: String,
    pub matched_snippet_masked: String,
    pub location: Option<String>,
}

/// Transaction-local row references; Earlier refers to a preceding write receipt.
#[derive(Debug, Clone, Copy)]
pub enum AuditRowTarget {
    New,
    Existing(i64),
    Earlier(usize),
}
#[derive(Clone)]
pub struct AuditWrite {
    pub target: AuditRowTarget,
    pub log: AuditLogEvent,
}
#[derive(Debug, Clone)]
pub struct AuditWriteReceipt {
    pub id: i64,
    pub trigger_count: i64,
    pub last_seen_at: i64,
}
#[derive(Debug)]
pub enum AuditBatchOutcome {
    Committed(Vec<AuditWriteReceipt>),
    NotCommitted,
    OutcomeUnknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogFilter {
    pub session_id: Option<String>,
    pub app_type: Option<String>,
    pub risk_level: Option<String>,
    pub rule_category: Option<String>,
    pub action: Option<String>,
    pub start_timestamp: Option<i64>,
    pub end_timestamp: Option<i64>,
    pub keyword: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogPageResult {
    pub items: Vec<AuditLogRecord>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomSecurityRule {
    pub id: String,
    pub name: String,
    pub rule_type: String, // "keyword" | "regex"
    pub pattern: String,
    pub risk_level: String, // "CRITICAL" | "HIGH" | "MEDIUM" | "LOW"
    pub action: String,     // "redact" | "audit" | "allow"; legacy "block" becomes "redact"
    pub enabled: bool,
    pub description: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditSettings {
    pub enabled: bool,
}

impl Default for SecurityAuditSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Rule configuration no longer exposes blocking. Normalize legacy/imported
/// actions at the persistence boundary without rewriting historical audit events.
fn normalize_security_rule_action(action: &str) -> String {
    let action = action.to_ascii_lowercase();
    if action == "block" {
        "redact".to_string()
    } else {
        action
    }
}

impl Database {
    pub fn insert_audit_logs_batch(&self, logs: &[AuditLogEvent]) -> Result<(), AppError> {
        if logs.is_empty() {
            return Ok(());
        }

        let mut conn = lock_conn!(self.extension_connection());
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("开启日志插入事务失败: {e}")))?;

        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT INTO security_audit_logs (
                    timestamp, request_id, app_type, provider_id,
                    rule_id, rule_name, rule_category, risk_level,
                    action_taken, matched_snippet_masked, location, last_seen_at, session_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?1, ?12)",
                )
                .map_err(|e| AppError::Database(e.to_string()))?;

            for log in logs {
                stmt.execute(params![
                    log.timestamp,
                    log.request_id,
                    log.app_type,
                    log.provider_id,
                    log.rule_id,
                    log.rule_name,
                    log.rule_category,
                    log.risk_level,
                    log.action_taken,
                    log.matched_snippet_masked,
                    log.location,
                    log.session_id,
                ])
                .map_err(|e| AppError::Database(format!("记录安全事件失败: {e}")))?;
            }
        }

        tx.commit()
            .map_err(|e| AppError::Database(format!("提交日志批量插入失败: {e}")))?;
        Ok(())
    }

    /// Background worker only. All effects are committed or explicitly classified.
    pub fn apply_audit_batch(&self, writes: &[AuditWrite]) -> AuditBatchOutcome {
        if writes.is_empty() {
            return AuditBatchOutcome::Committed(Vec::new());
        }
        let Ok(mut conn) = self.extension_connection().lock() else {
            return AuditBatchOutcome::OutcomeUnknown;
        };
        if !conn.is_autocommit() {
            return AuditBatchOutcome::OutcomeUnknown;
        }
        let tx = match conn.transaction() {
            Ok(tx) => tx,
            Err(_) => return AuditBatchOutcome::NotCommitted,
        };
        // RAII also rolls back if the background task unwinds during a write.
        let receipts = match apply_audit_writes(&tx, writes) {
            Ok(receipts) => receipts,
            Err(_) => {
                return if tx.rollback().is_ok() {
                    AuditBatchOutcome::NotCommitted
                } else {
                    AuditBatchOutcome::OutcomeUnknown
                }
            }
        };
        match tx.commit() {
            Ok(()) => AuditBatchOutcome::Committed(receipts),
            // A failed commit's drop may roll back, but does not return a rollback receipt.
            // Classify conservatively instead of inferring non-commit from autocommit.
            Err(_) => AuditBatchOutcome::OutcomeUnknown,
        }
    }

    pub fn get_security_audit_logs(
        &self,
        filter: &AuditLogFilter,
        page: u32,
        page_size: u32,
    ) -> Result<AuditLogPageResult, AppError> {
        let conn = lock_conn!(self.extension_connection());

        let mut where_clauses = Vec::new();
        let mut query_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref app) = filter.app_type {
            if !app.is_empty() {
                where_clauses.push("app_type = ?");
                query_params.push(Box::new(app.clone()));
            }
        }
        if let Some(session) = filter
            .session_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            where_clauses.push("session_id = ?");
            query_params.push(Box::new(session.to_string()));
        }
        if let Some(ref risk) = filter.risk_level {
            if !risk.is_empty() {
                where_clauses.push("risk_level = ?");
                query_params.push(Box::new(risk.clone()));
            }
        }
        if let Some(ref cat) = filter.rule_category {
            if !cat.is_empty() {
                where_clauses.push("rule_category = ?");
                query_params.push(Box::new(cat.clone()));
            }
        }
        if let Some(ref act) = filter.action {
            if !act.is_empty() {
                where_clauses.push("action_taken = ?");
                query_params.push(Box::new(act.clone()));
            }
        }
        if let Some(start) = filter.start_timestamp {
            where_clauses.push("timestamp >= ?");
            query_params.push(Box::new(start));
        }
        if let Some(end) = filter.end_timestamp {
            where_clauses.push("timestamp <= ?");
            query_params.push(Box::new(end));
        }
        if let Some(ref kw) = filter.keyword {
            if !kw.is_empty() {
                where_clauses.push("(rule_name LIKE ? OR matched_snippet_masked LIKE ?)");
                let pattern = format!("%{kw}%");
                query_params.push(Box::new(pattern.clone()));
                query_params.push(Box::new(pattern));
            }
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let count_sql = format!("SELECT COUNT(*) FROM security_audit_logs {where_sql}");
        let mut count_stmt = conn
            .prepare(&count_sql)
            .map_err(|e| AppError::Database(e.to_string()))?;

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            query_params.iter().map(|p| p.as_ref()).collect();
        let total: u64 = count_stmt
            .query_row(params_refs.as_slice(), |row| row.get(0))
            .unwrap_or(0);

        let limit = page_size.clamp(1, 100);
        let offset = page.saturating_sub(1).saturating_mul(limit);

        let data_sql = format!(
            "SELECT id, timestamp, request_id, app_type, provider_id,
                    rule_id, rule_name, rule_category, risk_level,
                    action_taken, matched_snippet_masked, location,
                    datetime(created_at, 'localtime'), trigger_count, COALESCE(last_seen_at, timestamp), session_id
             FROM security_audit_logs
             {where_sql}
             ORDER BY timestamp DESC
             LIMIT ? OFFSET ?"
        );

        let mut data_stmt = conn
            .prepare(&data_sql)
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut all_params_refs = params_refs;
        all_params_refs.push(&limit);
        all_params_refs.push(&offset);

        let items = data_stmt
            .query_map(all_params_refs.as_slice(), |row| {
                Ok(AuditLogRecord {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    request_id: row.get(2)?,
                    app_type: row.get(3)?,
                    provider_id: row.get(4)?,
                    rule_id: row.get(5)?,
                    rule_name: row.get(6)?,
                    rule_category: row.get(7)?,
                    risk_level: row.get(8)?,
                    action_taken: row.get(9)?,
                    matched_snippet_masked: row.get(10)?,
                    location: row.get(11)?,
                    created_at: row.get(12)?,
                    trigger_count: row.get(13)?,
                    last_seen_at: row.get(14)?,
                    session_id: row.get(15)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(AuditLogPageResult {
            items,
            total,
            page,
            page_size: limit,
        })
    }

    pub fn clear_security_audit_logs(
        &self,
        before_timestamp: Option<i64>,
    ) -> Result<u64, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let affected = match before_timestamp {
            Some(ts) => conn
                .execute(
                    "DELETE FROM security_audit_logs WHERE timestamp < ?",
                    params![ts],
                )
                .map_err(|e| AppError::Database(e.to_string()))?,
            None => conn
                .execute("DELETE FROM security_audit_logs", [])
                .map_err(|e| AppError::Database(e.to_string()))?,
        };
        Ok(affected as u64)
    }

    pub fn get_custom_security_rules(&self) -> Result<Vec<CustomSecurityRule>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let mut stmt = conn
            .prepare(
                "SELECT id, name, rule_type, pattern, risk_level, action, enabled, description, updated_at
                 FROM security_custom_rules
                 ORDER BY updated_at DESC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rules = stmt
            .query_map([], |row| {
                let enabled_int: i32 = row.get(6)?;
                Ok(CustomSecurityRule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    rule_type: row.get(2)?,
                    pattern: row.get(3)?,
                    risk_level: row.get(4)?,
                    action: normalize_security_rule_action(&row.get::<_, String>(5)?),
                    enabled: enabled_int != 0,
                    description: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(rules)
    }

    pub fn save_custom_security_rule(&self, rule: &CustomSecurityRule) -> Result<(), AppError> {
        let conn = lock_conn!(self.extension_connection());
        conn.execute(
            "INSERT OR REPLACE INTO security_custom_rules (
                id, name, rule_type, pattern, risk_level, action, enabled, description, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                rule.id,
                rule.name,
                rule.rule_type,
                rule.pattern,
                rule.risk_level,
                normalize_security_rule_action(&rule.action),
                if rule.enabled { 1 } else { 0 },
                rule.description,
                rule.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("保存自定义安全规则失败: {e}")))?;

        Ok(())
    }

    pub fn delete_custom_security_rule(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let affected = conn
            .execute(
                "DELETE FROM security_custom_rules WHERE id = ?",
                params![id],
            )
            .map_err(|e| AppError::Database(format!("删除自定义安全规则失败: {e}")))?;
        Ok(affected > 0)
    }

    pub fn toggle_custom_security_rule(&self, id: &str, enabled: bool) -> Result<bool, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let now = chrono::Utc::now().timestamp_millis();
        let affected = conn
            .execute(
                "UPDATE security_custom_rules SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![if enabled { 1 } else { 0 }, now, id],
            )
            .map_err(|e| AppError::Database(format!("更新自定义规则状态失败: {e}")))?;
        Ok(affected > 0)
    }

    pub fn update_custom_security_rule_action(
        &self,
        id: &str,
        action: &str,
    ) -> Result<bool, AppError> {
        let action = normalize_security_rule_action(action);
        if !matches!(action.as_str(), "audit" | "redact" | "allow") {
            return Err(AppError::Database(
                "无效的处理动作，仅支持 redact、audit、allow".to_string(),
            ));
        }

        let conn = lock_conn!(self.extension_connection());
        let now = chrono::Utc::now().timestamp_millis();
        let affected = conn
            .execute(
                "UPDATE security_custom_rules SET action = ?1, updated_at = ?2 WHERE id = ?3",
                params![action, now, id],
            )
            .map_err(|e| AppError::Database(format!("更新自定义规则动作失败: {e}")))?;
        Ok(affected > 0)
    }

    pub fn get_recent_high_risk_alerts(
        &self,
        since_timestamp: i64,
    ) -> Result<Vec<AuditLogRecord>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let mut stmt = conn
            .prepare(
                "SELECT id, timestamp, request_id, app_type, provider_id,
                        rule_id, rule_name, rule_category, risk_level,
                        action_taken, matched_snippet_masked, location,
                        datetime(created_at, 'localtime'), trigger_count, COALESCE(last_seen_at, timestamp), session_id
                 FROM security_audit_logs
                 WHERE timestamp > ? AND risk_level IN ('HIGH', 'CRITICAL')
                 ORDER BY timestamp DESC
                 LIMIT 10",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let items = stmt
            .query_map(params![since_timestamp], |row| {
                Ok(AuditLogRecord {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    request_id: row.get(2)?,
                    app_type: row.get(3)?,
                    provider_id: row.get(4)?,
                    rule_id: row.get(5)?,
                    rule_name: row.get(6)?,
                    rule_category: row.get(7)?,
                    risk_level: row.get(8)?,
                    action_taken: row.get(9)?,
                    matched_snippet_masked: row.get(10)?,
                    location: row.get(11)?,
                    created_at: row.get(12)?,
                    trigger_count: row.get(13)?,
                    last_seen_at: row.get(14)?,
                    session_id: row.get(15)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(items)
    }

    pub fn get_security_settings(&self) -> Result<SecurityAuditSettings, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let mut settings = SecurityAuditSettings::default();

        let mut stmt = conn
            .prepare("SELECT key, value FROM security_settings")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        for row in rows.flatten() {
            if row.0 == "enabled" {
                settings.enabled = row.1 == "true" || row.1 == "1";
            }
        }

        Ok(settings)
    }

    pub fn update_security_settings(
        &self,
        settings: &SecurityAuditSettings,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.extension_connection());
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let pairs = [("enabled", if settings.enabled { "true" } else { "false" })];

        for (k, v) in pairs {
            tx.execute(
                "INSERT OR REPLACE INTO security_settings (key, value) VALUES (?1, ?2)",
                params![k, v],
            )
            .map_err(|e| AppError::Database(format!("更新安全配置项 {k} 失败: {e}")))?;
        }

        tx.commit()
            .map_err(|e| AppError::Database(format!("提交安全配置更新失败: {e}")))?;
        Ok(())
    }
}

impl Database {
    pub fn get_builtin_security_rule_actions(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let mut stmt = conn
            .prepare("SELECT key, value FROM security_settings WHERE key LIKE 'builtin_action:%'")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut actions = std::collections::HashMap::new();
        for row in rows {
            let (key, action) = row.map_err(|e| AppError::Database(e.to_string()))?;
            let action = normalize_security_rule_action(&action);
            if let Some(id) = key.strip_prefix("builtin_action:") {
                if is_builtin_security_rule_id(id) && is_valid_builtin_action(&action) {
                    actions.insert(id.to_owned(), action);
                }
            }
        }
        Ok(actions)
    }

    pub fn update_builtin_security_rule_action(
        &self,
        id: &str,
        action: Option<&str>,
    ) -> Result<(), AppError> {
        if !is_builtin_security_rule_id(id) {
            return Err(AppError::Database("无效的内置规则 ID".to_string()));
        }
        let action = action.map(normalize_security_rule_action);
        if let Some(action) = action.as_deref() {
            if !is_valid_builtin_action(action) {
                return Err(AppError::Database(
                    "Invalid action; expected redact, audit or allow".into(),
                ));
            }
        }
        let key = format!("builtin_action:{id}");
        let conn = lock_conn!(self.extension_connection());
        if let Some(action) = action {
            conn.execute(
                "INSERT OR REPLACE INTO security_settings (key, value) VALUES (?1, ?2)",
                params![key, action],
            )
        } else {
            conn.execute("DELETE FROM security_settings WHERE key = ?1", params![key])
        }
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}

fn is_builtin_security_rule_id(id: &str) -> bool {
    matches!(id, "chinese_id_card" | "chinese_phone_number")
        || crate::proxy::security::builtin_rules::BUILTIN_RULES
            .iter()
            .any(|rule| rule.id == id)
}

fn is_valid_builtin_action(action: &str) -> bool {
    matches!(action, "audit" | "redact" | "allow")
}

#[cfg(test)]
mod home_stats_tests {
    use super::*;

    #[test]
    fn security_event_count_includes_all_actions_per_log_and_filters_app_and_time() {
        let db = Database::memory().unwrap();
        let event = |request: Option<&str>, app: &str, timestamp, action: &str| AuditLogEvent {
            timestamp,
            request_id: request.map(str::to_owned),
            session_id: None,
            app_type: app.into(),
            provider_id: "provider".into(),
            rule_id: "rule".into(),
            rule_name: "Rule".into(),
            rule_category: "test".into(),
            risk_level: "HIGH".into(),
            action_taken: action.into(),
            matched_snippet_masked: "***".into(),
            location: None,
        };
        db.insert_audit_logs_batch(&[
            event(Some("a"), "claude", 100, "block"),
            event(Some("a"), "claude", 101, "redact"),
            event(Some("a"), "claude", 150, "audit"),
            event(None, "claude", 200, "redact"),
            event(Some("c"), "codex", 150, "audit"),
            event(None, "claude", 99, "block"),
            event(None, "claude", 201, "block"),
        ])
        .unwrap();
        assert_eq!(db.get_security_event_count("claude", 100, 200).unwrap(), 4);
        assert_eq!(db.get_security_event_count("codex", 100, 200).unwrap(), 1);
        assert_eq!(db.get_security_event_count("gemini", 100, 200).unwrap(), 0);
    }
}

impl Database {
    /// Count committed audit events (one per matching log row), regardless of action.
    pub fn get_security_event_count(
        &self,
        app_type: &str,
        start: i64,
        end: i64,
    ) -> Result<u64, AppError> {
        let conn = lock_conn!(self.extension_connection());
        conn.query_row(
            "SELECT COUNT(*) FROM security_audit_logs WHERE app_type = ?1 AND timestamp >= ?2 AND timestamp <= ?3",
            params![app_type, start, end],
            |row| row.get(0),
        ).map_err(|e| AppError::Database(e.to_string()))
    }
}

impl Database {
    pub fn get_security_audit_notification_batch(
        &self,
        since_id: i64,
        mounted_at: i64,
        limit: u32,
    ) -> Result<Vec<AuditLogRecord>, AppError> {
        if since_id < 0 || mounted_at < 0 || !(1..=100).contains(&limit) {
            return Err(AppError::InvalidInput(
                "invalid notification query parameters".into(),
            ));
        }
        let conn = lock_conn!(self.extension_connection());
        let mut stmt = conn
            .prepare(
                "SELECT id, timestamp, request_id, app_type, provider_id,
                    rule_id, rule_name, rule_category, risk_level,
                    action_taken, matched_snippet_masked, location,
                    datetime(created_at, 'localtime'), trigger_count, COALESCE(last_seen_at, timestamp), session_id
             FROM security_audit_logs
             WHERE id > ?1 AND timestamp >= ?2
             ORDER BY id ASC LIMIT ?3",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![since_id, mounted_at, limit], |row| {
                Ok(AuditLogRecord {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    request_id: row.get(2)?,
                    app_type: row.get(3)?,
                    provider_id: row.get(4)?,
                    rule_id: row.get(5)?,
                    rule_name: row.get(6)?,
                    rule_category: row.get(7)?,
                    risk_level: row.get(8)?,
                    action_taken: row.get(9)?,
                    matched_snippet_masked: row.get(10)?,
                    location: row.get(11)?,
                    created_at: row.get(12)?,
                    trigger_count: row.get(13)?,
                    last_seen_at: row.get(14)?,
                    session_id: row.get(15)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }
}

#[cfg(test)]
mod notification_batch_tests {
    use super::*;

    #[test]
    fn cursor_pages_all_actions_same_millisecond_and_excludes_older_rows() {
        let db = Database::memory().unwrap();
        let event = |timestamp, index: usize| AuditLogEvent {
            timestamp,
            request_id: None,
            session_id: None,
            app_type: "claude".into(),
            provider_id: "provider".into(),
            rule_id: format!("rule-{index}"),
            rule_name: "Rule".into(),
            rule_category: "test".into(),
            risk_level: if index % 2 == 0 { "HIGH" } else { "LOW" }.into(),
            action_taken: if index % 2 == 0 { "block" } else { "audit" }.into(),
            matched_snippet_masked: "***".into(),
            location: None,
        };
        let mut events = vec![event(999, 0)];
        events.extend((1..=103).map(|i| event(1000, i)));
        db.insert_audit_logs_batch(&events).unwrap();
        let first = db
            .get_security_audit_notification_batch(0, 1000, 100)
            .unwrap();
        assert_eq!(first.len(), 100);
        assert_eq!(first[0].id, 2); // == mountedAt included
        assert_eq!(first.last().unwrap().id, 101);
        assert!(first
            .iter()
            .any(|row| row.risk_level == "LOW" && row.action_taken == "audit"));
        let second = db
            .get_security_audit_notification_batch(101, 1000, 100)
            .unwrap();
        assert_eq!(
            second.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![102, 103, 104]
        );
        assert!(db
            .get_security_audit_notification_batch(104, 1000, 100)
            .unwrap()
            .is_empty());
        db.insert_audit_logs_batch(&[event(1001, 104)]).unwrap();
        assert_eq!(
            db.get_security_audit_notification_batch(104, 1000, 100)
                .unwrap()[0]
                .id,
            105
        );
    }

    #[test]
    fn first_page_skips_representative_history_without_losing_recent_rows() {
        let db = Database::memory().unwrap();
        let mut conn = db.conn.lock().unwrap();
        let tx = conn.transaction().unwrap();
        {
            let mut insert = tx.prepare("INSERT INTO security_audit_logs (timestamp, app_type, provider_id, rule_id, rule_name, rule_category, risk_level, action_taken, matched_snippet_masked) VALUES (?1, 'claude', 'p', 'r', 'rule', 'test', 'LOW', 'audit', '***')").unwrap();
            for _ in 0..10_000 {
                insert.execute(params![1]).unwrap();
            }
            insert.execute(params![1000]).unwrap();
        }
        tx.commit().unwrap();
        drop(conn);
        let start = std::time::Instant::now();
        let rows = db
            .get_security_audit_notification_batch(0, 1000, 100)
            .unwrap();
        eprintln!(
            "security notification first-page 10k history: {:?}",
            start.elapsed()
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 10_001);
    }

    #[test]
    fn query_failure_is_not_an_empty_page_and_pk_cursor_uses_index() {
        let db = Database::memory().unwrap();
        let plan: String = db.conn.lock().unwrap().query_row(
            "EXPLAIN QUERY PLAN SELECT id FROM security_audit_logs WHERE id > ?1 AND timestamp >= ?2 ORDER BY id ASC LIMIT ?3",
            params![0, 1000, 100], |row| row.get(3),
        ).unwrap();
        assert!(plan.contains("INTEGER PRIMARY KEY"), "plan: {plan}");
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE security_audit_logs")
            .unwrap();
        assert!(db
            .get_security_audit_notification_batch(0, 1000, 100)
            .is_err());
    }
}

fn apply_audit_writes(
    conn: &rusqlite::Connection,
    writes: &[AuditWrite],
) -> rusqlite::Result<Vec<AuditWriteReceipt>> {
    let mut receipts: Vec<AuditWriteReceipt> = Vec::with_capacity(writes.len());
    for write in writes {
        let id = match write.target {
            AuditRowTarget::New => None,
            AuditRowTarget::Existing(id) => Some(id),
            AuditRowTarget::Earlier(index) => Some(
                receipts
                    .get(index)
                    .ok_or_else(|| {
                        rusqlite::Error::InvalidParameterName("audit row reference".into())
                    })?
                    .id,
            ),
        };
        let existing = if let Some(id) = id {
            conn.query_row("UPDATE security_audit_logs SET
                trigger_count = CASE WHEN trigger_count < 9223372036854775807 THEN trigger_count + 1 ELSE trigger_count END,
                last_seen_at = MAX(COALESCE(last_seen_at, timestamp), ?2)
                WHERE id = ?1 RETURNING trigger_count, last_seen_at", params![id, write.log.timestamp],
                |r| Ok(AuditWriteReceipt { id, trigger_count: r.get(0)?, last_seen_at: r.get(1)? })).optional()?
        } else {
            None
        };
        let receipt = if let Some(receipt) = existing {
            receipt
        } else {
            let log = &write.log;
            conn.execute("INSERT INTO security_audit_logs (timestamp, request_id, app_type, provider_id,
                rule_id, rule_name, rule_category, risk_level, action_taken, matched_snippet_masked, location, last_seen_at, session_id)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?1, ?12)", params![log.timestamp,
                log.request_id, log.app_type, log.provider_id, log.rule_id, log.rule_name, log.rule_category,
                log.risk_level, log.action_taken, log.matched_snippet_masked, log.location, log.session_id])?;
            AuditWriteReceipt {
                id: conn.last_insert_rowid(),
                trigger_count: 1,
                last_seen_at: log.timestamp,
            }
        };
        receipts.push(receipt);
    }
    Ok(receipts)
}
