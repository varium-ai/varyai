use super::Database;
use rusqlite::Connection;

#[test]
fn fresh_storage_ignores_a_legacy_data_override_without_migrating_files() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("legacy");
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(legacy.join("cc-switch.db"), b"synthetic source preserved").unwrap();
    std::fs::write(
        legacy.join("settings.json"),
        b"synthetic device state preserved",
    )
    .unwrap();
    let target = dir.path().join("native");
    assert_eq!(
        crate::storage_setup::resolve_target(Some(&legacy), &target).unwrap(),
        target
    );
    assert!(!target.exists());
    assert_eq!(
        std::fs::read(legacy.join("cc-switch.db")).unwrap(),
        b"synthetic source preserved"
    );
    assert!(!legacy.join("varyai.db").exists());
}

#[test]
fn missing_native_override_is_retained_for_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let configured = dir.path().join("missing-native");
    let default = dir.path().join("default-native");
    assert_eq!(
        crate::storage_setup::resolve_target(Some(&configured), &default).unwrap(),
        configured
    );
    assert!(!configured.exists());
}

#[tokio::test]
async fn native_proxy_defaults_use_an_independent_port() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::init_native_at(&dir.path().join("varyai.db")).unwrap();
    assert_eq!(
        db.get_global_proxy_config().await.unwrap().listen_port,
        15722
    );
    assert_eq!(db.get_proxy_config().await.unwrap().listen_port, 15722);
    assert_eq!(
        crate::proxy::types::ProxyConfig::default().listen_port,
        15722
    );
    db.conn
        .lock()
        .unwrap()
        .execute("DELETE FROM proxy_config", [])
        .unwrap();
    assert_eq!(
        db.get_global_proxy_config().await.unwrap().listen_port,
        15722
    );
    assert_eq!(db.get_proxy_config().await.unwrap().listen_port, 15722);
    db.conn
        .lock()
        .unwrap()
        .execute("DELETE FROM proxy_config", [])
        .unwrap();
    db.set_default_cost_multiplier("claude", "2").await.unwrap();
    assert_eq!(
        db.get_global_proxy_config().await.unwrap().listen_port,
        15722
    );
}

#[tokio::test]
async fn native_proxy_port_upgrade_runs_once_and_preserves_explicit_choices() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("varyai.db");
    let db = Database::init_native_at(&path).unwrap();
    {
        let conn = db.conn.lock().unwrap();
        conn.execute("DELETE FROM settings WHERE key='varyai_proxy_port_v1'", [])
            .unwrap();
        conn.execute("UPDATE proxy_config SET listen_port=15721", [])
            .unwrap();
    }
    drop(db);
    let db = Database::init_native_at(&path).unwrap();
    assert_eq!(
        db.get_global_proxy_config().await.unwrap().listen_port,
        15722
    );
    let mut chosen = db.get_global_proxy_config().await.unwrap();
    chosen.listen_port = 15721;
    db.update_global_proxy_config(chosen).await.unwrap();
    drop(db);
    let db = Database::init_native_at(&path).unwrap();
    assert_eq!(
        db.get_global_proxy_config().await.unwrap().listen_port,
        15721
    );
}

#[tokio::test]
async fn legacy_restore_moves_the_old_default_but_keeps_custom_ports() {
    for (source_port, expected) in [(15721, 15722), (23456, 23456)] {
        let source = tempfile::tempdir().unwrap();
        let source_path = source.path().join("cc-switch.db");
        let legacy = Database::init_at(&source_path).unwrap();
        legacy
            .conn
            .lock()
            .unwrap()
            .execute("UPDATE proxy_config SET listen_port=?1", [source_port])
            .unwrap();
        let sql = legacy.export_sql_string().unwrap();
        drop(legacy);
        let before = std::fs::read(&source_path).unwrap();
        let target = tempfile::tempdir().unwrap();
        let path = target.path().join("varyai.db");
        let db = Database::init_native_at(&path).unwrap();
        db.import_sql_string(&sql).unwrap();
        assert_eq!(
            db.get_global_proxy_config().await.unwrap().listen_port,
            expected
        );
        drop(db);
        let db = Database::init_native_at(&path).unwrap();
        assert_eq!(
            db.get_global_proxy_config().await.unwrap().listen_port,
            expected
        );
        assert_eq!(std::fs::read(&source_path).unwrap(), before);
    }
}

#[test]
fn native_storage_uses_a_private_identified_pair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("varyai.db");
    let db = Database::init_native_at(&path).unwrap();
    db.set_setting("synthetic-setting", "preserve").unwrap();
    drop(db);
    assert!(!dir.path().join("cc-switch.db").exists());
    assert!(!dir.path().join("vary-ai.db").exists());
    let main = Connection::open(&path).unwrap();
    let import_tables: i64 = main.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('varyai_import_origins','varyai_import_operations')",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(import_tables, 0);
    let extension = Connection::open(dir.path().join("varyai-ext.db")).unwrap();
    let main_id: String = main
        .query_row("SELECT storage_id FROM varyai_storage_identity", [], |r| {
            r.get(0)
        })
        .unwrap();
    let extension_id: String = extension
        .query_row(
            "SELECT storage_id FROM varyai_extension_identity",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(main_id, extension_id);
    assert_eq!(Database::get_user_version(&main).unwrap(), 24);
    assert_eq!(
        Database::get_user_version(&extension).unwrap(),
        super::NATIVE_EXTENSION_SCHEMA_VERSION
    );
    drop((main, extension));
    let reopened = Database::init_native_at(&path).unwrap();
    assert_eq!(
        reopened
            .get_setting("synthetic-setting")
            .unwrap()
            .as_deref(),
        Some("preserve")
    );
}

