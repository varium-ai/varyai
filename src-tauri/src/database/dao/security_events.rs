//! Unified queries over extension-only event storage. No detection/notification side effects.

use std::collections::HashMap;

use rusqlite::{params_from_iter, types::Value, Connection, Row};
use serde::{Deserialize, Serialize};

use super::{
    security::AuditLogRecord,
    tool_risk::{read_event, ToolRiskEventRecord, EVENT_COLUMNS},
};
use crate::database::Database;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventType {
    Dlp,
    ToolRisk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventView {
    SecurityEvents,
    ToolCalls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventResult {
    Redacted,
    Blocked,
    Warning,
    Allowed,
    Unchecked,
    Uncertain,
    LowRisk,
    Unknown,
}
impl SecurityEventResult {
    fn as_str(self) -> &'static str {
        match self {
            Self::Redacted => "redacted",
            Self::Blocked => "blocked",
            Self::Warning => "warning",
            Self::Allowed => "allowed",
            Self::Unchecked => "unchecked",
            Self::Uncertain => "uncertain",
            Self::LowRisk => "low_risk",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SecurityEventRiskLevel {
    Low,
    Medium,
    High,
    Critical,
}
impl SecurityEventRiskLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
        }
    }
    fn from_stored(value: &str) -> Option<Self> {
        match value {
            "LOW" => Some(Self::Low),
            "MEDIUM" => Some(Self::Medium),
            "HIGH" => Some(Self::High),
            "CRITICAL" => Some(Self::Critical),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventToolVerdict {
    Risky,
    Unchecked,
    Uncertain,
    LowRisk,
}
impl SecurityEventToolVerdict {
    fn as_str(self) -> &'static str {
        match self {
            Self::Risky => "risky",
            Self::Unchecked => "unchecked",
            Self::Uncertain => "uncertain",
            Self::LowRisk => "low_risk",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecurityEventFilter {
    pub view: SecurityEventView,
    pub event_type: Option<SecurityEventType>,
    pub start_timestamp: Option<i64>,
    pub end_timestamp: Option<i64>,
    pub app_type: Option<String>,
    pub risk_level: Option<SecurityEventRiskLevel>,
    pub result: Option<SecurityEventResult>,
    pub category: Option<String>,
    pub session_id: Option<String>,
    pub keyword: Option<String>,
    pub tool_verdict: Option<SecurityEventToolVerdict>,
}

/// DLP's original string fields intentionally retain unknown historical values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "eventType", content = "record", rename_all = "snake_case")]
pub enum SecurityEventRecord {
    Dlp(AuditLogRecord),
    ToolRisk(ToolRiskEventRecord),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityEvent {
    pub key: String,
    pub timestamp: i64,
    pub last_seen_at: i64,
    pub trigger_count: i64,
    pub app_type: Option<String>,
    pub session_id: Option<String>,
    pub risk_level: Option<SecurityEventRiskLevel>,
    pub result: SecurityEventResult,
    #[serde(flatten)]
    pub record: SecurityEventRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityEventPage {
    pub items: Vec<SecurityEvent>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityEventSummary {
    pub total: u64,
    pub dlp_count: u64,
    pub tool_risk_count: u64,
    pub redacted: u64,
    pub warnings: u64,
    pub protective_blocked: u64,
    pub other: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityEventsClearResult {
    pub deleted_total: u64,
    pub deleted_dlp: u64,
    pub deleted_tool_risk: u64,
}

/// Fixed messages only: never include SQL, stored content, lock errors or input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SecurityEventError {
    #[error("SECURITY_EVENTS_INVALID_PARAMETERS")]
    InvalidParameters,
    #[error("SECURITY_EVENTS_DATABASE_ERROR")]
    Database,
}
impl From<rusqlite::Error> for SecurityEventError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Database
    }
}

const MAX_JS_INTEGER: i64 = 9_007_199_254_740_991;
const DLP_RESULT: &str = "CASE action_taken WHEN 'redact' THEN 'redacted' WHEN 'block' THEN 'blocked' WHEN 'audit' THEN 'warning' WHEN 'allow' THEN 'allowed' ELSE 'unknown' END";
const TOOL_RESULT: &str = "CASE verdict WHEN 'risky' THEN 'warning' WHEN 'unchecked' THEN 'unchecked' WHEN 'uncertain' THEN 'uncertain' WHEN 'low_risk' THEN 'low_risk' ELSE 'unknown' END";
const TOOL_LEVEL: &str = "CASE WHEN verdict = 'risky' THEN CASE severity WHEN 'high' THEN 'HIGH' WHEN 'medium' THEN 'MEDIUM' END END";
const DLP_COLUMNS: &str = "id, timestamp, request_id, app_type, provider_id, rule_id, rule_name, rule_category, risk_level, action_taken, matched_snippet_masked, location, datetime(created_at, 'localtime'), trigger_count, COALESCE(last_seen_at, timestamp), session_id";

impl SecurityEventFilter {
    fn normalized(&self) -> Result<Self, SecurityEventError> {
        let mut filter = self.clone();
        for value in [&mut filter.session_id, &mut filter.keyword] {
            *value = value
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            if value.as_ref().is_some_and(|s| s.chars().count() > 512) {
                return Err(SecurityEventError::InvalidParameters);
            }
        }
        for value in [&mut filter.app_type, &mut filter.category] {
            if value.as_ref().is_some_and(String::is_empty) {
                *value = None;
            }
        }
        if filter
            .start_timestamp
            .into_iter()
            .chain(filter.end_timestamp)
            .any(|t| !(0..=MAX_JS_INTEGER).contains(&t))
            || matches!((filter.start_timestamp,filter.end_timestamp),(Some(start),Some(end)) if start > end)
            || (filter.view == SecurityEventView::SecurityEvents && filter.tool_verdict.is_some())
            || (filter.view == SecurityEventView::ToolCalls
                && filter.event_type == Some(SecurityEventType::Dlp))
        {
            return Err(SecurityEventError::InvalidParameters);
        }
        if let Some(category) = &filter.category {
            if !category
                .strip_prefix("dlp:")
                .or_else(|| category.strip_prefix("tool_risk:"))
                .is_some_and(|s| !s.is_empty())
            {
                return Err(SecurityEventError::InvalidParameters);
            }
        }
        Ok(filter)
    }
}

struct SourceQuery {
    predicate: String,
    values: Vec<Value>,
}
impl SourceQuery {
    fn new() -> Self {
        Self {
            predicate: "1=1".into(),
            values: Vec::new(),
        }
    }
    fn and(&mut self, clause: &str) {
        self.predicate.push_str(" AND ");
        self.predicate.push_str(clause);
    }
    fn bind(&mut self, clause: &str, value: impl Into<Value>) {
        self.and(clause);
        self.values.push(value.into());
    }
}
struct Queries {
    dlp: SourceQuery,
    tool: SourceQuery,
}
impl Queries {
    fn new(filter: &SecurityEventFilter) -> Self {
        let mut dlp = SourceQuery::new();
        let mut tool = SourceQuery::new();
        if filter.view == SecurityEventView::ToolCalls
            || filter.event_type == Some(SecurityEventType::ToolRisk)
        {
            dlp.and("0=1");
        }
        if filter.event_type == Some(SecurityEventType::Dlp) {
            tool.and("0=1");
        }
        if filter.view == SecurityEventView::SecurityEvents {
            tool.and("verdict = 'risky'");
        }
        if let Some(verdict) = filter.tool_verdict {
            tool.bind("verdict = ?", verdict.as_str().to_owned());
        }
        for query in [&mut dlp, &mut tool] {
            if let Some(start) = filter.start_timestamp {
                query.bind("timestamp >= ?", start);
            }
            if let Some(end) = filter.end_timestamp {
                query.bind("timestamp <= ?", end);
            }
        }
        if let Some(app) = &filter.app_type {
            dlp.bind("app_type = ?", app.clone());
            tool.bind("client_category = ?", app.clone());
        }
        if let Some(level) = filter.risk_level {
            if level == SecurityEventRiskLevel::High {
                dlp.and("risk_level IN ('HIGH', 'CRITICAL')");
                tool.and(&format!("({TOOL_LEVEL}) = 'HIGH'"));
            } else {
                dlp.bind("risk_level = ?", level.as_str().to_owned());
                tool.bind(&format!("({TOOL_LEVEL}) = ?"), level.as_str().to_owned());
            }
        }
        if let Some(result) = filter.result {
            dlp.bind(&format!("({DLP_RESULT}) = ?"), result.as_str().to_owned());
            tool.bind(&format!("({TOOL_RESULT}) = ?"), result.as_str().to_owned());
        }
        if let Some(category) = &filter.category {
            if let Some(value) = category.strip_prefix("dlp:") {
                dlp.bind("rule_category = ?", value.to_owned());
                tool.and("0=1");
            }
            if let Some(value) = category.strip_prefix("tool_risk:") {
                tool.bind("risk_category = ?", value.to_owned());
                dlp.and("0=1");
            }
        }
        if let Some(session) = &filter.session_id {
            dlp.bind("session_id = ?", session.clone());
            tool.and("0=1");
        }
        if let Some(keyword) = &filter.keyword {
            let escaped = keyword
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let pattern = format!("%{escaped}%");
            dlp.bind(
                "(rule_name LIKE ? ESCAPE '\\' OR matched_snippet_masked LIKE ? ESCAPE '\\')",
                pattern.clone(),
            );
            dlp.values.push(Value::Text(pattern));
            tool.and("0=1");
        }
        Self { dlp, tool }
    }
    fn index_sql(&self) -> String {
        format!("SELECT id, timestamp, 'dlp' AS event_type FROM security_audit_logs WHERE {} UNION ALL SELECT id, timestamp, 'tool_risk' AS event_type FROM tool_risk_events WHERE {}", self.dlp.predicate, self.tool.predicate)
    }
    fn values(&self) -> Vec<Value> {
        self.dlp
            .values
            .iter()
            .chain(&self.tool.values)
            .cloned()
            .collect()
    }
    fn count_sql(&self) -> String {
        format!("SELECT (SELECT COUNT(*) FROM security_audit_logs WHERE {}) + (SELECT COUNT(*) FROM tool_risk_events WHERE {})", self.dlp.predicate, self.tool.predicate)
    }
    fn summary_sql(&self) -> String {
        // Aggregate each table before UNION: never hydrate events or summaries to count.
        format!("SELECT SUM(total),SUM(dlp_count),SUM(tool_count),SUM(redacted),SUM(warnings),SUM(blocked),SUM(other) FROM (
            SELECT COUNT(*) total,COUNT(*) dlp_count,0 tool_count,
                COALESCE(SUM(action_taken='redact'),0) redacted,
                COALESCE(SUM(action_taken='audit'),0) warnings,
                COALESCE(SUM(action_taken='block'),0) blocked,
                COALESCE(SUM(action_taken NOT IN ('redact','audit','block')),0) other
            FROM security_audit_logs WHERE {} UNION ALL
            SELECT COUNT(*),0,COUNT(*),0,COUNT(*),0,0 FROM tool_risk_events WHERE {})", self.dlp.predicate, self.tool.predicate)
    }
}

fn checked_offset(page: u64, page_size: u64) -> Result<i64, SecurityEventError> {
    if page == 0 || !(1..=100).contains(&page_size) {
        return Err(SecurityEventError::InvalidParameters);
    }
    page.checked_sub(1)
        .and_then(|p| p.checked_mul(page_size))
        .and_then(|offset| i64::try_from(offset).ok())
        .ok_or(SecurityEventError::InvalidParameters)
}

impl Database {
    pub fn get_security_events(
        &self,
        filter: &SecurityEventFilter,
        page: u64,
        page_size: u64,
    ) -> Result<SecurityEventPage, SecurityEventError> {
        let offset = checked_offset(page, page_size)?;
        let queries = Queries::new(&filter.normalized()?);
        let mut conn = self
            .extension_connection()
            .lock()
            .map_err(|_| SecurityEventError::Database)?;
        #[cfg(test)]
        let _lock_timer = tests::LockTimer::start();
        let tx = conn.transaction()?;
        let values = queries.values();
        let total = tx.query_row(&queries.count_sql(), params_from_iter(&values), |row| {
            row.get(0)
        })?;
        let mut paging_values = values;
        paging_values.extend([Value::Integer(page_size as i64), Value::Integer(offset)]);
        let index = {
            let mut stmt = tx.prepare(&format!(
                "{} ORDER BY timestamp DESC, event_type ASC, id DESC LIMIT ? OFFSET ?",
                queries.index_sql()
            ))?;
            let rows = stmt.query_map(params_from_iter(&paging_values), |row| {
                Ok((row.get::<_, String>(2)?, row.get::<_, i64>(0)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut hydrated = hydrate(&tx, &index)?;
        let items = index
            .into_iter()
            .map(|(kind, id)| {
                hydrated
                    .remove(&(kind, id))
                    .ok_or(SecurityEventError::Database)
            })
            .collect::<Result<Vec<_>, _>>()?;
        tx.commit()?;
        Ok(SecurityEventPage {
            items,
            total,
            page,
            page_size,
        })
    }

    pub fn get_security_events_summary(
        &self,
        filter: &SecurityEventFilter,
    ) -> Result<SecurityEventSummary, SecurityEventError> {
        let filter = filter.normalized()?;
        if filter.view != SecurityEventView::SecurityEvents {
            return Err(SecurityEventError::InvalidParameters);
        }
        let queries = Queries::new(&filter);
        let mut conn = self
            .extension_connection()
            .lock()
            .map_err(|_| SecurityEventError::Database)?;
        #[cfg(test)]
        let _lock_timer = tests::LockTimer::start();
        let tx = conn.transaction()?;
        let result = tx.query_row(
            &queries.summary_sql(),
            params_from_iter(queries.values()),
            |row| {
                Ok(SecurityEventSummary {
                    total: row.get(0)?,
                    dlp_count: row.get(1)?,
                    tool_risk_count: row.get(2)?,
                    redacted: row.get(3)?,
                    warnings: row.get(4)?,
                    protective_blocked: row.get(5)?,
                    other: row.get(6)?,
                })
            },
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn clear_security_events(
        &self,
        filter: &SecurityEventFilter,
    ) -> Result<SecurityEventsClearResult, SecurityEventError> {
        let queries = Queries::new(&filter.normalized()?);
        let mut conn = self
            .extension_connection()
            .lock()
            .map_err(|_| SecurityEventError::Database)?;
        #[cfg(test)]
        let _lock_timer = tests::LockTimer::start();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let deleted_dlp = tx.execute(
            &format!(
                "DELETE FROM security_audit_logs WHERE {}",
                queries.dlp.predicate
            ),
            params_from_iter(&queries.dlp.values),
        )? as u64;
        let deleted_tool_risk = tx.execute(
            &format!(
                "DELETE FROM tool_risk_events WHERE {}",
                queries.tool.predicate
            ),
            params_from_iter(&queries.tool.values),
        )? as u64;
        tx.commit()?;
        Ok(SecurityEventsClearResult {
            deleted_total: deleted_dlp + deleted_tool_risk,
            deleted_dlp,
            deleted_tool_risk,
        })
    }
}

fn read_dlp(row: &Row<'_>) -> rusqlite::Result<AuditLogRecord> {
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
}
fn dlp_projection(record: AuditLogRecord) -> SecurityEvent {
    let result = match record.action_taken.as_str() {
        "redact" => SecurityEventResult::Redacted,
        "block" => SecurityEventResult::Blocked,
        "audit" => SecurityEventResult::Warning,
        "allow" => SecurityEventResult::Allowed,
        _ => SecurityEventResult::Unknown,
    };
    SecurityEvent {
        key: format!("dlp:{}", record.id),
        timestamp: record.timestamp,
        last_seen_at: record.last_seen_at,
        trigger_count: record.trigger_count,
        app_type: Some(record.app_type.clone()),
        session_id: record.session_id.clone(),
        risk_level: SecurityEventRiskLevel::from_stored(&record.risk_level),
        result,
        record: SecurityEventRecord::Dlp(record),
    }
}
fn tool_projection(record: ToolRiskEventRecord) -> SecurityEvent {
    let result = match record.verdict.as_deref() {
        Some("risky") => SecurityEventResult::Warning,
        Some("unchecked") => SecurityEventResult::Unchecked,
        Some("uncertain") => SecurityEventResult::Uncertain,
        Some("low_risk") => SecurityEventResult::LowRisk,
        _ => SecurityEventResult::Unknown,
    };
    let risk_level = if record.verdict.as_deref() == Some("risky") {
        match record.severity.as_deref() {
            Some("high") => Some(SecurityEventRiskLevel::High),
            Some("medium") => Some(SecurityEventRiskLevel::Medium),
            _ => None,
        }
    } else {
        None
    };
    SecurityEvent {
        key: format!("tool_risk:{}", record.id),
        timestamp: record.timestamp,
        last_seen_at: record.timestamp,
        trigger_count: 1,
        app_type: record.client_category.clone(),
        session_id: None,
        risk_level,
        result,
        record: SecurityEventRecord::ToolRisk(record),
    }
}
fn hydrate(
    conn: &Connection,
    index: &[(String, i64)],
) -> Result<HashMap<(String, i64), SecurityEvent>, SecurityEventError> {
    let mut result = HashMap::with_capacity(index.len());
    for kind in ["dlp", "tool_risk"] {
        let ids: Vec<_> = index
            .iter()
            .filter_map(|(source, id)| (source == kind).then_some(*id))
            .collect();
        if ids.is_empty() {
            continue;
        }
        let placeholders = vec!["?"; ids.len()].join(",");
        let (table, columns) = if kind == "dlp" {
            ("security_audit_logs", DLP_COLUMNS)
        } else {
            ("tool_risk_events", EVENT_COLUMNS)
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT {columns} FROM {table} WHERE id IN ({placeholders})"
        ))?;
        let rows = stmt.query_map(params_from_iter(&ids), |row| {
            if kind == "dlp" {
                read_dlp(row).map(|record| (record.id, dlp_projection(record)))
            } else {
                read_event(row).map(|record| (record.id, tool_projection(record)))
            }
        })?;
        for row in rows {
            let (id, event) = row?;
            result.insert((kind.to_owned(), id), event);
        }
    }
    Ok(result)
}

#[cfg(test)]
#[path = "security_events_tests.rs"]
mod tests;
