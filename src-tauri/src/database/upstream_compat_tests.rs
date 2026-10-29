//! Frozen CC Switch schemas exercise startup, edits, and portable round trips.
use super::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::Connection;

// Frozen flag layouts through the released CC Switch v4.0.21 schema (v23).
fn upstream_database(path: &std::path::Path, version: i32) -> Result<(), AppError> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE mcp_servers (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, server_config TEXT NOT NULL,
            description TEXT, homepage TEXT, docs TEXT, tags TEXT NOT NULL DEFAULT '[]',
            enabled_claude BOOLEAN NOT NULL DEFAULT 0, enabled_codex BOOLEAN NOT NULL DEFAULT 0,
            enabled_gemini BOOLEAN NOT NULL DEFAULT 0, enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
            enabled_opencode BOOLEAN NOT NULL DEFAULT 0, enabled_mcode BOOLEAN NOT NULL DEFAULT 0,
            enabled_hermes BOOLEAN NOT NULL DEFAULT 0, enabled_pi BOOLEAN NOT NULL DEFAULT 0
         );
         CREATE TABLE skills (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT, directory TEXT NOT NULL,
            repo_owner TEXT, repo_name TEXT, repo_branch TEXT DEFAULT 'main', readme_url TEXT,
            enabled_claude BOOLEAN NOT NULL DEFAULT 0, enabled_codex BOOLEAN NOT NULL DEFAULT 0,
            enabled_gemini BOOLEAN NOT NULL DEFAULT 0, enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
            enabled_opencode BOOLEAN NOT NULL DEFAULT 0, enabled_mcode BOOLEAN NOT NULL DEFAULT 0,
            enabled_hermes BOOLEAN NOT NULL DEFAULT 0, installed_at INTEGER NOT NULL DEFAULT 0,
            content_hash TEXT, updated_at INTEGER NOT NULL DEFAULT 0
         );
         INSERT INTO mcp_servers(id,name,server_config,enabled_codex,enabled_mcode,enabled_pi)
            VALUES ('kept','Kept','{}',1,1,1);
         INSERT INTO skills(id,name,directory,enabled_codex,enabled_mcode)
            VALUES ('kept','Kept','kept',1,1);",
    )?;
    // Unchanged shared tables use the existing builder; flag-bearing tables above
    // remain frozen so this fixture cannot silently acquire the columns under test.
    Database::create_shared_tables_on_conn(&conn)?;
    for (introduced, column, tables) in [
        (21, "enabled_agy", &["mcp_servers", "skills"][..]),
        (22, "enabled_kimi", &["mcp_servers", "skills"][..]),
        (23, "enabled_omp", &["mcp_servers"][..]),
    ] {
        if version >= introduced {
            for table in tables {
                conn.execute_batch(&format!(
                    "ALTER TABLE {table} ADD COLUMN {column} BOOLEAN NOT NULL DEFAULT 0;
                     UPDATE {table} SET {column}=1;"
                ))?;
            }
        }
    }
    Database::set_user_version(&conn, version)?;
    Ok(())
}

