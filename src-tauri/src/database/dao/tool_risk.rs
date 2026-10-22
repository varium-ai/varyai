use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::database::{lock_conn, to_json_string, Database};
use crate::error::AppError;
use crate::services::tool_risk::classifier::{CommandListEntry, MAX_COMMAND_LISTS};
use crate::services::tool_risk::engine::Verdict;
use crate::services::tool_risk::types::ToolRiskRule;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolRiskSettings {
    pub enabled: bool,
    pub policy_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRiskEventRecord {
    pub id: i64,
    pub timestamp: i64,
    pub source: String,
    pub stage: String,
    pub client_category: Option<String>,
    pub operation_category: Option<String>,

    pub command_summary: Option<String>,
    pub detection_scope: String,
    pub integrity_status: String,
    pub verdict: Option<String>,
    pub risk_category: Option<String>,
    pub severity: Option<String>,
    pub reason_code: Option<String>,
    pub policy_version: i64,
    pub rule_ids: Vec<String>,
    pub suppressed_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRiskEventInput {
    pub timestamp: i64,
    pub source: String,
    pub stage: String,
    pub client_category: Option<String>,
    pub operation_category: Option<String>,

    pub command_summary: Option<String>,
    pub detection_scope: String,
    pub integrity_status: String,
    pub verdict: Option<String>,
    pub risk_category: Option<String>,
    pub severity: Option<String>,
    pub reason_code: Option<String>,
    pub policy_version: i64,
    pub rule_ids: Vec<String>,
}

/// Actor identifies the local entrypoint, never an authenticated human identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskActor {
    LocalUi,
    System,
    Unknown,
}
impl ToolRiskActor {
    fn as_str(self) -> &'static str {
        match self {
            Self::LocalUi => "local_ui",
            Self::System => "system",
            Self::Unknown => "unknown",
        }
    }
    fn from_stored(value: &str) -> Self {
        match value {
            "local_ui" => Self::LocalUi,
            "system" => Self::System,
            _ => Self::Unknown,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskAuditStatus {
    Recorded,
    NotNeeded,
    AuditUnavailable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRiskWriteReceipt<T> {
    pub value: T,
    pub persisted_policy_version: u64,
    pub audit_status: ToolRiskAuditStatus,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskChangeKind {
    SettingsUpdate,
    RuleCreate,
    RuleUpdate,
    RuleDelete,
    BuiltinToggle,
    BuiltinReset,
    ListCreate,
    ListUpdate,
    ListDelete,
    Unknown,
}
impl ToolRiskChangeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SettingsUpdate => "settings_update",
            Self::RuleCreate => "rule_create",
            Self::RuleUpdate => "rule_update",
            Self::RuleDelete => "rule_delete",
            Self::BuiltinToggle => "builtin_toggle",
            Self::BuiltinReset => "builtin_reset",
            Self::ListCreate => "list_create",
            Self::ListUpdate => "list_update",
            Self::ListDelete => "list_delete",
            Self::Unknown => "unknown",
        }
    }
    fn from_stored(value: &str) -> Self {
        match value {
            "settings_update" => Self::SettingsUpdate,
            "rule_create" => Self::RuleCreate,
            "rule_update" => Self::RuleUpdate,
            "rule_delete" => Self::RuleDelete,
            "builtin_toggle" => Self::BuiltinToggle,
            "builtin_reset" => Self::BuiltinReset,
            "list_create" => Self::ListCreate,
            "list_update" => Self::ListUpdate,
            "list_delete" => Self::ListDelete,
            _ => Self::Unknown,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskStoredAuditStatus {
    Recorded,
    AuditUnavailable,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRiskPolicyChange {
    pub id: i64,
    pub at: i64,
    pub policy_version: u64,
    pub actor: ToolRiskActor,
    pub kind: ToolRiskChangeKind,
    pub target_id: Option<String>,
    pub changed_fields: Vec<String>,
    pub audit_status: ToolRiskStoredAuditStatus,
}
const POLICY_FIELDS: &[&str] = &[
    "enabled",
    "name",
    "description",
    "selector",
    "conditions",
    "risk_category",
    "severity",
    "reason_code",
    "local_review",
    "list_type",
    "match_kind",
    "pattern",
];
const RULE_FIELDS: &[&str] = &[
    "enabled",
    "name",
    "description",
    "selector",
    "conditions",
    "risk_category",
    "severity",
    "reason_code",
    "local_review",
];
const LIST_FIELDS: &[&str] = &[
    "enabled",
    "risk_category",
    "severity",
    "list_type",
    "match_kind",
    "pattern",
];
const POLICY_CHANGE_RETENTION: i64 = 1000;

fn safe_policy_target(id: &str) -> String {
    if crate::services::tool_risk::builtin::builtin_rules()
        .iter()
        .any(|rule| rule.id == id)
    {
        return id.to_owned();
    }
    for prefix in ["custom-", "shell-list-"] {
        if let Some(value) = id.strip_prefix(prefix) {
            if uuid::Uuid::parse_str(value).is_ok_and(|uuid| uuid.to_string() == value) {
                return id.to_owned();
            }
        }
    }
    "<unknown_target>".into()
}
fn rule_changes(before: Option<&ToolRiskRule>, after: Option<&ToolRiskRule>) -> Vec<&'static str> {
    let (Some(before), Some(after)) = (before, after) else {
        return RULE_FIELDS.to_vec();
    };
    let changes = [
        ("enabled", before.enabled != after.enabled),
        ("name", before.name != after.name),
        ("description", before.description != after.description),
        ("selector", before.selector != after.selector),
        ("conditions", before.conditions != after.conditions),
        ("risk_category", before.risk_category != after.risk_category),
        ("severity", before.severity != after.severity),
        ("reason_code", before.reason_code != after.reason_code),
        ("local_review", before.local_review != after.local_review),
    ];
    changes
        .into_iter()
        .filter_map(|(field, changed)| changed.then_some(field))
        .collect()
}
fn list_changes(
    before: Option<&CommandListEntry>,
    after: Option<&CommandListEntry>,
) -> Vec<&'static str> {
    let (Some(before), Some(after)) = (before, after) else {
        return LIST_FIELDS.to_vec();
    };
    let changes = [
        ("enabled", before.enabled != after.enabled),
        ("risk_category", before.risk_category != after.risk_category),
        ("severity", before.severity != after.severity),
        ("list_type", before.list_type != after.list_type),
        ("match_kind", before.match_kind != after.match_kind),
        ("pattern", before.pattern != after.pattern),
    ];
    changes
        .into_iter()
        .filter_map(|(field, changed)| changed.then_some(field))
        .collect()
}

fn append_policy_change(
    conn: &rusqlite::Connection,
    version: u64,
    actor: ToolRiskActor,
    kind: ToolRiskChangeKind,
    target: Option<&str>,
    fields: &[&str],
) -> Result<ToolRiskAuditStatus, AppError> {
    if fields.is_empty() {
        return Ok(ToolRiskAuditStatus::NotNeeded);
    }
    // Keep the audit side effect within a recoverable part of the config transaction.
    if conn
        .execute_batch("SAVEPOINT tool_risk_policy_audit;")
        .is_err()
    {
        if conn.is_autocommit() {
            return Err(AppError::Database("工具风险配置事务已失效".into()));
        }
        log::warn!("[ToolRisk] audit_unavailable");
        return Ok(ToolRiskAuditStatus::AuditUnavailable);
    }
    let result = (|| -> Result<(), rusqlite::Error> {
        // All inputs are fixed metadata; no user-supplied content enters this JSON.
        let encoded = serde_json::to_string(fields).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let inserted = conn.execute("INSERT INTO tool_risk_policy_changes (at, policy_version, actor, kind, target_id, changed_fields, audit_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'recorded')",
            params![chrono::Utc::now().timestamp_millis(), version as i64, actor.as_str(), kind.as_str(), target.map(safe_policy_target), encoded])?;
        if inserted != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        conn.execute("DELETE FROM tool_risk_policy_changes WHERE id <= (SELECT id FROM tool_risk_policy_changes ORDER BY id DESC LIMIT 1 OFFSET ?1)",params![POLICY_CHANGE_RETENTION])?;
        let retained: i64 =
            conn.query_row("SELECT COUNT(*) FROM tool_risk_policy_changes", [], |row| {
                row.get(0)
            })?;
        if retained > POLICY_CHANGE_RETENTION {
            return Err(rusqlite::Error::InvalidQuery);
        }
        conn.execute_batch("RELEASE tool_risk_policy_audit;")?;
        Ok(())
    })();
    if result.is_err() {
        if conn.is_autocommit()
            || conn
                .execute_batch(
                    "ROLLBACK TO tool_risk_policy_audit; RELEASE tool_risk_policy_audit;",
                )
                .is_err()
        {
            return Err(AppError::Database("工具风险配置事务无法安全恢复".into()));
        }
        log::warn!("[ToolRisk] audit_unavailable");
        return Ok(ToolRiskAuditStatus::AuditUnavailable);
    }
    Ok(ToolRiskAuditStatus::Recorded)
}
fn finish_policy_write<T>(
    tx: rusqlite::Transaction<'_>,
    value: T,
    actor: ToolRiskActor,
    kind: ToolRiskChangeKind,
    target: Option<&str>,
    fields: &[&str],
) -> Result<ToolRiskWriteReceipt<T>, AppError> {
    bump_policy_version(&tx)?;
    let persisted_policy_version = read_settings(&tx)?.policy_version;
    let audit_status =
        append_policy_change(&tx, persisted_policy_version, actor, kind, target, fields)?;
    tx.commit().map_err(db_error)?;
    Ok(ToolRiskWriteReceipt {
        value,
        persisted_policy_version,
        audit_status,
    })
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
}

fn require_config_rows(affected: usize, expected: usize) -> Result<(), AppError> {
    if affected != expected {
        return Err(AppError::Database("工具风险配置未完整保存".into()));
    }
    Ok(())
}

fn read_settings(conn: &rusqlite::Connection) -> Result<ToolRiskSettings, AppError> {
    let mut stmt = conn
        .prepare("SELECT key, value FROM tool_risk_settings")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut settings = ToolRiskSettings::default();
    for row in rows {
        let (key, value) = row.map_err(db_error)?;
        match key.as_str() {
            "enabled" => {
                settings.enabled = match value.as_str() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => return Err(AppError::Database("工具风险开关无效".into())),
                }
            }
            "policy_version" => {
                settings.policy_version = value
                    .parse::<u64>()
                    .ok()
                    .filter(|version| *version <= i64::MAX as u64)
                    .ok_or_else(|| AppError::Database("工具风险策略版本无效".into()))?;
            }
            _ => {}
        }
    }
    Ok(settings)
}

fn bump_policy_version(conn: &rusqlite::Connection) -> Result<(), AppError> {
    let version = read_settings(conn)?
        .policy_version
        .checked_add(1)
        .filter(|version| *version <= i64::MAX as u64)
        .ok_or_else(|| AppError::Database("工具风险策略版本已耗尽".into()))?;
    let affected = conn
        .execute(
            "INSERT OR REPLACE INTO tool_risk_settings (key, value) VALUES ('policy_version', ?1)",
            params![version.to_string()],
        )
        .map_err(db_error)?;
    require_config_rows(affected, 1)
}

fn decode_custom_rule(definition: &str) -> Result<ToolRiskRule, AppError> {
    let rule: ToolRiskRule = serde_json::from_str(definition)
        .map_err(|_| AppError::Database("工具风险规则定义损坏".into()))?;
    if rule.origin != crate::services::tool_risk::types::RuleOrigin::Custom
        || rule.validate().is_err()
    {
        return Err(AppError::Database("工具风险规则定义无效".into()));
    }
    Ok(rule)
}

fn read_custom_rules(conn: &rusqlite::Connection) -> Result<Vec<ToolRiskRule>, AppError> {
    let mut stmt = conn
        .prepare("SELECT definition FROM tool_risk_custom_rules ORDER BY updated_at DESC")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db_error)?;
    rows.map(|row| decode_custom_rule(&row.map_err(db_error)?))
        .collect()
}

fn decode_command_list(id: &str, definition: &str) -> Result<CommandListEntry, AppError> {
    let entry: CommandListEntry = serde_json::from_str(definition)
        .map_err(|_| AppError::Database("工具风险命令名单定义损坏".into()))?;
    if entry.id != id || entry.validate().is_err() {
        return Err(AppError::Database("工具风险命令名单定义无效".into()));
    }
    Ok(entry)
}

fn read_command_lists(conn: &rusqlite::Connection) -> Result<Vec<CommandListEntry>, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, definition FROM tool_risk_command_lists ORDER BY updated_at DESC, id ASC LIMIT ?1")
        .map_err(db_error)?;
    let rows = stmt
        .query_map(params![(MAX_COMMAND_LISTS + 1) as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut entries = Vec::new();
    for row in rows {
        let (id, definition) = row.map_err(db_error)?;
        if entries.len() >= MAX_COMMAND_LISTS {
            return Err(AppError::Database("工具风险命令名单超过数量上限".into()));
        }
        let entry = decode_command_list(&id, &definition)?;
        if entries.iter().any(|existing: &CommandListEntry| {
            existing.list_type == entry.list_type
                && existing.match_kind == entry.match_kind
                && existing.pattern == entry.pattern
        }) {
            return Err(AppError::Database("同一名单中存在重复命令模式".into()));
        }
        entries.push(entry);
    }
    Ok(entries)
}

fn read_builtin_overrides(
    conn: &rusqlite::Connection,
) -> Result<std::collections::HashMap<String, bool>, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, enabled FROM tool_risk_builtin_overrides")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)? != 0))
        })
        .map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

fn require_builtin(id: &str) -> Result<(), AppError> {
    if crate::services::tool_risk::builtin::builtin_rules()
        .iter()
        .any(|rule| rule.id == id)
    {
        Ok(())
    } else {
        Err(AppError::Database("内置工具风险规则不存在".into()))
    }
}

fn builtin_revision(conn: &rusqlite::Connection, id: &str) -> Result<u32, AppError> {
    use rusqlite::OptionalExtension;
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM tool_risk_settings WHERE key = ?1",
            params![format!("builtin_revision:{id}")],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    match value {
        None => Ok(1),
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|revision| *revision > 0)
            .ok_or_else(|| AppError::Database("内置规则版本无效".into())),
    }
}

