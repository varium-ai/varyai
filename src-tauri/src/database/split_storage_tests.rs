//! File-backed tests exercise the same two-file storage used at startup.
use super::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::Connection;

fn legacy_database(path: &std::path::Path, version: i32) -> Result<(), AppError> {
    let conn = Connection::open(path)?;
    Database::create_tables_on_conn(&conn)?;
    conn.execute_batch(
        "INSERT INTO providers (id, app_type, name, settings_config) VALUES ('kept', 'claude', 'Kept', '{}');
         INSERT INTO security_settings VALUES ('enabled', 'false');
         INSERT INTO security_custom_rules VALUES ('rule', 'Rule', 'keyword', 'private', 'HIGH', 'audit', 1, NULL, 123);
         INSERT INTO security_audit_logs (id, timestamp, app_type, provider_id, rule_id, rule_name, rule_category, risk_level, action_taken, matched_snippet_masked, trigger_count, last_seen_at, session_id)
         VALUES (42, 100, 'claude', 'kept', 'rule', 'Rule', 'custom', 'HIGH', 'audit', '***', 7, 200, 'session');
         INSERT INTO tool_risk_settings VALUES ('enabled', 'true'), ('policy_version', '9');
         INSERT INTO tool_risk_events (id, timestamp, source, stage, detection_scope, integrity_status, policy_version, command_summary)
         VALUES (73, 222, 'claude', 'response', 'tool', 'complete', 9, 'git status');
         UPDATE sqlite_sequence SET seq = 1000 WHERE name = 'security_audit_logs';",
    )?;
    Database::set_user_version(&conn, version)?;
    Ok(())
}

#[test]
fn existing_users_are_split_without_losing_data_and_can_reopen() -> Result<(), AppError> {
    for version in 19..=24 {
        let dir = tempfile::tempdir().expect("temporary storage");
        let path = dir.path().join("cc-switch.db");
        legacy_database(&path, version)?;
        let db = Database::init_at(&path)?;
        assert!(!db.get_security_settings()?.enabled);
        assert_eq!(db.get_custom_security_rules()?.len(), 1);
        assert_eq!(db.get_tool_risk_settings()?.policy_version, 9);
        {
            let core = lock_conn!(db.conn);
            assert_eq!(Database::get_user_version(&core)?, 20);
            assert!(!Database::table_exists(&core, "security_audit_logs")?);
            assert!(!Database::table_exists(&core, "tool_risk_settings")?);
            assert!(Database::has_column(&core, "mcp_servers", "enabled_mcode")?);
            assert!(Database::has_column(&core, "mcp_servers", "enabled_pi")?);
            assert_eq!(
                core.query_row("SELECT COUNT(*) FROM providers WHERE id='kept'", [], |r| {
                    r.get::<_, i64>(0)
                })?,
                1
            );
        }
        {
            let extension = lock_conn!(db.extension_connection());
            assert_eq!(
                Database::get_user_version(&extension)?,
                super::EXTENSION_SCHEMA_VERSION
            );
            let row: (i64, i64, String) = extension.query_row("SELECT trigger_count, last_seen_at, session_id FROM security_audit_logs WHERE id=42", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            assert_eq!(row, (7, 200, "session".into()));
            assert_eq!(
                extension.query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='security_audit_logs'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                1000
            );
        }
        assert!(dir.path().join("vary-ai.db").exists());
        assert!(dir.path().join("backups/storage-split").exists());
        drop(db);
        let reopened = Database::init_at(&path)?;
        assert_eq!(reopened.get_custom_security_rules()?.len(), 1);
        assert_eq!(reopened.get_tool_risk_settings()?.policy_version, 9);
    }
    Ok(())
}

#[test]
fn official_shared_database_keeps_its_version_and_has_no_extension_tables() -> Result<(), AppError>
{
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path)?;
    db.update_security_settings(&super::SecurityAuditSettings { enabled: false })?;
    db.save_tool_risk_settings(&super::ToolRiskSettings {
        enabled: true,
        policy_version: 0,
    })?;
    let official = Connection::open(&path)?;
    assert_eq!(Database::get_user_version(&official)?, 20);
    assert!(!Database::table_exists(&official, "security_settings")?);
    assert!(!Database::table_exists(&official, "tool_risk_settings")?);
    assert!(Database::has_column(&official, "skills", "enabled_mcode")?);
    Ok(())
}

