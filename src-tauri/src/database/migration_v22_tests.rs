//! Historical snapshots of both branches: fixtures must not call the migrations under test.

use super::{Database, SCHEMA_VERSION};
use crate::error::AppError;
use rusqlite::Connection;

pub(super) fn historical_fixture(
    version: i32,
    local_tool_risk: bool,
) -> Result<Connection, AppError> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE TABLE security_audit_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            request_id TEXT,
            app_type TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            rule_id TEXT NOT NULL,
            rule_name TEXT NOT NULL,
            rule_category TEXT NOT NULL,
            risk_level TEXT NOT NULL,
            action_taken TEXT NOT NULL,
            matched_snippet_masked TEXT NOT NULL,
            location TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE security_custom_rules (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, rule_type TEXT NOT NULL,
            pattern TEXT NOT NULL, risk_level TEXT NOT NULL, action TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1, description TEXT, updated_at INTEGER NOT NULL
        );
        CREATE TABLE security_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT INTO security_settings VALUES ('enabled', 'true');
        INSERT INTO security_custom_rules VALUES
            ('old-rule', 'Rule', 'keyword', 'private', 'HIGH', 'audit', 1, NULL, 900);
        INSERT INTO security_audit_logs (
            id, timestamp, request_id, app_type, provider_id, rule_id, rule_name,
            rule_category, risk_level, action_taken, matched_snippet_masked, created_at
        ) VALUES (42, 1234, 'request', 'claude', 'provider', 'old-rule', 'Rule',
            'custom', 'HIGH', 'audit', '***', '2026-01-02 03:04:05');",
    )?;
    if !local_tool_risk && version >= 20 {
        conn.execute_batch(
            "ALTER TABLE security_audit_logs ADD COLUMN trigger_count INTEGER NOT NULL DEFAULT 1;
             ALTER TABLE security_audit_logs ADD COLUMN last_seen_at INTEGER;
             UPDATE security_audit_logs SET trigger_count = 7, last_seen_at = 5678;",
        )?;
        if version >= 21 {
            conn.execute_batch(
                "ALTER TABLE security_audit_logs ADD COLUMN session_id TEXT;
                 UPDATE security_audit_logs SET session_id = 'old-session';
                 CREATE INDEX idx_security_audit_session_time
                    ON security_audit_logs(session_id, timestamp DESC);",
            )?;
        }
    }
    if local_tool_risk && version >= 20 {
        conn.execute_batch(
            "CREATE TABLE tool_risk_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE tool_risk_custom_rules (
                id TEXT PRIMARY KEY, revision INTEGER NOT NULL, name TEXT NOT NULL,
                definition TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL
             );
             CREATE TABLE tool_risk_builtin_overrides (
                id TEXT PRIMARY KEY, enabled INTEGER NOT NULL, updated_at INTEGER NOT NULL
             );
             CREATE TABLE tool_risk_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT, timestamp INTEGER NOT NULL,
                source TEXT NOT NULL, stage TEXT NOT NULL, client_category TEXT,
                operation_category TEXT, detection_scope TEXT NOT NULL,
                integrity_status TEXT NOT NULL, verdict TEXT, risk_category TEXT,
                severity TEXT, reason_code TEXT, policy_version INTEGER NOT NULL,
                rule_ids TEXT, suppressed_count INTEGER NOT NULL DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
             );
             INSERT INTO tool_risk_settings VALUES ('enabled', 'true'), ('policy_version', '9');
             INSERT INTO tool_risk_custom_rules VALUES
                ('tool-rule', 3, 'Tool Rule', '{}', 1, 901);
             INSERT INTO tool_risk_builtin_overrides VALUES ('builtin-rule', 0, 902);
             INSERT INTO tool_risk_events (
                id, timestamp, source, stage, client_category, operation_category,
                detection_scope, integrity_status, verdict, policy_version, rule_ids,
                suppressed_count, created_at
             ) VALUES (73, 2222, 'claude', 'response', 'claude', 'shell', 'tool',
                'complete', 'warn', 9, '[\"tool-rule\"]', 4, '2026-02-03 04:05:06');",
        )?;
        if version >= 21 {
            conn.execute_batch(
                "ALTER TABLE tool_risk_events ADD COLUMN command_summary TEXT;
                 UPDATE tool_risk_events SET command_summary = 'SECRET_LEGACY_COMMAND';",
            )?;
        }
    }
    Database::set_user_version(&conn, version)?;
    Ok(conn)
}