fn bump_builtin_revision(
    conn: &rusqlite::Connection,
    id: &str,
    expected: Option<u32>,
) -> Result<(), AppError> {
    let current = builtin_revision(conn, id)?;
    if expected.is_some_and(|expected| expected != current) {
        return Err(AppError::Database("规则版本冲突，请刷新后重试".into()));
    }
    let next = current
        .checked_add(1)
        .ok_or_else(|| AppError::Database("规则版本已耗尽".into()))?;
    let affected = conn
        .execute(
            "INSERT OR REPLACE INTO tool_risk_settings (key, value) VALUES (?1, ?2)",
            params![format!("builtin_revision:{id}"), next.to_string()],
        )
        .map_err(db_error)?;
    require_config_rows(affected, 1)
}

fn privacy_redacted_summary(summary: Option<&str>) -> Option<String> {
    summary
        .map(crate::services::tool_risk::sanitize::summarize_command)
        .filter(|value| !value.is_empty())
}

// Configuration reload/import is a background-only repair boundary. Process bounded
// pages, not all summaries at once; ordinary values are retained and repeat scrubs are idempotent.
fn sanitize_stored_command_summaries(conn: &rusqlite::Connection) -> Result<(), AppError> {
    let mut after_id = 0_i64;
    loop {
        let summaries = {
            let mut stmt = conn.prepare("SELECT id, command_summary FROM tool_risk_events WHERE id > ?1 AND command_summary IS NOT NULL ORDER BY id LIMIT 256").map_err(db_error)?;
            let rows = stmt
                .query_map([after_id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(db_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
        };
        if summaries.is_empty() {
            break;
        }
        for (id, original) in summaries {
            let sanitized = privacy_redacted_summary(Some(&original));
            if sanitized.as_deref() != Some(original.as_str()) {
                conn.execute(
                    "UPDATE tool_risk_events SET command_summary = ?1 WHERE id = ?2",
                    params![sanitized, id],
                )
                .map_err(db_error)?;
            }
            after_id = id;
        }
    }
    Ok(())
}

fn insert_events<'a>(
    conn: &rusqlite::Connection,
    events: impl Iterator<Item = &'a ToolRiskEventInput>,
) -> Result<usize, AppError> {
    let mut stmt = conn
        .prepare(
            "INSERT INTO tool_risk_events (
            timestamp, source, stage, client_category, operation_category, command_summary,
            detection_scope, integrity_status, verdict, risk_category, severity, reason_code,
            policy_version, rule_ids, suppressed_count
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0)",
        )
        .map_err(db_error)?;
    let mut inserted = 0;
    for e in events {
        inserted += stmt
            .execute(params![
                e.timestamp,
                e.source,
                e.stage,
                e.client_category,
                e.operation_category,
                privacy_redacted_summary(e.command_summary.as_deref()),
                e.detection_scope,
                e.integrity_status,
                e.verdict,
                e.risk_category,
                e.severity,
                e.reason_code,
                e.policy_version,
                to_json_string(&e.rule_ids)?,
            ])
            .map_err(db_error)?;
    }
    Ok(inserted)
}

fn read_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolRiskEventRecord> {
    let rule_ids: Option<String> = row.get(14)?;
    Ok(ToolRiskEventRecord {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        source: row.get(2)?,
        stage: row.get(3)?,
        client_category: row.get(4)?,
        operation_category: row.get(5)?,
        // Reapply the privacy boundary even for legacy/imported database rows.
        command_summary: privacy_redacted_summary(row.get::<_, Option<String>>(6)?.as_deref()),
        detection_scope: row.get(7)?,
        integrity_status: row.get(8)?,
        verdict: row.get(9)?,
        risk_category: row.get(10)?,
        severity: row.get(11)?,
        reason_code: row.get(12)?,
        policy_version: row.get(13)?,
        rule_ids: rule_ids
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default(),
        suppressed_count: row.get(15)?,
    })
}

const EVENT_COLUMNS: &str = "id, timestamp, source, stage, client_category, operation_category,
    command_summary, detection_scope, integrity_status, verdict, risk_category, severity,
    reason_code, policy_version, rule_ids, suppressed_count";

impl Database {
    pub fn get_tool_risk_policy_changes(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ToolRiskPolicyChange>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = lock_conn!(self.extension_connection());
        let mut statement = conn.prepare("SELECT id, at, policy_version, substr(actor,1,32), substr(kind,1,32), substr(target_id,1,129), substr(changed_fields,1,4097), substr(audit_status,1,32) FROM tool_risk_policy_changes ORDER BY id DESC LIMIT ?1 OFFSET ?2").map_err(db_error)?;
        let rows = statement
            .query_map(
                params![limit.min(POLICY_CHANGE_RETENTION as u32), offset],
                |row| {
                    let raw_fields: String = row.get(6)?;
                    let stored_fields: Vec<String> = if raw_fields.len() <= 4096 {
                        serde_json::from_str(&raw_fields).unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let target: Option<String> = row.get(5)?;
                    let status: String = row.get(7)?;
                    Ok(ToolRiskPolicyChange {
                        id: row.get(0)?,
                        at: row.get(1)?,
                        policy_version: row.get::<_, i64>(2)?.max(0) as u64,
                        actor: ToolRiskActor::from_stored(&row.get::<_, String>(3)?),
                        kind: ToolRiskChangeKind::from_stored(&row.get::<_, String>(4)?),
                        target_id: target.as_deref().map(safe_policy_target),
                        changed_fields: POLICY_FIELDS
                            .iter()
                            .filter(|field| stored_fields.iter().any(|stored| stored == **field))
                            .map(|field| (*field).to_owned())
                            .collect(),
                        audit_status: match status.as_str() {
                            "recorded" => ToolRiskStoredAuditStatus::Recorded,
                            "audit_unavailable" => ToolRiskStoredAuditStatus::AuditUnavailable,
                            _ => ToolRiskStoredAuditStatus::Unknown,
                        },
                    })
                },
            )
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub fn clear_tool_risk_policy_changes(&self) -> Result<usize, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let expected: usize = tx
            .query_row("SELECT COUNT(*) FROM tool_risk_policy_changes", [], |row| {
                row.get(0)
            })
            .map_err(db_error)?;
        let affected = tx
            .execute("DELETE FROM tool_risk_policy_changes", [])
            .map_err(db_error)?;
        if affected != expected {
            return Err(AppError::Database("工具风险变更记录未完整清空".into()));
        }
        tx.commit().map_err(db_error)?;
        Ok(affected)
    }
    pub fn get_tool_risk_settings(&self) -> Result<ToolRiskSettings, AppError> {
        let conn = lock_conn!(self.extension_connection());
        read_settings(&conn)
    }

    pub fn save_tool_risk_settings(&self, settings: &ToolRiskSettings) -> Result<(), AppError> {
        self.save_tool_risk_settings_with_actor(settings, ToolRiskActor::Unknown)
            .map(|receipt| receipt.value)
    }
    pub fn save_tool_risk_settings_with_actor(
        &self,
        settings: &ToolRiskSettings,
        actor: ToolRiskActor,
    ) -> Result<ToolRiskWriteReceipt<()>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let before = read_settings(&tx)?;
        let affected = tx
            .execute(
                "INSERT OR REPLACE INTO tool_risk_settings (key, value) VALUES ('enabled', ?1)",
                params![if settings.enabled { "true" } else { "false" }],
            )
            .map_err(db_error)?;
        require_config_rows(affected, 1)?;
        let fields: &[&str] = if before.enabled != settings.enabled {
            &["enabled"]
        } else {
            &[]
        };
        finish_policy_write(
            tx,
            (),
            actor,
            ToolRiskChangeKind::SettingsUpdate,
            None,
            fields,
        )
    }

    pub fn get_tool_risk_configuration(
        &self,
    ) -> Result<(ToolRiskSettings, Vec<ToolRiskRule>), AppError> {
        self.get_tool_risk_configuration_snapshot()
            .map(|(settings, rules, _)| (settings, rules))
    }

    pub fn get_tool_risk_configuration_snapshot(
        &self,
    ) -> Result<(ToolRiskSettings, Vec<ToolRiskRule>, Vec<CommandListEntry>), AppError> {
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        sanitize_stored_command_summaries(&tx)?;
        let settings = read_settings(&tx)?;
        let overrides = read_builtin_overrides(&tx)?;
        let mut rules = crate::services::tool_risk::builtin::builtin_rules();
        for rule in &mut rules {
            if let Some(enabled) = overrides.get(&rule.id) {
                rule.enabled = *enabled;
            }
            rule.revision = builtin_revision(&tx, &rule.id)?;
        }
        rules.extend(read_custom_rules(&tx)?);
        let command_lists = read_command_lists(&tx)?;
        tx.commit().map_err(db_error)?;
        Ok((settings, rules, command_lists))
    }

    pub fn get_tool_risk_command_lists(&self) -> Result<Vec<CommandListEntry>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        read_command_lists(&conn)
    }

    pub fn save_tool_risk_command_list(
        &self,
        entry: &CommandListEntry,
        expected_revision: Option<u32>,
    ) -> Result<(), AppError> {
        self.save_tool_risk_command_list_with_actor(
            entry,
            expected_revision,
            ToolRiskActor::Unknown,
        )
        .map(|receipt| receipt.value)
    }
    pub fn save_tool_risk_command_list_with_actor(
        &self,
        entry: &CommandListEntry,
        expected_revision: Option<u32>,
        actor: ToolRiskActor,
    ) -> Result<ToolRiskWriteReceipt<()>, AppError> {
        entry
            .validate()
            .map_err(|_| AppError::Database("工具风险命令名单定义无效".into()))?;
        let definition = to_json_string(entry)?;
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let entries = read_command_lists(&tx)?;
        let current = entries.iter().find(|current| current.id == entry.id);
        match (expected_revision, current) {
            (None, None) if entry.revision == 1 => {
                if entries.len() >= MAX_COMMAND_LISTS {
                    return Err(AppError::Database("工具风险命令名单超过数量上限".into()));
                }
            }
            (Some(expected), Some(current))
                if current.revision == expected
                    && expected.checked_add(1) == Some(entry.revision) => {}
            _ => {
                return Err(AppError::Database(
                    "名单版本冲突或不存在，请刷新后重试".into(),
                ))
            }
        }
        if entries.iter().any(|current| {
            current.id != entry.id
                && current.list_type == entry.list_type
                && current.match_kind == entry.match_kind
                && current.pattern == entry.pattern
        }) {
            return Err(AppError::Database("同一名单中存在重复命令模式".into()));
        }
        let affected = tx.execute(
            "INSERT OR REPLACE INTO tool_risk_command_lists (id, definition, updated_at) VALUES (?1, ?2, ?3)",
            params![entry.id, definition, chrono::Utc::now().timestamp_millis()],
        ).map_err(db_error)?;
        require_config_rows(affected, 1)?;
        let fields = list_changes(current, Some(entry));
        let kind = if current.is_some() {
            ToolRiskChangeKind::ListUpdate
        } else {
            ToolRiskChangeKind::ListCreate
        };
        finish_policy_write(tx, (), actor, kind, Some(&entry.id), &fields)
    }

    pub fn delete_tool_risk_command_list(
        &self,
        id: &str,
        expected_revision: u32,
    ) -> Result<bool, AppError> {
        self.delete_tool_risk_command_list_with_actor(id, expected_revision, ToolRiskActor::Unknown)
            .map(|receipt| receipt.value)
    }
    pub fn delete_tool_risk_command_list_with_actor(
        &self,
        id: &str,
        expected_revision: u32,
        actor: ToolRiskActor,
    ) -> Result<ToolRiskWriteReceipt<bool>, AppError> {
        use rusqlite::OptionalExtension;
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let definition: Option<String> = tx
            .query_row(
                "SELECT definition FROM tool_risk_command_lists WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let current = definition
            .as_deref()
            .map(|definition| decode_command_list(id, definition))
            .transpose()?;
        if !current.is_some_and(|current| current.revision == expected_revision) {
            return Err(AppError::Database(
                "名单版本冲突或不存在，请刷新后重试".into(),
            ));
        }
        let affected = tx
            .execute(
                "DELETE FROM tool_risk_command_lists WHERE id = ?1",
                params![id],
            )
            .map_err(db_error)?;
        require_config_rows(affected, 1)?;
        finish_policy_write(
            tx,
            true,
            actor,
            ToolRiskChangeKind::ListDelete,
            Some(id),
            LIST_FIELDS,
        )
    }

    pub fn get_tool_risk_command_summary_preview(&self) -> Result<Vec<(i64, String)>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let mut stmt = conn
            .prepare(
                "SELECT id, command_summary FROM tool_risk_events WHERE command_summary IS NOT NULL
                AND TRIM(command_summary) != '' ORDER BY timestamp DESC, id DESC LIMIT 20",
            )
            .map_err(db_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_error)?;
        let mut summaries = Vec::new();
        for row in rows {
            let (id, summary) = row.map_err(db_error)?;
            if let Some(summary) = privacy_redacted_summary(Some(&summary)) {
                summaries.push((id, summary));
            }
        }
        Ok(summaries)
    }

    pub fn get_custom_tool_risk_rules(&self) -> Result<Vec<ToolRiskRule>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        read_custom_rules(&conn)
    }

    pub fn get_custom_tool_risk_rule(&self, id: &str) -> Result<Option<ToolRiskRule>, AppError> {
        use rusqlite::OptionalExtension;
        let conn = lock_conn!(self.extension_connection());
        let definition: Option<String> = conn
            .query_row(
                "SELECT definition FROM tool_risk_custom_rules WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        definition.as_deref().map(decode_custom_rule).transpose()
    }

    pub fn save_custom_tool_risk_rule(
        &self,
        rule: &ToolRiskRule,
        expected_revision: Option<u32>,
    ) -> Result<(), AppError> {
        self.save_custom_tool_risk_rule_with_actor(rule, expected_revision, ToolRiskActor::Unknown)
            .map(|receipt| receipt.value)
    }
    pub fn save_custom_tool_risk_rule_with_actor(
        &self,
        rule: &ToolRiskRule,
        expected_revision: Option<u32>,
        actor: ToolRiskActor,
    ) -> Result<ToolRiskWriteReceipt<()>, AppError> {
        use rusqlite::OptionalExtension;
        if rule.origin != crate::services::tool_risk::types::RuleOrigin::Custom
            || rule.validate().is_err()
        {
            return Err(AppError::Database("工具风险规则定义无效".into()));
        }
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT revision FROM tool_risk_custom_rules WHERE id = ?1",
                params![rule.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        match (expected_revision, current) {
            (None, None) if rule.revision == 1 => {}
            (Some(expected), Some(current))
                if current == i64::from(expected)
                    && expected.checked_add(1) == Some(rule.revision) => {}
            _ => {
                return Err(AppError::Database(
                    "规则版本冲突或不存在，请刷新后重试".into(),
                ))
            }
        }
        let previous_definition: Option<String> = tx
            .query_row(
                "SELECT definition FROM tool_risk_custom_rules WHERE id = ?1",
                params![rule.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let previous = previous_definition
            .as_deref()
            .map(decode_custom_rule)
            .transpose()?;
        if let Err(code) = rule.validate_custom_policy() {
            // An existing conflicting definition may only be disabled unchanged, not
            // created, edited or re-enabled. This preserves the cleanup controls.
            let disabling_existing = if !rule.enabled && current.is_some() {
                let definition: String = tx
                    .query_row(
                        "SELECT definition FROM tool_risk_custom_rules WHERE id = ?1",
                        params![rule.id],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                let mut previous = decode_custom_rule(&definition)?;
                previous.revision = rule.revision;
                previous.enabled = false;
                previous == *rule
            } else {
                false
            };
            if !disabling_existing {
                return Err(AppError::Database(code.into()));
            }
        }
        let affected = tx.execute(
            "INSERT OR REPLACE INTO tool_risk_custom_rules (id, revision, name, definition, enabled, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![rule.id, i64::from(rule.revision), rule.name, to_json_string(rule)?, rule.enabled, chrono::Utc::now().timestamp_millis()],
        ).map_err(db_error)?;
        require_config_rows(affected, 1)?;
        let fields = rule_changes(previous.as_ref(), Some(rule));
        let kind = if previous.is_some() {
            ToolRiskChangeKind::RuleUpdate
        } else {
            ToolRiskChangeKind::RuleCreate
        };
        finish_policy_write(tx, (), actor, kind, Some(&rule.id), &fields)
    }

    #[cfg(test)]
    pub fn delete_custom_tool_risk_rule(&self, id: &str) -> Result<bool, AppError> {
        self.delete_custom_tool_risk_rule_checked(id, None, ToolRiskActor::Unknown)
            .map(|receipt| receipt.value)
    }

    pub fn delete_custom_tool_risk_rule_at_revision(
        &self,
        id: &str,
        expected_revision: u32,
    ) -> Result<bool, AppError> {
        self.delete_custom_tool_risk_rule_with_actor(id, expected_revision, ToolRiskActor::Unknown)
            .map(|receipt| receipt.value)
    }
    pub fn delete_custom_tool_risk_rule_with_actor(
        &self,
        id: &str,
        expected_revision: u32,
        actor: ToolRiskActor,
    ) -> Result<ToolRiskWriteReceipt<bool>, AppError> {
        self.delete_custom_tool_risk_rule_checked(id, Some(expected_revision), actor)
    }

    fn delete_custom_tool_risk_rule_checked(
        &self,
        id: &str,
        expected_revision: Option<u32>,
        actor: ToolRiskActor,
    ) -> Result<ToolRiskWriteReceipt<bool>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let affected = match expected_revision {
            Some(revision) => tx.execute(
                "DELETE FROM tool_risk_custom_rules WHERE id = ?1 AND revision = ?2",
                params![id, revision],
            ),
            None => tx.execute(
                "DELETE FROM tool_risk_custom_rules WHERE id = ?1",
                params![id],
            ),
        }
        .map_err(db_error)?;
        if affected == 0 {
            return Err(AppError::Database(
                "规则版本冲突或不存在，请刷新后重试".into(),
            ));
        }
        finish_policy_write(
            tx,
            true,
            actor,
            ToolRiskChangeKind::RuleDelete,
            Some(id),
            RULE_FIELDS,
        )
    }

    pub fn get_builtin_tool_risk_overrides(
        &self,
    ) -> Result<std::collections::HashMap<String, bool>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        read_builtin_overrides(&conn)
    }

    #[cfg(test)]
    pub fn set_builtin_tool_risk_override(&self, id: &str, enabled: bool) -> Result<(), AppError> {
        self.change_builtin_tool_risk_override(id, Some(enabled), None, ToolRiskActor::Unknown)
            .map(|receipt| receipt.value)
    }

    #[cfg(test)]
    pub fn clear_builtin_tool_risk_override(&self, id: &str) -> Result<(), AppError> {
        self.change_builtin_tool_risk_override(id, None, None, ToolRiskActor::Unknown)
            .map(|receipt| receipt.value)
    }

    pub fn set_builtin_tool_risk_override_at_revision(
        &self,
        id: &str,
        enabled: bool,
        expected_revision: u32,
    ) -> Result<(), AppError> {
        self.set_builtin_tool_risk_override_with_actor(
            id,
            enabled,
            expected_revision,
            ToolRiskActor::Unknown,
        )
        .map(|receipt| receipt.value)
    }
    pub fn set_builtin_tool_risk_override_with_actor(
        &self,
        id: &str,
        enabled: bool,
        expected_revision: u32,
        actor: ToolRiskActor,
    ) -> Result<ToolRiskWriteReceipt<()>, AppError> {
        self.change_builtin_tool_risk_override(id, Some(enabled), Some(expected_revision), actor)
    }

    pub fn clear_builtin_tool_risk_override_at_revision(
        &self,
        id: &str,
        expected_revision: u32,
    ) -> Result<(), AppError> {
        self.clear_builtin_tool_risk_override_with_actor(
            id,
            expected_revision,
            ToolRiskActor::Unknown,
        )
        .map(|receipt| receipt.value)
    }
    pub fn clear_builtin_tool_risk_override_with_actor(
        &self,
        id: &str,
        expected_revision: u32,
        actor: ToolRiskActor,
    ) -> Result<ToolRiskWriteReceipt<()>, AppError> {
        self.change_builtin_tool_risk_override(id, None, Some(expected_revision), actor)
    }

    fn change_builtin_tool_risk_override(
        &self,
        id: &str,
        enabled: Option<bool>,
        expected_revision: Option<u32>,
        actor: ToolRiskActor,
    ) -> Result<ToolRiskWriteReceipt<()>, AppError> {
        require_builtin(id)?;
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let previous = read_builtin_overrides(&tx)?.get(id).copied();
        bump_builtin_revision(&tx, id, expected_revision)?;
        let affected = match enabled {
            Some(enabled) => tx.execute(
                "INSERT OR REPLACE INTO tool_risk_builtin_overrides (id, enabled, updated_at) VALUES (?1, ?2, ?3)",
                params![id, enabled, chrono::Utc::now().timestamp_millis()],
            ),
            None => tx.execute("DELETE FROM tool_risk_builtin_overrides WHERE id = ?1", params![id]),
        }.map_err(db_error)?;
        require_config_rows(
            affected,
            usize::from(enabled.is_some() || previous.is_some()),
        )?;
        let fields: &[&str] = if previous != enabled {
            &["enabled"]
        } else {
            &[]
        };
        let kind = if enabled.is_some() {
            ToolRiskChangeKind::BuiltinToggle
        } else {
            ToolRiskChangeKind::BuiltinReset
        };
        finish_policy_write(tx, (), actor, kind, Some(id), fields)
    }

    pub fn insert_current_tool_risk_events_batch(
        &self,
        events: &[ToolRiskEventInput],
    ) -> Result<usize, AppError> {
        if events.is_empty() {
            return Ok(0);
        }
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let settings = read_settings(&tx)?;
        let inserted = if settings.enabled {
            insert_events(
                &tx,
                events.iter().filter(|event| {
                    u64::try_from(event.policy_version).ok() == Some(settings.policy_version)
                }),
            )?
        } else {
            0
        };
        tx.commit().map_err(db_error)?;
        Ok(inserted)
    }

    #[cfg(test)]
    pub fn insert_tool_risk_events_batch(
        &self,
        events: &[ToolRiskEventInput],
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        insert_events(&tx, events.iter())?;
        tx.commit().map_err(db_error)
    }

    pub fn get_tool_risk_events(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ToolRiskEventRecord>, AppError> {
        self.get_tool_risk_events_filtered(limit, offset, None)
    }

    pub fn get_tool_risk_events_filtered(
        &self,
        limit: u32,
        offset: u32,
        verdict: Option<Verdict>,
    ) -> Result<Vec<ToolRiskEventRecord>, AppError> {
        let verdict = verdict.map(|verdict| match verdict {
            Verdict::Risky => "risky",
            Verdict::Unchecked => "unchecked",
            Verdict::Uncertain => "uncertain",
            Verdict::LowRisk => "low_risk",
        });
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let events = {
            let mut stmt = tx
                .prepare(&format!(
                    "SELECT {EVENT_COLUMNS} FROM tool_risk_events \
                     WHERE (?3 IS NULL OR verdict = ?3) \
                     ORDER BY timestamp DESC, id DESC LIMIT ?1 OFFSET ?2"
                ))
                .map_err(db_error)?;
            let rows = stmt
                .query_map(params![limit, offset, verdict], read_event)
                .map_err(db_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
        };
        tx.commit().map_err(db_error)?;
        Ok(events)
    }

    pub fn get_tool_risk_events_after(
        &self,
        after_id: i64,
        limit: u32,
        policy_version: u64,
    ) -> Result<Vec<ToolRiskEventRecord>, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        let settings = read_settings(&tx)?;
        let events = if settings.enabled && settings.policy_version == policy_version {
            let mut stmt = tx.prepare(&format!("SELECT {EVENT_COLUMNS} FROM tool_risk_events WHERE id > ?1 AND policy_version = ?2 ORDER BY id ASC LIMIT ?3")).map_err(db_error)?;
            let rows = stmt
                .query_map(params![after_id, policy_version as i64, limit], read_event)
                .map_err(db_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
        } else {
            Vec::new()
        };
        tx.commit().map_err(db_error)?;
        Ok(events)
    }

    pub fn clear_tool_risk_events(&self, before_timestamp: Option<i64>) -> Result<u64, AppError> {
        let conn = lock_conn!(self.extension_connection());
        let affected = match before_timestamp {
            Some(ts) => conn.execute(
                "DELETE FROM tool_risk_events WHERE timestamp < ?1",
                params![ts],
            ),
            None => conn.execute("DELETE FROM tool_risk_events", []),
        }
        .map_err(db_error)?;
        Ok(affected as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::tool_risk::types::*;

    fn custom_rule(id: &str) -> ToolRiskRule {
        ToolRiskRule {
            schema_version: 1,
            id: id.into(),
            revision: 1,
            origin: RuleOrigin::Custom,
            name: "test".into(),
            description: None,
            enabled: true,
            selector: RuleSelector {
                sources: None,
                clients: Some(vec!["claude".into()]),
                target: RuleTarget::Operation {
                    types: vec![OperationType::FileRead],
                },
            },
            conditions: vec![RuleCondition {
                field: "/target_path".into(),
                op: ConditionOp::Detector,
                value: Some("sensitive_path".into()),
                values: None,
            }],
            risk_category: RiskCategory::SensitiveRead,
            severity: Severity::High,
            reason_code: "sensitive_target".into(),
            local_review: LocalReview::Off,
        }
    }

    fn event() -> ToolRiskEventInput {
        ToolRiskEventInput {
            timestamp: 1000,
            source: "model_response".into(),
            stage: "proposed".into(),
            client_category: Some("claude".into()),
            operation_category: Some("file_read".into()),
            command_summary: Some("cat <path>".into()),
            detection_scope: "in_scope".into(),
            integrity_status: "complete".into(),
            verdict: Some("risky".into()),
            risk_category: Some("sensitive_read".into()),
            severity: Some("high".into()),
            reason_code: Some("sensitive_target".into()),
            policy_version: 1,
            rule_ids: vec!["builtin.sensitive_read".into()],
        }
    }

    fn insert_verdict_events(db: &Database, entries: &[(i64, Option<&str>)]) {
        let inputs: Vec<_> = entries
            .iter()
            .map(|(timestamp, verdict)| {
                let mut input = event();
                input.timestamp = *timestamp;
                input.verdict = verdict.map(str::to_owned);
                if verdict.is_none() {
                    input.detection_scope = "out_of_scope".into();
                }
                input
            })
            .collect();
        db.insert_tool_risk_events_batch(&inputs).unwrap();
    }

    fn event_ids(events: &[ToolRiskEventRecord]) -> Vec<i64> {
        events.iter().map(|event| event.id).collect()
    }

    #[test]
    fn filtered_events_cover_four_verdicts_and_none() {
        use crate::services::tool_risk::engine::Verdict;

        let db = Database::memory().unwrap();
        insert_verdict_events(
            &db,
            &[
                (100, Some("risky")),
                (200, Some("unchecked")),
                (300, Some("uncertain")),
                (400, Some("low_risk")),
                (500, None),
            ],
        );
        for (verdict, stored, expected_id) in [
            (Verdict::Risky, "risky", 1),
            (Verdict::Unchecked, "unchecked", 2),
            (Verdict::Uncertain, "uncertain", 3),
            (Verdict::LowRisk, "low_risk", 4),
        ] {
            let rows = db
                .get_tool_risk_events_filtered(10, 0, Some(verdict))
                .unwrap();
            assert_eq!(event_ids(&rows), vec![expected_id]);
            assert_eq!(rows[0].verdict.as_deref(), Some(stored));
        }
        let all = db.get_tool_risk_events_filtered(10, 0, None).unwrap();
        assert_eq!(event_ids(&all), vec![5, 4, 3, 2, 1]);
        assert_eq!(all[0].verdict, None);
        assert_eq!(all[0].detection_scope, "out_of_scope");
    }

    #[test]
    fn filtered_events_apply_offset_after_filtering_mixed_timestamps() {
        use crate::services::tool_risk::engine::Verdict;

        let db = Database::memory().unwrap();
        insert_verdict_events(
            &db,
            &[
                (300, Some("risky")),
                (900, Some("unchecked")),
                (700, Some("risky")),
                (800, Some("uncertain")),
                (500, Some("risky")),
                (1000, Some("low_risk")),
                (200, Some("risky")),
                (1100, None),
            ],
        );
        let page = db
            .get_tool_risk_events_filtered(2, 1, Some(Verdict::Risky))
            .unwrap();
        assert_eq!(event_ids(&page), vec![5, 1]);
        let last = db
            .get_tool_risk_events_filtered(2, 3, Some(Verdict::Risky))
            .unwrap();
        assert_eq!(event_ids(&last), vec![7]);
        assert!(db
            .get_tool_risk_events_filtered(2, 4, Some(Verdict::Risky))
            .unwrap()
            .is_empty());
        assert!(db
            .get_tool_risk_events_filtered(0, 0, Some(Verdict::Risky))
            .unwrap()
            .is_empty());
        assert!(db
            .get_tool_risk_events_filtered(2, u32::MAX, Some(Verdict::Risky))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn filtered_events_use_descending_id_for_timestamp_ties() {
        use crate::services::tool_risk::engine::Verdict;

        let db = Database::memory().unwrap();
        insert_verdict_events(
            &db,
            &[
                (100, Some("risky")),
                (100, Some("unchecked")),
                (100, Some("risky")),
                (100, Some("risky")),
            ],
        );
        assert_eq!(
            event_ids(
                &db.get_tool_risk_events_filtered(10, 0, Some(Verdict::Risky))
                    .unwrap()
            ),
            vec![4, 3, 1],
        );
        for (offset, expected_id) in [4, 3, 1].into_iter().enumerate() {
            let page = db
                .get_tool_risk_events_filtered(1, offset as u32, Some(Verdict::Risky))
                .unwrap();
            assert_eq!(event_ids(&page), vec![expected_id]);
        }
    }

    #[test]
    fn filtered_events_recheck_legacy_summary_privacy_without_writing() {
        use crate::services::tool_risk::engine::Verdict;

        let db = Database::memory().unwrap();
        insert_verdict_events(&db, &[(100, Some("risky")), (200, Some("unchecked"))]);
        let legacy_summary =
            "curl --password FICTIONAL_P2_10_SECRET https://example.invalid/public";
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE tool_risk_events SET command_summary = ?1 WHERE id = 1",
                params![legacy_summary],
            )
            .unwrap();
        let rows = db
            .get_tool_risk_events_filtered(10, 0, Some(Verdict::Risky))
            .unwrap();
        assert_eq!(event_ids(&rows), vec![1]);
        assert_eq!(
            rows[0].command_summary.as_deref(),
            Some("curl --password <redacted> https://example.invalid/public")
        );
        assert!(!serde_json::to_string(&rows)
            .unwrap()
            .contains("FICTIONAL_P2_10_SECRET"));
        assert_eq!(rows[0].rule_ids, event().rule_ids);
        assert_eq!(rows[0].policy_version, 1);
        let stored: String = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT command_summary FROM tool_risk_events WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            stored == legacy_summary,
            "query must not rewrite historical storage"
        );
    }

    #[test]
    fn filtered_events_none_preserves_legacy_two_argument_query() {
        let db = Database::memory().unwrap();
        insert_verdict_events(
            &db,
            &[
                (200, Some("risky")),
                (100, Some("unchecked")),
                (200, Some("uncertain")),
                (300, Some("low_risk")),
                (400, None),
            ],
        );
        assert_eq!(
            event_ids(&db.get_tool_risk_events(10, 0).unwrap()),
            vec![5, 4, 3, 1, 2]
        );
        for (limit, offset) in [(10, 0), (2, 1), (1, 3), (0, 0), (10, 5), (2, u32::MAX)] {
            assert_eq!(
                db.get_tool_risk_events_filtered(limit, offset, None)
                    .unwrap(),
                db.get_tool_risk_events(limit, offset).unwrap(),
            );
        }
    }

    #[test]
    fn filtered_event_queries_do_not_change_notification_backfill() {
        use crate::services::tool_risk::engine::Verdict;

        let db = Database::memory().unwrap();
        insert_verdict_events(&db, &[(9999, Some("risky"))]);
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 0,
        })
        .unwrap();
        db.set_builtin_tool_risk_override("builtin.sensitive_read", false)
            .unwrap();
        let mut inputs = Vec::new();
        for (timestamp, verdict) in [
            (500, Some("risky")),
            (100, Some("unchecked")),
            (400, Some("uncertain")),
            (200, Some("low_risk")),
            (300, None),
        ] {
            let mut input = event();
            input.timestamp = timestamp;
            input.verdict = verdict.map(str::to_owned);
            input.policy_version = 2;
            inputs.push(input);
        }
        db.insert_current_tool_risk_events_batch(&inputs).unwrap();
        db.get_tool_risk_events_filtered(1, 0, Some(Verdict::Risky))
            .unwrap();
        let backfill = db.get_tool_risk_events_after(0, 10, 2).unwrap();
        assert_eq!(event_ids(&backfill), vec![2, 3, 4, 5, 6]);
        assert_eq!(
            backfill
                .iter()
                .map(|event| event.verdict.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("risky"),
                Some("unchecked"),
                Some("uncertain"),
                Some("low_risk"),
                None
            ]
        );
        assert_eq!(
            event_ids(&db.get_tool_risk_events_after(3, 2, 2).unwrap()),
            vec![4, 5]
        );
        assert!(db.get_tool_risk_events_after(0, 10, 1).unwrap().is_empty());
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: false,
            policy_version: 0,
        })
        .unwrap();
        assert!(db.get_tool_risk_events_after(0, 10, 3).unwrap().is_empty());
    }

    #[test]
    fn builtin_duplicate_rejection_is_atomic_and_ignores_rule_metadata() {
        let db = Database::memory().unwrap();
        db.set_builtin_tool_risk_override("builtin.destructive_change", false)
            .unwrap();
        let mut rule = crate::services::tool_risk::builtin::builtin_rules().remove(0);
        rule.origin = RuleOrigin::Custom;
        rule.id = "custom-copy".into();
        rule.name = "renamed".into();
        rule.enabled = false;
        rule.severity = Severity::Medium;
        rule.risk_category = RiskCategory::SensitiveRead;
        rule.reason_code = "sensitive_target".into();
        if let RuleTarget::Operation { types } = &mut rule.selector.target {
            types.reverse();
            types.push(types[0]);
        }
        rule.conditions.push(rule.conditions[0].clone());
        let version = db.get_tool_risk_settings().unwrap().policy_version;
        let error = db
            .save_custom_tool_risk_rule(&rule, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("tool_risk_builtin_duplicate"), "{error}");
        assert!(db.get_custom_tool_risk_rules().unwrap().is_empty());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, version);

        let mut narrowed = rule.clone();
        narrowed.selector.clients = Some(vec!["claude".into()]);
        db.save_custom_tool_risk_rule(&narrowed, None).unwrap();
        rule.revision = 2;
        assert!(db.save_custom_tool_risk_rule(&rule, Some(1)).is_err());
        assert_eq!(
            db.get_custom_tool_risk_rule(&rule.id).unwrap(),
            Some(narrowed)
        );
        assert_eq!(
            db.get_tool_risk_settings().unwrap().policy_version,
            version + 1
        );
    }

    #[test]
    fn custom_risk_rule_cannot_save_none_severity_or_claim_builtin_id() {
        let db = Database::memory().unwrap();
        let mut rule = custom_rule("custom-none");
        rule.selector.clients = Some(vec!["claude".into()]);
        rule.severity = Severity::None;
        assert!(db.save_custom_tool_risk_rule(&rule, None).is_err());
        // Historical definitions remain readable; write-time policy prevents new exemptions.
        assert!(rule.validate().is_ok());
        rule.severity = Severity::Medium;
        rule.id = "builtin.sensitive_read".into();
        assert!(db.save_custom_tool_risk_rule(&rule, None).is_err());
        assert!(db.get_custom_tool_risk_rules().unwrap().is_empty());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
    }
    #[test]
    fn historical_duplicate_rules_remain_readable_and_can_be_disabled_or_deleted() {
        let db = Database::memory().unwrap();
        let mut rule = crate::services::tool_risk::builtin::builtin_rules().remove(1);
        rule.origin = RuleOrigin::Custom;
        rule.id = "custom-legacy-copy".into();
        db.conn.lock().unwrap().execute(
            "INSERT INTO tool_risk_custom_rules (id, revision, name, definition, enabled, updated_at) VALUES (?1, 1, ?2, ?3, 1, 0)",
            params![rule.id, rule.name, to_json_string(&rule).unwrap()],
        ).unwrap();
        assert_eq!(
            db.get_custom_tool_risk_rule(&rule.id).unwrap(),
            Some(rule.clone())
        );
        rule.revision = 2;
        rule.enabled = false;
        let mut renamed = rule.clone();
        renamed.name = "edited while disabling".into();
        assert!(db.save_custom_tool_risk_rule(&renamed, Some(1)).is_err());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
        db.save_custom_tool_risk_rule(&rule, Some(1)).unwrap();
        rule.revision = 3;
        rule.enabled = true;
        assert!(db.save_custom_tool_risk_rule(&rule, Some(2)).is_err());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 1);
        db.delete_custom_tool_risk_rule_at_revision(&rule.id, 2)
            .unwrap();
        assert!(db.get_custom_tool_risk_rules().unwrap().is_empty());
    }

    #[test]
    fn policy_versions_are_generated_for_every_configuration_change() {
        let db = Database::memory().unwrap();
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: u64::MAX,
        })
        .unwrap();
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 1);
        db.save_custom_tool_risk_rule(&custom_rule("c1"), None)
            .unwrap();
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 2);
        let mut rule = custom_rule("c1");
        rule.revision = 2;
        rule.enabled = false;
        db.save_custom_tool_risk_rule(&rule, Some(1)).unwrap();
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 3);
        db.set_builtin_tool_risk_override("builtin.sensitive_read", false)
            .unwrap();
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 4);
        db.clear_builtin_tool_risk_override("builtin.sensitive_read")
            .unwrap();
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 5);
        db.delete_custom_tool_risk_rule("c1").unwrap();
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 6);
    }

    #[test]
    fn failed_policy_version_write_rolls_back_configuration() {
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute_batch(
            "CREATE TRIGGER reject_policy BEFORE INSERT ON tool_risk_settings WHEN NEW.key = 'policy_version' BEGIN SELECT RAISE(ABORT, 'reject'); END;"
        ).unwrap();
        assert!(db
            .save_tool_risk_settings(&ToolRiskSettings {
                enabled: true,
                policy_version: 9
            })
            .is_err());
        assert!(!db.get_tool_risk_settings().unwrap().enabled);
        assert!(db
            .save_custom_tool_risk_rule(&custom_rule("c1"), None)
            .is_err());
        assert!(db.get_custom_tool_risk_rules().unwrap().is_empty());
        assert!(db
            .set_builtin_tool_risk_override("builtin.sensitive_read", false)
            .is_err());
        assert!(db.get_builtin_tool_risk_overrides().unwrap().is_empty());
    }

    #[test]
    fn create_cannot_overwrite_existing_rule_without_revision() {
        let db = Database::memory().unwrap();
        let rule = custom_rule("c1");
        db.save_custom_tool_risk_rule(&rule, None).unwrap();
        assert!(db.save_custom_tool_risk_rule(&rule, None).is_err());
        assert_eq!(db.get_custom_tool_risk_rule("c1").unwrap(), Some(rule));
    }

    #[test]
    fn unknown_configuration_ids_do_not_succeed() {
        let db = Database::memory().unwrap();
        assert!(db.delete_custom_tool_risk_rule("missing").is_err());
        assert!(db
            .set_builtin_tool_risk_override("builtin.missing", true)
            .is_err());
        assert!(db
            .clear_builtin_tool_risk_override("builtin.missing")
            .is_err());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
    }

    #[test]
    fn corrupted_rules_fail_without_echoing_definition() {
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute("INSERT INTO tool_risk_custom_rules (id, revision, name, definition, enabled, updated_at) VALUES ('c1', 1, 'x', ?1, 1, 0)", params![r#"{"schema_version":"SECRET_DO_NOT_ECHO"}"#]).unwrap();
        let error = db.get_custom_tool_risk_rules().unwrap_err().to_string();
        assert!(!error.contains("SECRET_DO_NOT_ECHO"));
        let error = db.get_custom_tool_risk_rule("c1").unwrap_err().to_string();
        assert!(!error.contains("SECRET_DO_NOT_ECHO"));
    }

    #[test]
    fn invalid_settings_do_not_default_to_version_zero() {
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute("INSERT INTO tool_risk_settings (key, value) VALUES ('policy_version', 'secret-invalid')", []).unwrap();
        let error = db.get_tool_risk_settings().unwrap_err().to_string();
        assert!(!error.contains("secret-invalid"));
    }

    #[test]
    fn settings_roundtrip() {
        let db = Database::memory().unwrap();
        assert!(!db.get_tool_risk_settings().unwrap().enabled);
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 5,
        })
        .unwrap();
        let s = db.get_tool_risk_settings().unwrap();
        assert!(s.enabled);
        assert_eq!(s.policy_version, 1);
    }

    #[test]
    fn custom_rule_save_and_get() {
        let db = Database::memory().unwrap();
        db.save_custom_tool_risk_rule(&custom_rule("c1"), None)
            .unwrap();
        let rules = db.get_custom_tool_risk_rules().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "c1");
        assert_eq!(
            db.get_custom_tool_risk_rule("c1").unwrap().unwrap().id,
            "c1"
        );
    }

    #[test]
    fn custom_rule_revision_conflict() {
        let db = Database::memory().unwrap();
        db.save_custom_tool_risk_rule(&custom_rule("c1"), None)
            .unwrap();

        let mut r = custom_rule("c1");
        r.revision = 2;
        assert!(db.save_custom_tool_risk_rule(&r, Some(2)).is_err());

        assert!(db.save_custom_tool_risk_rule(&r, Some(1)).is_ok());
    }

    #[test]
    fn custom_rule_delete() {
        let db = Database::memory().unwrap();
        db.save_custom_tool_risk_rule(&custom_rule("c1"), None)
            .unwrap();
        assert!(db.delete_custom_tool_risk_rule("c1").unwrap());
        assert!(db.delete_custom_tool_risk_rule("c1").is_err());
        assert!(db.get_custom_tool_risk_rules().unwrap().is_empty());
    }

    #[test]
    fn builtin_override_roundtrip() {
        let db = Database::memory().unwrap();
        db.set_builtin_tool_risk_override("builtin.sensitive_read", false)
            .unwrap();
        let map = db.get_builtin_tool_risk_overrides().unwrap();
        assert_eq!(map.get("builtin.sensitive_read"), Some(&false));
        db.clear_builtin_tool_risk_override("builtin.sensitive_read")
            .unwrap();
        assert!(db.get_builtin_tool_risk_overrides().unwrap().is_empty());
    }

    #[test]
    fn events_insert_and_query() {
        let db = Database::memory().unwrap();
        db.insert_tool_risk_events_batch(&[event()]).unwrap();
        let events = db.get_tool_risk_events(10, 0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].verdict.as_deref(), Some("risky"));
        assert_eq!(events[0].rule_ids, vec!["builtin.sensitive_read"]);
    }

    #[test]
    fn events_clear() {
        let db = Database::memory().unwrap();
        db.insert_tool_risk_events_batch(&[event()]).unwrap();
        assert_eq!(db.clear_tool_risk_events(None).unwrap(), 1);
        assert!(db.get_tool_risk_events(10, 0).unwrap().is_empty());
    }

    #[test]
    fn empty_batch_is_noop() {
        let db = Database::memory().unwrap();
        assert!(db.insert_tool_risk_events_batch(&[]).is_ok());
    }

    #[test]
    fn event_insert_persists_only_privacy_redacted_command_summary() {
        let db = Database::memory().unwrap();
        let mut input = event();
        input.command_summary = Some(format!(
            "{}={} curl https://example.com/public",
            "API_KEY", "SECRET_RAW_COMMAND"
        ));
        let expected = format!(
            "{}={} curl https://example.com/public",
            "API_KEY", "<redacted>"
        );
        db.insert_tool_risk_events_batch(&[input]).unwrap();
        let summary: Option<String> = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT command_summary FROM tool_risk_events", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(summary.as_deref(), Some(expected.as_str()));
        assert_eq!(
            db.get_tool_risk_events(10, 0).unwrap()[0]
                .command_summary
                .as_deref(),
            Some(expected.as_str())
        );
        db.get_tool_risk_configuration().unwrap();
        assert_eq!(
            db.get_tool_risk_events(10, 0).unwrap()[0]
                .command_summary
                .as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn event_queries_sanitize_legacy_summaries_and_configuration_scrubs_storage() {
        let db = Database::memory().unwrap();
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 0,
        })
        .unwrap();
        db.insert_current_tool_risk_events_batch(&[event()])
            .unwrap();
        db.conn.lock().unwrap().execute(
            "UPDATE tool_risk_events SET command_summary = 'curl --password SECRET_LEGACY_COMMAND https://example.com'", [],
        ).unwrap();
        let expected = "curl --password <redacted> https://example.com";
        assert_eq!(
            db.get_tool_risk_events(10, 0).unwrap()[0]
                .command_summary
                .as_deref(),
            Some(expected)
        );
        assert_eq!(
            db.get_tool_risk_events_after(0, 10, 1).unwrap()[0]
                .command_summary
                .as_deref(),
            Some(expected)
        );
        db.get_tool_risk_configuration().unwrap();
        let summary: Option<String> = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT command_summary FROM tool_risk_events", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(summary.as_deref(), Some(expected));
    }

    #[test]
    fn ordinary_summary_survives_current_policy_writes_reads_and_reload() {
        let db = Database::memory().unwrap();
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 0,
        })
        .unwrap();
        let mut input = event();
        input.command_summary =
            Some("unknown-program --public-option=value /workspace/project | cat".into());
        db.insert_current_tool_risk_events_batch(&[input.clone()])
            .unwrap();
        db.insert_current_tool_risk_events_batch(&[input.clone()])
            .unwrap();
        db.get_tool_risk_configuration().unwrap();
        let page = db.get_tool_risk_events(1, 1).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].command_summary, input.command_summary);
        let rows = db.get_tool_risk_events_after(0, 10, 1).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].command_summary, input.command_summary);
    }

    #[test]
    fn rule_revision_must_be_server_next_revision() {
        let db = Database::memory().unwrap();
        let mut rule = custom_rule("c1");
        rule.revision = 99;
        assert!(db.save_custom_tool_risk_rule(&rule, None).is_err());
        rule.revision = 1;
        db.save_custom_tool_risk_rule(&rule, None).unwrap();
        rule.revision = 99;
        assert!(db.save_custom_tool_risk_rule(&rule, Some(1)).is_err());
        assert_eq!(
            db.get_custom_tool_risk_rule("c1")
                .unwrap()
                .unwrap()
                .revision,
            1
        );
    }

    #[test]
    fn enabling_preserves_rule_selection_and_custom_definitions() {
        let db = Database::memory().unwrap();
        db.set_builtin_tool_risk_override("builtin.sensitive_read", false)
            .unwrap();
        let mut rule = custom_rule("c1");
        rule.enabled = false;
        db.save_custom_tool_risk_rule(&rule, None).unwrap();
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 0,
        })
        .unwrap();
        assert_eq!(db.get_custom_tool_risk_rule("c1").unwrap(), Some(rule));
        assert_eq!(
            db.get_builtin_tool_risk_overrides()
                .unwrap()
                .get("builtin.sensitive_read"),
            Some(&false)
        );
    }

    #[test]
    fn current_batch_rejects_disabled_and_stale_policy_events() {
        let db = Database::memory().unwrap();
        assert_eq!(
            db.insert_current_tool_risk_events_batch(&[event()])
                .unwrap(),
            0
        );
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 0,
        })
        .unwrap();
        assert_eq!(
            db.insert_current_tool_risk_events_batch(&[event()])
                .unwrap(),
            1
        );
        db.set_builtin_tool_risk_override("builtin.sensitive_read", false)
            .unwrap();
        let mut current = event();
        current.policy_version = 2;
        assert_eq!(
            db.insert_current_tool_risk_events_batch(&[event(), current])
                .unwrap(),
            1
        );
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: false,
            policy_version: 0,
        })
        .unwrap();
        let mut disabled = event();
        disabled.policy_version = 3;
        assert_eq!(
            db.insert_current_tool_risk_events_batch(&[disabled])
                .unwrap(),
            0
        );
        assert_eq!(db.get_tool_risk_events(10, 0).unwrap().len(), 2);
    }

    #[test]
    fn current_batch_is_atomic_on_insert_failure() {
        let db = Database::memory().unwrap();
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 0,
        })
        .unwrap();
        db.conn.lock().unwrap().execute_batch(
            "CREATE TRIGGER reject_event BEFORE INSERT ON tool_risk_events WHEN NEW.timestamp = 2 BEGIN SELECT RAISE(ABORT, 'reject'); END;"
        ).unwrap();
        let mut bad = event();
        bad.timestamp = 2;
        assert!(db
            .insert_current_tool_risk_events_batch(&[event(), bad])
            .is_err());
        assert!(db.get_tool_risk_events(10, 0).unwrap().is_empty());
    }

    #[test]
    fn event_backfill_is_current_policy_only_and_id_ordered() {
        let db = Database::memory().unwrap();
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 0,
        })
        .unwrap();
        db.insert_current_tool_risk_events_batch(&[event()])
            .unwrap();
        db.set_builtin_tool_risk_override("builtin.sensitive_read", false)
            .unwrap();
        let mut first = event();
        first.policy_version = 2;
        first.timestamp = 9000;
        let mut second = first.clone();
        second.timestamp = 1;
        db.insert_current_tool_risk_events_batch(&[first, second])
            .unwrap();
        let rows = db.get_tool_risk_events_after(0, 10, 2).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].id < rows[1].id);
        assert_eq!(rows[0].command_summary.as_deref(), Some("cat <path>"));
        assert_eq!(
            db.get_tool_risk_events_after(rows[0].id, 10, 2)
                .unwrap()
                .len(),
            1
        );
        assert!(db.get_tool_risk_events_after(0, 10, 1).unwrap().is_empty());
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: false,
            policy_version: 0,
        })
        .unwrap();
        assert!(db.get_tool_risk_events_after(0, 10, 3).unwrap().is_empty());
    }
}

