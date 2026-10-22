use super::{Database, SCHEMA_VERSION};
use crate::error::AppError;
use rusqlite::{hooks::AuthAction, hooks::Authorization, params, types::Value, Connection};

fn v23_fixture() -> Result<Connection, AppError> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE TABLE tool_risk_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO tool_risk_settings VALUES ('enabled', 'true'), ('policy_version', '17');
         CREATE TABLE tool_risk_custom_rules (
            id TEXT PRIMARY KEY, revision INTEGER NOT NULL, name TEXT NOT NULL,
            definition TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL
         );
         INSERT INTO tool_risk_custom_rules VALUES
            ('legacy-rule', 3, 'Legacy rule', '{\"condition\":\"SECRET_V23_CONDITION\"}', 1, 123);
         CREATE TABLE tool_risk_builtin_overrides (
            id TEXT PRIMARY KEY, enabled INTEGER NOT NULL, updated_at INTEGER NOT NULL
         );
         INSERT INTO tool_risk_builtin_overrides VALUES ('builtin-rule', 0, 124);
         CREATE TABLE tool_risk_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT, timestamp INTEGER NOT NULL,
            source TEXT NOT NULL, stage TEXT NOT NULL, client_category TEXT,
            operation_category TEXT, detection_scope TEXT NOT NULL,
            integrity_status TEXT NOT NULL, verdict TEXT, risk_category TEXT,
            severity TEXT, reason_code TEXT, policy_version INTEGER NOT NULL,
            rule_ids TEXT, command_summary TEXT,
            suppressed_count INTEGER NOT NULL DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
         );
         INSERT INTO tool_risk_events (
            id, timestamp, source, stage, detection_scope, integrity_status, verdict,
            policy_version, command_summary, suppressed_count, created_at
         ) VALUES (42, 456, 'claude', 'response', 'tool', 'complete', 'risky',
            17, 'git status', 2, '2026-01-02 03:04:05');
         CREATE TABLE tool_risk_command_lists (
            id TEXT PRIMARY KEY, definition TEXT NOT NULL, updated_at INTEGER NOT NULL
         );
         INSERT INTO tool_risk_command_lists VALUES
            ('legacy-list', '{\"pattern\":\"SECRET_V23_PATTERN\"}', 789);
         PRAGMA user_version = 23;",
    )?;
    Ok(conn)
}

fn policy_rows(conn: &Connection) -> Result<Vec<Vec<Vec<Value>>>, AppError> {
    let mut tables = Vec::new();
    for table in [
        "tool_risk_settings",
        "tool_risk_custom_rules",
        "tool_risk_builtin_overrides",
        "tool_risk_events",
        "tool_risk_command_lists",
    ] {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {table} ORDER BY 1"))?;
        let column_count = stmt.column_count();
        let rows = stmt
            .query_map([], |row| {
                (0..column_count).map(|column| row.get(column)).collect()
            })?
            .collect::<Result<_, _>>()?;
        tables.push(rows);
    }
    Ok(tables)
}

fn insert_change(conn: &Connection) -> Result<i64, AppError> {
    conn.execute(
        "INSERT INTO tool_risk_policy_changes
            (at, policy_version, actor, kind, target_id, changed_fields, audit_status)
         VALUES (1234, 18, 'local_ui', 'settings_update', NULL, '[\"enabled\"]', 'recorded')",
        [],
    )?;
    Ok(conn.last_insert_rowid())
}

#[test]
fn v24_migration_preserves_v23_policy_events_and_lists() -> Result<(), AppError> {
    let conn = v23_fixture()?;
    let before = policy_rows(&conn)?;
    Database::apply_schema_migrations_on_conn(&conn)?;
    assert_eq!(Database::get_user_version(&conn)?, 24);
    assert_eq!(SCHEMA_VERSION, 24);
    assert_eq!(policy_rows(&conn)?, before);
    assert!(Database::table_exists(&conn, "tool_risk_policy_changes")?);
    let count: i64 =
        conn.query_row("SELECT COUNT(*) FROM tool_risk_policy_changes", [], |row| {
            row.get(0)
        })?;
    assert_eq!(count, 0, "迁移不能伪造历史审计");
    Ok(())
}

#[test]
fn v24_fresh_database_has_exact_audit_columns_and_autoincrement() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = super::lock_conn!(db.conn);
    let columns: Vec<(String, String, i64, i64)> = conn
        .prepare("PRAGMA table_info(tool_risk_policy_changes)")?
        .query_map([], |row| {
            Ok((row.get(1)?, row.get(2)?, row.get(3)?, row.get(5)?))
        })?
        .collect::<Result<_, _>>()?;
    let expected = [
        ("id", "INTEGER", 0, 1),
        ("at", "INTEGER", 1, 0),
        ("policy_version", "INTEGER", 1, 0),
        ("actor", "TEXT", 1, 0),
        ("kind", "TEXT", 1, 0),
        ("target_id", "TEXT", 0, 0),
        ("changed_fields", "TEXT", 1, 0),
        ("audit_status", "TEXT", 1, 0),
    ]
    .map(|(name, ty, required, pk)| (name.to_string(), ty.to_string(), required, pk));
    assert_eq!(columns, expected);
    let first_id = insert_change(&conn)?;
    conn.execute("DELETE FROM tool_risk_policy_changes", [])?;
    assert!(insert_change(&conn)? > first_id, "审计 ID 不可复用");
    Database::apply_schema_migrations_on_conn(&conn)?;
    assert_eq!(Database::get_user_version(&conn)?, 24);
    Ok(())
}

