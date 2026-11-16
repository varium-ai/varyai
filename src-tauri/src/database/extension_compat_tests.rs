//! Conflicting historical extension versions are identified by schema, not rows.
use super::{lock_conn, Database, EXTENSION_SCHEMA_VERSION};
use crate::error::AppError;
use rusqlite::Connection;
use std::path::Path;

#[test]
fn v7_tool_session_migration_preserves_both_storage_layouts_and_old_events(
) -> Result<(), Box<dyn std::error::Error>> {
    for native in [false, true] {
        let directory = tempfile::tempdir()?;
        let path = directory
            .path()
            .join(if native { "varyai.db" } else { "cc-switch.db" });
        let open = || {
            if native {
                Database::init_native_at(&path)
            } else {
                Database::init_at(&path)
            }
        };
        let db = open()?;
        db.set_setting("synthetic-session-migration", "preserved")?;
        {
            let conn = lock_conn!(db.extension_connection());
            conn.execute("INSERT INTO intent_routing_preferences VALUES ('synthetic-session-migration','preserved')", [])?;
            conn.execute("INSERT INTO tool_risk_events(timestamp,source,stage,detection_scope,integrity_status,policy_version) VALUES (1000,'model_response','proposed','in_scope','complete',1)", [])?;
        }
        drop(db);
        let extension_path = Database::extension_path(&path);
        let extension = Connection::open(&extension_path)?;
        extension.execute_batch("DROP INDEX IF EXISTS idx_tool_risk_events_session_time; ALTER TABLE tool_risk_events DROP COLUMN session_id; PRAGMA user_version=7;")?;
        drop(extension);
        let db = open()?;
        assert_eq!(
            db.get_setting("synthetic-session-migration")?.as_deref(),
            Some("preserved")
        );
        let events = db.get_tool_risk_events(10, 0)?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id, None);
        {
            let conn = lock_conn!(db.extension_connection());
            assert_eq!(Database::get_user_version(&conn)?, EXTENSION_SCHEMA_VERSION);
            assert_eq!(conn.query_row("SELECT value FROM intent_routing_preferences WHERE key='synthetic-session-migration'", [], |row| row.get::<_, String>(0))?, "preserved");
        }
        drop(db);
        let backups = || {
            std::fs::read_dir(directory.path().join("backups/storage-split"))
                .map(|entries| entries.count())
        };
        let count = backups()?;
        assert!(count >= 2);
        drop(open()?);
        assert_eq!(backups()?, count);
    }
    Ok(())
}

fn intent_extension(path: &Path, version: i32) -> Result<(), AppError> {
    let db = Database::init_at(path)?;
    drop(db);
    let conn = Connection::open(path.with_file_name("vary-ai.db"))?;
    conn.execute_batch(
        "DROP TABLE gateway_agent_bindings;
         DROP TABLE gateway_usage_details;
         DROP TABLE gateway_usage_models;
         DROP TABLE routing_upstreams;
         DROP TABLE IF EXISTS routing_upstream_origins;
         DROP TABLE IF EXISTS intent_routing_groups;
         DROP TABLE IF EXISTS intent_routing_preferences;
         CREATE TABLE intent_routing_groups (
            id TEXT PRIMARY KEY, revision INTEGER NOT NULL, config TEXT NOT NULL);
         INSERT INTO gateway_keys VALUES ('synthetic-key','synthetic-hash','{}',0);
         INSERT INTO gateway_usage VALUES ('synthetic-key',0,31);
         INSERT INTO tool_risk_settings VALUES ('policy_version','9');",
    )?;
    let group = serde_json::json!({
        "id":"synthetic-group", "name":"Synthetic group", "revision":7,
        "client":"codex", "enabled":false,
        "members":[{"id":"member","providerId":"synthetic-provider","modelId":"synthetic-model"}],
        "defaultMemberId":"member", "fallbackMemberIds":[],
        "intentRecognitionEnabled":false, "classifierTarget":null,
        "classificationBudgetMs":2000, "rules":[]
    });
    conn.execute(
        "INSERT INTO intent_routing_groups VALUES ('synthetic-group',7,?1)",
        [group.to_string()],
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

fn assert_intent_data(db: &Database, _version: i32) -> Result<(), AppError> {
    let conn = lock_conn!(db.extension_connection());
    assert_eq!(Database::get_user_version(&conn)?, EXTENSION_SCHEMA_VERSION);
    let groups: i64 = conn.query_row("SELECT COUNT(*) FROM intent_routing_groups", [], |r| {
        r.get(0)
    })?;
    assert_eq!(groups, 0);
    let prefs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM intent_routing_preferences WHERE key='synthetic-preference'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(prefs, 0);
    let tokens: i64 = conn.query_row(
        "SELECT tokens FROM gateway_usage WHERE key_id='synthetic-key'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(tokens, 31);
    assert!(Database::table_exists(&conn, "gateway_agent_bindings")?);
    assert!(Database::table_exists(&conn, "gateway_usage_details")?);
    assert!(Database::table_exists(&conn, "gateway_usage_models")?);
    assert!(Database::table_exists(&conn, "routing_upstreams")?);
    assert!(!Database::table_exists(&conn, "routing_upstream_origins")?);
    drop(conn);
    assert_eq!(db.get_tool_risk_settings()?.policy_version, 9);
    assert_eq!(
        db.list_intent_routing_groups()
            .map_err(AppError::Database)?
            .len(),
        0
    );
    Ok(())
}

#[test]
fn historical_intent_extensions_reset_routing_but_preserve_unrelated_data() -> Result<(), AppError>
{
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
        assert!(!Database::table_exists(&backup, "routing_upstreams")?);
        assert!(!Database::table_exists(
            &backup,
            "routing_upstream_origins"
        )?);
    }
    Ok(())
}

#[test]
fn reset_extension_survives_portable_sql_and_binary_restore() -> Result<(), AppError> {
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
             DROP TABLE routing_upstreams;
             DROP TABLE IF EXISTS routing_upstream_origins;
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
        assert!(Database::table_exists(&ext, "routing_upstreams")?);
        assert!(!Database::table_exists(&ext, "routing_upstream_origins")?);
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