fn assert_upstream_flags(db: &Database, source_version: i32) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    assert_eq!(Database::get_user_version(&conn)?, 23);
    for table in ["mcp_servers", "skills"] {
        let flags: (bool, bool, bool, bool) = conn.query_row(
            &format!(
                "SELECT enabled_codex,enabled_mcode,enabled_agy,enabled_kimi
                 FROM {table} WHERE id='kept'"
            ),
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        assert_eq!(
            flags,
            (true, true, source_version >= 21, source_version >= 22)
        );
    }
    let mcp_flags: (bool, bool) = conn.query_row(
        "SELECT enabled_pi,enabled_omp FROM mcp_servers WHERE id='kept'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(mcp_flags, (true, source_version >= 23));
    assert!(!Database::has_column(&conn, "skills", "enabled_omp")?);
    for table in ["mcp_servers", "skills"] {
        assert!(!Database::has_column(&conn, table, "enabled_dsh")?);
    }
    assert!(!Database::table_exists(&conn, "security_settings")?);
    assert!(!Database::table_exists(&conn, "tool_risk_settings")?);
    Ok(())
}

#[test]
fn upstream_v21_through_v23_pass_read_only_startup_preflight() -> Result<(), AppError> {
    for version in 21..=23 {
        let dir = tempfile::tempdir().expect("temporary storage");
        let path = dir.path().join("cc-switch.db");
        upstream_database(&path, version)?;
        let before = std::fs::read(&path).expect("read synthetic database");
        assert!(Database::stored_user_version_exceeds_supported(&path)?.is_none());
        assert_eq!(
            std::fs::read(&path).expect("read synthetic database"),
            before
        );
        assert!(!dir.path().join("vary-ai.db").exists());
    }
    Ok(())
}

#[test]
fn upstream_v20_through_v23_preserve_flags_across_startup_and_edits() -> Result<(), AppError> {
    for version in 20..=23 {
        let dir = tempfile::tempdir().expect("temporary storage");
        let path = dir.path().join("cc-switch.db");
        upstream_database(&path, version)?;
        let db = Database::init_at(&path)?;
        assert_upstream_flags(&db, version)?;
        let mut server = db
            .get_all_mcp_servers()?
            .shift_remove("kept")
            .expect("server");
        server.name = "Edited".into();
        server.apps.claude = true;
        db.save_mcp_server(&server)?;
        let mut skill = db.get_installed_skill("kept")?.expect("skill");
        skill.name = "Edited".into();
        skill.apps.claude = true;
        db.save_skill(&skill)?;
        assert_upstream_flags(&db, version)?;
        drop(db);
        assert_upstream_flags(&Database::init_at(&path)?, version)?;
    }
    Ok(())
}

#[test]
fn upstream_v23_stays_readable_by_cc_switch_v23_after_repeated_startup() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    upstream_database(&path, 23)?;
    for _ in 0..2 {
        let db = Database::init_at(&path)?;
        assert_upstream_flags(&db, 23)?;
        let cc_switch =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        assert_eq!(Database::get_user_version(&cc_switch)?, 23);
    }
    Ok(())
}

#[test]
fn upstream_v24_is_rejected_without_downgrading_or_modifying_either_file() -> Result<(), AppError> {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path)?;
    drop(db);
    let conn = Connection::open(&path)?;
    for table in ["mcp_servers", "skills"] {
        if !Database::has_column(&conn, table, "enabled_dsh")? {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN enabled_dsh BOOLEAN NOT NULL DEFAULT 0;"
            ))?;
        }
    }
    Database::set_user_version(&conn, 24)?;
    drop(conn);
    let ext_path = path.with_file_name("vary-ai.db");
    let before = std::fs::read(&path).expect("read synthetic shared database");
    let before_ext = std::fs::read(&ext_path).expect("read synthetic extension database");
    let unsupported = Database::stored_user_version_exceeds_supported(&path)?
        .expect("unsupported upstream version");
    assert_eq!(unsupported.version, 24);
    assert_eq!(unsupported.supported_version, 23);
    assert!(Database::init_at(&path).is_err());
    assert_eq!(
        std::fs::read(&path).expect("read synthetic shared database"),
        before
    );
    assert_eq!(
        std::fs::read(&ext_path).expect("read synthetic extension database"),
        before_ext
    );
    Ok(())
}

#[test]
fn upstream_v23_survives_sql_import_and_binary_restore() -> Result<(), AppError> {
    let version = 23;
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    upstream_database(&path, version)?;
    // Model a genuine upstream export, with no Vary extension tables.
    let upstream = Database {
        conn: std::sync::Mutex::new(Connection::open(&path)?),
        extension_conn: None,
    };
    let sql = upstream.export_sql_string()?;
    let target_dir = tempfile::tempdir().expect("temporary storage");
    let target = Database::init_at(&target_dir.path().join("cc-switch.db"))?;
    target.import_sql_string(&sql)?;
    assert_upstream_flags(&target, version)?;
    target.update_security_settings(&super::SecurityAuditSettings { enabled: false })?;

    let portable_sql = target.export_sql_string()?;
    target.import_sql_string(&portable_sql)?;
    assert_upstream_flags(&target, version)?;
    assert!(!target.get_security_settings()?.enabled);

    let backup = target.backup_database_file()?.expect("backup");
    target.delete_mcp_server("kept")?;
    target.restore_from_backup(
        backup
            .file_name()
            .and_then(|s| s.to_str())
            .expect("filename"),
    )?;
    assert_upstream_flags(&target, version)?;
    assert!(!target.get_security_settings()?.enabled);
    Ok(())
}
