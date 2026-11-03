use super::{
    register_db_change_hook, Database, NATIVE_EXTENSION_SCHEMA_VERSION, NATIVE_MAIN_SCHEMA_VERSION,
};
use crate::error::AppError;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

pub(super) const APPLICATION_ID: i32 = 0x56525941;

#[derive(Serialize, Deserialize)]
struct StorageManifest {
    product: String,
    layout_version: u32,
    storage_id: String,
}

fn invalid() -> AppError {
    AppError::Config("VaryAI storage is incomplete or incompatible. Restore a matching database pair or select another native data directory.".into())
}

impl Database {
    pub(crate) fn is_native_storage(&self) -> Result<bool, AppError> {
        let conn = super::lock_conn!(self.conn);
        Self::is_native_connection(&conn)
    }

    pub(crate) fn owns_native_client(&self, app: &crate::AppType) -> Result<bool, AppError> {
        if !self.is_native_storage()? {
            return Ok(true);
        }
        if !app.is_additive_mode() && self.get_current_provider(app.as_str())?.is_some() {
            return Ok(true);
        }
        Ok(self
            .get_all_providers(app.as_str())?
            .values()
            .any(|provider| {
                provider
                    .meta
                    .as_ref()
                    .is_some_and(|meta| meta.live_config_managed == Some(true))
            }))
    }
    pub(crate) fn extension_path(path: &Path) -> PathBuf {
        path.with_file_name(
            if path.file_name().is_some_and(|name| name == "varyai.db") {
                "varyai-ext.db"
            } else {
                "vary-ai.db"
            },
        )
    }

    pub(super) fn is_native_connection(conn: &Connection) -> Result<bool, AppError> {
        Ok(conn.query_row("PRAGMA application_id", [], |r| r.get::<_, i32>(0))? == APPLICATION_ID)
    }

