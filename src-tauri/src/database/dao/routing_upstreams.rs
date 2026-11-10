//! Native provider registry. Client configuration is not a provider source.
use crate::database::Database;
use crate::provider::Provider;
use crate::services::routing_upstreams::{
    resolve_provider, valid_identity, RoutingUpstream, UpstreamSource, INVALID, MAX_CONFIG_BYTES,
    MAX_UPSTREAMS,
};
use rusqlite::{params, Connection};
use std::collections::HashSet;
const FAILED: &str = "routingUpstreams.failed";
const CONFLICT: &str = "routingUpstreams.conflict";
const LIMIT: &str = "routingUpstreams.limit";
#[path = "routing_upstreams_identity.rs"]
mod identity;
fn read_records(conn: &Connection) -> Result<Vec<RoutingUpstream>, String> {
    let mut stmt = conn
        .prepare("SELECT id,revision,config FROM routing_upstreams ORDER BY rowid LIMIT 257")
        .map_err(|_| FAILED)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| FAILED)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| FAILED)?;
    if rows.len() > MAX_UPSTREAMS {
        return Err(LIMIT.into());
    }
    let mut ids = HashSet::new();
    rows.into_iter()
        .map(|(id, revision, config)| {
            if config.len() > MAX_CONFIG_BYTES {
                return Err(INVALID.into());
            }
            let record: RoutingUpstream = serde_json::from_str(&config).map_err(|_| INVALID)?;
            record.validate()?;
            if record.id != id || record.revision != revision || revision == 0 || !ids.insert(id) {
                return Err(INVALID.into());
            }
            Ok(record)
        })
        .collect()
}

fn write_record(conn: &Connection, record: &RoutingUpstream) -> Result<(), String> {
    let config = serde_json::to_string(record).map_err(|_| FAILED)?;
    conn.execute("INSERT INTO routing_upstreams(id,revision,config) VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,config=excluded.config",
        params![record.id, record.revision, config]).map_err(|_| FAILED)?;
    Ok(())
}

fn restore_transport(
    record: &mut RoutingUpstream,
    current: &RoutingUpstream,
) -> Result<(), String> {
    let (Some(next), Some(old)) = (&mut record.transport, &current.transport) else {
        return Ok(());
    };
    let same_destination = next.endpoints == old.endpoints
        && next.full_url == old.full_url
        && next.primary_protocol == old.primary_protocol
        && match (&record.source, &current.source) {
            (
                UpstreamSource::Api {
                    base_url: a,
                    api_format,
                    auth_type: aa,
                    ..
                },
                UpstreamSource::Api {
                    base_url: b,
                    auth_type: ba,
                    ..
                },
            ) => {
                (a == b
                    || old.primary_protocol.is_none() && old.endpoints.get(api_format) == Some(a))
                    && aa == ba
            }
            _ => false,
        };
    if next.headers.is_none() && old.headers.as_ref().is_some_and(|h| !h.is_empty()) {
        if !same_destination {
            return Err(INVALID.into());
        }
        next.headers = old.headers.clone();
    }
    for key in &mut next.keys {
        if key.key.is_none() {
            let original = old.keys.iter().find(|k| k.id == key.id).ok_or(INVALID)?;
            if !same_destination || original.api_format != key.api_format {
                return Err(INVALID.into());
            }
            key.key = original.key.clone();
        }
    }
    Ok(())
}

