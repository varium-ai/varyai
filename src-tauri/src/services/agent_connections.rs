//! Agents consume a shared catalog. Receipts own only explicitly changed fields.
mod bridge;
mod claude_policy;
pub(crate) mod desktop;
pub(crate) mod desktop_config;
mod document;
pub(crate) mod native;
#[cfg(test)]
mod parity_tests;
mod projections;
mod reasoning;
mod selection;
mod visibility;
use crate::{config::atomic_write_private, proxy::intent::Menu};
use document::Format;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
pub(crate) use visibility::ModelVisibility;

pub(crate) const FAILED: &str = "agentConnections.failed";
pub(crate) const CONFLICT: &str = "agentConnections.conflict";
pub(crate) const EMPTY: &str = "agentConnections.noModels";
pub(crate) const APPS: &[&str] = &[
    "claude-desktop",
    "claude",
    "pi",
    "workbuddy",
    "codex",
    "opencode",
    "gemini",
    "grokbuild",
    "openclaw",
    "hermes",
    "qoder",
    "kimi",
    "qwen",
    "zcode",
    "deepseek-harness",
];
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Model {
    pub output_tokens: Option<u64>,
    pub cost: Option<Value>,
    pub id: String,
    pub name: String,
    pub context_window: Option<u64>,
    #[serde(skip)]
    pub compact_at: Option<u64>,
    pub group: String,
    pub reasoning_levels: Vec<String>,
    pub reasoning: bool,
    pub default_reasoning_level: Option<String>,
    pub images: bool,
    pub icon: Option<String>,
}
pub(crate) fn models(menu: &Menu) -> Vec<Model> {
    let mut models = menu
        .models
        .values()
        .filter_map(|model| {
            let session = menu.groups.get(&model.alias)?;
            let metadata = crate::services::intent_routing::context::Metadata::for_session(session);
            let mut published = json!({});
            metadata.publish(
                &menu.context_settings,
                crate::services::intent_routing::context::Metadata::session_typed_window(session),
                !session.targets.is_empty() && session.targets.iter().all(|target| target.official),
                &mut published,
            );
            Some(Model {
                output_tokens: session
                    .menu_targets()
                    .map(|target| target.effort.output_tokens)
                    .collect::<Option<Vec<_>>>()
                    .and_then(|values| values.into_iter().min()),
                cost: {
                    let mut values = session
                        .menu_targets()
                        .map(|target| target.effort.cost.as_ref());
                    let first = values.next().flatten();
                    first
                        .filter(|first| values.all(|next| next == Some(*first)))
                        .cloned()
                },
                id: model.alias.clone(),
                name: model.label.clone(),
                context_window: metadata.context_window,
                compact_at: published["auto_compact_token_limit"].as_u64(),
                reasoning_levels: session.offered_levels(),
                reasoning: session.menu_targets().any(|target| target.effort.thinks()),
                default_reasoning_level: session.default_reasoning_level(),
                images: session
                    .menu_targets()
                    .any(|target| target.effort.images != Some(false)),
                icon: session
                    .targets
                    .first()
                    .and_then(|target| target.upstream.as_ref())
                    .and_then(|provider| provider.icon.clone()),
                group: if model.description == "VaryAI routing group" {
                    "routing".into()
                } else {
                    session
                        .targets
                        .first()
                        .and_then(|target| target.upstream.as_ref())
                        .map(|p| p.name.clone())
                        .or_else(|| {
                            session
                                .targets
                                .first()?
                                .credentials
                                .as_ref()
                                .map(|credentials| credentials.provider_name().to_owned())
                        })
                        .unwrap_or_else(|| session.group.name.clone())
                },
            })
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| {
        (menu.order_of(&a.id), &a.name, &a.id).cmp(&(menu.order_of(&b.id), &b.name, &b.id))
    });
    models
}

fn working_window(model: &Model) -> Option<u64> {
    model
        .context_window
        .map(|window| model.compact_at.map_or(window, |at| at.min(window)))
}

fn output_limit(model: &Model) -> Option<u64> {
    model.output_tokens.map(|tokens| {
        model
            .context_window
            .map_or(tokens, |window| tokens.min(window))
    })
}

pub(crate) fn pi_model(model: &Model) -> Value {
    let mut row = json!({"id":model.id,"name":model.name,"api":"openai-completions","input":if model.images { vec!["text", "image"] } else { vec!["text"] },"reasoning":model.reasoning || model.reasoning_levels.iter().any(|level| level != "none")});
    if let Some(context) = model.context_window {
        row["contextWindow"] = json!(context);
    }
    if let Some(tokens) = output_limit(model) {
        row["maxTokens"] = json!(tokens);
    }
    if let Some(cost) = &model.cost {
        row["cost"] = cost.clone();
    }
    if !model.reasoning_levels.is_empty() {
        row["thinkingLevelMap"] =
            json!(["off", "minimal", "low", "medium", "high", "xhigh", "max"]
                .into_iter()
                .map(|level| {
                    let wire = if level == "off" { "none" } else { level };
                    (
                        level.to_owned(),
                        if model.reasoning_levels.iter().any(|known| known == wire) {
                            json!(wire)
                        } else {
                            Value::Null
                        },
                    )
                })
                .collect::<serde_json::Map<_, _>>());
    }
    row
}

pub(crate) fn opencode_model(model: &Model) -> Value {
    let mut row = json!({"name":model.name});
    row["modalities"] = json!({"input":if model.images {vec!["text", "image"]} else {vec!["text"]},"output":["text"]});
    row["reasoning"] =
        json!(model.reasoning || model.reasoning_levels.iter().any(|level| level != "none"));
    if let Some(context) = model.context_window {
        row["limit"] = json!({"context":context});
    }
    if let Some(tokens) = output_limit(model) {
        row["limit"]["output"] = json!(tokens);
    }
    if let Some(cost) = &model.cost {
        row["cost"] = json!({"input":cost["input"],"output":cost["output"],"cache_read":cost["cacheRead"],"cache_write":cost["cacheWrite"]});
    }
    if !model.reasoning_levels.is_empty() {
        row["variants"] = json!(model
            .reasoning_levels
            .iter()
            .map(|level| (level.clone(), json!({"reasoningEffort":level})))
            .collect::<serde_json::Map<_, _>>());
    }
    row
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub app: String,
    pub enabled: bool,
    pub configured: bool,
    pub recovery_pending: bool,
    pub recovery_reason: Option<RecoveryReason>,
    pub model: Option<String>,
    pub restart_required: bool,
    pub desktop_state: Option<desktop::State>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_compatibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_issue: Option<super::intent_routing::client::ConfigurationIssue>,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryReason {
    BridgePending,
    ConfigurationChanged,
    ConnectionInactive,
    Orphaned,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub app: String,
    pub token: String,
    pub model: String,
    pub model_map: BTreeMap<String, String>,
    #[serde(default)]
    pub visible_models: Option<Vec<String>>,
    pub files: Vec<FileEdit>,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub applied_at_ms: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct FileEdit {
    #[serde(default)]
    format: Format,
    path: PathBuf,
    original: Option<String>,
    installed: Value,
    #[serde(default)]
    previous: Option<Value>,
    fields: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    desktop: Option<desktop_config::Kind>,
    #[serde(default)]
    desktop_carried: bool,
}
fn read(path: &Path) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || meta.len() > 4 * 1024 * 1024 => {
            Err(FAILED.into())
        }
        Ok(_) => fs::read_to_string(path)
            .map(Some)
            .map_err(|_| FAILED.into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(FAILED.into()),
    }
}
fn replace_checked(
    path: &Path,
    expected: Option<&str>,
    replacement: Option<&str>,
) -> Result<(), String> {
    if read(path)?.as_deref() != expected {
        return Err(CONFLICT.into());
    }
    if expected == replacement {
        return Ok(());
    }
    let parent = path.parent().ok_or(FAILED)?;
    fs::create_dir_all(parent).map_err(|_| FAILED)?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let staged = parent.join(format!(".vary-connection-{suffix}.staged"));
    let held = parent.join(format!(".vary-connection-{suffix}.original"));
    if let Some(text) = replacement {
        atomic_write_private(&staged, text.as_bytes()).map_err(|_| FAILED)?;
    }
    let result = (|| {
        if expected.is_some() {
            fs::rename(path, &held).map_err(|_| CONFLICT)?;
            if read(&held)?.as_deref() != expected {
                let _ = fs::hard_link(&held, path).map(|_| fs::remove_file(&held));
                return Err(CONFLICT.into());
            }
        }
        if replacement.is_some() && fs::hard_link(&staged, path).is_err() {
            if expected.is_some() {
                let _ = fs::hard_link(&held, path).map(|_| fs::remove_file(&held));
            }
            return Err(CONFLICT.into());
        }
        if expected.is_some() {
            fs::remove_file(&held).map_err(|_| FAILED)?;
        }
        Ok(())
    })();
    let _ = fs::remove_file(staged);
    result
}
fn parse(text: Option<&str>) -> Result<Value, String> {
    let value: Value = serde_json::from_str(text.unwrap_or("{}")).map_err(|_| FAILED)?;
    if !value.is_object() && !value.is_array() {
        return Err(FAILED.into());
    }
    Ok(value)
}
fn at<'a>(value: &'a Value, parts: &[String]) -> Option<&'a Value> {
    parts.iter().try_fold(value, |node, part| node.get(part))
}
fn put(value: &mut Value, parts: &[String], next: Option<Value>) -> Result<(), String> {
    let Some((first, rest)) = parts.split_first() else {
        *value = next.ok_or(FAILED)?;
        return Ok(());
    };
    let obj = value.as_object_mut().ok_or(FAILED)?;
    if rest.is_empty() {
        match next {
            Some(next) => {
                obj.insert(first.clone(), next);
            }
            None => {
                obj.remove(first);
            }
        }
    } else {
        if !obj.contains_key(first) {
            if next.is_none() {
                return Ok(());
            }
            obj.insert(first.clone(), json!({}));
        }
        let node = obj.get_mut(first).ok_or(FAILED)?;
        put(node, rest, next)?;
        if node.as_object().is_some_and(|node| node.is_empty()) {
            obj.remove(first);
        }
    }
    Ok(())
}
fn changes(
    before: Option<&Value>,
    after: Option<&Value>,
    prefix: Vec<String>,
    out: &mut Vec<Vec<String>>,
) {
    if before == after {
        return;
    }
    if let Some(b) = after
        .and_then(Value::as_object)
        .filter(|_| before.is_none_or(Value::is_object))
    {
        let empty = serde_json::Map::new();
        let a = before.and_then(Value::as_object).unwrap_or(&empty);
        let mut keys = a.keys().chain(b.keys()).collect::<Vec<_>>();
        keys.sort();
        keys.dedup();
        for key in keys {
            let mut path = prefix.clone();
            path.push(key.clone());
            changes(a.get(key), b.get(key), path, out);
        }
    } else {
        out.push(prefix);
    }
}
impl FileEdit {
    pub(crate) fn prepare(
        path: PathBuf,
        update: impl FnOnce(&mut Value) -> Result<(), String>,
    ) -> Result<Self, String> {
        Self::prepare_document(path, false, update)
    }
    fn prepare_document(
        path: PathBuf,
        allow_array: bool,
        update: impl FnOnce(&mut Value) -> Result<(), String>,
    ) -> Result<Self, String> {
        Self::prepare_inner(path, Format::Json, allow_array, update)
    }
    fn prepare_format(
        path: PathBuf,
        format: Format,
        update: impl FnOnce(&mut Value) -> Result<(), String>,
    ) -> Result<Self, String> {
        Self::prepare_inner(path, format, false, update)
    }
    fn prepare_inner(
        path: PathBuf,
        format: Format,
        allow_array: bool,
        update: impl FnOnce(&mut Value) -> Result<(), String>,
    ) -> Result<Self, String> {
        let original = read(&path)?;
        let before = format.parse(original.as_deref())?;
        if !allow_array && !before.is_object() {
            return Err(FAILED.into());
        }
        let mut installed = before.clone();
        update(&mut installed)?;
        let mut fields = Vec::new();
        changes(Some(&before), Some(&installed), vec![], &mut fields);
        if !allow_array && fields.iter().any(Vec::is_empty) {
            return Err(FAILED.into());
        }
        Ok(Self {
            format,
            path,
            original,
            installed,
            previous: None,
            fields,
            desktop: None,
            desktop_carried: false,
        })
    }
    pub(crate) fn for_desktop(mut self, kind: desktop_config::Kind) -> Self {
        self.desktop = Some(kind);
        self.desktop_carried = matches!(
            kind,
            desktop_config::Kind::Mode | desktop_config::Kind::Carry
        );
        for field in &mut self.fields {
            if field.first().is_some_and(|key| key == "mcpServers") && field.len() > 2 {
                field.truncate(2);
            }
        }
        self.fields.sort();
        self.fields.dedup();
        self
    }
    fn intact(&self) -> Result<bool, String> {
        let live = self.format.parse(read(&self.path)?.as_deref())?;
        if let Some(kind) = self.desktop {
            return desktop_config::intact(self, &live, kind);
        }
        Ok(self
            .fields
            .iter()
            .all(|field| at(&live, field) == at(&self.installed, field)))
    }
    fn restorable(&self) -> Result<bool, String> {
        if let Some(kind) = self.desktop {
            return desktop_config::restoration(self, kind, false).map(|_| true);
        }
        let live = self.format.parse(read(&self.path)?.as_deref())?;
        let original = self.format.parse(self.original.as_deref())?;
        Ok(self.fields.iter().all(|field| {
            at(&live, field) == at(&self.installed, field)
                || at(&live, field) == at(&original, field)
                || self
                    .previous
                    .as_ref()
                    .is_some_and(|previous| at(&live, field) == at(previous, field))
        }))
    }
    fn write(&self) -> Result<(), String> {
        if self.format.parse(self.original.as_deref())? == self.installed {
            return replace_checked(
                &self.path,
                self.original.as_deref(),
                self.original.as_deref(),
            );
        }
        replace_checked(
            &self.path,
            self.original.as_deref(),
            Some(
                &self
                    .format
                    .encode(self.original.as_deref(), &self.installed)?,
            ),
        )
    }
    fn restore(&self) -> Result<(), String> {
        self.restore_fields(false)
    }
    fn restore_fields(&self, keep_changed: bool) -> Result<(), String> {
        if let Some(kind) = self.desktop {
            return desktop_config::restoration(self, kind, keep_changed)?.write();
        }
        let text = read(&self.path)?;
        let mut live = self.format.parse(text.as_deref())?;
        let original = self.format.parse(self.original.as_deref())?;
        // An already restored projection must not reformat later user edits.
        if self
            .fields
            .iter()
            .all(|field| at(&live, field) == at(&original, field))
        {
            return Ok(());
        }
        let ours = |field: &Vec<String>| {
            at(&live, field) == at(&self.installed, field)
                || at(&live, field) == at(&original, field)
                || self
                    .previous
                    .as_ref()
                    .is_some_and(|previous| at(&live, field) == at(previous, field))
        };
        if !keep_changed && !self.fields.iter().all(ours) {
            return Err(CONFLICT.into());
        }
        let fields = self
            .fields
            .iter()
            .filter(|field| ours(field))
            .collect::<Vec<_>>();
        for field in fields {
            put(&mut live, field, at(&original, field).cloned())?;
        }
        if matches!(self.format, Format::Cordis) && live.get("providers").is_none() {
            live["providers"] = json!({});
        }
        if matches!(self.format, Format::Cordis) && live.get("selection").is_none() {
            live["selection"] = json!([]);
        }
        if matches!(self.format, Format::Cordis) && live.get("legacyProviders").is_none() {
            live["legacyProviders"] = json!(false);
        }
        if matches!(self.format, Format::Cordis) && keep_changed {
            let selected = crate::services::deepseek_harness::selection(&live);
            let removed = selected["provider"].as_str().is_some_and(|provider| {
                provider.starts_with("varyai-")
                    && !live["providers"].as_object().is_some_and(|entries| {
                        entries
                            .values()
                            .any(|entry| entry["config"]["providers"].get(provider).is_some())
                    })
            });
            if removed {
                live["selection"] = original["selection"].clone();
            }
        }
        let restored = if live == original {
            self.original.clone()
        } else {
            Some(self.format.encode(text.as_deref(), &live)?)
        };
        replace_checked(&self.path, text.as_deref(), restored.as_deref())?;
        Ok(())
    }
}
pub(crate) struct Manager {
    pub root: PathBuf,
}
impl Manager {
    pub fn build(
        &self,
        app: &str,
        menu: &Menu,
        origin: &str,
        selected: &str,
    ) -> Result<Receipt, String> {
        build_with_token(
            app,
            menu,
            origin,
            selected,
            new_token(),
            &self.visibility_for(app)?,
            None,
        )
    }
    pub fn owns(&self, app: &str) -> bool {
        if app == "trae" {
            return self.bridge_owned();
        }
        APPS.contains(&app) && app != "codex" && self.load(app).map_or(true, |r| r.is_some())
    }
    fn path(&self, app: &str) -> Result<PathBuf, String> {
        if !APPS.contains(&app) || app == "codex" {
            return Err(FAILED.into());
        }
        Ok(self.root.join(format!("{app}.json")))
    }
    pub fn load(&self, app: &str) -> Result<Option<Receipt>, String> {
        let Some(text) = read(&self.path(app)?)? else {
            return Ok(None);
        };
        let mut receipt: Receipt = serde_json::from_str(&text).map_err(|_| FAILED)?;
        if receipt.app != app || receipt.files.is_empty() {
            return Err(FAILED.into());
        }
        desktop_config::adopt(&mut receipt);
        let mut adopted = receipt.clone();
        if selection::adopt(&mut adopted).is_ok() {
            receipt = adopted;
        }
        Ok(Some(receipt))
    }
    pub fn status(&self, app: &str) -> Result<Status, String> {
        let receipt = self.load(app)?;
        let configured = receipt.as_ref().is_some_and(|receipt| {
            !receipt.suspended
                && receipt
                    .files
                    .iter()
                    .all(|file| file.intact().unwrap_or(false))
        });
        let desktop_state = configured
            .then(|| desktop::state(app, receipt.as_ref().and_then(|r| r.applied_at_ms)))
            .flatten();
        Ok(Status {
            app: app.into(),
            enabled: configured,
            configured,
            recovery_pending: receipt.as_ref().is_some_and(|r| !r.suspended) && !configured,
            recovery_reason: (receipt.as_ref().is_some_and(|r| !r.suspended) && !configured)
                .then_some(RecoveryReason::ConfigurationChanged),
            model: receipt.map(|r| r.model),
            restart_required: configured
                && desktop_state.is_none_or(|state| state == desktop::State::RestartRequired),
            desktop_state,
            native_compatibility: None,
            configuration_issue: None,
        })
    }
    pub fn restart_desktop(&self, app: &str) -> Result<(), String> {
        let receipt = self.load(app)?.ok_or(CONFLICT)?;
        let valid = || {
            self.load(app).is_ok_and(|current| {
                current.is_some_and(|current| {
                    !current.suspended
                        && current.token == receipt.token
                        && current.applied_at_ms == receipt.applied_at_ms
                        && current
                            .files
                            .iter()
                            .all(|file| file.intact().unwrap_or(false))
                })
            })
        };
        desktop::restart(app, receipt.applied_at_ms, valid)
    }
    pub fn install(&self, mut receipt: Receipt) -> Result<(), String> {
        let path = self.path(&receipt.app)?;
        if read(&path)?.is_some() {
            return Err(CONFLICT.into());
        }
        desktop_config::adopt(&mut receipt);
        desktop_config::restore_settings(&self.root, &mut receipt)?;
        // Journal before changing clients, including partial failures and crash recovery.
        atomic_write_private(&path, &serde_json::to_vec(&receipt).map_err(|_| FAILED)?)
            .map_err(|_| FAILED)?;
        for (index, file) in receipt.files.iter().enumerate() {
            if let Err(error) = file.write() {
                let mut restored = true;
                for previous in receipt.files[..=index].iter().rev() {
                    restored &= previous.restore().is_ok();
                }
                if restored {
                    let _ = fs::remove_file(&path);
                }
                return Err(error);
            }
        }
        Ok(())
    }
    pub fn refresh(&self, app: &str, menu: &Menu, origin: &str) -> Result<Option<Receipt>, String> {
        self.update(app, menu, origin, None)
    }
    pub fn update(
        &self,
        app: &str,
        menu: &Menu,
        origin: &str,
        selected: Option<&str>,
    ) -> Result<Option<Receipt>, String> {
        let Some(old) = self.load(app)? else {
            return Ok(None);
        };
        let model = selected
            .or_else(|| menu.stand_in_direct(&old.model))
            .or_else(|| {
                menu.models
                    .contains_key(&old.model)
                    .then_some(old.model.as_str())
            })
            .or(menu.default_model.as_deref())
            .or_else(|| menu.models.keys().min().map(String::as_str))
            .unwrap_or(&old.model);
        let visibility = self.visibility_for(app)?;
        if old.suspended {
            // Suspension already restored the projection, so fields edited after
            // that point are no longer ours. Keep them as the baseline instead of
            // failing on the receipt's now-stale original.
            for file in old.files.iter().rev() {
                file.restore_fields(true)?;
            }
            let mut next = build_with_token(
                app,
                menu,
                origin,
                model,
                old.token.clone(),
                &visibility,
                Some(&old),
            )?;
            desktop_config::restore_settings(&self.root, &mut next)?;
            atomic_write_private(
                &self.path(app)?,
                &serde_json::to_vec(&next).map_err(|_| FAILED)?,
            )
            .map_err(|_| FAILED)?;
            for file in &next.files {
                file.write()?;
            }
            return Ok(Some(next));
        }
        if !old.files.iter().all(|file| {
            if app == "claude-desktop" {
                desktop_config::refreshable(file).unwrap_or(false)
            } else {
                file.intact().unwrap_or(false)
            }
        }) {
            return Err(CONFLICT.into());
        }
        let mut next = build_with_token(
            app,
            menu,
            origin,
            model,
            old.token.clone(),
            &visibility,
            Some(&old),
        )?;
        if next.files.len() != old.files.len()
            && !(app == "claude-desktop"
                && old
                    .files
                    .iter()
                    .all(|prior| next.files.iter().any(|file| file.path == prior.path))
                && next
                    .files
                    .iter()
                    .filter(|file| !old.files.iter().any(|prior| prior.path == file.path))
                    .all(|file| matches!(file.desktop, Some(desktop_config::Kind::Carry))))
        {
            return Err(CONFLICT.into());
        }
        if selected.is_none() {
            selection::preserve(&old, &mut next)?;
        }
        let writes = next.files.clone();
        let unchanged = writes.iter().all(|file| {
            file.format
                .parse(file.original.as_deref())
                .is_ok_and(|original| original == file.installed)
        });
        if unchanged {
            next.applied_at_ms = old.applied_at_ms;
        }
        for file in &mut next.files {
            let prior = old.files.iter().find(|prior| prior.path == file.path);
            let Some(prior) = prior else {
                continue;
            };
            file.original = prior.original.clone();
            file.previous = Some(prior.installed.clone());
            file.fields.extend(prior.fields.iter().cloned());
            file.fields.sort();
            file.fields.dedup();
            let mut owned: Vec<Vec<String>> = Vec::new();
            for field in std::mem::take(&mut file.fields) {
                if !owned.iter().any(|parent| field.starts_with(parent)) {
                    owned.push(field);
                }
            }
            file.fields = owned;
        }
        if unchanged
            && next.model_map == old.model_map
            && next.visible_models == old.visible_models
            && next
                .files
                .iter()
                .zip(&old.files)
                .all(|(next, old)| next.installed == old.installed)
        {
            return Ok(Some(old));
        }
        atomic_write_private(
            &self.path(app)?,
            &serde_json::to_vec(&next).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED)?;
        for file in &writes {
            file.write()?;
        }
        Ok(Some(next))
    }
    pub fn disconnect(&self, app: &str) -> Result<(), String> {
        let Some(receipt) = self.load(app)? else {
            return Ok(());
        };
        // Check all fields before restoring any file; unrelated edits survive.
        if !receipt
            .files
            .iter()
            .all(|file| file.restorable().unwrap_or(false))
        {
            return Err(CONFLICT.into());
        }
        desktop_config::save_settings(&self.root, &receipt)?;
        for file in receipt.files.iter().rev() {
            file.restore()?;
        }
        fs::remove_file(self.path(app)?).map_err(|_| FAILED.into())
    }
    pub fn release_for_reapply(&self, app: &str) -> Result<(), String> {
        let receipt = self.load(app)?.ok_or(CONFLICT)?;
        // Explicit reapplication retains external edits as the next connection's baseline.
        for file in &receipt.files {
            file.format.parse(read(&file.path)?.as_deref())?;
            if let Some(kind) = file.desktop {
                desktop_config::restoration(file, kind, true)?;
            }
            let has_catalog_array = file.fields.iter().any(|field| {
                at(&file.installed, field).is_some_and(Value::is_array)
                    && !(matches!(file.format, Format::Cordis) && field.as_slice() == ["selection"])
            });
            if has_catalog_array && file.desktop.is_none() {
                // Keep a changed Harness default while still guarding its model catalog.
                let mut catalog = file.clone();
                catalog.fields.retain(|field| {
                    at(&file.installed, field).is_some_and(Value::is_array)
                        && !(matches!(file.format, Format::Cordis)
                            && field.as_slice() == ["selection"])
                });
                let restorable = catalog.restorable()?;
                if !restorable {
                    return Err(CONFLICT.into());
                }
            }
        }
        desktop_config::save_settings(&self.root, &receipt)?;
        for file in receipt.files.iter().rev() {
            file.restore_fields(true)?;
        }
        fs::remove_file(self.path(app)?).map_err(|_| FAILED.into())
    }
    pub fn suspend(&self, app: &str) -> Result<(), String> {
        let Some(mut receipt) = self.load(app)? else {
            return Ok(());
        };
        if receipt.suspended {
            return Ok(());
        }
        if !receipt
            .files
            .iter()
            .all(|file| file.restorable().unwrap_or(false))
        {
            return Err(CONFLICT.into());
        }
        desktop_config::save_settings(&self.root, &receipt)?;
        receipt.suspended = true;
        atomic_write_private(
            &self.path(app)?,
            &serde_json::to_vec(&receipt).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED)?;
        for file in receipt.files.iter().rev() {
            file.restore()?;
        }
        Ok(())
    }
    pub fn codex_intent(&self) -> Result<Option<Option<String>>, String> {
        read(&self.root.join("codex-intent.json"))?
            .map(|text| serde_json::from_str(&text).map_err(|_| FAILED.into()))
            .transpose()
    }
    pub fn set_codex_intent(&self, selected: Option<&str>, enabled: bool) -> Result<(), String> {
        let path = self.root.join("codex-intent.json");
        if enabled {
            atomic_write_private(&path, &serde_json::to_vec(&selected).map_err(|_| FAILED)?)
                .map_err(|_| FAILED.into())
        } else {
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(FAILED.into()),
            }
        }
    }
}
#[cfg(test)]
pub(crate) fn build(
    app: &str,
    menu: &Menu,
    origin: &str,
    selected: &str,
) -> Result<Receipt, String> {
    build_with_token(
        app,
        menu,
        origin,
        selected,
        new_token(),
        &ModelVisibility::default(),
        None,
    )
}
fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn build_with_token(
    app: &str,
    menu: &Menu,
    origin: &str,
    selected: &str,
    token: String,
    visibility: &ModelVisibility,
    previous: Option<&Receipt>,
) -> Result<Receipt, String> {
    let catalog = models(menu);
    if !catalog.iter().any(|m| m.id == selected) {
        return Err(EMPTY.into());
    }
    let base = format!("{origin}/connections/{app}");
    if app == "claude" {
        claude_policy::check()?;
    }
    let mut receipt = Receipt {
        app: app.into(),
        token: token.clone(),
        model: selected.into(),
        model_map: previous
            .map(|old| old.model_map.clone())
            .unwrap_or_default(),
        visible_models: None,
        files: vec![],
        suspended: false,
        applied_at_ms: Some(chrono::Utc::now().timestamp_millis()),
    };
    match app {
        "claude" => receipt.files.push(FileEdit::prepare(crate::config::get_claude_settings_path(), |value| {
            for key in ["ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_DEFAULT_SONNET_MODEL", "ANTHROPIC_DEFAULT_OPUS_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL"] {
                let next = if key == "ANTHROPIC_BASE_URL" { base.as_str() } else if key == "ANTHROPIC_AUTH_TOKEN" { &token } else { selected };
                put(value, &["env".into(), key.into()], Some(json!(next)))?;
            }
            for &key in claude_policy::CLOUD_SWITCHES {
                put(value, &["env".into(), key.into()], Some(json!("0")))?;
            }
            put(value, &["env".into(), "ANTHROPIC_MODEL".into()], None)?;
            put(value, &["model".into()], Some(json!(selected)))?;
            let mut options = value.pointer("/modelPicker/options").and_then(Value::as_array).cloned().unwrap_or_default();
            options.retain(|row| !row["model"].as_str().is_some_and(|id| id.starts_with("vary-provider/") || id.starts_with("vary-auto/")));
            options.extend(catalog.iter().map(|model| json!({"model":model.id,"label":model.name,"description":"via VaryAI"})));
            put(value, &["modelPicker".into(), "options".into()], Some(json!(options)))?;
            let context = catalog.iter().find(|m| m.id == selected).and_then(working_window);
            put(value, &["env".into(), "CLAUDE_CODE_MAX_CONTEXT_TOKENS".into()], context.map(|n| json!(n.to_string())))?;
            let capabilities = reasoning::claude_capabilities(&catalog);
            put(value, &["env".into(), "CLAUDE_CODE_MODEL_CAPABILITIES".into()], (!capabilities.is_empty()).then(|| json!(capabilities)))?;
            put(value, &["env".into(), "ANTHROPIC_API_KEY".into()], None)
        })?),
        "pi" => {
            let (catalog, selected) = projections::named_catalog(&catalog, selected, &mut receipt.model_map);
            receipt.files.push(FileEdit::prepare(crate::pi_config::get_pi_models_path().map_err(|_| FAILED)?, |value| {
                let models = catalog.iter().map(pi_model).collect::<Vec<_>>();
                put(value, &["providers".into(), "varyai".into()], Some(json!({"baseUrl":format!("{base}/v1"),"apiKey":token,"api":"openai-completions","models":models})))
            })?);
            receipt.files.push(FileEdit::prepare(crate::pi_config::get_pi_settings_path().map_err(|_| FAILED)?, |value| {
                put(value, &["defaultProvider".into()], Some(json!("varyai")))?;
                put(value, &["defaultModel".into()], Some(json!(selected)))?;
                if let Some(scope) = value.get_mut("enabledModels").and_then(Value::as_array_mut) {
                    for entry in scope.iter_mut() {
                        if let Some(alias) = entry.as_str().and_then(|id| id.strip_prefix("varyai/")) {
                            if let Some((name, _)) = receipt.model_map.iter().find(|(_, internal)| internal.as_str() == alias) {
                                *entry = json!(format!("varyai/{name}"));
                            }
                        }
                    }
                    if !scope.is_empty() { let reference = json!(format!("varyai/{selected}")); if !scope.contains(&reference) { scope.push(reference); } }
                }
                Ok(())
            })?);
        }
        "workbuddy" => {
            let (named, _) = projections::named_catalog(&catalog, selected, &mut receipt.model_map);
            let named_models = named.iter().map(|model| (model.id.clone(), model)).collect::<Vec<_>>();
            receipt.files.push(FileEdit::prepare_document(crate::workbuddy_config::get_workbuddy_config_path(), true, |value| {
            let array = value.is_array();
            if array { *value = json!({"models":value.take()}); }
            if value.get("models").is_some_and(|rows| !rows.is_array()) { return Err(FAILED.into()); }
            let mut rows = value.get("models").and_then(Value::as_array).cloned().unwrap_or_default();
            let previous = rows.iter().filter(|row| row["apiKey"].as_str() == Some(&token)).filter_map(|row| row["id"].as_str().map(str::to_owned)).collect::<Vec<_>>();
            rows.retain(|row| row["apiKey"].as_str() != Some(&token));
            if rows.iter().any(|row| named_models.iter().any(|(name, _)| row["id"] == *name)) { return Err(CONFLICT.into()); }
            for (name, model) in &named_models {
                // WorkBuddy appends the ID unless it exactly matches the name.
                let mut row = json!({"id":name,"name":name,"vendor":"Vary AI","apiKey":token,"url":format!("{base}/v1/chat/completions"),"supportsToolCall":true});
                reasoning::workbuddy(&mut row, model);
                if let Some(context) = model.context_window {
                    row["maxInputTokens"] = json!(context);
                    if context > 200_000 { row["contextWindow"] = json!({"supportedLengths":[200_000,context],"defaultLength":context}); }
                }
                rows.push(row);
            }
            value["models"] = json!(rows);
            if let Some(available) = value.get_mut("availableModels") { let available = available.as_array_mut().ok_or(FAILED)?; available.retain(|id| !id.as_str().is_some_and(|id| previous.iter().any(|old| old == id))); for (name, _) in &named_models { let id = json!(name); if !available.contains(&id) { available.push(id); } } }
            if array { *value = value["models"].take(); }
            Ok(())
        })?);
        },
        "opencode" => receipt.files.push(FileEdit::prepare_format(crate::opencode_config::get_opencode_projection_path(), Format::Json5, |value| {
            let models = catalog.iter().map(|m| (m.id.clone(), opencode_model(m))).collect::<serde_json::Map<_,_>>();
            put(value, &["provider".into(), "varyai".into()], Some(json!({"npm":"@ai-sdk/openai-compatible","name":"Vary AI","options":{"baseURL":format!("{base}/v1"),"apiKey":token},"models":models})))?;
            put(value, &["model".into()], Some(json!(format!("varyai/{selected}"))))
        })?),
        "claude-desktop" => {
            let mut visible = catalog.iter().filter(|m| visibility.visible(&m.id)).cloned().collect::<Vec<_>>();
            visibility.apply_order(&mut visible);
            let (mut files, _) = crate::claude_desktop_config::shared_catalog_edits(&base, &token, previous.filter(|old| !old.suspended).map(|old| old.token.as_str()), &visible)?;
            desktop_config::seed_tool_search(&mut files, previous);
            desktop_config::preserve_carried(&mut files, previous)?;
            receipt.files = files;
            receipt.visible_models = Some(visible.iter().map(|m| m.id.clone()).collect());
            // Keep hidden models addressable by existing Desktop conversations.
            receipt.model_map = catalog.iter().map(|m| (crate::claude_desktop_config::shared_model_wire(m), m.id.clone())).collect();
        }
        _ => projections::build(app, &catalog, &base, &token, selected, &mut receipt)?,
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_formats_restore_owned_fields_and_preserve_external_edits() -> Result<(), String> {
        for (format, original, external) in [
            (
                Format::Toml,
                "# keep\ntheme = 'light'\n",
                "theme = 'dark'\n",
            ),
            (Format::Yaml, "# keep\ntheme: light\n", "theme: dark\n"),
            (Format::Env, "# keep\nTHEME=light\n", "THEME=dark\n"),
            (Format::Json5, "{theme: 'light',}", "{theme: 'dark',}"),
        ] {
            let dir = tempfile::tempdir().map_err(|_| FAILED)?;
            let path = dir.path().join("native");
            fs::write(&path, original).map_err(|_| FAILED)?;
            let edit = FileEdit::prepare_format(path.clone(), format, |value| {
                value["LOCAL"] = json!("synthetic");
                Ok(())
            })?;
            edit.write()?;
            assert!(edit.intact()?);
            let mut value = format.parse(Some(external))?;
            value["LOCAL"] = json!("synthetic");
            fs::write(&path, format.encode(Some(external), &value)?).map_err(|_| FAILED)?;
            edit.restore()?;
            assert_eq!(
                format.parse(read(&path)?.as_deref())?,
                format.parse(Some(external))?
            );
            let next = FileEdit::prepare_format(path.clone(), format, |value| {
                value["LOCAL"] = json!("synthetic");
                Ok(())
            })?;
            next.write()?;
            let mut changed = format.parse(read(&path)?.as_deref())?;
            changed["LOCAL"] = json!("external");
            fs::write(&path, format.encode(read(&path)?.as_deref(), &changed)?)
                .map_err(|_| FAILED)?;
            assert_eq!(next.restore(), Err(CONFLICT.into()));
        }
        Ok(())
    }
    #[test]
    fn mixed_format_receipts_reopen_and_restore_without_legacy_imports() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let root = dir.path().join("receipts");
        let manager = Manager { root: root.clone() };
        for (app, format, source) in [
            ("grokbuild", Format::Toml, "theme = 'light'\n"),
            ("hermes", Format::Yaml, "theme: light\n"),
            ("gemini", Format::Env, "THEME=light\n"),
            ("openclaw", Format::Json5, "{// keep\ntheme:'light'}"),
        ] {
            let path = dir.path().join(app);
            fs::write(&path, source).map_err(|_| FAILED)?;
            let file = FileEdit::prepare_format(path.clone(), format, |value| {
                value["LOCAL"] = json!("synthetic");
                Ok(())
            })?;
            manager.install(Receipt {
                app: app.into(),
                token: concat!("synthetic", "-local").into(),
                model: "shared".into(),
                model_map: Default::default(),
                visible_models: None,
                files: vec![file],
                suspended: false,
                applied_at_ms: None,
            })?;
            let reopened = Manager { root: root.clone() };
            assert!(reopened.status(app)?.configured);
            reopened.suspend(app)?;
            assert!(reopened.load(app)?.is_some_and(|receipt| receipt.suspended));
            assert_eq!(fs::read_to_string(&path).map_err(|_| FAILED)?, source);
            reopened.disconnect(app)?;
            assert!(!reopened.owns(app));
        }
        let mut legacy =
            serde_json::to_value(FileEdit::prepare(dir.path().join("legacy"), |value| {
                value["LOCAL"] = json!(true);
                Ok(())
            })?)
            .map_err(|_| FAILED)?;
        legacy.as_object_mut().ok_or(FAILED)?.remove("format");
        let legacy: FileEdit = serde_json::from_value(legacy).map_err(|_| FAILED)?;
        legacy.write()?;
        legacy.restore()?;
        Ok(())
    }
    #[test]
    fn array_documents_restore_without_changing_the_native_shape() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("models.json");
        fs::write(&path, r#"[{"id":"native"}]"#).map_err(|_| FAILED)?;
        let edit = FileEdit::prepare_document(path.clone(), true, |value| {
            value
                .as_array_mut()
                .ok_or(FAILED)?
                .push(json!({"id":"shared"}));
            Ok(())
        })?;
        edit.write()?;
        assert!(parse(read(&path)?.as_deref())?.is_array());
        edit.restore()?;
        assert_eq!(read(&path)?.as_deref(), Some(r#"[{"id":"native"}]"#));
        Ok(())
    }
    #[test]
    fn explicit_null_is_not_confused_with_an_absent_container() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("config.json");
        fs::write(&path, r#"{"provider":null}"#).map_err(|_| FAILED)?;
        let edit = FileEdit::prepare(path.clone(), |value| {
            value["provider"] = json!({"model":"shared"});
            Ok(())
        })?;
        edit.write()?;
        edit.restore()?;
        assert_eq!(parse(read(&path)?.as_deref())?, json!({"provider":null}));
        Ok(())
    }
    #[test]
    fn replacing_a_scalar_container_restores_its_original_value() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("client.json");
        fs::write(&path, r#"{"provider":"original"}"#).map_err(|_| FAILED)?;
        let edit = FileEdit::prepare(path.clone(), |value| {
            value["provider"] = json!({"varyai":{"model":"shared"}});
            Ok(())
        })?;
        edit.write()?;
        edit.restore()?;
        assert_eq!(
            parse(read(&path)?.as_deref())?,
            json!({"provider":"original"})
        );
        Ok(())
    }
    #[test]
    fn partially_restored_receipts_can_finish_recovery() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().join("receipts"),
        };
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        let receipt = Receipt {
            app: "claude".into(),
            token: "synthetic".into(),
            model: "shared".into(),
            model_map: BTreeMap::new(),
            visible_models: None,
            suspended: false,
            applied_at_ms: None,
            files: vec![
                FileEdit::prepare(a.clone(), |v| {
                    v["model"] = json!("shared");
                    Ok(())
                })?,
                FileEdit::prepare(b.clone(), |v| {
                    v["model"] = json!("shared");
                    Ok(())
                })?,
            ],
        };
        manager.install(receipt.clone())?;
        manager.suspend("claude")?;
        assert!(manager.load("claude")?.is_some_and(|r| r.suspended));
        assert!(!a.exists());
        assert!(!b.exists());
        assert!(!manager.status("claude")?.recovery_pending);
        manager.disconnect("claude")?;
        manager.install(receipt.clone())?;
        receipt.files[1].restore()?;
        manager.disconnect("claude")?;
        assert!(!a.exists());
        assert!(!b.exists());
        assert!(manager.load("claude")?.is_none());
        Ok(())
    }
    #[test]
    fn newly_created_nested_fields_do_not_own_unrelated_siblings() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("client.json");
        let edit = FileEdit::prepare(path.clone(), |v| {
            v["env"] = json!({"LOCAL":"synthetic"});
            Ok(())
        })?;
        edit.write()?;
        let mut v = parse(read(&path)?.as_deref())?;
        v["env"]["USER_SETTING"] = json!(true);
        fs::write(&path, v.to_string()).map_err(|_| FAILED)?;
        edit.restore()?;
        assert_eq!(
            parse(read(&path)?.as_deref())?,
            json!({"env":{"USER_SETTING":true}})
        );
        Ok(())
    }

    #[test]
    fn disconnect_restores_owned_fields_and_keeps_unrelated_edits() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("client.json");
        fs::write(
            &path,
            r#"{"env":{"TOKEN":"synthetic-original"},"theme":"light"}"#,
        )
        .map_err(|_| FAILED)?;
        let file = FileEdit::prepare(path.clone(), |v| {
            v["env"]["TOKEN"] = json!("synthetic-local");
            v["model"] = json!("shared-model");
            Ok(())
        })?;
        file.write()?;
        let mut live = parse(read(&path)?.as_deref())?;
        live["theme"] = json!("dark");
        fs::write(&path, live.to_string()).map_err(|_| FAILED)?;
        file.restore()?;
        assert_eq!(
            parse(read(&path)?.as_deref())?,
            json!({"env":{"TOKEN":"synthetic-original"},"theme":"dark"})
        );
        Ok(())
    }
    #[test]
    fn changed_owned_fields_are_never_overwritten() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("client.json");
        let file = FileEdit::prepare(path.clone(), |v| {
            v["model"] = json!("shared");
            Ok(())
        })?;
        file.write()?;
        fs::write(&path, r#"{"model":"user-choice"}"#).map_err(|_| FAILED)?;
        assert_eq!(file.restore(), Err(CONFLICT.into()));
        assert_eq!(parse(read(&path)?.as_deref())?["model"], "user-choice");
        Ok(())
    }
    #[test]
    fn explicit_reapply_release_checks_all_files_before_preserving_external_changes(
    ) -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().join("receipts"),
        };
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        let mut files = Vec::new();
        for path in [&a, &b] {
            files.push(FileEdit::prepare(path.clone(), |value| {
                value["model"] = json!("shared");
                Ok(())
            })?);
        }
        manager.install(Receipt {
            app: "claude".into(),
            token: "fixture-reapply-token".into(),
            model: "shared".into(),
            model_map: BTreeMap::new(),
            visible_models: None,
            files,
            suspended: false,
            applied_at_ms: None,
        })?;
        fs::write(&a, r#"{"model":"external","theme":"keep"}"#).map_err(|_| FAILED)?;
        fs::write(&b, "malformed").map_err(|_| FAILED)?;
        assert!(manager.release_for_reapply("claude").is_err());
        assert_eq!(
            read(&a)?.as_deref(),
            Some(r#"{"model":"external","theme":"keep"}"#)
        );
        assert!(manager.load("claude")?.is_some());
        fs::write(&b, r#"{"model":"shared","theme":"keep"}"#).map_err(|_| FAILED)?;
        manager.release_for_reapply("claude")?;
        assert_eq!(
            parse(read(&a)?.as_deref())?,
            json!({"model":"external","theme":"keep"})
        );
        assert_eq!(parse(read(&b)?.as_deref())?, json!({"theme":"keep"}));
        assert!(manager.load("claude")?.is_none());
        Ok(())
    }
    #[test]
    fn explicit_reapply_does_not_guess_ownership_inside_a_changed_array() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().join("receipts"),
        };
        let path = dir.path().join("models.json");
        let edit = FileEdit::prepare_document(path.clone(), true, |value| {
            *value = json!([{"id":"shared"}]);
            Ok(())
        })?;
        manager.install(Receipt {
            app: "workbuddy".into(),
            token: "fixture-array-token".into(),
            model: "shared".into(),
            model_map: BTreeMap::new(),
            visible_models: None,
            files: vec![edit],
            suspended: false,
            applied_at_ms: None,
        })?;
        let changed = r#"[{"id":"shared"},{"id":"external"}]"#;
        fs::write(&path, changed).map_err(|_| FAILED)?;
        assert_eq!(
            manager.release_for_reapply("workbuddy"),
            Err(CONFLICT.into())
        );
        assert_eq!(read(&path)?.as_deref(), Some(changed));
        assert!(manager.load("workbuddy")?.is_some());
        Ok(())
    }
    fn fixture_menu() -> Menu {
        use crate::{
            proxy::intent::{MenuModel, Session, Target},
            services::intent_routing::RoutingGroup,
        };
        use std::{collections::HashMap, sync::Arc, time::Instant};
        let group: RoutingGroup = serde_json::from_value(json!({
            "id":"fixture","name":"Fixture","revision":0,"enabled":true,"members":[],
            "defaultMemberId":"a","fallbackMemberIds":[],"intentRecognitionEnabled":false,
            "classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
        }))
        .expect("fixture group");
        let alias = group.alias();
        let target = Target {
            provider_id: "fixture".into(),
            model: "exact".into(),
            upstream: None,
            effort: Default::default(),
            url: "http://127.0.0.1:1/responses".parse().expect("fixture url"),
            headers: Default::default(),
            official: false,
            credentials: None,
            scheduling: Arc::default(),
            fixed_effort: None,
        };
        Menu {
            token: String::new(),
            models: HashMap::from([(
                alias.clone(),
                MenuModel {
                    alias: alias.clone(),
                    label: "Fixture".into(),
                    description: "VaryAI routing group",
                    order: 0,
                },
            )]),
            default_model: Some(alias.clone()),
            omitted_entries: 0,
            context_settings: Default::default(),
            groups: HashMap::from([(
                alias,
                Arc::new(Session {
                    group,
                    targets: vec![target],
                    classifier: None,
                    client: reqwest::Client::new(),
                    created: Instant::now(),
                    bindings: Arc::default(),
                }),
            )]),
        }
    }
    #[test]
    fn suspended_receipts_reconnect_after_a_restored_field_is_edited() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "default_model = \"native/one\"\n\n[providers.native]\ntype = \"kimi\"\napi_key = \"synthetic-native\"\n\n[models.\"native/one\"]\nprovider = \"native\"\nmodel = \"one\"\nmax_context_size = 128000\n",
        )
        .map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().join("receipts"),
        };
        let menu = fixture_menu();
        let selected = menu.default_model.clone().ok_or(FAILED)?;
        let origin = "http://127.0.0.1:9";
        let receipt = crate::services::agent_clients::with_test_config(&path, || {
            manager.build("kimi", &menu, origin, &selected)
        })?;
        manager.install(receipt)?;
        manager.suspend("kimi")?;
        assert!(manager
            .load("kimi")?
            .is_some_and(|receipt| receipt.suspended));
        // Kimi Code rewrites default_model when the user picks another model
        // while the shared projection is suspended.
        fs::write(
            &path,
            "default_model = \"native/two\"\n\n[providers.native]\ntype = \"kimi\"\napi_key = \"synthetic-native\"\n\n[models.\"native/one\"]\nprovider = \"native\"\nmodel = \"one\"\nmax_context_size = 128000\n",
        )
        .map_err(|_| FAILED)?;
        crate::services::agent_clients::with_test_config(&path, || {
            manager.update("kimi", &menu, origin, Some(&selected))
        })?
        .ok_or(FAILED)?;
        assert!(manager.status("kimi")?.configured);
        let live = Format::Toml.parse(read(&path)?.as_deref())?;
        assert_eq!(live["default_model"], json!("Fixture"));
        assert_eq!(
            live["providers"]["native"]["api_key"],
            json!("synthetic-native")
        );
        assert!(live["providers"].get("Vary AI").is_some());
        Ok(())
    }
    #[test]
    fn original_absence_and_unrelated_siblings_survive_disconnect() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("client.json");
        let file = FileEdit::prepare(path.clone(), |v| {
            v["env"] = json!({"LOCAL":"synthetic"});
            Ok(())
        })?;
        file.write()?;
        let mut value = parse(read(&path)?.as_deref())?;
        value["theme"] = json!("dark");
        fs::write(&path, value.to_string()).map_err(|_| FAILED)?;
        file.restore()?;
        assert_eq!(parse(read(&path)?.as_deref())?, json!({"theme":"dark"}));
        Ok(())
    }
}