#[test]
fn unknown_future_databases_are_never_downgraded_or_modified() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    let conn = Connection::open(&path)?;
    conn.execute_batch("CREATE TABLE future(value TEXT); INSERT INTO future VALUES ('kept'); PRAGMA user_version=25;")?;
    drop(conn);
    assert!(Database::init_at(&path).is_err());
    let conn = Connection::open(&path)?;
    assert_eq!(Database::get_user_version(&conn)?, 25);
    assert!(!Database::table_exists(&conn, "providers")?);
    assert!(!dir.path().join("vary-ai.db").exists());
    Ok(())
}

#[test]
fn failed_migration_rolls_back_both_files_and_retains_recovery_backup() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    legacy_database(&path, 24)?;
    let extension = Connection::open(dir.path().join("vary-ai.db"))?;
    extension.execute_batch("CREATE TABLE security_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL); INSERT INTO security_settings VALUES ('enabled', 'true');")?;
    drop(extension);
    assert!(Database::init_at(&path).is_err());
    let core = Connection::open(&path)?;
    assert_eq!(Database::get_user_version(&core)?, 24);
    assert!(Database::table_exists(&core, "security_audit_logs")?);
    let extension = Connection::open(dir.path().join("vary-ai.db"))?;
    assert_eq!(
        extension.query_row("SELECT value FROM security_settings", [], |r| r
            .get::<_, String>(0))?,
        "true"
    );
    assert!(dir.path().join("backups/storage-split").exists());
    Ok(())
}

#[test]
fn historical_shapes_are_upgraded_before_splitting() -> Result<(), AppError> {
    for version in 19..=21 {
        for local in [false, true] {
            let dir = tempfile::tempdir().expect("temporary storage");
            let path = dir.path().join("cc-switch.db");
            let old = super::migration_v22_tests::historical_fixture(version, local)?;
            Database::create_shared_tables_on_conn(&old)?;
            let mut disk = Connection::open(&path)?;
            let backup = rusqlite::backup::Backup::new(&old, &mut disk)?;
            backup.run_to_completion(100, std::time::Duration::from_millis(1), None)?;
            drop(backup);
            drop(disk);
            let db = Database::init_at(&path)?;
            let ext = lock_conn!(db.extension_connection());
            assert!(Database::has_column(
                &ext,
                "security_audit_logs",
                "trigger_count"
            )?);
            assert!(Database::has_column(
                &ext,
                "security_audit_logs",
                "session_id"
            )?);
            assert!(Database::has_column(
                &ext,
                "tool_risk_events",
                "command_summary"
            )?);
            assert_eq!(
                ext.query_row(
                    "SELECT COUNT(*) FROM security_audit_logs WHERE id=42",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
        }
    }
    Ok(())
}

#[test]
fn portable_sql_export_and_import_include_both_files() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let source = Database::init_at(&dir.path().join("cc-switch.db"))?;
    source.update_security_settings(&super::SecurityAuditSettings { enabled: false })?;
    source.save_tool_risk_settings(&super::ToolRiskSettings {
        enabled: true,
        policy_version: 0,
    })?;
    let sql = source.export_sql_string()?;
    let target_dir = tempfile::tempdir().expect("temporary storage");
    let target = Database::init_at(&target_dir.path().join("cc-switch.db"))?;
    target.import_sql_string(&sql)?;
    assert!(!target.get_security_settings()?.enabled);
    assert!(target.get_tool_risk_settings()?.enabled);
    let shared = lock_conn!(target.conn);
    assert_eq!(Database::get_user_version(&shared)?, 20);
    assert!(!Database::table_exists(&shared, "security_settings")?);
    Ok(())
}

#[test]
fn future_extension_is_rejected_without_touching_shared_file() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path)?;
    drop(db);
    let ext = Connection::open(dir.path().join("vary-ai.db"))?;
    ext.execute_batch("PRAGMA user_version=999;")?;
    drop(ext);
    let before = std::fs::read(&path).expect("read shared file");
    assert!(Database::init_at(&path).is_err());
    assert_eq!(std::fs::read(&path).expect("read shared file"), before);
    Ok(())
}

