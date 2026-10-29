use crate::database::Database;
use crate::services::intent_routing::{discover_groups, RoutingCatalog};
use crate::services::intent_routing::{eligible_provider, valid_id, RoutingGroup, MAX_GROUPS};
use rusqlite::{params, OptionalExtension};

const FAILED: &str = "intentRouting.failed";

impl Database {
    pub(crate) fn set_intent_routing_auto_create(&self, enabled: bool) -> Result<(), String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        conn.execute(
            "INSERT INTO intent_routing_preferences(key,value) VALUES ('auto_create',?1)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [if enabled { "true" } else { "false" }],
        )
        .map_err(|_| FAILED)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn intent_routing_catalog(&self) -> Result<RoutingCatalog, String> {
        self.intent_routing_catalog_with_providers(None)
    }

    pub(crate) fn intent_routing_catalog_with_providers(
        &self,
        providers: Option<Vec<crate::provider::Provider>>,
    ) -> Result<RoutingCatalog, String> {
        let mut groups = self.list_intent_routing_groups()?;
        let (auto_create, hidden) = {
            let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
            let enabled: Option<String> = conn
                .query_row(
                    "SELECT value FROM intent_routing_preferences WHERE key='auto_create'",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|_| FAILED)?;
            let mut stmt = conn
                .prepare("SELECT key FROM intent_routing_preferences WHERE key LIKE 'hidden:%'")
                .map_err(|_| FAILED)?;
            let hidden = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|_| FAILED)?
                .collect::<Result<std::collections::HashSet<_>, _>>()
                .map_err(|_| FAILED)?;
            (enabled.as_deref() == Some("true"), hidden)
        };
        let mut omitted_groups = 0;
        if auto_create {
            // No extension lock is held while reading the shared provider database.
            let providers = match providers {
                Some(p) => p,
                None => self
                    .get_all_providers("codex")
                    .map_err(|_| FAILED)?
                    .into_values()
                    .collect(),
            };
            for group in discover_groups(providers) {
                let legacy = crate::services::intent_routing::legacy_ids(&group);
                if groups.iter().any(|g| g.id == group.id)
                    || hidden.contains(&format!("hidden:{}", group.id))
                    || legacy.iter().any(|id| {
                        groups.iter().any(|g| &g.id == id)
                            || hidden.contains(&format!("hidden:{id}"))
                    })
                {
                    continue;
                }
                if groups.len() >= MAX_GROUPS || group.members.len() > 16 {
                    omitted_groups += 1;
                    continue;
                }
                groups.push(group);
            }
        }
        Ok(RoutingCatalog {
            groups,
            auto_create,
            omitted_groups,
        })
    }

    pub(crate) fn normalize_imported_intent_routing(
        conn: &rusqlite::Connection,
    ) -> Result<(), crate::error::AppError> {
        use crate::error::AppError;
        let invalid = || AppError::Database("Invalid intent-routing import".into());
        let mut stmt =
            conn.prepare("SELECT id, revision, config FROM intent_routing_groups LIMIT 33")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() > MAX_GROUPS {
            return Err(invalid());
        }
        for (id, revision, config) in rows {
            if config.len() > 65536 {
                return Err(invalid());
            }
            let mut group: RoutingGroup = serde_json::from_str(&config).map_err(|_| invalid())?;
            group.validate().map_err(|_| invalid())?;
            if group.id != id || group.revision != revision {
                return Err(invalid());
            }
            group.enabled = false;
            group.intent_recognition_enabled = false;
            group.auto_generated = false;
            conn.execute(
                "UPDATE intent_routing_groups SET config=?1 WHERE id=?2",
                params![serde_json::to_string(&group).map_err(|_| invalid())?, id],
            )?;
        }
        conn.execute(
            "INSERT INTO intent_routing_preferences(key,value) VALUES ('auto_create','false')
            ON CONFLICT(key) DO UPDATE SET value='false'",
            [],
        )?;
        conn.execute(
            "DELETE FROM intent_routing_preferences WHERE key LIKE 'model_cache:%'",
            [],
        )?;
        Ok(())
    }

    pub(crate) fn list_intent_routing_groups(&self) -> Result<Vec<RoutingGroup>, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let mut stmt = conn
            .prepare("SELECT config FROM intent_routing_groups ORDER BY id LIMIT 33")
            .map_err(|_| FAILED)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|_| FAILED)?;
        let groups: Vec<RoutingGroup> = rows
            .map(|r| {
                let json = r.map_err(|_| FAILED)?;
                if json.len() > 65536 {
                    return Err(FAILED.into());
                }
                let group: RoutingGroup = serde_json::from_str(&json).map_err(|_| FAILED)?;
                group.validate().map_err(|_| FAILED)?;
                Ok(group)
            })
            .collect::<Result<_, String>>()?;
        if groups.len() > MAX_GROUPS {
            return Err(FAILED.into());
        }
        Ok(groups)
    }

    pub(crate) fn validate_intent_routing_providers(
        &self,
        group: &RoutingGroup,
    ) -> Result<(), String> {
        self.validate_intent_routing_provider_references(group, true)
    }

    fn validate_intent_routing_provider_references(
        &self,
        group: &RoutingGroup,
        require_task_transport: bool,
    ) -> Result<(), String> {
        // Resolve shared providers before acquiring the extension connection.
        let providers = self.get_all_providers("codex").map_err(|_| FAILED)?;
        if group.members.iter().any(|m| {
            if require_task_transport && !group.includes_in_menu(m) {
                return false;
            }
            !providers
                .get(&m.provider_id)
                .is_some_and(|p| !require_task_transport || eligible_provider(p, false))
        }) || (group.routing != crate::services::intent_routing::RoutingStrategy::Manual
            && group.classifier_target.as_ref().is_some_and(|t| {
                !providers
                    .get(&t.provider_id)
                    .is_some_and(|p| eligible_provider(p, true))
            }))
        {
            return Err("intentRouting.providerUnavailable".into());
        }
        Ok(())
    }

    pub(crate) fn save_intent_routing_group(
        &self,
        mut group: RoutingGroup,
    ) -> Result<RoutingGroup, String> {
        group.validate()?;
        // Draft-only until continuity, privacy and exact-model forwarding are integrated.
        if group.enabled {
            return Err("intentRouting.notReady".into());
        }
        // Draft membership requires an existing provider, not a supported transport.
        self.validate_intent_routing_provider_references(&group, false)?;
        group.auto_generated = false;
        let mut conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let tx = conn.transaction().map_err(|_| FAILED)?;
        let current: Option<u32> = tx
            .query_row(
                "SELECT revision FROM intent_routing_groups WHERE id=?1",
                [&group.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| FAILED)?;
        if current.unwrap_or(0) != group.revision {
            return Err("intentRouting.conflict".into());
        }
        if current.is_none() {
            let count: usize = tx
                .query_row("SELECT COUNT(*) FROM intent_routing_groups", [], |r| {
                    r.get(0)
                })
                .map_err(|_| FAILED)?;
            if count >= MAX_GROUPS {
                return Err("intentRouting.limit".into());
            }
        }
        group.revision = group.revision.checked_add(1).ok_or(FAILED)?;
        let config = serde_json::to_string(&group).map_err(|_| FAILED)?;
        if config.len() > 65536 {
            return Err("intentRouting.invalid".into());
        }
        tx.execute(
            "INSERT INTO intent_routing_groups(id,revision,config) VALUES (?1,?2,?3)
            ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,config=excluded.config",
            params![group.id, group.revision, config],
        )
        .map_err(|_| FAILED)?;
        tx.commit().map_err(|_| FAILED)?;
        Ok(group)
    }

    #[cfg(test)]
    pub(crate) fn delete_intent_routing_group(
        &self,
        id: &str,
        revision: u32,
    ) -> Result<(), String> {
        self.delete_intent_routing_group_with_providers(id, revision, None)
    }

    pub(crate) fn delete_intent_routing_group_with_providers(
        &self,
        id: &str,
        revision: u32,
        providers: Option<Vec<crate::provider::Provider>>,
    ) -> Result<(), String> {
        if !valid_id(id) {
            return Err("intentRouting.invalid".into());
        }
        if revision == 0
            && !self
                .intent_routing_catalog_with_providers(providers)?
                .groups
                .iter()
                .any(|g| g.id == id && g.auto_generated)
        {
            return Err("intentRouting.conflict".into());
        }
        let mut conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let tx = conn.transaction().map_err(|_| FAILED)?;
        if revision == 0 {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM intent_routing_groups WHERE id=?1)",
                    [id],
                    |r| r.get(0),
                )
                .map_err(|_| FAILED)?;
            if exists {
                return Err("intentRouting.conflict".into());
            }
        }
        let changed = tx
            .execute(
                "DELETE FROM intent_routing_groups WHERE id=?1 AND revision=?2",
                params![id, revision],
            )
            .map_err(|_| FAILED)?;
        if revision != 0 && changed != 1 {
            return Err("intentRouting.conflict".into());
        }
        if id.starts_with("auto-") {
            tx.execute(
                "INSERT OR REPLACE INTO intent_routing_preferences(key,value) VALUES (?1,'true')",
                [format!("hidden:{id}")],
            )
            .map_err(|_| FAILED)?;
        }
        tx.commit().map_err(|_| FAILED)?;
        Ok(())
    }
}

impl Database {
    pub(crate) fn routing_model_cache(
        &self,
        key: &str,
    ) -> Result<Option<crate::services::intent_routing::catalog::ModelCache>, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM intent_routing_preferences WHERE key=?1",
                [format!("model_cache:{key}")],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| FAILED)?;
        Ok(value
            .filter(|s| s.len() <= 1024 * 1024)
            .and_then(|s| serde_json::from_str(&s).ok()))
    }
    pub(crate) fn save_routing_model_cache(
        &self,
        key: &str,
        cache: &crate::services::intent_routing::catalog::ModelCache,
    ) -> Result<(), String> {
        let value = serde_json::to_string(cache).map_err(|_| FAILED)?;
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        // Bounded cache history; entries contain only hashed identity, timestamps and model IDs.
        conn.execute("DELETE FROM intent_routing_preferences WHERE key IN (SELECT key FROM intent_routing_preferences WHERE key LIKE 'model_cache:%' ORDER BY rowid DESC LIMIT -1 OFFSET 63)", []).map_err(|_| FAILED)?;
        conn.execute("INSERT INTO intent_routing_preferences(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![format!("model_cache:{key}"),value]).map_err(|_| FAILED)?;
        Ok(())
    }
}