impl Database {
    pub(crate) fn routing_upstream_keys(
        &self,
        id: &str,
        revision: u32,
    ) -> Result<Vec<crate::services::routing_upstreams::KeySummary>, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let record = read_records(&conn)?
            .into_iter()
            .find(|record| record.id == id && record.revision == revision)
            .ok_or(CONFLICT)?;
        let UpstreamSource::Api {
            api_key: Some(primary),
            ..
        } = record.source
        else {
            return Err(INVALID.into());
        };
        let summary = |id: String, key: &str| crate::services::routing_upstreams::KeySummary {
            id,
            label: crate::services::routing_upstreams::masked_key(key),
            running: None,
            observation: crate::services::routing_upstreams::key_observation(key),
        };
        let mut keys = vec![summary(String::new(), &primary)];
        if let Some(transport) = record.transport {
            for key in transport.keys {
                keys.push(summary(key.id, key.key.as_deref().ok_or(INVALID)?));
            }
        }
        Ok(keys)
    }
    pub(crate) fn reveal_routing_upstream_key(
        &self,
        id: &str,
        revision: u32,
        key_id: &str,
    ) -> Result<String, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let record = read_records(&conn)?
            .into_iter()
            .find(|record| record.id == id && record.revision == revision)
            .ok_or(CONFLICT)?;
        if key_id.is_empty() {
            if let UpstreamSource::Api {
                api_key: Some(key), ..
            } = record.source
            {
                return Ok(key);
            }
        } else if let Some(transport) = record.transport {
            if let Some(key) = transport
                .keys
                .into_iter()
                .find(|key| key.id == key_id)
                .and_then(|key| key.key)
            {
                return Ok(key);
            }
        }
        Err(INVALID.into())
    }
    pub(crate) fn private_routing_upstreams(&self) -> Result<Vec<RoutingUpstream>, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        read_records(&conn)
    }
    pub(crate) fn import_native_providers(
        &self,
        selected: Vec<crate::services::provider_import::Selected>,
    ) -> Result<crate::services::provider_import::ImportResult, String> {
        use crate::services::provider_import::{
            already_added, join_keys, same_vendor, ImportResult,
        };
        let mut conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let tx = conn.transaction().map_err(|_| FAILED)?;
        let mut records = read_records(&tx)?;
        let mut result = ImportResult {
            added: 0,
            skipped: 0,
        };
        for selection in selected {
            let mut incoming = selection.record;
            if selection.overwrite && !selection.join {
                incoming.has_api_key = false;
                incoming.validate()?;
                let current = records
                    .iter()
                    .find(|record| record.id == incoming.id)
                    .cloned();
                let legacy = records
                    .iter()
                    .filter(|record| {
                        record.id != incoming.id
                            && uuid::Uuid::parse_str(&record.id).is_ok()
                            && already_added(record, &incoming)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if current.is_none() && records.len().saturating_sub(legacy.len()) >= MAX_UPSTREAMS
                {
                    return Err(LIMIT.into());
                }
                incoming.revision = current.as_ref().map_or(Ok(1), |record| {
                    record.revision.checked_add(1).ok_or(CONFLICT)
                })?;
                for old in &legacy {
                    identity::retire(&tx, old)?;
                    identity::migrate_references(&tx, &old.id, &incoming.id)?;
                    tx.execute("DELETE FROM routing_upstreams WHERE id=?1", [&old.id])
                        .map_err(|_| FAILED)?;
                }
                identity::update(&tx, current.as_ref(), &incoming)?;
                write_record(&tx, &incoming)?;
                records.retain(|record| {
                    record.id != incoming.id && !legacy.iter().any(|old| old.id == record.id)
                });
                records.push(incoming);
                result.added += 1;
                continue;
            }
            if records.iter().any(|r| already_added(r, &incoming)) {
                result.skipped += 1;
                continue;
            }
            if selection.join {
                let target = records
                    .iter_mut()
                    .find(|r| same_vendor(r, &incoming))
                    .ok_or("providerImport.conflict")?;
                join_keys(target, &incoming)?;
                target.revision = target.revision.checked_add(1).ok_or(CONFLICT)?;
                target.validate()?;
                write_record(&tx, target)?;
            } else {
                if records.len() >= MAX_UPSTREAMS {
                    return Err(LIMIT.into());
                }
                incoming.id = uuid::Uuid::new_v4().to_string();
                let base = incoming.name.clone();
                let mut suffix = 2;
                while records.iter().any(|r| r.name == incoming.name) {
                    incoming.name =
                        format!("{} ({suffix})", base.chars().take(108).collect::<String>());
                    suffix += 1;
                }
                incoming.revision = 1;
                incoming.has_api_key = false;
                incoming.validate()?;
                identity::update(&tx, None, &incoming)?;
                write_record(&tx, &incoming)?;
                records.push(incoming);
            }
            result.added += 1;
        }
        tx.commit().map_err(|_| FAILED)?;
        self.invalidate_routing_resources();
        Ok(result)
    }
    pub(crate) fn routing_upstream_editor(
        &self,
        id: &str,
        revision: u32,
    ) -> Result<RoutingUpstream, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        read_records(&conn)?
            .into_iter()
            .find(|r| r.id == id && r.revision == revision)
            .map(RoutingUpstream::redacted)
            .ok_or_else(|| CONFLICT.into())
    }
    pub(crate) fn list_routing_upstreams(&self) -> Result<Vec<RoutingUpstream>, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        Ok(read_records(&conn)?
            .into_iter()
            .map(RoutingUpstream::redacted)
            .collect())
    }
    pub(crate) fn save_routing_upstream(
        &self,
        record: RoutingUpstream,
    ) -> Result<RoutingUpstream, String> {
        self.save_routing_upstream_as(record, None)
    }
    pub(crate) fn save_routing_upstream_as(
        &self,
        mut record: RoutingUpstream,
        original_id: Option<&str>,
    ) -> Result<RoutingUpstream, String> {
        let mut conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let tx = conn.transaction().map_err(|_| FAILED)?;
        let records = read_records(&tx)?;
        if original_id.is_some_and(|id| !valid_identity(id)) {
            return Err(INVALID.into());
        }
        if record.id.is_empty() && original_id.is_some() {
            return Err(INVALID.into());
        }
        if record.id.is_empty() {
            let base: String = record
                .name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect();
            let base = base.trim_matches('-');
            let base = if base.is_empty() { "provider" } else { base };
            record.id = base.chars().take(100).collect();
            let mut suffix = 2;
            while records.iter().any(|r| r.id == record.id) {
                record.id = format!("{base}-{suffix}");
                suffix += 1;
            }
        }
        let current = records
            .iter()
            .find(|r| r.id == original_id.unwrap_or(&record.id));
        if original_id.is_some() && current.is_none() {
            return Err(CONFLICT.into());
        }
        if original_id.is_some_and(|id| id != record.id)
            && (records.iter().any(|r| r.id == record.id)
                || !current.is_some_and(|r| matches!(r.source, UpstreamSource::Api { .. })))
        {
            return Err(CONFLICT.into());
        }
        if current.map_or(0, |r| r.revision) != record.revision {
            return Err(CONFLICT.into());
        }
        if let Some(current) = current {
            restore_transport(&mut record, current)?;
            if std::mem::discriminant(&current.source) != std::mem::discriminant(&record.source) {
                return Err(INVALID.into());
            }
            if let (
                UpstreamSource::Api {
                    api_key,
                    base_url,
                    api_format,
                    auth_type,
                    ..
                },
                UpstreamSource::Api {
                    api_key: old_key,
                    base_url: old_url,
                    api_format: old_format,
                    auth_type: old_auth,
                    ..
                },
            ) = (&mut record.source, &current.source)
            {
                if api_key.as_deref().is_none_or(str::is_empty) {
                    // A saved key must not silently move to a different endpoint.
                    let known_endpoint = current.transport.as_ref().is_some_and(|t| {
                        t.primary_protocol.is_none()
                            && t.endpoints.get(api_format) == Some(base_url)
                    });
                    if (!known_endpoint && (base_url != old_url || api_format != old_format))
                        || auth_type != old_auth
                    {
                        return Err(INVALID.into());
                    }
                    *api_key = old_key.clone();
                }
            }
        } else if records.len() >= MAX_UPSTREAMS {
            return Err(LIMIT.into());
        }
        record.has_api_key = false;
        record.validate()?;
        record.revision = record.revision.checked_add(1).ok_or(CONFLICT)?;
        identity::update(&tx, current, &record)?;
        if let Some(current) = current.filter(|old| old.id != record.id) {
            tx.execute(
                "UPDATE routing_upstreams SET id=?1 WHERE id=?2 AND revision=?3",
                params![record.id, current.id, current.revision],
            )
            .map_err(|_| FAILED)?;
        }
        write_record(&tx, &record)?;
        tx.commit().map_err(|_| FAILED)?;
        self.invalidate_routing_resources();
        Ok(record.redacted())
    }
    pub(crate) fn save_routing_upstream_copy(
        &self,
        mut record: RoutingUpstream,
        source_id: &str,
        source_revision: u32,
    ) -> Result<RoutingUpstream, String> {
        if record.revision != 0 || record.id == source_id {
            return Err(INVALID.into());
        }
        let original = {
            let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
            read_records(&conn)?
                .into_iter()
                .find(|source| source.id == source_id && source.revision == source_revision)
                .ok_or(CONFLICT)?
        };
        restore_transport(&mut record, &original)?;
        match (&mut record.source, original.source) {
            (
                UpstreamSource::Api {
                    api_key,
                    base_url,
                    api_format,
                    auth_type,
                    ..
                },
                UpstreamSource::Api {
                    api_key: key,
                    base_url: old_url,
                    api_format: old_format,
                    auth_type: old_auth,
                    ..
                },
            ) => {
                if api_key.as_deref().is_none_or(str::is_empty) {
                    if *base_url != old_url || *api_format != old_format || *auth_type != old_auth {
                        return Err(INVALID.into());
                    }
                    *api_key = key;
                }
            }
            _ => return Err(INVALID.into()),
        }
        self.save_routing_upstream(record)
    }

    pub(crate) fn delete_routing_upstream(&self, id: &str, revision: u32) -> Result<(), String> {
        if !valid_identity(id) {
            return Err(INVALID.into());
        }
        let mut conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let tx = conn.transaction().map_err(|_| FAILED)?;
        let current = read_records(&tx)?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or(CONFLICT)?;
        if current.revision != revision {
            return Err(CONFLICT.into());
        }
        let mut stmt = tx
            .prepare("SELECT config FROM intent_routing_groups LIMIT 33")
            .map_err(|_| FAILED)?;
        let groups = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|_| FAILED)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| FAILED)?;
        if groups.len() > 32 {
            return Err(FAILED.into());
        }
        for config in groups {
            if config.len() > 65536 {
                return Err(FAILED.into());
            }
            let group: serde_json::Value = serde_json::from_str(&config).map_err(|_| FAILED)?;
            let members = group
                .get("members")
                .and_then(serde_json::Value::as_array)
                .ok_or(FAILED)?;
            if members
                .iter()
                .any(|m| m.get("providerId").and_then(serde_json::Value::as_str) == Some(id))
                || group
                    .pointer("/classifierTarget/providerId")
                    .and_then(serde_json::Value::as_str)
                    == Some(id)
            {
                return Err("routingUpstreams.referenced".into());
            }
        }
        drop(stmt);
        identity::retire(&tx, &current)?;
        tx.execute(
            "DELETE FROM routing_upstreams WHERE id=?1 AND revision=?2",
            params![id, revision],
        )
        .map_err(|_| FAILED)?;
        tx.commit().map_err(|_| FAILED)?;
        self.invalidate_routing_resources();
        Ok(())
    }

    pub(crate) fn routing_providers(&self) -> Result<Vec<Provider>, String> {
        self.routing_catalog_providers(false)
    }
    pub(crate) fn routing_catalog_providers(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<Provider>, String> {
        let (records, identities) = {
            let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
            (read_records(&conn)?, identity::read(&conn)?)
        };
        records
            .into_iter()
            .enumerate()
            .filter(|(_, r)| include_disabled || r.enabled && r.has_enabled_key())
            .map(|(position, record)| {
                let mut provider = resolve_provider(&record)?;
                if let Some(identity) = identities.get(&record.id) {
                    provider.settings_config
                        [crate::services::routing_upstreams::EXECUTION_IDENTITY] =
                        serde_json::json!(identity);
                }
                provider.sort_index = Some(position);
                Ok(provider)
            })
            .collect()
    }
    pub(crate) fn routing_discovery_provider(
        &self,
        id: &str,
        revision: u32,
    ) -> Result<Provider, String> {
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let record = read_records(&conn)?
            .into_iter()
            .find(|r| r.id == id && r.revision == revision)
            .ok_or(CONFLICT)?;
        resolve_provider(&record)
    }
    pub(crate) fn normalize_imported_routing_upstreams(
        conn: &Connection,
    ) -> Result<(), crate::error::AppError> {
        // Portable files with the retired contract are reset, never translated.
        let incompatible: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM routing_upstreams WHERE json_valid(config)=0 OR json_extract(config,'$.schemaVersion') IS NOT 1)", [], |r| r.get(0))?;
        if incompatible {
            conn.execute_batch("DELETE FROM routing_upstreams; DELETE FROM intent_routing_groups; DELETE FROM intent_routing_preferences;")?;
        }
        read_records(conn).map_err(|_| {
            crate::error::AppError::Database("Invalid native provider snapshot".into())
        })?;
        Ok(())
    }
}