#[test]
fn native_storage_rejects_a_mismatched_extension_without_repair() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    drop(Database::init_native_at(&left.path().join("varyai.db")).unwrap());
    drop(Database::init_native_at(&right.path().join("varyai.db")).unwrap());
    std::fs::copy(
        right.path().join("varyai-ext.db"),
        left.path().join("varyai-ext.db"),
    )
    .unwrap();
    let before = std::fs::read(left.path().join("varyai-ext.db")).unwrap();
    assert!(Database::init_native_at(&left.path().join("varyai.db")).is_err());
    assert_eq!(
        before,
        std::fs::read(left.path().join("varyai-ext.db")).unwrap()
    );
}

#[test]
fn native_restore_preserves_destination_identity_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("varyai.db");
    let db = Database::init_native_at(&path).unwrap();
    db.set_setting("synthetic-setting", "before").unwrap();
    let sql = db.export_sql_string().unwrap();
    db.set_setting("synthetic-setting", "after").unwrap();
    db.import_sql_string(&sql).unwrap();
    drop(db);
    let db = Database::init_native_at(&path).unwrap();
    assert_eq!(
        db.get_setting("synthetic-setting").unwrap().as_deref(),
        Some("before")
    );
}

#[test]
fn native_storage_recovers_an_interrupted_owned_transaction() {
    if let Some(directory) = std::env::var_os("VARYAI_NATIVE_CRASH_FIXTURE") {
        let directory = std::path::PathBuf::from(directory);
        let conn = Connection::open(directory.join("varyai.db")).unwrap();
        conn.execute(
            "ATTACH DATABASE ?1 AS vary",
            [directory
                .join("varyai-ext.db")
                .to_string_lossy()
                .into_owned()],
        )
        .unwrap();
        conn.execute_batch("PRAGMA cache_size=2; PRAGMA vary.cache_size=2; BEGIN IMMEDIATE")
            .unwrap();
        let payload = "synthetic".repeat(100_000);
        conn.execute(
            "UPDATE settings SET value=?1 WHERE key='synthetic-recovery'",
            [&payload],
        )
        .unwrap();
        conn.execute(
            "UPDATE vary.security_settings SET value=?1 WHERE key='synthetic-recovery'",
            [&payload],
        )
        .unwrap();
        std::process::exit(0);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("varyai.db");
    let db = Database::init_native_at(&path).unwrap();
    db.set_setting("synthetic-recovery", "before").unwrap();
    db.extension_connection()
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO security_settings(key,value) VALUES ('synthetic-recovery','before')",
            [],
        )
        .unwrap();
    drop(db);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "database::native_tests::native_storage_recovers_an_interrupted_owned_transaction",
        ])
        .env("VARYAI_NATIVE_CRASH_FIXTURE", dir.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(dir.path().join("varyai.db-journal").exists());
    let db = Database::init_native_at(&path).unwrap();
    assert_eq!(
        db.get_setting("synthetic-recovery").unwrap().as_deref(),
        Some("before")
    );
    let value: String = db
        .extension_connection()
        .lock()
        .unwrap()
        .query_row(
            "SELECT value FROM security_settings WHERE key='synthetic-recovery'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(value, "before");
}

#[test]
fn native_recovery_rejects_foreign_super_journal_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("varyai.db");
    drop(Database::init_native_at(&path).unwrap());
    let foreign = tempfile::tempdir().unwrap();
    let master = foreign.path().join("synthetic-master");
    std::fs::write(&master, b"preserve").unwrap();
    let name = master.to_string_lossy().as_bytes().to_vec();
    let mut journal = vec![0u8; 32];
    journal.extend_from_slice(&name);
    journal.extend_from_slice(&(name.len() as u32).to_be_bytes());
    journal.extend_from_slice(
        &name
            .iter()
            .map(|byte| u32::from(*byte))
            .sum::<u32>()
            .to_be_bytes(),
    );
    journal.extend_from_slice(&[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]);
    std::fs::write(dir.path().join("varyai.db-journal"), &journal).unwrap();
    assert!(Database::init_native_at(&path).is_err());
    assert_eq!(std::fs::read(master).unwrap(), b"preserve");
    assert_eq!(
        std::fs::read(dir.path().join("varyai.db-journal")).unwrap(),
        journal
    );
}

#[test]
fn native_v6_upgrade_adds_global_providers_without_losing_private_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("varyai.db");
    let db = Database::init_native_at(&path).unwrap();
    db.set_setting("synthetic-native-setting", "preserve")
        .unwrap();
    drop(db);
    let extension = Connection::open(dir.path().join("varyai-ext.db")).unwrap();
    extension
        .execute_batch("DROP TABLE routing_upstreams; PRAGMA user_version=6;")
        .unwrap();
    drop(extension);
    let db = Database::init_native_at(&path).unwrap();
    assert_eq!(
        db.get_setting("synthetic-native-setting")
            .unwrap()
            .as_deref(),
        Some("preserve")
    );
    assert!(db.list_routing_upstreams().unwrap().is_empty());
    drop(db);
    let extension = Connection::open(dir.path().join("varyai-ext.db")).unwrap();
    assert_eq!(
        Database::get_user_version(&extension).unwrap(),
        super::NATIVE_EXTENSION_SCHEMA_VERSION
    );
}
