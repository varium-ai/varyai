use super::{Database, SCHEMA_VERSION};
use crate::error::AppError;
use rusqlite::Connection;

fn v22_fixture() -> Result<Connection, AppError> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE TABLE tool_risk_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO tool_risk_settings VALUES ('enabled', 'true'), ('policy_version', '7');
         CREATE TABLE tool_risk_custom_rules (
            id TEXT PRIMARY KEY, revision INTEGER NOT NULL, name TEXT NOT NULL,
            definition TEXT NOT NULL, enabled INTEGER NOT NULL, updated_at INTEGER NOT NULL
         );
         INSERT INTO tool_risk_custom_rules VALUES ('legacy', 2, 'Legacy', '{}', 1, 123);
         CREATE TABLE tool_risk_events (id INTEGER PRIMARY KEY, command_summary TEXT);
         INSERT INTO tool_risk_events VALUES (9, 'git status');
         PRAGMA user_version = 22;",
    )?;
    Ok(conn)
}

#[test]
fn v23_migration_creates_empty_command_lists_without_changing_existing_policy(
) -> Result<(), AppError> {
    let conn = v22_fixture()?;
    Database::apply_schema_migrations_on_conn(&conn)?;

    assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
    assert!(SCHEMA_VERSION >= 23);
    assert!(Database::table_exists(&conn, "tool_risk_command_lists")?);
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM tool_risk_command_lists", [], |row| {
        row.get(0)
    })?;
    assert_eq!(count, 0);
    let version: String = conn.query_row(
        "SELECT value FROM tool_risk_settings WHERE key = 'policy_version'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(version, "7");
    let summary: String = conn.query_row(
        "SELECT command_summary FROM tool_risk_events WHERE id = 9",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(summary, "git status");
    let rule: (String, i64) = conn.query_row(
        "SELECT id, revision FROM tool_risk_custom_rules",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(rule, ("legacy".into(), 2));
    conn.execute(
        "INSERT INTO tool_risk_command_lists VALUES ('shell-list-test', '{}', 456)",
        [],
    )?;
    Database::apply_schema_migrations_on_conn(&conn)?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM tool_risk_command_lists", [], |row| {
        row.get(0)
    })?;
    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn v23_fresh_database_has_independent_command_list_schema() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = db.conn.lock().unwrap();
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(tool_risk_command_lists)")?
        .query_map([], |row| row.get(1))?
        .collect::<Result<_, _>>()?;
    assert_eq!(columns, ["id", "definition", "updated_at"]);
    assert!(conn
        .execute(
            "INSERT INTO tool_risk_command_lists (id, updated_at) VALUES ('bad', 1)",
            []
        )
        .is_err());
    Ok(())
}
