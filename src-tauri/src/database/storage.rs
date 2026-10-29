//! Shared CC Switch storage and independently versioned Vary extensions.
//! File migrations use an attached rollback-journal transaction so both files
//! commit together, including recovery after a process interruption.
use super::{
    register_db_change_hook, Database, EXTENSION_SCHEMA_VERSION, SCHEMA_VERSION,
    SHARED_SCHEMA_VERSION,
};
use crate::error::AppError;
use rusqlite::{backup::Backup, Connection, OptionalExtension};
use std::{path::Path, sync::Mutex, time::Duration};

pub(super) const EXTENSION_TABLES: &[&str] = &[
    "intent_routing_groups",
    "intent_routing_preferences",
    "gateway_keys",
    "gateway_usage",
    "gateway_agent_bindings",
    "gateway_usage_details",
    "gateway_usage_models",
    "security_audit_logs",
    "security_custom_rules",
    "security_settings",
    "tool_risk_settings",
    "tool_risk_custom_rules",
    "tool_risk_builtin_overrides",
    "tool_risk_events",
    "tool_risk_command_lists",
    "tool_risk_policy_changes",
];

impl Database {
    pub(super) fn lock_extension_connection(
        &self,
    ) -> Result<Option<std::sync::MutexGuard<'_, Connection>>, AppError> {
        self.extension_conn
            .as_ref()
            .map(|m| {
                m.lock()
                    .map_err(|e| AppError::Database(format!("Mutex lock failed: {e}")))
            })
            .transpose()
    }

    pub(crate) fn extension_connection(&self) -> &Mutex<Connection> {
        self.extension_conn.as_ref().unwrap_or(&self.conn)
    }

    pub(crate) fn is_legacy_vary_database(conn: &Connection) -> Result<bool, AppError> {
        let version = Self::get_user_version(conn)?;
        // A version alone is ambiguous: upstream and historical Vary v19-v24 differ.
        if !(19..=SCHEMA_VERSION).contains(&version) {
            return Ok(false);
        }
        for (table, columns) in [
            (
                "security_audit_logs",
                &["id", "timestamp", "rule_id", "matched_snippet_masked"][..],
            ),
            ("security_custom_rules", &["id", "pattern", "action"][..]),
            ("security_settings", &["key", "value"][..]),
        ] {
            for column in columns {
                if !Self::has_column(conn, table, column)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    pub(crate) fn ensure_upstream_shared_columns(conn: &Connection) -> Result<(), AppError> {
        Self::ensure_upstream_shared_columns_through(conn, SHARED_SCHEMA_VERSION)
    }

    pub(super) fn ensure_upstream_shared_columns_through(
        conn: &Connection,
        version: i32,
    ) -> Result<(), AppError> {
        for (introduced, column, tables) in [
            (19, "enabled_mcode", &["mcp_servers", "skills"][..]),
            (20, "enabled_pi", &["mcp_servers"][..]),
            (21, "enabled_agy", &["mcp_servers", "skills"][..]),
            (22, "enabled_kimi", &["mcp_servers", "skills"][..]),
            (23, "enabled_omp", &["mcp_servers"][..]),
        ] {
            if version >= introduced {
                for table in tables {
                    Self::add_column_if_missing(conn, table, column, "BOOLEAN NOT NULL DEFAULT 0")?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn init_at(path: &Path) -> Result<Self, AppError> {
        let parent = path
            .parent()
            .ok_or_else(|| AppError::Database("无效的数据库路径".into()))?;
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let version = Self::get_user_version(&conn)?;
        let legacy = Self::is_legacy_vary_database(&conn)?;
        if version > SHARED_SCHEMA_VERSION && !legacy {
            return Err(AppError::Database(format!(
                "数据库版本过新或结构未知（{version}），当前公共库仅支持 {SHARED_SCHEMA_VERSION}。"
            )));
        }
        if !legacy {
            for table in EXTENSION_TABLES {
                if Self::table_exists(&conn, table)? {
                    return Err(AppError::Database(
                        "无法识别历史 Vary 数据库结构，已保留原库并停止自动迁移。".into(),
                    ));
                }
            }
        }
        let ext_path = parent.join("vary-ai.db");
        // Inspect before any shared schema writes. Unknown extension formats are
        // preserved for a newer application to read.
        let extension_needs_upgrade = if ext_path.exists() {
            let ext =
                Connection::open_with_flags(&ext_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            Self::check_extension_version(&ext)?;
            Self::validate_extension_schema(&ext)?;
            Self::get_user_version(&ext)? < EXTENSION_SCHEMA_VERSION
        } else {
            false
        };
        if legacy || (version > 0 && version < SHARED_SCHEMA_VERSION) || extension_needs_upgrade {
            Self::storage_migration_backup(&conn, path)?;
            if ext_path.exists() {
                let ext = Connection::open_with_flags(
                    &ext_path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                Self::storage_migration_backup(&ext, &ext_path)?;
            }
        }
        Self::attach_extension(&conn, &ext_path)?;
        let result = (|| {
            let tx = rusqlite::Transaction::new_unchecked(
                &conn,
                rusqlite::TransactionBehavior::Immediate,
            )?;
            // Reserve both files before reading/copying data to prevent concurrent
            // startup/import from racing the emptiness check.
            tx.execute_batch(&format!(
                "PRAGMA vary.user_version={EXTENSION_SCHEMA_VERSION};"
            ))?;
            if legacy {
                Self::create_tables_on_conn(&tx)?;
                Self::apply_schema_migrations_on_conn(&tx)?;
            } else {
                Self::create_shared_tables_on_conn(&tx)?;
                Self::apply_shared_schema_migrations(&tx)?;
            }
            Self::ensure_upstream_shared_columns(&tx)?;
            Self::validate_shared_columns(&tx)?;
            let template = Connection::open_in_memory()?;
            Self::create_extension_tables_on_conn(&template)?;
            Self::create_attached_extension_tables(&tx, &template)?;
            if legacy {
                for table in EXTENSION_TABLES {
                    let count: i64 =
                        tx.query_row(&format!("SELECT COUNT(*) FROM vary.\"{table}\""), [], |r| {
                            r.get(0)
                        })?;
                    if count != 0 {
                        return Err(AppError::Database(
                            "Vary 扩展库已有数据，已停止自动迁移以避免覆盖；迁移前备份已保留。"
                                .into(),
                        ));
                    }
                    if Self::table_exists(&tx, table)? {
                        Self::copy_attached_table(&tx, "main", "vary", table)?;
                    }
                }
                for table in EXTENSION_TABLES {
                    tx.execute(&format!("DROP TABLE IF EXISTS main.\"{table}\""), [])?;
                }
            }
            tx.execute_batch(&format!("PRAGMA main.user_version={SHARED_SCHEMA_VERSION}; PRAGMA vary.user_version={EXTENSION_SCHEMA_VERSION};"))?;
            Self::validate_attached_integrity(&tx)?;
            tx.commit()?;
            Ok::<_, AppError>(())
        })();
        let detach = conn.execute_batch("DETACH DATABASE vary;");
        result?;
        detach?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let extension = Connection::open(&ext_path)?;
        extension.busy_timeout(Duration::from_secs(5))?;
        extension.execute_batch("PRAGMA foreign_keys=ON;")?;
        register_db_change_hook(&conn);
        register_db_change_hook(&extension);
        Ok(Self {
            conn: Mutex::new(conn),
            extension_conn: Some(Mutex::new(extension)),
        })
    }

    fn check_extension_version(conn: &Connection) -> Result<(), AppError> {
        let version = Self::get_user_version(conn)?;
        if version > EXTENSION_SCHEMA_VERSION {
            return Err(AppError::Database(format!("Vary 扩展数据库版本过新（{version}），当前支持 {EXTENSION_SCHEMA_VERSION}，请升级应用。")));
        }
        Ok(())
    }

    fn validate_extension_schema(conn: &Connection) -> Result<(), AppError> {
        let template = Connection::open_in_memory()?;
        Self::create_extension_tables_on_conn(&template)?;
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )?;
        let tables = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for table in &tables {
            if !EXTENSION_TABLES.contains(&table.as_str())
                || Self::column_signature(conn, table)? != Self::column_signature(&template, table)?
            {
                return Err(AppError::Database(
                    "未知的 Vary 扩展数据库结构，已保留原库并停止自动迁移。".into(),
                ));
            }
        }
        let version = Self::get_user_version(conn)?;
        let contains = |table: &str| tables.iter().any(|name| name == table);
        // Two development branches independently used v3/v4. Recognize only
        // their complete known layouts; never treat arbitrary missing tables as optional.
        let intent_layout = (version == 3
            && contains("intent_routing_groups")
            && !contains("intent_routing_preferences")
            || version == 4
                && contains("intent_routing_groups")
                && contains("intent_routing_preferences"))
            && !contains("gateway_agent_bindings")
            && !contains("gateway_usage_details")
            && !contains("gateway_usage_models");
        if (3..=4).contains(&version)
            && (contains("intent_routing_groups") || contains("intent_routing_preferences"))
            && !intent_layout
        {
            return Err(AppError::Database(
                "Unknown Vary extension database structure; automatic migration stopped without changing the files.".into(),
            ));
        }
        if version > 0
            && EXTENSION_TABLES.iter().any(|table| {
                let required = match *table {
                    "gateway_keys" | "gateway_usage" => version >= 2,
                    "gateway_agent_bindings" | "gateway_usage_details" => {
                        version >= 3 && !intent_layout
                    }
                    "gateway_usage_models" => version >= 4 && !intent_layout,
                    "intent_routing_groups" => version >= 5 || intent_layout,
                    "intent_routing_preferences" => version >= 5 || intent_layout && version == 4,
                    _ => true,
                };
                required && !contains(table)
            })
        {
            return Err(AppError::Database(
                "Vary 扩展数据库结构不完整，已保留原库。".into(),
            ));
        }
        Ok(())
    }

    fn column_signature(
        conn: &Connection,
        table: &str,
    ) -> Result<Vec<(String, String, bool, i64)>, AppError> {
        let mut stmt = conn.prepare(&format!(
            "PRAGMA table_info({})",
            Self::quote_identifier(table)
        ))?;
        let columns = stmt
            .query_map([], |r| Ok((r.get(1)?, r.get(2)?, r.get(3)?, r.get(5)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(columns)
    }

    fn validate_shared_columns(conn: &Connection) -> Result<(), AppError> {
        let template = Connection::open_in_memory()?;
        Self::create_shared_tables_on_conn(&template)?;
        Self::ensure_upstream_shared_columns(&template)?;
        let mut stmt = template.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )?;
        let tables = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for table in tables {
            for (column, _, _, _) in Self::column_signature(&template, &table)? {
                if !Self::has_column(conn, &table, &column)? {
                    return Err(AppError::Database(
                        "公共数据库结构不兼容，已保留原库并停止自动迁移。".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn attach_extension(conn: &Connection, path: &Path) -> Result<(), AppError> {
        conn.execute(
            "ATTACH DATABASE ?1 AS vary",
            [path.to_string_lossy().as_ref()],
        )?;
        // WAL does not provide cross-file atomic commit. Changing journal modes
        // fails safely if another application owns an incompatible active lock.
        let result = (|| {
            conn.execute_batch("PRAGMA main.journal_mode=DELETE; PRAGMA vary.journal_mode=DELETE; PRAGMA main.synchronous=FULL; PRAGMA vary.synchronous=FULL;")?;
            for schema in ["main", "vary"] {
                let mode: String =
                    conn.query_row(&format!("PRAGMA {schema}.journal_mode"), [], |r| r.get(0))?;
                if mode != "delete" {
                    return Err(AppError::Database("无法建立数据库原子迁移事务".into()));
                }
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = conn.execute_batch("DETACH DATABASE vary;");
        }
        result
    }

    fn storage_migration_backup(conn: &Connection, path: &Path) -> Result<(), AppError> {
        let parent = path
            .parent()
            .ok_or_else(|| AppError::Database("无效的备份路径".into()))?;
        let dir = parent.join("backups/storage-split");
        std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
        let mut file = tempfile::Builder::new()
            .prefix(
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("database"),
            )
            .suffix(".db")
            .tempfile_in(&dir)
            .map_err(|e| AppError::io(&dir, e))?;
        let mut backup_conn = Connection::open(file.path())?;
        let backup = Backup::new(conn, &mut backup_conn)?;
        Self::complete_backup(&backup, "创建分库迁移前备份")?;
        drop(backup);
        drop(backup_conn);
        file.as_file_mut()
            .sync_all()
            .map_err(|e| AppError::io(file.path(), e))?;
        file.keep().map_err(|e| AppError::io(&dir, e.error))?;
        Ok(())
    }

    fn create_attached_extension_tables(
        conn: &Connection,
        template: &Connection,
    ) -> Result<(), AppError> {
        let mut stmt = template.prepare("SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY type='table' DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (kind, name, sql) = row?;
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM vary.sqlite_master WHERE name=?1)",
                [&name],
                |r| r.get(0),
            )?;
            if !exists {
                conn.execute_batch(&Self::qualify_schema_sql(&sql, &kind, &name, "vary")?)?;
            }
        }
        Ok(())
    }

    fn qualify_schema_sql(
        sql: &str,
        kind: &str,
        name: &str,
        schema: &str,
    ) -> Result<String, AppError> {
        // Only locally generated or already staged sqlite_master SQL reaches here.
        // Keep the complete definition, including constraints and table options.
        let re = regex::Regex::new(r#"(?is)^CREATE\s+(?:UNIQUE\s+)?(?:TABLE|INDEX|VIEW|TRIGGER)\s+(?:IF\s+NOT\s+EXISTS\s+)?(?:"(?:[^"]|"")*"|`[^`]*`|\[[^\]]*\]|[^\s(]+)"#)
            .map_err(|_| AppError::Database("无法解析数据库对象定义".into()))?;
        let matched = re
            .find(sql)
            .ok_or_else(|| AppError::Database("未知数据库对象定义，已停止迁移".into()))?;
        let unique = if kind == "index" && sql.to_uppercase().starts_with("CREATE UNIQUE") {
            "UNIQUE "
        } else {
            ""
        };
        Ok(format!(
            "CREATE {unique}{} {schema}.{}{}",
            kind.to_uppercase(),
            Self::quote_identifier(name),
            &sql[matched.end()..]
        ))
    }

    fn copy_attached_table(
        conn: &Connection,
        source: &str,
        target: &str,
        table: &str,
    ) -> Result<(), AppError> {
        let quoted = Self::quote_identifier(table);
        let columns = {
            let mut stmt = conn.prepare(&format!("PRAGMA {source}.table_info({quoted})"))?;
            let names = stmt
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            names
                .iter()
                .map(|n| Self::quote_identifier(n))
                .collect::<Vec<_>>()
                .join(",")
        };
        conn.execute(
            &format!(
                "INSERT INTO {target}.{quoted} ({columns}) SELECT {columns} FROM {source}.{quoted}"
            ),
            [],
        )?;
        let seq: Option<i64> = conn
            .query_row(
                &format!("SELECT seq FROM {source}.sqlite_sequence WHERE name=?1"),
                [table],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(seq) = seq {
            conn.execute(
                &format!("DELETE FROM {target}.sqlite_sequence WHERE name=?1"),
                [table],
            )?;
            conn.execute(
                &format!("INSERT INTO {target}.sqlite_sequence(name,seq) VALUES (?1,?2)"),
                rusqlite::params![table, seq],
            )?;
        }
        Ok(())
    }

    fn validate_attached_integrity(conn: &Connection) -> Result<(), AppError> {
        for schema in ["main", "vary"] {
            let result: String =
                conn.query_row(&format!("PRAGMA {schema}.integrity_check"), [], |r| {
                    r.get(0)
                })?;
            let mut foreign_keys = conn.prepare(&format!("PRAGMA {schema}.foreign_key_check"))?;
            let invalid_foreign_key = foreign_keys.query([])?.next()?.is_some();
            if result != "ok" || invalid_foreign_key {
                return Err(AppError::Database("分库完整性校验失败，已回滚迁移".into()));
            }
        }
        Ok(())
    }

    /// Normalize official and historical exports to the portable monolithic
    /// format. That format is a transport artifact, never the live shared file.
    pub(super) fn prepare_portable_database(conn: &Connection) -> Result<(), AppError> {
        let legacy = Self::is_legacy_vary_database(conn)?;
        if Self::get_user_version(conn)? > SHARED_SCHEMA_VERSION && !legacy {
            return Err(AppError::Database("数据库版本过新或结构未知".into()));
        }
        if legacy {
            Self::create_tables_on_conn(conn)?;
            Self::apply_schema_migrations_on_conn(conn)?;
        } else {
            Self::create_shared_tables_on_conn(conn)?;
            Self::apply_shared_schema_migrations(conn)?;
            Self::create_extension_tables_on_conn(conn)?;
            Self::set_user_version(conn, SCHEMA_VERSION)?;
        }
        Self::ensure_upstream_shared_columns(conn)?;
        Ok(())
    }

    /// Replace both live schemas from an already validated portable staging
    /// file. The caller owns shared then extension mutexes throughout.
    pub(super) fn replace_split_storage(conn: &Connection, staging: &Path) -> Result<(), AppError> {
        let path = conn
            .path()
            .ok_or_else(|| AppError::Database("数据库文件路径不可用".into()))?;
        let ext_path = Path::new(path).with_file_name("vary-ai.db");
        Self::attach_extension(conn, &ext_path)?;
        let result = (|| {
            conn.execute(
                "ATTACH DATABASE ?1 AS incoming",
                [staging.to_string_lossy().as_ref()],
            )?;
            conn.execute_batch("PRAGMA foreign_keys=OFF;")?;
            let tx = conn.unchecked_transaction()?;
            for target in ["main", "vary"] {
                let objects = {
                    let mut stmt = tx.prepare(&format!("SELECT type,name FROM {target}.sqlite_master WHERE name NOT LIKE 'sqlite_%' AND type IN ('table','view','trigger') ORDER BY type='table'"))?;
                    let rows = stmt
                        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                        .collect::<Result<Vec<_>, _>>()?;
                    rows
                };
                for (kind, name) in objects {
                    tx.execute_batch(&format!(
                        "DROP {} IF EXISTS {target}.{}",
                        kind.to_uppercase(),
                        Self::quote_identifier(&name)
                    ))?;
                }
            }
            let objects = {
                let mut stmt = tx.prepare("SELECT type,name,tbl_name,sql FROM incoming.sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY type='table' DESC, type='index' DESC")?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            for (kind, name, table, sql) in objects {
                let target = if EXTENSION_TABLES.contains(&table.as_str()) {
                    "vary"
                } else {
                    "main"
                };
                tx.execute_batch(&Self::qualify_schema_sql(&sql, &kind, &name, target)?)?;
                if kind == "table" {
                    Self::copy_attached_table(&tx, "incoming", target, &name)?;
                }
            }
            tx.execute_batch(&format!("PRAGMA main.user_version={SHARED_SCHEMA_VERSION}; PRAGMA vary.user_version={EXTENSION_SCHEMA_VERSION};"))?;
            Self::validate_attached_integrity(&tx)?;
            tx.commit()?;
            Ok::<_, AppError>(())
        })();
        let _ = conn.execute_batch("DETACH DATABASE incoming;");
        let detach = conn.execute_batch("DETACH DATABASE vary; PRAGMA foreign_keys=ON;");
        result?;
        detach?;
        Ok(())
    }
}