    pub(super) fn native_storage_id(conn: &Connection) -> Result<String, AppError> {
        if !Self::is_native_connection(conn)? {
            return Err(invalid());
        }
        let (id, product, role): (String, String, String) = conn
            .query_row(
                "SELECT storage_id, product, role FROM varyai_storage_identity",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| invalid())?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM varyai_storage_identity", [], |r| {
                r.get(0)
            })?;
        if count != 1
            || product != "varyai"
            || role != "main"
            || uuid::Uuid::parse_str(&id).is_err()
        {
            return Err(invalid());
        }
        Ok(id)
    }

    pub(super) fn write_native_identity(conn: &Connection, id: &str) -> Result<(), AppError> {
        Self::migrate_native_proxy_port(conn)?;
        for (schema, table, role) in [
            ("main", "varyai_storage_identity", "main"),
            ("vary", "varyai_extension_identity", "extension"),
        ] {
            conn.execute_batch(&format!(
                "CREATE TABLE IF NOT EXISTS {schema}.{table} (storage_id TEXT PRIMARY KEY, product TEXT NOT NULL, role TEXT NOT NULL); DELETE FROM {schema}.{table}; PRAGMA {schema}.application_id={APPLICATION_ID};"
            ))?;
            conn.execute(
                &format!("INSERT INTO {schema}.{table} VALUES (?1,'varyai',?2)"),
                [id, role],
            )?;
        }
        conn.execute_batch(&format!(
            "PRAGMA main.user_version={NATIVE_MAIN_SCHEMA_VERSION}; PRAGMA vary.user_version={NATIVE_EXTENSION_SCHEMA_VERSION};"
        ))?;
        Ok(())
    }

    fn migrate_native_proxy_port(conn: &Connection) -> Result<(), AppError> {
        let migrated: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM settings WHERE key='varyai_proxy_port_v1' AND value='true')",
            [], |row| row.get(0),
        )?;
        if !migrated {
            // Keep legacy schema defaults intact; only native destination rows change.
            conn.execute(
                "UPDATE proxy_config SET listen_port=?1, updated_at=datetime('now') WHERE listen_port=15721",
                [crate::proxy::types::DEFAULT_PROXY_PORT],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO settings(key,value) VALUES ('varyai_proxy_port_v1','true')",
                [],
            )?;
        }
        Ok(())
    }

    /// Only validated, private staging copies may be promoted from the legacy layout.
    pub(crate) fn promote_native_pair(path: &Path) -> Result<(), AppError> {
        let conn = Connection::open(path)?;
        if Self::get_user_version(&conn)? != super::SHARED_SCHEMA_VERSION {
            return Err(invalid());
        }
        Self::validate_shared_columns(&conn)?;
        let extension_path = Self::extension_path(path);
        if !extension_path.is_file() {
            return Err(invalid());
        }
        let extension =
            Connection::open_with_flags(&extension_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Self::check_extension_version(&extension)?;
        Self::validate_extension_schema(&extension)?;
        drop(extension);
        Self::attach_extension(&conn, &extension_path)?;
        let tx = conn.unchecked_transaction()?;
        Self::write_native_identity(&tx, &uuid::Uuid::new_v4().to_string())?;
        tx.commit()?;
        conn.execute_batch("DETACH DATABASE vary")?;
        Ok(())
    }

    pub(crate) fn init_native_at(path: &Path) -> Result<Self, AppError> {
        crate::config::validate_independent_data_dir(path.parent().ok_or_else(invalid)?)?;
        let ext_path = Self::extension_path(path);
        let initializing = path.with_file_name(".native-initializing");
        let known_initialization = std::fs::metadata(&initializing)
            .is_ok_and(|meta| meta.len() <= 32)
            && std::fs::read(&initializing).is_ok_and(|bytes| bytes == b"varyai-native-v1");
        super::native_recovery::recover_owned_journals(path, known_initialization)?;
        if !path.exists() {
            if ext_path.exists() {
                return Err(invalid());
            }
            let parent = path.parent().ok_or_else(invalid)?;
            std::fs::create_dir_all(parent).map_err(|_| invalid())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| invalid())?;
            }
            crate::config::atomic_write_private(&initializing, b"varyai-native-v1")?;
        }
        if initializing.exists() {
            if std::fs::metadata(&initializing)
                .map_err(|_| invalid())?
                .len()
                > 32
                || std::fs::read(&initializing).map_err(|_| invalid())? != b"varyai-native-v1"
            {
                return Err(invalid());
            }
            let identified = if path.exists() {
                let inspection =
                    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                Self::is_native_connection(&inspection)?
            } else {
                false
            };
            if !identified {
                drop(Self::init_at(path)?);
                Self::promote_native_pair(path)?;
            }
            std::fs::remove_file(&initializing).map_err(|_| invalid())?;
        }
        if !ext_path.is_file() {
            return Err(invalid());
        }
        for file in [path, ext_path.as_path()] {
            let metadata = std::fs::symlink_metadata(file).map_err(|_| invalid())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(invalid());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() != 1 {
                    return Err(invalid());
                }
            }
        }
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let extension = Connection::open_with_flags(&ext_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let id = Self::native_storage_id(&conn)?;
        let extension_identity: (String, String, String) = extension
            .query_row(
                "SELECT storage_id, product, role FROM varyai_extension_identity",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| invalid())?;
        if !Self::is_native_connection(&extension)?
            || extension_identity != (id.clone(), "varyai".into(), "extension".into())
            || Self::get_user_version(&conn)? != NATIVE_MAIN_SCHEMA_VERSION
            || !matches!(
                Self::get_user_version(&extension)?,
                6 | NATIVE_EXTENSION_SCHEMA_VERSION
            )
        {
            return Err(invalid());
        }
        let count: i64 =
            extension.query_row("SELECT COUNT(*) FROM varyai_extension_identity", [], |r| {
                r.get(0)
            })?;
        if count != 1 {
            return Err(invalid());
        }
        Self::validate_shared_columns(&conn)?;
        let extension_version = Self::get_user_version(&extension)?;
        Self::validate_extension_schema(&extension)?;
        let manifest_path = path.with_file_name("storage.json");
        if manifest_path.exists() {
            let manifest: StorageManifest =
                serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|_| invalid())?)
                    .map_err(|_| invalid())?;
            if manifest.product != "varyai"
                || manifest.layout_version != 1
                || manifest.storage_id != id
            {
                return Err(invalid());
            }
        } else {
            let bytes = serde_json::to_vec_pretty(&StorageManifest {
                product: "varyai".into(),
                layout_version: 1,
                storage_id: id,
            })
            .map_err(|_| invalid())?;
            crate::config::atomic_write_private(&manifest_path, &bytes)?;
        }
        if extension_version == 6 {
            Self::storage_migration_backup(&conn, path)?;
            Self::storage_migration_backup(&extension, &ext_path)?;
        }
        drop((conn, extension));
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        if extension_version == 6 {
            Self::attach_extension(&conn, &ext_path)?;
            let upgrade = (|| {
                let tx = conn.unchecked_transaction()?;
                let template = Connection::open_in_memory()?;
                Self::create_extension_tables_on_conn(&template)?;
                Self::create_attached_extension_tables(&tx, &template)?;
                tx.execute_batch(&format!(
                    "PRAGMA vary.user_version={NATIVE_EXTENSION_SCHEMA_VERSION}"
                ))?;
                tx.commit()?;
                Ok::<_, AppError>(())
            })();
            let detach = conn.execute_batch("DETACH DATABASE vary");
            upgrade?;
            detach?;
        }
        let extension = Connection::open_with_flags(&ext_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let tx = conn.unchecked_transaction()?;
        Self::migrate_native_proxy_port(&tx)?;
        tx.commit()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for file in [path, ext_path.as_path()] {
                std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))
                    .map_err(|_| invalid())?;
            }
        }
        for connection in [&conn, &extension] {
            connection.busy_timeout(Duration::from_secs(5))?;
            connection.execute_batch("PRAGMA foreign_keys=ON")?;
            register_db_change_hook(connection);
        }
        Ok(Self {
            conn: Mutex::new(conn),
            extension_conn: Some(Mutex::new(extension)),
            routing_generation: std::sync::atomic::AtomicU64::new(0),
        })
    }
}