#[test]
fn v24_audit_requires_metadata_and_fixed_enums() -> Result<(), AppError> {
    let conn = v23_fixture()?;
    Database::apply_schema_migrations_on_conn(&conn)?;
    let sql = "INSERT INTO tool_risk_policy_changes
        (at, policy_version, actor, kind, changed_fields, audit_status)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)";
    for actor in ["local_ui", "system", "unknown"] {
        for kind in [
            "settings_update",
            "rule_create",
            "rule_update",
            "rule_delete",
            "builtin_toggle",
            "builtin_reset",
            "list_create",
            "list_update",
            "list_delete",
        ] {
            for status in ["recorded", "audit_unavailable", "unknown"] {
                conn.execute(sql, params![1, 18, actor, kind, "[]", status])?;
            }
        }
    }
    for values in [
        params![
            1,
            18,
            "arbitrary_actor",
            "settings_update",
            "[]",
            "recorded"
        ],
        params![1, 18, "local_ui", "arbitrary_kind", "[]", "recorded"],
        params![
            1,
            18,
            "local_ui",
            "settings_update",
            "[]",
            "arbitrary_status"
        ],
        params![
            None::<i64>,
            18,
            "local_ui",
            "settings_update",
            "[]",
            "recorded"
        ],
        params![
            1,
            None::<i64>,
            "local_ui",
            "settings_update",
            "[]",
            "recorded"
        ],
        params![1, 18, None::<String>, "settings_update", "[]", "recorded"],
        params![1, 18, "local_ui", None::<String>, "[]", "recorded"],
        params![
            1,
            18,
            "local_ui",
            "settings_update",
            None::<String>,
            "recorded"
        ],
        params![1, 18, "local_ui", "settings_update", "[]", None::<String>],
    ] {
        assert!(conn.execute(sql, values).is_err());
    }
    Ok(())
}

#[test]
fn v24_repeated_migration_keeps_existing_audit_and_policy() -> Result<(), AppError> {
    let conn = v23_fixture()?;
    Database::apply_schema_migrations_on_conn(&conn)?;
    let id = insert_change(&conn)?;
    let before = policy_rows(&conn)?;
    Database::apply_schema_migrations_on_conn(&conn)?;
    assert_eq!(Database::get_user_version(&conn)?, 24);
    assert_eq!(policy_rows(&conn)?, before);
    let row: (i64, i64, String) = conn.query_row(
        "SELECT id, policy_version, changed_fields FROM tool_risk_policy_changes",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(row, (id, 18, "[\"enabled\"]".into()));
    Ok(())
}

#[test]
fn v24_failed_migration_rolls_back_table_and_version_with_savepoint() -> Result<(), AppError> {
    for nested in [false, true] {
        let conn = v23_fixture()?;
        if nested {
            conn.execute_batch(
                "BEGIN; UPDATE tool_risk_settings SET value = 'false' WHERE key = 'enabled';",
            )?;
        }
        let before = policy_rows(&conn)?;

        conn.authorizer(Some(
            |context: rusqlite::hooks::AuthContext<'_>| match context.action {
                AuthAction::Pragma {
                    pragma_name: "user_version",
                    pragma_value: Some("24"),
                } => Authorization::Deny,
                _ => Authorization::Allow,
            },
        ));
        let result = Database::apply_schema_migrations_on_conn(&conn);
        conn.authorizer(None::<fn(rusqlite::hooks::AuthContext<'_>) -> Authorization>);
        assert!(result.is_err(), "故障注入必须使迁移失败");
        assert_eq!(Database::get_user_version(&conn)?, 23);
        assert!(!Database::table_exists(&conn, "tool_risk_policy_changes")?);
        assert_eq!(policy_rows(&conn)?, before);
        assert_eq!(conn.is_autocommit(), !nested, "不得撤销调用方事务");

        Database::apply_schema_migrations_on_conn(&conn)?;
        assert_eq!(Database::get_user_version(&conn)?, 24);
        if nested {
            conn.execute_batch("ROLLBACK;")?;
            assert_eq!(Database::get_user_version(&conn)?, 23);
            assert!(!Database::table_exists(&conn, "tool_risk_policy_changes")?);
        }
    }
    Ok(())
}

#[test]
fn v24_future_schema_is_rejected_without_mutation() -> Result<(), AppError> {
    let conn = v23_fixture()?;
    conn.execute_batch("PRAGMA user_version = 25;")?;
    let before = policy_rows(&conn)?;
    let error = Database::apply_schema_migrations_on_conn(&conn).unwrap_err();
    assert!(error.to_string().contains("数据库版本过新"));
    assert_eq!(Database::get_user_version(&conn)?, 25);
    assert!(!Database::table_exists(&conn, "tool_risk_policy_changes")?);
    assert_eq!(policy_rows(&conn)?, before);
    assert!(conn.is_autocommit());
    Ok(())
}