pub(super) fn assert_v22_state(
    conn: &Connection,
    version: i32,
    local_tool_risk: bool,
) -> Result<(), AppError> {
    assert_eq!(Database::get_user_version(conn)?, SCHEMA_VERSION);
    assert!(SCHEMA_VERSION >= 22);
    for table in [
        "security_custom_rules",
        "security_settings",
        "tool_risk_settings",
        "tool_risk_custom_rules",
        "tool_risk_builtin_overrides",
        "tool_risk_events",
    ] {
        assert!(Database::table_exists(conn, table)?, "missing {table}");
    }
    for column in ["trigger_count", "last_seen_at", "session_id"] {
        assert!(
            Database::has_column(conn, "security_audit_logs", column)?,
            "missing {column}"
        );
    }
    assert!(Database::has_column(
        conn,
        "tool_risk_events",
        "command_summary"
    )?);
    for index in [
        "idx_security_audit_session_time",
        "idx_tool_risk_events_timestamp",
        "idx_tool_risk_events_verdict",
    ] {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1)",
            [index],
            |row| row.get(0),
        )?;
        assert!(exists, "missing {index}");
    }
    let row: (i64, i64, i64, i64, Option<String>, String) = conn.query_row(
        "SELECT id, timestamp, trigger_count, last_seen_at, session_id, created_at
         FROM security_audit_logs",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    )?;
    let counted = !local_tool_risk && version >= 20;
    let session = (!local_tool_risk && version >= 21).then(|| "old-session".into());
    assert_eq!(
        row,
        (
            42,
            1234,
            if counted { 7 } else { 1 },
            if counted { 5678 } else { 1234 },
            session,
            "2026-01-02 03:04:05".into()
        )
    );
    let rule: (String, i64) = conn.query_row(
        "SELECT id, updated_at FROM security_custom_rules",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(rule, ("old-rule".into(), 900));
    if local_tool_risk && version >= 20 {
        let row: (i64, i64, i64, i64, String, Option<String>) = conn.query_row(
            "SELECT id, timestamp, policy_version, suppressed_count, created_at, command_summary
             FROM tool_risk_events",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        assert_eq!(row, (73, 2222, 9, 4, "2026-02-03 04:05:06".into(), None));
        let rule: (String, i64, i64) = conn.query_row(
            "SELECT id, revision, updated_at FROM tool_risk_custom_rules",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(rule, ("tool-rule".into(), 3, 901));
        let settings: String = conn.query_row(
            "SELECT value FROM tool_risk_settings WHERE key = 'policy_version'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(settings, "9");
        let override_row: (String, i64, i64) = conn.query_row(
            "SELECT id, enabled, updated_at FROM tool_risk_builtin_overrides",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(override_row, ("builtin-rule".into(), 0, 902));
    }
    Ok(())
}

fn migrate_fixture(version: i32, local_tool_risk: bool) -> Result<(), AppError> {
    // Exercise both direct migration and startup/import's create-tables-first order.
    for create_tables_first in [false, true] {
        let conn = historical_fixture(version, local_tool_risk)?;
        if create_tables_first {
            Database::create_tables_on_conn(&conn)?;
        }
        Database::apply_schema_migrations_on_conn(&conn)?;
        assert_v22_state(&conn, version, local_tool_risk)?;
        Database::apply_schema_migrations_on_conn(&conn)?;
        assert_v22_state(&conn, version, local_tool_risk)?;
    }
    Ok(())
}

#[test]
fn v22_migrates_shared_v19() -> Result<(), AppError> {
    migrate_fixture(19, false)
}
#[test]
fn v22_migrates_upstream_v20() -> Result<(), AppError> {
    migrate_fixture(20, false)
}
#[test]
fn v22_migrates_upstream_v21() -> Result<(), AppError> {
    migrate_fixture(21, false)
}
#[test]
fn v22_migrates_local_v20() -> Result<(), AppError> {
    migrate_fixture(20, true)
}
#[test]
fn v22_migrates_local_v21_and_erases_legacy_summaries() -> Result<(), AppError> {
    migrate_fixture(21, true)
}

#[test]
fn v22_rolls_back_if_summary_scrub_fails() -> Result<(), AppError> {
    let conn = historical_fixture(21, true)?;
    conn.execute_batch(
        "CREATE TRIGGER reject_summary_scrub BEFORE UPDATE OF command_summary ON tool_risk_events
         BEGIN SELECT RAISE(ABORT, 'scrub rejected'); END;",
    )?;
    assert!(Database::apply_schema_migrations_on_conn(&conn).is_err());
    assert_eq!(Database::get_user_version(&conn)?, 21);
    assert!(!Database::has_column(
        &conn,
        "security_audit_logs",
        "trigger_count"
    )?);
    let summary: String =
        conn.query_row("SELECT command_summary FROM tool_risk_events", [], |row| {
            row.get(0)
        })?;
    assert_eq!(summary, "SECRET_LEGACY_COMMAND");
    conn.execute_batch("DROP TRIGGER reject_summary_scrub;")?;
    Database::apply_schema_migrations_on_conn(&conn)?;
    assert_v22_state(&conn, 21, true)
}

#[test]
fn v22_rolls_back_if_tool_risk_index_creation_fails() -> Result<(), AppError> {
    let conn = historical_fixture(21, false)?;
    conn.execute_batch("CREATE TABLE idx_tool_risk_events_timestamp (id INTEGER);")?;
    assert!(Database::apply_schema_migrations_on_conn(&conn).is_err());
    assert_eq!(Database::get_user_version(&conn)?, 21);
    assert!(!Database::table_exists(&conn, "tool_risk_events")?);
    conn.execute_batch("DROP TABLE idx_tool_risk_events_timestamp;")?;
    Database::apply_schema_migrations_on_conn(&conn)?;
    assert_v22_state(&conn, 21, false)
}
