use crate::database::Database;
use crate::services::intent_routing::{discover_groups, RoutingCatalog};
use crate::services::intent_routing::{eligible_provider, valid_id, RoutingGroup, MAX_GROUPS};
use rusqlite::{params, OptionalExtension};

const FAILED: &str = "intentRouting.failed";

impl Database {
    pub(crate) fn routing_context_settings(
        &self,
    ) -> Result<crate::services::intent_routing::context::Settings, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM intent_routing_preferences WHERE key='context_settings'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| FAILED)?;
        let settings: crate::services::intent_routing::context::Settings = value
            .map(|text| serde_json::from_str(&text).map_err(|_| FAILED))
            .transpose()?
            .unwrap_or_default();
        if !settings.valid() {
            return Err(FAILED.into());
        }
        Ok(settings)
    }

    pub(crate) fn set_routing_context_settings(
        &self,
        settings: &crate::services::intent_routing::context::Settings,
    ) -> Result<(), String> {
        if !settings.valid() {
            return Err("intentRouting.invalid".into());
        }
        let value = serde_json::to_string(settings).map_err(|_| FAILED)?;
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        conn.execute("INSERT INTO intent_routing_preferences(key,value) VALUES ('context_settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [value]).map_err(|_| FAILED)?;
        drop(conn);
        self.invalidate_routing_resources();
        Ok(())
    }

    pub(crate) fn routing_auto_group_skip(&self) -> Result<Vec<String>, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM intent_routing_preferences WHERE key='auto_group_skip'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| FAILED)?;
        let ids: Vec<String> = value
            .map(|text| serde_json::from_str(&text).map_err(|_| FAILED))
            .transpose()?
            .unwrap_or_default();
        if ids.len() > 256
            || ids
                .iter()
                .any(|id| !crate::services::routing_upstreams::valid_identity(id))
        {
            return Err(FAILED.into());
        }
        Ok(ids)
    }

    pub(crate) fn set_routing_auto_group_skip(&self, ids: Vec<String>) -> Result<(), String> {
        if ids.len() > 256
            || ids
                .iter()
                .any(|id| !crate::services::routing_upstreams::valid_identity(id))
        {
            return Err("intentRouting.invalid".into());
        }
        let ids = ids.into_iter().collect::<std::collections::BTreeSet<_>>();
        let value = serde_json::to_string(&ids).map_err(|_| FAILED)?;
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        conn.execute("INSERT INTO intent_routing_preferences(key,value) VALUES ('auto_group_skip',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [value]).map_err(|_| FAILED)?;
        drop(conn);
        self.invalidate_routing_resources();
        Ok(())
    }

    pub(crate) fn set_intent_routing_auto_create(&self, enabled: bool) -> Result<(), String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        conn.execute(
            "INSERT INTO intent_routing_preferences(key,value) VALUES ('auto_create',?1)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [if enabled { "true" } else { "false" }],
        )
        .map_err(|_| FAILED)?;
        drop(conn);
        self.invalidate_routing_resources();
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
                .prepare(
                    "SELECT key,value FROM intent_routing_preferences WHERE key LIKE 'hidden:%'",
                )
                .map_err(|_| FAILED)?;
            let hidden = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(|_| FAILED)?
                .collect::<Result<std::collections::BTreeMap<_, _>, _>>()
                .map_err(|_| FAILED)?;
            (enabled.as_deref() != Some("false"), hidden)
        };
        let mut omitted_groups = 0;
        let providers = match providers {
            Some(providers) => providers,
            None => self.routing_providers().map_err(|_| FAILED)?,
        };
        let active = providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let mut discovery = providers.clone();
        // Disabled resources remain in discovered groups, but never in execution snapshots.
        discovery.extend(
            self.routing_catalog_providers(true)
                .map_err(|_| FAILED)?
                .into_iter()
                .filter(|provider| !active.contains(provider.id.as_str())),
        );
        let skipped = self.routing_auto_group_skip()?;
        let stand_in_sources = discovery.clone();
        discovery.retain(|provider| !skipped.contains(&provider.id));
        let mut removed_groups = hidden
            .iter()
            .filter_map(|(key, value)| {
                let id = key.strip_prefix("hidden:")?;
                let record: crate::services::intent_routing::RemovedGroup =
                    serde_json::from_str(value).unwrap_or_else(|_| {
                        crate::services::intent_routing::RemovedGroup {
                            id: id.into(),
                            name: id.into(),
                        }
                    });
                (record.id == id
                    && valid_id(id)
                    && crate::services::intent_routing::valid_text(&record.name, 80))
                .then_some(record)
            })
            .collect::<Vec<_>>();
        let mut stand_ins = Vec::new();
        {
            // No extension lock is held while reading the shared provider database.
            let found = if auto_create {
                discover_groups(discovery)
            } else {
                crate::services::intent_routing::discover_stand_ins(discovery)
            };
            for mut group in found {
                let legacy = crate::services::intent_routing::legacy_ids(&group);
                if groups.iter().any(|g| g.id == group.id)
                    || legacy.iter().any(|id| groups.iter().any(|g| &g.id == id))
                {
                    continue;
                }
                if auto_create
                    && (hidden.contains_key(&format!("hidden:{}", group.id))
                        || legacy
                            .iter()
                            .any(|id| hidden.contains_key(&format!("hidden:{id}"))))
                {
                    removed_groups.retain(|record| !legacy.contains(&record.id));
                    if !removed_groups.iter().any(|record| record.id == group.id) {
                        removed_groups.push(crate::services::intent_routing::RemovedGroup {
                            id: group.id,
                            name: group.name,
                        });
                    } else if let Some(record) = removed_groups
                        .iter_mut()
                        .find(|record| record.id == group.id && record.name == group.id)
                    {
                        record.name = group.name;
                    }
                    continue;
                }
                group.off = group
                    .members
                    .iter()
                    .filter(|member| !active.contains(member.provider_id.as_str()))
                    .map(|member| member.id.clone())
                    .collect();
                if !auto_create {
                    if let Some(member) = group
                        .members
                        .iter()
                        .find(|member| active.contains(member.provider_id.as_str()))
                        .cloned()
                    {
                        group.members = vec![member.clone()];
                        group.default_member_id = member.id;
                        group.fallback_member_ids.clear();
                        group.off.clear();
                        stand_ins.push(group);
                    }
                    continue;
                }
                if groups.len() >= MAX_GROUPS || group.members.len() > 256 {
                    omitted_groups += 1;
                    continue;
                }
                groups.push(group);
            }
        }
        if auto_create && !skipped.is_empty() {
            for mut group in crate::services::intent_routing::discover_stand_ins(stand_in_sources) {
                if groups.iter().any(|current| current.id == group.id)
                    || hidden.contains_key(&format!("hidden:{}", group.id))
                {
                    continue;
                }
                if let Some(member) = group
                    .members
                    .iter()
                    .find(|member| {
                        active.contains(member.provider_id.as_str())
                            && !skipped.contains(&member.provider_id)
                    })
                    .or_else(|| {
                        group
                            .members
                            .iter()
                            .find(|member| active.contains(member.provider_id.as_str()))
                    })
                    .cloned()
                {
                    group.members = vec![member.clone()];
                    group.default_member_id = member.id;
                    group.fallback_member_ids.clear();
                    group.off.clear();
                    stand_ins.push(group);
                }
            }
        }
        self.apply_routing_group_order(&mut groups)?;
        let mut matched_members = std::collections::BTreeMap::new();
        for group in &groups {
            if group.matches.is_empty() {
                continue;
            }
            match crate::services::intent_routing::membership::resolve(group.clone(), &providers) {
                Ok(resolved) => {
                    matched_members.insert(
                        group.id.clone(),
                        resolved
                            .members
                            .into_iter()
                            .filter(|member| resolved.matched.contains(&member.id))
                            .collect(),
                    );
                }
                Err(_) => omitted_groups += 1,
            }
        }
        Ok(RoutingCatalog {
            groups,
            removed_groups,
            stand_ins,
            matched_members,
            auto_create,
            omitted_groups,
            context_settings: self.routing_context_settings()?,
        })
    }

    pub(crate) fn restore_discovered_routing_group(&self, id: &str) -> Result<(), String> {
        if !valid_id(id) || !id.starts_with("auto-") {
            return Err("intentRouting.invalid".into());
        }
        let providers = self.routing_catalog_providers(true).map_err(|_| FAILED)?;
        let skipped = self.routing_auto_group_skip()?;
        let found = discover_groups(
            providers
                .into_iter()
                .filter(|provider| !skipped.contains(&provider.id)),
        )
        .into_iter()
        .find(|group| group.id == id);
        let mut keys = vec![format!("hidden:{id}")];
        if let Some(group) = found {
            keys.extend(
                crate::services::intent_routing::legacy_ids(&group)
                    .into_iter()
                    .map(|id| format!("hidden:{id}")),
            );
        }
        let mut conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let tx = conn.transaction().map_err(|_| FAILED)?;
        for key in keys {
            tx.execute("DELETE FROM intent_routing_preferences WHERE key=?1", [key])
                .map_err(|_| FAILED)?;
        }
        tx.commit().map_err(|_| FAILED)?;
        self.invalidate_routing_resources();
        Ok(())
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
        let mut aliases = std::collections::HashSet::new();
        for (id, revision, config) in rows {
            if config.len() > 65536 {
                return Err(invalid());
            }
            let mut group: RoutingGroup = serde_json::from_str(&config).map_err(|_| invalid())?;
            group.validate().map_err(|_| invalid())?;
            if group.id != id || group.revision != revision || !aliases.insert(group.alias()) {
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
        let mut groups: Vec<RoutingGroup> = rows
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
        let mut aliases = std::collections::HashSet::new();
        if groups.len() > MAX_GROUPS || groups.iter().any(|g| !aliases.insert(g.alias())) {
            return Err(FAILED.into());
        }
        drop(stmt);
        drop(conn);
        self.apply_routing_group_order(&mut groups)?;
        Ok(groups)
    }

    fn apply_routing_group_order(&self, groups: &mut [RoutingGroup]) -> Result<(), String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM intent_routing_preferences WHERE key='group_order'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| FAILED)?;
        if let Some(value) = value {
            if value.len() > 16384 {
                return Err(FAILED.into());
            }
            let order: Vec<String> = serde_json::from_str(&value).map_err(|_| FAILED)?;
            if order.len() > MAX_GROUPS
                || order.iter().any(|id| !valid_id(id))
                || order.iter().collect::<std::collections::HashSet<_>>().len() != order.len()
            {
                return Err(FAILED.into());
            }
            groups.sort_by_key(|group| {
                order
                    .iter()
                    .position(|id| id == &group.id)
                    .unwrap_or(usize::MAX)
            });
        }
        Ok(())
    }

    pub(crate) fn set_routing_group_order(
        &self,
        order: Vec<String>,
        providers: Option<Vec<crate::provider::Provider>>,
    ) -> Result<(), String> {
        let catalog = self.intent_routing_catalog_with_providers(providers)?;
        let ids = order.iter().collect::<std::collections::HashSet<_>>();
        if order.len() > MAX_GROUPS
            || ids.len() != order.len()
            || order.len() != catalog.groups.len()
            || catalog.groups.iter().any(|group| !ids.contains(&group.id))
        {
            return Err("intentRouting.conflict".into());
        }
        let value = serde_json::to_string(&order).map_err(|_| FAILED)?;
        let mut conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let tx = conn.transaction().map_err(|_| FAILED)?;
        {
            let mut stmt = tx
                .prepare("SELECT id FROM intent_routing_groups")
                .map_err(|_| FAILED)?;
            let saved = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|_| FAILED)?;
            let actual = saved
                .collect::<Result<std::collections::HashSet<_>, _>>()
                .map_err(|_| FAILED)?;
            let expected = catalog
                .groups
                .iter()
                .filter(|group| !group.auto_generated)
                .map(|group| group.id.clone())
                .collect();
            if actual != expected {
                return Err("intentRouting.conflict".into());
            }
        }
        tx.execute("INSERT INTO intent_routing_preferences(key,value) VALUES ('group_order',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [value]).map_err(|_| FAILED)?;
        tx.commit().map_err(|_| FAILED)?;
        self.invalidate_routing_resources();
        Ok(())
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
        let identities: std::collections::HashSet<_> = self
            .list_routing_upstreams()
            .map_err(|_| FAILED)?
            .into_iter()
            .map(|p| p.id)
            .collect();
        let providers: std::collections::HashMap<_, _> = self
            .routing_providers()
            .map_err(|_| FAILED)?
            .into_iter()
            .map(|p| (p.id.clone(), p))
            .collect();
        if group.members.iter().any(|m| {
            if require_task_transport && !group.includes_in_menu(m) {
                return false;
            }
            if require_task_transport {
                !providers
                    .get(&m.provider_id)
                    .is_some_and(|p| eligible_provider(p, false))
            } else {
                !identities.contains(&m.provider_id)
            }
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
        group = crate::services::intent_routing::membership::definition(group);
        group.validate()?;
        group.client = "global".into();
        // Resource edits do not activate any client configuration.
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
        {
            let mut stmt = tx
                .prepare("SELECT config FROM intent_routing_groups WHERE id != ?1 LIMIT 33")
                .map_err(|_| FAILED)?;
            let rows = stmt
                .query_map([&group.id], |r| r.get::<_, String>(0))
                .map_err(|_| FAILED)?;
            for row in rows {
                let config = row.map_err(|_| FAILED)?;
                let existing: RoutingGroup = serde_json::from_str(&config).map_err(|_| FAILED)?;
                if existing.alias() == group.alias() {
                    return Err("intentRouting.invalid".into());
                }
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
        if group.id.starts_with("auto-") {
            for id in std::iter::once(group.id.clone())
                .chain(crate::services::intent_routing::legacy_ids(&group))
            {
                tx.execute(
                    "DELETE FROM intent_routing_preferences WHERE key=?1",
                    [format!("hidden:{id}")],
                )
                .map_err(|_| FAILED)?;
            }
        }
        tx.commit().map_err(|_| FAILED)?;
        self.invalidate_routing_resources();
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
        let group = self
            .intent_routing_catalog_with_providers(providers)?
            .groups
            .into_iter()
            .find(|g| g.id == id);
        if revision == 0 && !group.as_ref().is_some_and(|g| g.auto_generated) {
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
            let value = group
                .map(|group| {
                    serde_json::to_string(&crate::services::intent_routing::RemovedGroup {
                        id: group.id,
                        name: group.name,
                    })
                })
                .transpose()
                .map_err(|_| FAILED)?
                .unwrap_or_else(|| "true".into());
            tx.execute(
                "INSERT OR REPLACE INTO intent_routing_preferences(key,value) VALUES (?1,?2)",
                [format!("hidden:{id}"), value],
            )
            .map_err(|_| FAILED)?;
        }
        tx.commit().map_err(|_| FAILED)?;
        self.invalidate_routing_resources();
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