#[test]
fn official_v19_and_v20_flags_survive_native_schema_upgrade() -> Result<(), AppError> {
    for version in [19, 20] {
        let dir = tempfile::tempdir().expect("temporary storage");
        let path = dir.path().join("cc-switch.db");
        let conn = Connection::open(&path)?;
        // Frozen upstream shapes, independent of the schema builder under test.
        conn.execute_batch("CREATE TABLE mcp_servers (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, server_config TEXT NOT NULL,
            description TEXT, homepage TEXT, docs TEXT, tags TEXT NOT NULL DEFAULT '[]',
            enabled_claude BOOLEAN NOT NULL DEFAULT 0, enabled_codex BOOLEAN NOT NULL DEFAULT 0,
            enabled_gemini BOOLEAN NOT NULL DEFAULT 0, enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
            enabled_opencode BOOLEAN NOT NULL DEFAULT 0, enabled_mcode BOOLEAN NOT NULL DEFAULT 0,
            enabled_hermes BOOLEAN NOT NULL DEFAULT 0);
            CREATE TABLE skills (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT, directory TEXT NOT NULL,
            repo_owner TEXT, repo_name TEXT, repo_branch TEXT DEFAULT 'main', readme_url TEXT,
            enabled_claude BOOLEAN NOT NULL DEFAULT 0, enabled_codex BOOLEAN NOT NULL DEFAULT 0,
            enabled_gemini BOOLEAN NOT NULL DEFAULT 0, enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
            enabled_opencode BOOLEAN NOT NULL DEFAULT 0, enabled_mcode BOOLEAN NOT NULL DEFAULT 0,
            enabled_hermes BOOLEAN NOT NULL DEFAULT 0, installed_at INTEGER NOT NULL DEFAULT 0,
            content_hash TEXT, updated_at INTEGER NOT NULL DEFAULT 0);
            INSERT INTO mcp_servers(id,name,server_config,enabled_mcode) VALUES ('kept','Kept','{}',1);
            INSERT INTO skills(id,name,directory,enabled_mcode) VALUES ('kept','Kept','kept',1);")?;
        if version == 20 {
            conn.execute_batch("ALTER TABLE mcp_servers ADD COLUMN enabled_pi BOOLEAN NOT NULL DEFAULT 0; UPDATE mcp_servers SET enabled_pi=1;")?;
        }
        Database::set_user_version(&conn, version)?;
        drop(conn);
        let db = Database::init_at(&path)?;
        let shared = lock_conn!(db.conn);
        assert_eq!(Database::get_user_version(&shared)?, 20);
        let flags: (bool, bool) = shared.query_row(
            "SELECT enabled_mcode, enabled_pi FROM mcp_servers WHERE id='kept'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(flags, (true, version == 20));
        assert!(shared.query_row(
            "SELECT enabled_mcode FROM skills WHERE id='kept'",
            [],
            |r| r.get::<_, bool>(0)
        )?);
        assert!(!Database::table_exists(&shared, "security_settings")?);
    }
    Ok(())
}

#[test]
fn split_sync_preserves_local_tool_policy_and_sequences() -> Result<(), AppError> {
    let local_dir = tempfile::tempdir().expect("temporary storage");
    let local_path = local_dir.path().join("cc-switch.db");
    legacy_database(&local_path, 24)?;
    let local = Database::init_at(&local_path)?;
    let remote_dir = tempfile::tempdir().expect("temporary storage");
    let remote = Database::init_at(&remote_dir.path().join("cc-switch.db"))?;
    let sql = remote.export_sql_string_for_sync()?;
    local.import_sql_string_for_sync(&sql)?;
    assert_eq!(local.get_tool_risk_settings()?.policy_version, 9);
    let ext = lock_conn!(local.extension_connection());
    assert_eq!(
        ext.query_row(
            "SELECT command_summary FROM tool_risk_events WHERE id=73",
            [],
            |r| r.get::<_, String>(0)
        )?,
        "git status"
    );
    assert_eq!(
        Database::get_user_version(&ext)?,
        super::EXTENSION_SCHEMA_VERSION
    );
    drop(ext);
    let shared = lock_conn!(local.conn);
    assert_eq!(Database::get_user_version(&shared)?, 20);
    assert!(!Database::table_exists(&shared, "tool_risk_events")?);
    Ok(())
}

#[test]
fn binary_backup_restores_both_schemas() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    legacy_database(&path, 24)?;
    let db = Database::init_at(&path)?;
    let backup = db.backup_database_file()?.expect("backup path");
    assert_eq!(
        backup
            .parent()
            .expect("parent")
            .canonicalize()
            .expect("canonical backup"),
        dir.path()
            .join("backups")
            .canonicalize()
            .expect("canonical directory")
    );
    let snapshot = Connection::open(&backup)?;
    assert_eq!(
        snapshot.query_row(
            "SELECT trigger_count FROM security_audit_logs WHERE id=42",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        7
    );
    drop(snapshot);
    db.clear_security_audit_logs(None)?;
    db.clear_tool_risk_events(None)?;
    db.restore_from_backup(
        backup
            .file_name()
            .and_then(|s| s.to_str())
            .expect("filename"),
    )?;
    let ext = lock_conn!(db.extension_connection());
    assert_eq!(
        ext.query_row(
            "SELECT trigger_count FROM security_audit_logs WHERE id=42",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        7
    );
    assert_eq!(
        ext.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='security_audit_logs'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        1000
    );
    assert_eq!(
        ext.query_row(
            "SELECT COUNT(*) FROM tool_risk_events WHERE id=73",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        1
    );
    drop(ext);
    let shared = lock_conn!(db.conn);
    assert_eq!(Database::get_user_version(&shared)?, 20);
    assert!(!Database::table_exists(&shared, "security_audit_logs")?);
    Ok(())
}

#[test]
fn unrecognized_extension_shapes_are_preserved() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path)?;
    drop(db);
    let ext_path = dir.path().join("vary-ai.db");
    let ext = Connection::open(&ext_path)?;
    ext.execute_batch("ALTER TABLE security_settings RENAME TO security_settings_old; CREATE TABLE security_settings(key TEXT PRIMARY KEY); INSERT INTO security_settings VALUES ('kept');")?;
    drop(ext);
    let before = std::fs::read(&path).expect("read shared");
    let extension_before = std::fs::read(&ext_path).expect("read extension");
    assert!(Database::init_at(&path).is_err());
    assert_eq!(std::fs::read(&path).expect("read shared"), before);
    assert_eq!(
        std::fs::read(&ext_path).expect("read extension"),
        extension_before
    );
    Ok(())
}

#[test]
fn partial_legacy_signatures_are_never_treated_as_official() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    let conn = Connection::open(&path)?;
    Database::create_shared_tables_on_conn(&conn)?;
    conn.execute_batch("CREATE TABLE security_settings(key TEXT PRIMARY KEY,value TEXT); INSERT INTO security_settings VALUES ('kept','value'); PRAGMA user_version=20;")?;
    drop(conn);
    let before = std::fs::read(&path).expect("read shared");
    assert!(Database::init_at(&path).is_err());
    assert_eq!(std::fs::read(&path).expect("read shared"), before);
    assert!(!dir.path().join("vary-ai.db").exists());
    Ok(())
}