#[cfg(test)]
impl Database {
    /// Register a synthetic test connection using the current native contract.
    pub(crate) fn register_routing_fixture(
        &self,
        app: &str,
        id: &str,
    ) -> Result<RoutingUpstream, String> {
        use crate::services::codex_provider::{ModelMode, Settings};
        use serde_json::json;
        let provider = self
            .get_provider_by_id(id, app)
            .map_err(|_| FAILED)?
            .ok_or(INVALID)?;
        let mut models = provider.settings_config["modelCatalog"]["models"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if let Some(model) = provider.settings_config["config"]
            .as_str()
            .and_then(|s| s.parse::<toml::Value>().ok())
            .and_then(|doc| doc.get("model")?.as_str().map(str::to_owned))
        {
            if !models
                .iter()
                .any(|row| crate::services::routing_upstreams::model_id(row) == Some(&model))
            {
                models.push(json!({"model":model}));
            }
        }
        let mut seen = HashSet::new();
        models.retain(|row| {
            crate::services::routing_upstreams::model_id(row)
                .is_some_and(|id| seen.insert(id.to_owned()))
        });
        let account = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("codex_oauth"));
        let official = crate::services::intent_routing::catalog::official_source(&provider);
        let source = if official {
            UpstreamSource::Codex
        } else {
            let app: crate::app_config::AppType = app.parse().map_err(|_| INVALID)?;
            let (base_url, key) = provider.resolve_usage_credentials(&app);
            let config = provider.settings_config["config"]
                .as_str()
                .and_then(|s| s.parse::<toml::Value>().ok());
            let wire = config.as_ref().and_then(|doc| {
                doc.get("model_providers")?
                    .get(doc.get("model_provider")?.as_str()?)?
                    .get("wire_api")?
                    .as_str()
            });
            UpstreamSource::Api {
                base_url,
                api_key: Some(key),
                api_format: provider.settings_config["apiFormat"]
                    .as_str()
                    .filter(|s| crate::services::routing_upstreams::valid_format(s))
                    .unwrap_or(match wire {
                        Some("chat") => "openai_chat",
                        Some("anthropic") => "anthropic",
                        _ => "openai_responses",
                    })
                    .into(),
                models: models.clone(),
                catalog: models.clone(),
                mode: ModelMode::Picked,
                groups_only: false,
                auth_type: None,
            }
        };
        let existing = self
            .list_routing_upstreams()?
            .into_iter()
            .find(|r| r.id == id);
        self.save_routing_upstream(RoutingUpstream {
            notes: None,
            icon: provider.icon,
            transport: None,
            schema_version: 1,
            id: id.into(),
            name: provider.name,
            revision: existing.map_or(0, |r| r.revision),
            source,
            has_api_key: false,
            enabled: true,
            preset_id: None,
            region: None,
            codex_settings: official.then(|| Settings {
                accounts: vec![account.unwrap_or_else(|| "native".into())],
                mode: if models.is_empty() {
                    ModelMode::Vendor
                } else {
                    ModelMode::Picked
                },
                models,
                ..Default::default()
            }),
        })
    }
}
