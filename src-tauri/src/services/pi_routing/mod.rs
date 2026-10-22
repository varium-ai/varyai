//! Pi routing projects local endpoints onto the original provider identities.
pub(crate) mod catalog;

use crate::database::Database;
use catalog::{Catalog, Issue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
};

#[derive(Default)]
pub(crate) struct Runtime {
    pub catalog: RwLock<Option<Arc<Catalog>>>,
    pub last_request: AtomicI64,
    pub incomplete_usage_count: AtomicU64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    enabled: bool,
    provider_ids: Vec<String>,
    directory: PathBuf,
    token: String,
    receipt: Option<Value>,
    #[serde(default)]
    previous_receipt: Option<Value>,
    #[serde(default)]
    known_models: Vec<String>,
    #[serde(default)]
    providers: BTreeMap<String, Value>,
    #[serde(default)]
    previous_providers: BTreeMap<String, Value>,
    #[serde(default)]
    originals: BTreeMap<String, Option<Value>>,
    #[serde(default)]
    excluded_provider_ids: Vec<String>,
    // Separate from native membership: a saved-only provider has original=None.
    #[serde(default)]
    source_snapshots: BTreeMap<String, Value>,
    // Preserve the overwritten values before repairing the source DB. This file
    // already uses private atomic writes because both maps contain credentials.
    #[serde(default)]
    recovery_backup: BTreeMap<String, Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub enabled: bool,
    pub configured: bool,
    pub running: bool,
    pub provider_ids: Vec<String>,
    pub model_count: usize,
    pub issues: Vec<Issue>,
    pub last_request_at: Option<i64>,
    pub incomplete_usage_count: u64,
    pub error_code: Option<String>,
}

