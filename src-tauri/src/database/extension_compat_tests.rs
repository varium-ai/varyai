//! Conflicting historical extension versions are identified by schema, not rows.
use super::{lock_conn, Database, EXTENSION_SCHEMA_VERSION};
use crate::error::AppError;
use rusqlite::Connection;
use std::path::Path;

fn intent_extension(path: &Path, version: i32) -> Result<(), AppError> {
    let db = Database::init_at(path)?;
    drop(db);
    let conn = Connection::open(path.with_file_name("vary-ai.db"))?;
    conn.execute_batch(
        "DROP TABLE gateway_agent_bindings;
         DROP TABLE gateway_usage_details;
         DROP TABLE gateway_usage_models;
         DROP TABLE IF EXISTS intent_routing_groups;
         DROP TABLE IF EXISTS intent_routing_preferences;
         CREATE TABLE intent_routing_groups (
            id TEXT PRIMARY KEY, revision INTEGER NOT NULL, config TEXT NOT NULL);
         INSERT INTO intent_routing_groups VALUES ('synthetic-group',7,'{}');
         INSERT INTO gateway_keys VALUES ('synthetic-key','synthetic-hash','{}',0);
         INSERT INTO gateway_usage VALUES ('synthetic-key',0,31);
         INSERT INTO tool_risk_settings VALUES ('policy_version','9');",
    )?;
    if version == 4 {
        conn.execute_batch(
            "CREATE TABLE intent_routing_preferences (key TEXT PRIMARY KEY,value TEXT NOT NULL);
             INSERT INTO intent_routing_preferences VALUES ('synthetic-preference','true');",
        )?;
    }
    Database::set_user_version(&conn, version)?;
    Ok(())
}