#[test]
fn startup_preflight_reports_the_file_and_its_own_supported_version() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    legacy_database(&path, 24)?;
    assert!(Database::stored_user_version_exceeds_supported(&path)?.is_none());
    let db = Database::init_at(&path)?;
    drop(db);
    let extension_path = dir.path().join("vary-ai.db");
    let ext = Connection::open(&extension_path)?;
    ext.execute_batch("PRAGMA user_version=999;")?;
    drop(ext);
    let unsupported =
        Database::stored_user_version_exceeds_supported(&path)?.expect("future extension");
    assert_eq!(unsupported.path, extension_path);
    assert_eq!(unsupported.version, 999);
    assert_eq!(
        unsupported.supported_version,
        super::EXTENSION_SCHEMA_VERSION
    );
    let shared = Connection::open(&path)?;
    shared.execute_batch("PRAGMA user_version=25;")?;
    drop(shared);
    let unsupported =
        Database::stored_user_version_exceeds_supported(&path)?.expect("future shared");
    assert_eq!(unsupported.path, path);
    assert_eq!(unsupported.version, 25);
    assert_eq!(unsupported.supported_version, 20);
    Ok(())
}

#[test]
fn failed_replacement_rolls_back_after_schema_changes_and_can_retry() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    legacy_database(&path, 24)?;
    let db = Database::init_at(&path)?;
    let staging_path = dir.path().join("staging.db");
    let mut staging = Connection::open(&staging_path)?;
    let snapshot = db.snapshot_to_memory()?;
    {
        let backup = rusqlite::backup::Backup::new(&snapshot, &mut staging)?;
        backup.run_to_completion(100, std::time::Duration::from_millis(1), None)?;
    }
    // SQLite keeps this table's original CREATE prefix. The importer must
    // reject unsupported objects and roll back even after dropping live tables.
    staging.execute_batch("CREATE VIRTUAL TABLE unknown_virtual USING fts5(content);")?;
    {
        let core = lock_conn!(db.conn);
        let extension = lock_conn!(db.extension_connection());
        assert!(Database::replace_split_storage(&core, &staging_path).is_err());
        assert_eq!(Database::get_user_version(&core)?, 20);
        assert_eq!(
            core.query_row("SELECT COUNT(*) FROM providers WHERE id='kept'", [], |r| {
                r.get::<_, i64>(0)
            })?,
            1
        );
        assert_eq!(
            extension.query_row(
                "SELECT trigger_count FROM security_audit_logs WHERE id=42",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            7
        );
    }
    staging.execute_batch("DROP TABLE unknown_virtual;")?;
    let core = lock_conn!(db.conn);
    let _extension = lock_conn!(db.extension_connection());
    Database::replace_split_storage(&core, &staging_path)?;
    assert_eq!(Database::get_user_version(&core)?, 20);
    Ok(())
}