#[cfg(test)]
mod command_list_tests {
    use super::*;
    use crate::services::tool_risk::classifier::{
        CommandListEntry, CommandListType, CommandMatchKind, MAX_COMMAND_LISTS,
    };
    use crate::services::tool_risk::types::{RiskCategory, Severity};

    fn entry(id: &str) -> CommandListEntry {
        CommandListEntry {
            id: id.into(),
            revision: 1,
            list_type: CommandListType::Blacklist,
            match_kind: CommandMatchKind::Keyword,
            pattern: "rm -rf".into(),
            enabled: true,
            severity: Severity::High,
            risk_category: RiskCategory::DestructiveChange,
        }
    }

    #[test]
    fn command_lists_crud_is_revision_checked_and_bumps_policy_atomically() {
        let db = Database::memory().unwrap();
        let mut item = entry("shell-list-one");
        db.save_tool_risk_command_list(&item, None).unwrap();
        assert_eq!(
            db.get_tool_risk_command_lists().unwrap(),
            vec![item.clone()]
        );
        assert!(db.save_tool_risk_command_list(&item, None).is_err());
        item.revision = 2;
        item.enabled = false;
        assert!(db.save_tool_risk_command_list(&item, Some(2)).is_err());
        db.save_tool_risk_command_list(&item, Some(1)).unwrap();
        assert!(db
            .delete_tool_risk_command_list("shell-list-one", 1)
            .is_err());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 2);
        assert_eq!(db.get_tool_risk_command_lists().unwrap(), vec![item]);
        assert!(db
            .delete_tool_risk_command_list("shell-list-one", 2)
            .unwrap());
        assert!(db.get_tool_risk_command_lists().unwrap().is_empty());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 3);
    }

    #[test]
    fn command_lists_reject_same_list_duplicates_but_allow_cross_list_conflicts() {
        let db = Database::memory().unwrap();
        db.save_tool_risk_command_list(&entry("shell-list-one"), None)
            .unwrap();
        let mut duplicate = entry("shell-list-two");
        duplicate.enabled = false;
        assert!(db.save_tool_risk_command_list(&duplicate, None).is_err());
        duplicate.list_type = CommandListType::Whitelist;
        duplicate.severity = Severity::None;
        db.save_tool_risk_command_list(&duplicate, None).unwrap();
        assert_eq!(db.get_tool_risk_command_lists().unwrap().len(), 2);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 2);
    }

    #[test]
    fn command_lists_invalid_definitions_never_enter_database() {
        let db = Database::memory().unwrap();
        for mut invalid in [
            entry("shell-list-empty"),
            entry("shell-list-regex"),
            entry("shell-list-large"),
        ] {
            match invalid.id.as_str() {
                "shell-list-empty" => invalid.pattern.clear(),
                "shell-list-regex" => {
                    invalid.match_kind = CommandMatchKind::Regex;
                    invalid.pattern = "[".into();
                }
                _ => invalid.pattern = "x".repeat(4097),
            }
            assert!(db.save_tool_risk_command_list(&invalid, None).is_err());
        }
        assert!(db.get_tool_risk_command_lists().unwrap().is_empty());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
    }

    #[test]
    fn command_lists_policy_failure_rolls_back_create_edit_and_delete() {
        let db = Database::memory().unwrap();
        let item = entry("shell-list-one");
        db.save_tool_risk_command_list(&item, None).unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_policy BEFORE INSERT ON tool_risk_settings WHEN NEW.key = 'policy_version' BEGIN SELECT RAISE(ABORT, 'reject'); END;").unwrap();
        let mut create = entry("shell-list-two");
        create.pattern = "unique-pattern".into();
        assert!(db.save_tool_risk_command_list(&create, None).is_err());
        let mut edit = item.clone();
        edit.revision = 2;
        edit.enabled = false;
        assert!(db.save_tool_risk_command_list(&edit, Some(1)).is_err());
        assert!(db.delete_tool_risk_command_list(&item.id, 1).is_err());
        assert_eq!(db.get_tool_risk_command_lists().unwrap(), vec![item]);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 1);
    }

    #[test]
    fn command_lists_configuration_snapshot_includes_lists_rules_and_same_version() {
        let db = Database::memory().unwrap();
        let item = entry("shell-list-one");
        db.save_tool_risk_command_list(&item, None).unwrap();
        let (settings, rules, lists) = db.get_tool_risk_configuration_snapshot().unwrap();
        assert_eq!(settings.policy_version, 1);
        assert!(!rules.is_empty());
        assert_eq!(lists, vec![item]);
    }

    #[test]
    fn command_lists_corrupt_or_mismatched_identity_is_rejected_without_echoing_data() {
        let db = Database::memory().unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO tool_risk_command_lists VALUES ('shell-list-corrupt', ?1, 0)",
                ["SECRET_DEFINITION"],
            )
            .unwrap();
        let error = db
            .get_tool_risk_configuration_snapshot()
            .unwrap_err()
            .to_string();
        assert!(!error.contains("SECRET_DEFINITION"));
        let item = entry("shell-list-different");
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE tool_risk_command_lists SET definition = ?1",
                [serde_json::to_string(&item).unwrap()],
            )
            .unwrap();
        assert!(db.get_tool_risk_command_lists().is_err());
    }

    #[test]
    fn command_lists_are_bounded_and_existing_entries_remain_editable_at_limit() {
        let db = Database::memory().unwrap();
        for index in 0..MAX_COMMAND_LISTS {
            let mut item = entry(&format!("shell-list-{index}"));
            item.pattern = format!("pattern-{index}");
            db.save_tool_risk_command_list(&item, None).unwrap();
        }
        assert!(db
            .save_tool_risk_command_list(&entry("shell-list-overflow"), None)
            .is_err());
        let mut edit = entry("shell-list-0");
        edit.pattern = "pattern-0".into();
        edit.revision = 2;
        edit.enabled = false;
        db.save_tool_risk_command_list(&edit, Some(1)).unwrap();
        let mut imported = entry("shell-list-imported-overflow");
        imported.pattern = "imported".into();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO tool_risk_command_lists VALUES (?1, ?2, 0)",
                params![imported.id, serde_json::to_string(&imported).unwrap()],
            )
            .unwrap();
        assert!(db.get_tool_risk_configuration_snapshot().is_err());
    }
}