fn assert_intent_data(db: &Database, version: i32) -> Result<(), AppError> {
    let conn = lock_conn!(db.extension_connection());
    assert_eq!(Database::get_user_version(&conn)?, EXTENSION_SCHEMA_VERSION);
    let revision: i64 = conn.query_row(
        "SELECT revision FROM intent_routing_groups WHERE id='synthetic-group'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(revision, 7);
    let prefs: i64 =
        conn.query_row("SELECT COUNT(*) FROM intent_routing_preferences", [], |r| {
            r.get(0)
        })?;
    assert_eq!(prefs, i64::from(version == 4));
    let tokens: i64 = conn.query_row(
        "SELECT tokens FROM gateway_usage WHERE key_id='synthetic-key'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(tokens, 31);
    assert!(Database::table_exists(&conn, "gateway_agent_bindings")?);
    assert!(Database::table_exists(&conn, "gateway_usage_details")?);
    assert!(Database::table_exists(&conn, "gateway_usage_models")?);
    drop(conn);
    assert_eq!(db.get_tool_risk_settings()?.policy_version, 9);
    Ok(())
}

#[test]
fn historical_intent_extensions_upgrade_without_losing_data() -> Result<(), AppError> {
    for version in [3, 4] {
        let dir = tempfile::tempdir().expect("temporary storage");
        let path = dir.path().join("cc-switch.db");
        intent_extension(&path, version)?;
        assert!(Database::stored_user_version_exceeds_supported(&path)?.is_none());
        let db = Database::init_at(&path)?;
        assert_intent_data(&db, version)?;
        drop(db);
        let db = Database::init_at(&path)?;
        assert_intent_data(&db, version)?;
        let backups = std::fs::read_dir(dir.path().join("backups/storage-split"))
            .expect("migration backups")
            .collect::<Result<Vec<_>, _>>()
            .expect("backup entries");
        assert_eq!(backups.len(), 2);
        let extension_backup = backups
            .iter()
            .find(|entry| entry.file_name().to_string_lossy().starts_with("vary-ai"))
            .expect("extension backup");
        let backup = Connection::open(extension_backup.path())?;
        assert_eq!(Database::get_user_version(&backup)?, version);
        assert!(!Database::table_exists(&backup, "gateway_agent_bindings")?);
    }
    Ok(())
}

#[test]
fn historical_intent_data_survives_portable_sql_and_binary_restore() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    intent_extension(&path, 4)?;
    let db = Database::init_at(&path)?;
    let sql = db.export_sql_string()?;
    let target_dir = tempfile::tempdir().expect("temporary storage");
    let target = Database::init_at(&target_dir.path().join("cc-switch.db"))?;
    target.import_sql_string(&sql)?;
    assert_intent_data(&target, 4)?;
    let backup = target.backup_database_file()?.expect("backup");
    {
        let conn = lock_conn!(target.extension_connection());
        conn.execute("DELETE FROM intent_routing_groups", [])?;
    }
    target.restore_from_backup(
        backup
            .file_name()
            .and_then(|s| s.to_str())
            .expect("backup filename"),
    )?;
    assert_intent_data(&target, 4)?;
    Ok(())
}

#[test]
fn malformed_or_partial_intent_extensions_are_preserved() -> Result<(), AppError> {
    for mutation in [
        "ALTER TABLE intent_routing_groups RENAME COLUMN config TO unknown_config;",
        "DROP TABLE intent_routing_preferences;",
        "DROP TABLE intent_routing_groups;",
    ] {
        let dir = tempfile::tempdir().expect("temporary storage");
        let path = dir.path().join("cc-switch.db");
        intent_extension(&path, 4)?;
        let ext_path = path.with_file_name("vary-ai.db");
        let conn = Connection::open(&ext_path)?;
        conn.execute_batch(mutation)?;
        drop(conn);
        let before_shared = std::fs::read(&path).expect("read synthetic shared file");
        let before_ext = std::fs::read(&ext_path).expect("read synthetic extension file");
        assert!(Database::init_at(&path).is_err());
        assert_eq!(
            std::fs::read(&path).expect("read synthetic shared file"),
            before_shared
        );
        assert_eq!(
            std::fs::read(&ext_path).expect("read synthetic extension file"),
            before_ext
        );
    }
    Ok(())
}

#[test]
fn historical_gateway_extensions_upgrade_to_the_combined_schema() -> Result<(), AppError> {
    for version in [3, 4] {
        let dir = tempfile::tempdir().expect("temporary storage");
        let path = dir.path().join("cc-switch.db");
        let db = Database::init_at(&path)?;
        drop(db);
        let ext = Connection::open(path.with_file_name("vary-ai.db"))?;
        ext.execute_batch(
            "DROP TABLE intent_routing_groups;
             DROP TABLE intent_routing_preferences;
             INSERT INTO gateway_keys VALUES ('synthetic-key','synthetic-hash','{}',0);
             INSERT INTO gateway_usage VALUES ('synthetic-key',0,31);
             INSERT INTO gateway_agent_bindings VALUES ('synthetic-app','synthetic-key');",
        )?;
        if version == 3 {
            ext.execute("DROP TABLE gateway_usage_models", [])?;
        }
        Database::set_user_version(&ext, version)?;
        drop(ext);
        let db = Database::init_at(&path)?;
        let ext = lock_conn!(db.extension_connection());
        assert_eq!(Database::get_user_version(&ext)?, EXTENSION_SCHEMA_VERSION);
        let tokens: i64 = ext.query_row("SELECT tokens FROM gateway_usage", [], |r| r.get(0))?;
        assert_eq!(tokens, 31);
        let bindings: i64 =
            ext.query_row("SELECT COUNT(*) FROM gateway_agent_bindings", [], |r| {
                r.get(0)
            })?;
        assert_eq!(bindings, 1);
        assert!(Database::table_exists(&ext, "intent_routing_groups")?);
        assert!(Database::table_exists(&ext, "intent_routing_preferences")?);
        assert!(Database::table_exists(&ext, "gateway_usage_models")?);
    }
    Ok(())
}

#[test]
fn intent_extension_backup_failure_leaves_both_files_unchanged() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    intent_extension(&path, 4)?;
    let ext_path = path.with_file_name("vary-ai.db");
    let before_shared = std::fs::read(&path).expect("read synthetic shared file");
    let before_ext = std::fs::read(&ext_path).expect("read synthetic extension file");
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).expect("backup directory");
    std::fs::write(backups.join("storage-split"), b"synthetic blocker").expect("backup blocker");
    assert!(Database::init_at(&path).is_err());
    assert_eq!(
        std::fs::read(&path).expect("read synthetic shared file"),
        before_shared
    );
    assert_eq!(
        std::fs::read(&ext_path).expect("read synthetic extension file"),
        before_ext
    );
    Ok(())
}