pub(crate) struct Manager {
    path: PathBuf,
    operation: Mutex<()>,
    pub runtime: Arc<Runtime>,
}
impl Manager {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            operation: Mutex::new(()),
            runtime: Arc::new(Runtime::default()),
        }
    }
    fn read(&self) -> Result<Settings, String> {
        match std::fs::read(&self.path) {
            Ok(bytes) if bytes.len() <= 8 * 1024 * 1024 => {
                serde_json::from_slice(&bytes).map_err(|_| "piRouting.settingsUnreadable".into())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            _ => Err("piRouting.settingsUnreadable".into()),
        }
    }
    fn write(&self, settings: &Settings) -> Result<(), String> {
        let bytes =
            serde_json::to_vec_pretty(settings).map_err(|_| "piRouting.settingsUnreadable")?;
        crate::config::atomic_write_private(&self.path, &bytes)
            .map_err(|_| "piRouting.settingsWriteFailed".into())
    }
    pub fn enabled(&self) -> Result<bool, String> {
        Ok(self.read()?.enabled)
    }
    /// Older releases could import models.json projections into the source DB.
    /// Only repair existing rows matching our receipts, from a validated upstream
    /// snapshot. Never infer credentials, resurrect deleted rows or undo edits.
    pub fn recover_sources(&self, db: &Database) -> Result<usize, String> {
        let _guard = self.operation.lock().map_err(|_| "piRouting.busy")?;
        let mut settings = self.read()?;
        if settings.directory != Path::new("")
            && settings.directory
                != crate::pi_config::get_pi_agent_dir().map_err(|_| "piRouting.configUnreadable")?
        {
            return Ok(0);
        }
        let sources = db
            .get_all_providers("pi")
            .map_err(|_| "piRouting.sourceUnreadable")?;
        let mut repairs = Vec::new();
        for (id, source) in &sources {
            let current = &source.settings_config;
            if id == "vary"
                || !catalog::is_local_projection(current)
                || (settings.providers.get(id) != Some(current)
                    && settings.previous_providers.get(id) != Some(current))
            {
                continue;
            }
            let original = settings
                .source_snapshots
                .get(id)
                .into_iter()
                .chain(settings.originals.get(id).and_then(Option::as_ref))
                .find(|node| {
                    !catalog::is_local_projection(node)
                        && crate::pi_config::validate_provider_node(id, node).is_ok()
                        && crate::pi_config::provider_base_url(node).is_ok()
                })
                .cloned();
            if let Some(mut original) = original {
                // The saved display name may have been edited after this backup.
                // Keep it consistent when restoring the native configuration.
                if let Some(object) = original.as_object_mut() {
                    if object.contains_key("name") {
                        object.insert("name".into(), Value::String(source.name.clone()));
                    }
                }
                settings
                    .recovery_backup
                    .entry(id.clone())
                    .or_insert_with(|| current.clone());
                repairs.push((id.clone(), original));
            }
        }
        if repairs.is_empty() {
            return Ok(0);
        }
        // A failed backup write must leave the DB untouched and remain retryable.
        self.write(&settings)?;
        let mut restored = 0;
        for (id, original) in &repairs {
            if let Some(source) = sources.get(id) {
                if db
                    .update_provider_settings_config_if_matches(
                        "pi",
                        id,
                        &source.settings_config,
                        original,
                    )
                    .map_err(|_| "piRouting.sourceRepairFailed")?
                {
                    restored += 1;
                }
            }
        }
        log::info!("[Pi] restored {restored} upstream source configuration(s)");
        Ok(restored)
    }
    pub fn source_ids(&self, db: &Database) -> Result<Vec<String>, String> {
        self.recover_sources(db)?;
        let sources = db
            .get_all_providers("pi")
            .map_err(|_| "piRouting.sourceUnreadable")?;
        let excluded = self.read()?.excluded_provider_ids;
        Ok(sources
            .into_values()
            .filter(|p| p.id != "vary" && !excluded.contains(&p.id))
            .map(|p| p.id)
            .collect())
    }
    pub fn preview(
        &self,
        db: &Database,
        selected: &[String],
        origin: &str,
    ) -> Result<Catalog, String> {
        self.recover_sources(db)?;
        let sources = db
            .get_all_providers("pi")
            .map_err(|_| "piRouting.sourceUnreadable")?;
        Catalog::build(
            &sources.into_values().collect::<Vec<_>>(),
            selected,
            origin,
            "preview",
        )
    }
    pub fn configure(
        &self,
        db: &Database,
        selected: &[String],
        origin: &str,
    ) -> Result<(), String> {
        self.recover_sources(db)?;
        let _guard = self.operation.lock().map_err(|_| "piRouting.busy")?;
        let previous = self.read()?;
        let directory =
            crate::pi_config::get_pi_agent_dir().map_err(|_| "piRouting.configUnreadable")?;
        if previous.enabled && previous.directory != directory {
            return Err("piRouting.directoryChanged".into());
        }
        let current = crate::pi_config::read_pi_native_providers()
            .map_err(|_| "piRouting.configUnreadable")?;
        let legacy = current.get("vary");
        if legacy.is_some()
            && legacy != previous.receipt.as_ref()
            && legacy != previous.previous_receipt.as_ref()
        {
            return Err("piRouting.conflict".into());
        }
        let sources = db
            .get_all_providers("pi")
            .map_err(|_| "piRouting.sourceUnreadable")?;
        for (id, receipt) in &previous.providers {
            if let Some(node) = current.get(id) {
                if node != receipt
                    && previous.previous_providers.get(id) != Some(node)
                    && sources.get(id).map(|p| &p.settings_config) != Some(node)
                {
                    return Err("piRouting.conflict".into());
                }
            }
        }
        let token = if previous.token.is_empty() {
            format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )
        } else {
            previous.token.clone()
        };
        let mut catalog = Catalog::build(
            &sources.values().cloned().collect::<Vec<_>>(),
            selected,
            origin,
            "preview",
        )?;
        let unresolved: Vec<String> = catalog
            .issues
            .iter()
            .filter(|issue| issue.code == "piRouting.sourceRecoveryRequired")
            .map(|issue| issue.provider_id.clone())
            .collect();
        if catalog.targets.is_empty() {
            return Err(if unresolved.is_empty() {
                "piRouting.noAvailableModels"
            } else {
                "piRouting.sourceRecoveryRequired"
            }
            .into());
        }
        catalog.node["apiKey"] = Value::String(token.clone());
        for node in catalog.providers.values_mut() {
            node["apiKey"] = Value::String(token.clone());
        }
        let mut known = previous.known_models.clone();
        for alias in catalog.targets.keys() {
            if !known.contains(alias) {
                known.push(alias.clone());
            }
        }
        let mut originals = previous.originals.clone();
        let mut next = current.clone();
        next.shift_remove("vary");
        for id in previous
            .providers
            .keys()
            .filter(|id| !catalog.providers.contains_key(*id))
        {
            // A failed recovery is not a request to remove this native provider.
            if unresolved.contains(id) {
                continue;
            }
            if let Some(Some(original)) = originals.get(id) {
                next.insert(id.clone(), original.clone());
            } else {
                next.shift_remove(id);
            }
            originals.remove(id);
        }
        for (id, node) in &catalog.providers {
            if current.get(id).is_some_and(catalog::is_local_projection)
                && !previous.providers.contains_key(id)
            {
                return Err("piRouting.conflict".into());
            }
            originals
                .entry(id.clone())
                .or_insert_with(|| current.get(id).cloned());
            // Restore the latest saved upstream configuration after an authorized edit.
            if originals.get(id).is_some_and(Option::is_some) {
                if let Some(source) = sources.get(id) {
                    originals.insert(id.clone(), Some(source.settings_config.clone()));
                }
            }
            next.insert(id.clone(), node.clone());
        }
        let mut receipts = catalog.providers.clone();
        for id in &unresolved {
            if let Some(receipt) = previous.providers.get(id) {
                receipts.insert(id.clone(), receipt.clone());
            }
        }
        let mut source_snapshots = previous.source_snapshots.clone();
        for id in catalog.providers.keys() {
            if let Some(source) = sources.get(id) {
                source_snapshots.insert(id.clone(), source.settings_config.clone());
            }
        }
        let settings = Settings {
            enabled: true,
            provider_ids: selected.to_vec(),
            directory,
            token,
            receipt: None,
            previous_receipt: legacy.cloned().or(previous.receipt.clone()),
            known_models: known,
            providers: receipts,
            previous_providers: previous.providers.clone(),
            originals,
            excluded_provider_ids: previous.excluded_provider_ids.clone(),
            source_snapshots,
            recovery_backup: previous.recovery_backup.clone(),
        };
        self.write(&settings)?;
        if crate::pi_config::replace_pi_providers(&current, &next).is_err() {
            if self.write(&previous).is_err() {
                log::warn!("Pi routing settings rollback failed");
            }
            return Err("piRouting.conflict".into());
        }
        // Retaining the previous receipt makes a crash between the two atomic writes recoverable.
        *self.runtime.catalog.write().map_err(|_| "piRouting.busy")? = Some(Arc::new(catalog));
        Ok(())
    }
    pub fn disable(&self) -> Result<(), String> {
        let _guard = self.operation.lock().map_err(|_| "piRouting.busy")?;
        let mut settings = self.read()?;
        if settings.directory != Path::new("")
            && settings.directory
                != crate::pi_config::get_pi_agent_dir().map_err(|_| "piRouting.configUnreadable")?
        {
            return Err("piRouting.directoryChanged".into());
        }
        let current = crate::pi_config::read_pi_native_providers()
            .map_err(|_| "piRouting.configUnreadable")?;
        let mut next = current.clone();
        if let Some(node) = current.get("vary") {
            if Some(node) != settings.receipt.as_ref()
                && Some(node) != settings.previous_receipt.as_ref()
            {
                return Err("piRouting.conflict".into());
            }
            next.shift_remove("vary");
        }
        for (id, receipt) in &settings.providers {
            if let Some(node) = current.get(id) {
                if node != receipt
                    && settings.previous_providers.get(id) != Some(node)
                    && settings.originals.get(id).and_then(Option::as_ref) != Some(node)
                {
                    return Err("piRouting.conflict".into());
                }
                if let Some(Some(original)) = settings.originals.get(id) {
                    next.insert(id.clone(), original.clone());
                } else {
                    next.shift_remove(id);
                }
            }
        }
        crate::pi_config::replace_pi_providers(&current, &next)
            .map_err(|_| "piRouting.conflict")?;
        settings.enabled = false;
        settings.providers.clear();
        settings.previous_providers.clear();
        settings.originals.clear();
        self.write(&settings)?;
        *self.runtime.catalog.write().map_err(|_| "piRouting.busy")? = None;
        Ok(())
    }
    pub fn refresh(&self, db: &Database, origin: &str) -> Result<(), String> {
        let settings = self.read()?;
        if settings.enabled {
            let result = self
                .source_ids(db)
                .and_then(|ids| self.configure(db, &ids, origin));
            if let Err(error) = result {
                // A source edit/deletion must never leave the previous target usable.
                *self.runtime.catalog.write().map_err(|_| "piRouting.busy")? = None;
                return Err(error);
            }
        }
        Ok(())
    }
    pub fn status(&self, db: &Database, running: bool, origin: &str) -> Result<Status, String> {
        let settings = self.read()?;
        let current = crate::pi_config::read_pi_native_providers()
            .map_err(|_| "piRouting.configUnreadable")?;
        let same_directory = settings.directory
            == crate::pi_config::get_pi_agent_dir().map_err(|_| "piRouting.configUnreadable")?;
        let configured = same_directory
            && !settings.providers.is_empty()
            && settings
                .providers
                .iter()
                .all(|(id, node)| current.get(id) == Some(node));
        let conflict = settings
            .providers
            .iter()
            .any(|(id, node)| current.get(id).is_some_and(|n| n != node));
        let mut catalog = self.preview(db, &settings.provider_ids, origin)?;
        // Saved-only or excluded providers can also be polluted. Keep their
        // repair instructions visible even while routing is switched off.
        for source in db
            .get_all_providers("pi")
            .map_err(|_| "piRouting.sourceUnreadable")?
            .into_values()
        {
            if source.id != "vary"
                && catalog::is_local_projection(&source.settings_config)
                && !catalog.issues.iter().any(|issue| {
                    issue.provider_id == source.id
                        && issue.code == "piRouting.sourceRecoveryRequired"
                })
            {
                catalog.issues.push(Issue {
                    provider_id: source.id,
                    model_id: None,
                    code: "piRouting.sourceRecoveryRequired".into(),
                });
            }
        }
        let at = self.runtime.last_request.load(Ordering::Relaxed);
        let active = self
            .runtime
            .catalog
            .read()
            .map_err(|_| "piRouting.busy")?
            .is_some();
        let error = if catalog.targets.is_empty()
            && catalog
                .issues
                .iter()
                .any(|issue| issue.code == "piRouting.sourceRecoveryRequired")
        {
            Some("piRouting.sourceRecoveryRequired")
        } else if settings.enabled && !same_directory {
            Some("piRouting.directoryChanged")
        } else if settings.enabled && conflict {
            Some("piRouting.conflict")
        } else if settings.enabled && !configured {
            Some("piRouting.configMissing")
        } else if settings.enabled && !active && running {
            Some("piRouting.unavailable")
        } else {
            None
        };
        Ok(Status {
            enabled: settings.enabled,
            configured,
            running: running && active,
            provider_ids: settings.provider_ids,
            model_count: catalog.targets.len(),
            issues: catalog.issues,
            last_request_at: (at > 0).then_some(at),
            incomplete_usage_count: self.runtime.incomplete_usage_count.load(Ordering::Relaxed),
            error_code: error.map(str::to_string),
        })
    }
    /// Membership sync must never import a local projection as an upstream.
    pub fn owns_provider(&self, id: &str, node: &Value) -> bool {
        self.read().is_ok_and(|s| {
            s.providers.get(id) == Some(node) || s.previous_providers.get(id) == Some(node)
        }) || catalog::is_local_projection(node)
    }
    pub fn owns_node(&self, node: &Value) -> bool {
        self.read().is_ok_and(|s| {
            s.receipt.as_ref() == Some(node) || s.previous_receipt.as_ref() == Some(node)
        }) || catalog::is_local_projection(node)
    }
    pub fn detach_source(&self, id: &str) -> Result<(), String> {
        let _guard = self.operation.lock().map_err(|_| "piRouting.busy")?;
        let mut s = self.read()?;
        s.providers.remove(id);
        s.previous_providers.remove(id);
        s.originals.remove(id);
        s.source_snapshots.remove(id);
        if !s.excluded_provider_ids.iter().any(|v| v == id) {
            s.excluded_provider_ids.push(id.into());
        }
        self.write(&s)
    }
    pub fn include_source(&self, id: &str) -> Result<(), String> {
        let _guard = self.operation.lock().map_err(|_| "piRouting.busy")?;
        let mut s = self.read()?;
        s.excluded_provider_ids.retain(|v| v != id);
        self.write(&s)
    }
    pub fn known_models(&self) -> Vec<String> {
        self.read().map(|s| s.known_models).unwrap_or_default()
    }
    #[cfg(test)]
    pub fn owns_session(&self, provider: &str, model: &str) -> bool {
        provider == "vary"
            && self
                .read()
                .is_ok_and(|s| s.known_models.iter().any(|id| id == model))
    }
}

pub(crate) fn settings_path() -> PathBuf {
    crate::config::get_app_config_dir().join("pi-routing.json")
}

#[cfg(test)]
mod tests;