#[cfg(test)]
mod source_scope_duplicate_tests {
    use super::*;
    use crate::services::tool_risk::types::*;

    #[test]
    fn explicit_all_sources_is_not_a_new_detection_scope() {
        let db = Database::memory().unwrap();
        let mut rule = crate::services::tool_risk::builtin::builtin_rules().remove(1);
        rule.id = "custom-all-sources".into();
        rule.origin = RuleOrigin::Custom;
        rule.selector.sources = Some(vec![
            "mcp_rpc".into(),
            "model_response".into(),
            "client_hook".into(),
            "model_response".into(),
        ]);
        assert!(rule.validate().is_ok());
        assert!(db.save_custom_tool_risk_rule(&rule, None).is_err());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
        rule.selector.sources = Some(vec!["client_hook".into()]);
        db.save_custom_tool_risk_rule(&rule, None).unwrap();
        let saved = rule.clone();
        rule.revision = 2;
        rule.selector.sources = Some(
            KNOWN_SOURCES
                .iter()
                .map(|source| source.to_string())
                .collect(),
        );
        assert!(db.save_custom_tool_risk_rule(&rule, Some(1)).is_err());
        assert_eq!(db.get_custom_tool_risk_rule(&rule.id).unwrap(), Some(saved));
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 1);
    }
}