#[test]
fn split_import_preserves_unique_constraints_with_sql_whitespace() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let source = Database::init_at(&dir.path().join("cc-switch.db"))?;
    {
        let conn = lock_conn!(source.conn);
        conn.execute_batch("CREATE TABLE unique_rows(value TEXT); CREATE     UNIQUE INDEX unique_value ON unique_rows(value); INSERT INTO unique_rows VALUES ('kept');")?;
    }
    let sql = source.export_sql_string()?;
    let target_dir = tempfile::tempdir().expect("temporary storage");
    let target = Database::init_at(&target_dir.path().join("cc-switch.db"))?;
    target.import_sql_string(&sql)?;
    let conn = lock_conn!(target.conn);
    assert!(conn
        .execute("INSERT INTO unique_rows VALUES ('kept')", [])
        .is_err());
    Ok(())
}

#[test]
fn vary_mcp_edits_preserve_flags_owned_by_cc_switch() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let db = Database::init_at(&dir.path().join("cc-switch.db"))?;
    {
        let conn = lock_conn!(db.conn);
        conn.execute_batch("INSERT INTO mcp_servers(id,name,server_config,enabled_mcode,enabled_pi) VALUES ('kept','Kept','{}',1,1);")?;
    }
    let mut server = db
        .get_all_mcp_servers()?
        .shift_remove("kept")
        .expect("server");
    server.name = "Edited".into();
    server.apps.claude = true;
    db.save_mcp_server(&server)?;
    let conn = lock_conn!(db.conn);
    let flags: (bool, bool, bool) = conn.query_row(
        "SELECT enabled_claude,enabled_mcode,enabled_pi FROM mcp_servers WHERE id='kept'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(flags, (true, true, true));
    Ok(())
}

#[test]
fn vary_skill_edits_preserve_flags_owned_by_cc_switch() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let db = Database::init_at(&dir.path().join("cc-switch.db"))?;
    {
        let conn = lock_conn!(db.conn);
        conn.execute_batch(
            "INSERT INTO skills(id,name,directory,enabled_mcode) VALUES ('kept','Kept','kept',1);",
        )?;
    }
    let mut skill = db.get_installed_skill("kept")?.expect("skill");
    skill.name = "Edited".into();
    skill.apps.claude = true;
    db.save_skill(&skill)?;
    let conn = lock_conn!(db.conn);
    let flags: (bool, bool) = conn.query_row(
        "SELECT enabled_claude,enabled_mcode FROM skills WHERE id='kept'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(flags, (true, true));
    Ok(())
}