#[cfg(test)]
mod policy_audit_tests {
    use super::*;
    use crate::services::tool_risk::{
        builtin::builtin_rules,
        classifier::{CommandListType, CommandMatchKind},
        types::{LocalReview, RuleOrigin, RuleSelector},
    };

    fn db() -> Database {
        let db = Database::memory().unwrap();
        // Independent audit schema fixture isolates DAO behavior from migration work.
        db.conn.lock().unwrap().execute_batch("CREATE TABLE IF NOT EXISTS tool_risk_policy_changes (
            id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, policy_version INTEGER NOT NULL,
            actor TEXT NOT NULL, kind TEXT NOT NULL, target_id TEXT, changed_fields TEXT NOT NULL, audit_status TEXT NOT NULL
        );").unwrap();
        db
    }
    fn rule() -> ToolRiskRule {
        let mut rule = builtin_rules().remove(0);
        rule.id = "custom-123e4567-e89b-42d3-a456-426614174000".into();
        rule.origin = RuleOrigin::Custom;
        rule.revision = 1;
        rule.name = "SECRET-NAME".into();
        rule.description = Some("SECRET-DESCRIPTION".into());
        rule.selector = RuleSelector {
            clients: Some(vec!["claude".into()]),
            ..rule.selector
        };
        rule.local_review = LocalReview::Off;
        rule
    }
    fn entry() -> CommandListEntry {
        CommandListEntry {
            id: "shell-list-123e4567-e89b-42d3-a456-426614174001".into(),
            revision: 1,
            list_type: CommandListType::Blacklist,
            match_kind: CommandMatchKind::Exact,
            pattern: "SECRET-PATTERN".into(),
            enabled: true,
            severity: crate::services::tool_risk::types::Severity::High,
            risk_category: crate::services::tool_risk::types::RiskCategory::RemoteCodeExecution,
        }
    }
    fn rows(db: &Database) -> Vec<(i64, String, Option<String>, String)> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT policy_version, kind, target_id, changed_fields FROM tool_risk_policy_changes ORDER BY id").unwrap();
        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    #[test]
    fn settings_audit_only_real_enabled_change_and_preserves_version_behavior() {
        let db = db();
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: false,
            policy_version: 999,
        })
        .unwrap();
        assert!(rows(&db).is_empty());
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 999,
        })
        .unwrap();
        let events = rows(&db);
        assert_eq!(
            events,
            vec![(2, "settings_update".into(), None, "[\"enabled\"]".into())]
        );
    }
    #[test]
    fn custom_rules_audit_fixed_fields_and_never_secret_values() {
        let db = db();
        let mut rule = rule();
        db.save_custom_tool_risk_rule(&rule, None).unwrap();
        rule.revision = 2;
        rule.name = "SECRET-NEW-NAME".into();
        db.save_custom_tool_risk_rule(&rule, Some(1)).unwrap();
        rule.revision = 3;
        db.save_custom_tool_risk_rule(&rule, Some(2)).unwrap();
        db.delete_custom_tool_risk_rule_at_revision(&rule.id, 3)
            .unwrap();
        let events = rows(&db);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].1, "rule_create");
        assert_eq!(events[1].1, "rule_update");
        assert_eq!(events[2].1, "rule_delete");
        assert_eq!(events[1].3, "[\"name\"]");
        assert_eq!(events[2].0, 4);
        assert!(!format!("{events:?}").contains("SECRET"));
    }
    #[test]
    fn lists_create_update_toggle_delete_audit_only_changed_fields() {
        let db = db();
        let mut entry = entry();
        db.save_tool_risk_command_list(&entry, None).unwrap();
        entry.revision = 2;
        entry.enabled = false;
        db.save_tool_risk_command_list(&entry, Some(1)).unwrap();
        db.delete_tool_risk_command_list(&entry.id, 2).unwrap();
        let events = rows(&db);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].1, "list_create");
        assert_eq!(events[1].1, "list_update");
        assert_eq!(events[2].1, "list_delete");
        assert_eq!(events[1].3, "[\"enabled\"]");
        assert!(!format!("{events:?}").contains("SECRET"));
    }
    #[test]
    fn builtin_override_records_storage_changes_not_revision_only() {
        let db = db();
        db.set_builtin_tool_risk_override_at_revision("builtin.sensitive_read", false, 1)
            .unwrap();
        db.set_builtin_tool_risk_override_at_revision("builtin.sensitive_read", false, 2)
            .unwrap();
        db.clear_builtin_tool_risk_override_at_revision("builtin.sensitive_read", 3)
            .unwrap();
        db.clear_builtin_tool_risk_override_at_revision("builtin.sensitive_read", 4)
            .unwrap();
        let events = rows(&db);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1, "builtin_toggle");
        assert_eq!(events[1].1, "builtin_reset");
        assert_eq!(events[1].0, 3);
    }
    #[test]
    fn legacy_arbitrary_ids_are_projected_to_unknown_target() {
        let db = db();
        let mut rule = rule();
        rule.id = "SECRET-LEGACY-ID".into();
        db.save_custom_tool_risk_rule(&rule, None).unwrap();
        let events = rows(&db);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].2.as_deref(), Some("<unknown_target>"));
    }
    #[test]
    fn retention_keeps_the_latest_thousand_without_changing_configuration() {
        let db = db();
        for index in 0..1003 {
            db.save_tool_risk_settings(&ToolRiskSettings {
                enabled: index % 2 == 0,
                policy_version: 0,
            })
            .unwrap();
        }
        let events = rows(&db);
        assert_eq!(events.len(), 1000);
        assert_eq!(events[0].0, 4);
        assert_eq!(events.last().unwrap().0, 1003);
    }
    #[test]
    fn configuration_failure_and_conflict_create_no_success_history() {
        let db = db();
        let rule = rule();
        db.save_custom_tool_risk_rule(&rule, None).unwrap();
        assert!(db.save_custom_tool_risk_rule(&rule, None).is_err());
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_policy BEFORE INSERT ON tool_risk_settings WHEN NEW.key='policy_version' BEGIN SELECT RAISE(ABORT,'SECRET-FAIL'); END;").unwrap();
        assert!(db
            .save_tool_risk_settings(&ToolRiskSettings {
                enabled: true,
                policy_version: 0
            })
            .is_err());
        assert_eq!(rows(&db).len(), 1);
        assert!(!db.get_tool_risk_settings().unwrap().enabled);
    }
    #[test]
    fn query_and_clear_audit_metadata_are_safe_and_configuration_independent() {
        let db = db();
        for index in 0..3 {
            db.save_tool_risk_settings(&ToolRiskSettings {
                enabled: index % 2 == 0,
                policy_version: 0,
            })
            .unwrap();
        }
        let records = db.get_tool_risk_policy_changes(2, 0).unwrap();
        assert_eq!(
            records.iter().map(|r| r.policy_version).collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert_eq!(db.get_tool_risk_policy_changes(2, 2).unwrap().len(), 1);
        assert!(db.get_tool_risk_policy_changes(0, 0).unwrap().is_empty());
        let settings = db.get_tool_risk_settings().unwrap();
        assert_eq!(db.clear_tool_risk_policy_changes().unwrap(), 3);
        assert!(db.get_tool_risk_policy_changes(50, 0).unwrap().is_empty());
        assert_eq!(db.get_tool_risk_settings().unwrap(), settings);
    }
    #[test]
    fn query_projects_malformed_fields_and_arbitrary_metadata_without_echo() {
        let db = db();
        let conn = db.conn.lock().unwrap();
        conn.execute_batch("PRAGMA ignore_check_constraints=ON;
            INSERT INTO tool_risk_policy_changes(at,policy_version,actor,kind,target_id,changed_fields,audit_status) VALUES
            (1,1,'SECRET-ACTOR','SECRET-KIND','SECRET-ID','[\"pattern\",\"SECRET-FIELD\",\"enabled\",\"pattern\"]','SECRET-STATUS'),
            (2,2,'system','rule_update','custom-not-a-uuid','SECRET-JSON','recorded');").unwrap();
        drop(conn);
        let rows = db.get_tool_risk_policy_changes(50, 0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].changed_fields, Vec::<String>::new());
        assert_eq!(
            rows[1].changed_fields,
            vec!["enabled".to_owned(), "pattern".to_owned()]
        );
        assert_eq!(rows[1].actor, ToolRiskActor::Unknown);
        assert_eq!(rows[1].kind, ToolRiskChangeKind::Unknown);
        assert!(!serde_json::to_string(&rows).unwrap().contains("SECRET"));
    }
    #[test]
    fn audit_failure_returns_gap_status_without_rolling_back_successful_config() {
        let db = db();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_audit BEFORE INSERT ON tool_risk_policy_changes BEGIN SELECT RAISE(ABORT,'SECRET-AUDIT-ERROR'); END;").unwrap();
        let receipt = db
            .save_tool_risk_settings_with_actor(
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: 999,
                },
                ToolRiskActor::LocalUi,
            )
            .unwrap();
        assert_eq!(receipt.audit_status, ToolRiskAuditStatus::AuditUnavailable);
        assert_eq!(receipt.persisted_policy_version, 1);
        assert!(db.get_tool_risk_settings().unwrap().enabled);
        assert!(rows(&db).is_empty());
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_audit;")
            .unwrap();
        let receipt = db
            .save_tool_risk_settings_with_actor(
                &ToolRiskSettings {
                    enabled: false,
                    policy_version: 0,
                },
                ToolRiskActor::System,
            )
            .unwrap();
        assert_eq!(receipt.audit_status, ToolRiskAuditStatus::Recorded);
        assert_eq!(
            db.get_tool_risk_policy_changes(10, 0).unwrap()[0].actor,
            ToolRiskActor::System
        );
    }
    #[test]
    fn retention_failure_rolls_back_only_audit_append() {
        let db = db();
        for index in 0..1000 {
            db.save_tool_risk_settings(&ToolRiskSettings {
                enabled: index % 2 == 0,
                policy_version: 0,
            })
            .unwrap();
        }
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_cleanup BEFORE DELETE ON tool_risk_policy_changes BEGIN SELECT RAISE(ABORT,'SECRET-CLEANUP'); END;").unwrap();
        let receipt = db
            .save_tool_risk_settings_with_actor(
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: 0,
                },
                ToolRiskActor::LocalUi,
            )
            .unwrap();
        assert_eq!(receipt.audit_status, ToolRiskAuditStatus::AuditUnavailable);
        assert_eq!(receipt.persisted_policy_version, 1001);
        assert_eq!(rows(&db).len(), 1000);
        assert!(db.get_tool_risk_settings().unwrap().enabled);
    }
    #[test]
    fn transaction_destroying_audit_error_must_not_claim_saved() {
        let db = db();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER destroy_transaction BEFORE INSERT ON tool_risk_policy_changes BEGIN SELECT RAISE(ROLLBACK,'SECRET-FATAL'); END;").unwrap();
        let error = db
            .save_tool_risk_settings_with_actor(
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: 0,
                },
                ToolRiskActor::LocalUi,
            )
            .unwrap_err();
        assert!(!error.to_string().contains("SECRET"));
        assert_eq!(
            db.get_tool_risk_settings().unwrap(),
            ToolRiskSettings::default()
        );
    }
    #[test]
    fn conditions_and_patterns_only_record_field_names_not_values() {
        let db = db();
        let mut rule = rule();
        rule.enabled = false;
        rule.conditions[0].op = crate::services::tool_risk::types::ConditionOp::Eq;
        rule.conditions[0].value = Some("SECRET-CONDITION".into());
        db.save_custom_tool_risk_rule(&rule, None).unwrap();
        rule.revision += 1;
        rule.conditions[0].value = Some("SECRET-CONDITION-UPDATED".into());
        db.save_custom_tool_risk_rule(&rule, Some(1)).unwrap();
        let mut entry = entry();
        db.save_tool_risk_command_list(&entry, None).unwrap();
        entry.revision += 1;
        entry.pattern = "SECRET-PATTERN-UPDATED".into();
        db.save_tool_risk_command_list(&entry, Some(1)).unwrap();
        let records = db.get_tool_risk_policy_changes(50, 0).unwrap();
        assert_eq!(records[0].changed_fields, vec!["pattern"]);
        assert_eq!(records[2].changed_fields, vec!["conditions"]);
        let conn = db.conn.lock().unwrap();
        let mut statement = conn.prepare("SELECT at, policy_version, actor, kind, target_id, changed_fields, audit_status FROM tool_risk_policy_changes").unwrap();
        let data: Vec<String> = statement
            .query_map([], |row| {
                Ok(format!(
                    "{} {} {} {} {:?} {} {}",
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!data.join("\n").contains("SECRET"));
    }
    #[test]
    fn no_op_receipts_and_failed_clear_do_not_change_history_or_configuration() {
        let db = db();
        let first = db
            .save_tool_risk_settings_with_actor(
                &ToolRiskSettings::default(),
                ToolRiskActor::LocalUi,
            )
            .unwrap();
        assert_eq!(first.persisted_policy_version, 1);
        assert_eq!(first.audit_status, ToolRiskAuditStatus::NotNeeded);
        db.save_tool_risk_settings_with_actor(
            &ToolRiskSettings {
                enabled: true,
                policy_version: 0,
            },
            ToolRiskActor::LocalUi,
        )
        .unwrap();
        let before = db.get_tool_risk_configuration_snapshot().unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_clear BEFORE DELETE ON tool_risk_policy_changes BEGIN SELECT RAISE(ABORT,'SECRET-CLEAR'); END;").unwrap();
        assert!(db.clear_tool_risk_policy_changes().is_err());
        assert_eq!(db.get_tool_risk_policy_changes(50, 0).unwrap().len(), 1);
        assert_eq!(db.get_tool_risk_configuration_snapshot().unwrap(), before);
    }
    #[test]
    fn oversized_history_queries_are_bounded_and_unknown_metadata_is_safe() {
        let db = db();
        let conn = db.conn.lock().unwrap();
        conn.execute_batch("PRAGMA ignore_check_constraints=ON; WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i < 1001) INSERT INTO tool_risk_policy_changes(at,policy_version,actor,kind,target_id,changed_fields,audit_status) SELECT 1, -1, 'SECRET-ACTOR', 'SECRET-KIND', 'SECRET-ID', '[\"SECRET-FIELD\"]', 'SECRET-STATUS' FROM n;").unwrap();
        drop(conn);
        let records = db.get_tool_risk_policy_changes(u32::MAX, 0).unwrap();
        assert_eq!(records.len(), 1000);
        assert_eq!(records[0].policy_version, 0);
        assert_eq!(records[0].audit_status, ToolRiskStoredAuditStatus::Unknown);
        assert_eq!(records[0].target_id.as_deref(), Some("<unknown_target>"));
        assert!(!serde_json::to_string(&records).unwrap().contains("SECRET"));
        assert!(db
            .get_tool_risk_policy_changes(50, u32::MAX)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn silently_ignored_audit_append_reports_a_gap_and_keeps_config() {
        let db = db();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER skip_audit BEFORE INSERT ON tool_risk_policy_changes BEGIN SELECT RAISE(IGNORE); END;").unwrap();
        let receipt = db
            .save_tool_risk_settings_with_actor(
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: 0,
                },
                ToolRiskActor::LocalUi,
            )
            .unwrap();
        assert_eq!(receipt.audit_status, ToolRiskAuditStatus::AuditUnavailable);
        assert_eq!(receipt.persisted_policy_version, 1);
        assert!(db.get_tool_risk_settings().unwrap().enabled);
        assert!(rows(&db).is_empty());
    }
    #[test]
    fn silently_ignored_config_writes_never_report_a_successful_change() {
        for (table, predicate, operation) in [
            ("tool_risk_settings", "WHEN NEW.key='enabled'", "settings"),
            (
                "tool_risk_settings",
                "WHEN NEW.key='policy_version'",
                "version",
            ),
            ("tool_risk_custom_rules", "", "rule"),
            ("tool_risk_command_lists", "", "list"),
            ("tool_risk_builtin_overrides", "", "builtin"),
        ] {
            let db = db();
            db.conn.lock().unwrap().execute_batch(&format!("CREATE TRIGGER skip_config BEFORE INSERT ON {table} {predicate} BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
            let failed = match operation {
                "rule" => db.save_custom_tool_risk_rule(&rule(), None).is_err(),
                "list" => db.save_tool_risk_command_list(&entry(), None).is_err(),
                "builtin" => db
                    .set_builtin_tool_risk_override_at_revision("builtin.sensitive_read", false, 1)
                    .is_err(),
                _ => db
                    .save_tool_risk_settings(&ToolRiskSettings {
                        enabled: true,
                        policy_version: 0,
                    })
                    .is_err(),
            };
            assert!(failed, "ignored {operation} must not report persisted");
            assert_eq!(
                db.get_tool_risk_settings().unwrap(),
                ToolRiskSettings::default()
            );
            assert!(rows(&db).is_empty());
            assert!(db.get_custom_tool_risk_rules().unwrap().is_empty());
            assert!(db.get_tool_risk_command_lists().unwrap().is_empty());
            assert!(db.get_builtin_tool_risk_overrides().unwrap().is_empty());
        }
    }
    #[test]
    fn silently_ignored_deletes_roll_back_config_and_policy() {
        for operation in ["rule", "list", "builtin"] {
            let db = db();
            let rule = rule();
            let entry = entry();
            let table = match operation {
                "rule" => {
                    db.save_custom_tool_risk_rule(&rule, None).unwrap();
                    "tool_risk_custom_rules"
                }
                "list" => {
                    db.save_tool_risk_command_list(&entry, None).unwrap();
                    "tool_risk_command_lists"
                }
                _ => {
                    db.set_builtin_tool_risk_override_at_revision(
                        "builtin.sensitive_read",
                        false,
                        1,
                    )
                    .unwrap();
                    "tool_risk_builtin_overrides"
                }
            };
            let before = db.get_tool_risk_configuration_snapshot().unwrap();
            db.conn.lock().unwrap().execute_batch(&format!("CREATE TRIGGER skip_delete BEFORE DELETE ON {table} BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
            let failed = match operation {
                "rule" => db
                    .delete_custom_tool_risk_rule_at_revision(&rule.id, 1)
                    .is_err(),
                "list" => db.delete_tool_risk_command_list(&entry.id, 1).is_err(),
                _ => db
                    .clear_builtin_tool_risk_override_at_revision("builtin.sensitive_read", 2)
                    .is_err(),
            };
            assert!(
                failed,
                "ignored {operation} delete must not report persisted"
            );
            assert_eq!(db.get_tool_risk_configuration_snapshot().unwrap(), before);
            assert_eq!(rows(&db).len(), 1);
        }
    }
    #[test]
    fn silently_ignored_history_clear_never_reports_success() {
        let db = db();
        db.save_tool_risk_settings(&ToolRiskSettings {
            enabled: true,
            policy_version: 0,
        })
        .unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER skip_clear BEFORE DELETE ON tool_risk_policy_changes BEGIN SELECT RAISE(IGNORE); END;").unwrap();
        assert!(db.clear_tool_risk_policy_changes().is_err());
        assert_eq!(rows(&db).len(), 1);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 1);
    }
    #[test]
    fn silently_ignored_retention_rolls_back_only_audit_append() {
        let db = db();
        for index in 0..1000 {
            db.save_tool_risk_settings(&ToolRiskSettings {
                enabled: index % 2 == 0,
                policy_version: 0,
            })
            .unwrap();
        }
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER skip_cleanup BEFORE DELETE ON tool_risk_policy_changes BEGIN SELECT RAISE(IGNORE); END;").unwrap();
        let receipt = db
            .save_tool_risk_settings_with_actor(
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: 0,
                },
                ToolRiskActor::LocalUi,
            )
            .unwrap();
        assert_eq!(receipt.audit_status, ToolRiskAuditStatus::AuditUnavailable);
        assert_eq!(receipt.persisted_policy_version, 1001);
        assert_eq!(rows(&db).len(), 1000);
        assert!(db.get_tool_risk_settings().unwrap().enabled);
    }
}
