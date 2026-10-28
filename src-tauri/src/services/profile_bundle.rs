mod files;
mod library;
mod native;
mod trae;
// Reviewed cross-agent profiles. Secrets never enter preview DTOs.
// Undo is bounded to the last operation in this application session.
use super::{profile::ProfilePayload, ProviderService};
use crate::{
    app_config::AppType, database::Profile, error::AppError, provider::Provider, store::AppState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

// Reserved selection marker for the shared menu, independent of client scopes.
pub(crate) const BUNDLE_SCOPE: &str = "bundle";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSelection {
    pub app: AppType,
    pub provider_id: String,
    /// Only model selectors, never credentials or complete provider settings.
    pub models: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub providers: Option<Vec<ProviderModels>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "native_snapshot"
    )]
    pub native_models: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<library::LibrarySelection>,
}

// A stored null means restore the native default; a missing field is a legacy
// bundle that did not capture this setting and must leave it untouched.
fn native_snapshot<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

const WORKBUDDY_MODEL_FIELDS: &[&str] = &[
    "id",
    "name",
    "maxInputTokens",
    "maxOutputTokens",
    "supportsToolCall",
    "supportsImages",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModels {
    pub id: String,
    pub models: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfileBundle {
    pub selections: Vec<ProfileSelection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileChange {
    pub app: String,
    pub current_provider: String,
    pub target_provider: String,
    pub models: Vec<String>,
    pub enabled_providers: Vec<String>,
    pub library: Option<library::LibraryPreview>,
    pub current_library: Option<library::LibraryPreview>,
    pub current_models: Vec<String>,
    pub client_sync_pending: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfilePreview {
    pub revision: String,
    pub changes: Vec<ProfileChange>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileApplyResult {
    pub success: bool,
    pub undo_id: Option<String>,
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub scopes: Vec<AppType>,
}

#[derive(Default)]
pub(crate) struct ProfileBundleState {
    operation: Mutex<()>,
    undo: Mutex<Option<Undo>>,
}

impl ProfileBundleState {
    fn lock_operation(&self) -> MutexGuard<'_, ()> {
        match self.operation.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let guard = poisoned.into_inner();
                // This mutex carries no data. Re-read live state on the next
                // operation, but never reuse undo from an interrupted operation.
                *self.lock_undo() = None;
                self.operation.clear_poison();
                log::warn!("Recovered an interrupted profile operation; previous undo expired");
                guard
            }
        }
    }

    fn lock_undo(&self) -> MutexGuard<'_, Option<Undo>> {
        match self.undo.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                // An unwind may have interrupted an undo record update. Drop
                // that record instead of restoring from a partial snapshot.
                *guard = None;
                self.undo.clear_poison();
                log::warn!("Discarded an interrupted profile undo record");
                guard
            }
        }
    }
}

struct Undo {
    id: String,
    profile_id: String,
    previous_bundle_id: Option<String>,
    bundle: ProfileBundle,
    revision: String,
    before: Vec<AppSnapshot>,
    applied: Vec<Value>,
}

struct FileState {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}

struct AppSnapshot {
    app: AppType,
    providers: Vec<Provider>,
    current: Option<String>,
    local_current: Option<String>,
    profile_current: Option<String>,
    snippet: Option<String>,
    backup: Option<String>,
    files: Vec<FileState>,
    codex: Option<crate::codex_config::CodexLiveStateSnapshot>,
    library: library::LibraryBefore,
    library_fingerprint: Value,
    trees: Vec<files::Tree>,
    sources: Vec<files::Tree>,
    settings: Vec<(String, Option<String>)>,
}

fn error(key: &str) -> AppError {
    AppError::Message(format!("profiles.bundle.{key}"))
}

fn initialize_providers(
    state: &AppState,
    apps: impl IntoIterator<Item = AppType>,
) -> Result<(), AppError> {
    for app in apps {
        if matches!(
            app,
            AppType::WorkBuddy
                | AppType::Qoder
                | AppType::Kimi
                | AppType::Qwen
                | AppType::ZCode
                | AppType::Trae
        ) {
            // The UI lists seed official entries. Do that before the undo snapshot,
            // so its first read does not look like an external configuration edit.
            ProviderService::list(state, app)?;
        }
    }
    Ok(())
}

fn captures_current_supplier(app: &AppType) -> bool {
    matches!(
        app,
        AppType::WorkBuddy
            | AppType::Qoder
            | AppType::Kimi
            | AppType::Qwen
            | AppType::ZCode
            | AppType::Trae
    )
}

fn scopes(apps: &[String]) -> Result<Vec<AppType>, AppError> {
    let mut result = Vec::new();
    for candidate in AppType::all() {
        if apps.iter().any(|app| app == candidate.as_str()) {
            if candidate == AppType::DoubaoWork {
                return Err(error("nativeLoginOnly"));
            }
            result.push(candidate);
        }
    }
    if result.is_empty() || result.len() != apps.len() {
        return Err(error("selectAgents"));
    }
    Ok(result)
}

const ENV_MODELS: &[&str] = &[
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
];
const CODEX_MODELS: &[&str] = &[
    "model",
    "model_reasoning_effort",
    "model_reasoning_summary",
    "model_verbosity",
];

fn model_settings(app: &AppType, provider: &Provider) -> Result<Value, AppError> {
    if *app == AppType::Gemini {
        return Ok(
            json!({"GEMINI_MODEL":provider.settings_config["env"]["GEMINI_MODEL"], "model":provider.settings_config["config"]["model"]}),
        );
    }
    if *app == AppType::GrokBuild {
        return native::grok_models(provider);
    }
    if *app == AppType::WorkBuddy {
        let mut models = native::provider_models(provider);
        models["singleModel"] = json!(WORKBUDDY_MODEL_FIELDS
            .iter()
            .map(|key| (key.to_string(), provider.settings_config[*key].clone()))
            .collect::<serde_json::Map<_, _>>());
        return Ok(models);
    }
    let mut values = serde_json::Map::new();
    if *app == AppType::ClaudeDesktop {
        values.insert(
            "routes".into(),
            serde_json::to_value(
                provider
                    .meta
                    .as_ref()
                    .map(|m| &m.claude_desktop_model_routes),
            )
            .map_err(|_| error("invalidConfig"))?,
        );
    } else if *app == AppType::Codex {
        let config = provider.settings_config["config"]
            .as_str()
            .unwrap_or_default();
        let doc = config
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| error("invalidConfig"))?;
        for key in CODEX_MODELS {
            if doc.get(key).is_some_and(|value| value.as_str().is_none()) {
                return Err(error("invalidConfig"));
            }
            values.insert(
                (*key).into(),
                doc.get(key)
                    .and_then(|v| v.as_str())
                    .map(|s| json!(s))
                    .unwrap_or(Value::Null),
            );
        }
    } else if *app == AppType::Claude {
        for key in ENV_MODELS {
            if !provider.settings_config["env"][*key].is_null()
                && !provider.settings_config["env"][*key].is_string()
            {
                return Err(error("invalidConfig"));
            }
            values.insert((*key).into(), provider.settings_config["env"][*key].clone());
        }
    }
    if !matches!(
        app,
        AppType::Claude | AppType::ClaudeDesktop | AppType::Codex
    ) {
        return Ok(native::provider_models(provider));
    }
    if *app == AppType::Codex {
        values.insert(
            "catalog".into(),
            native::provider_models(provider)["modelCatalog"].clone(),
        );
    }
    Ok(Value::Object(values))
}

fn with_models(app: &AppType, provider: &Provider, models: &Value) -> Result<Provider, AppError> {
    if !provider.settings_config.is_object() || !models.is_object() {
        return Err(error("invalidConfig"));
    }
    if *app == AppType::GrokBuild {
        return native::with_grok_models(provider, models);
    }
    if *app == AppType::WorkBuddy {
        let mut p = native::with_provider_models(provider, models)?;
        if let Some(saved) = models.get("singleModel") {
            for key in WORKBUDDY_MODEL_FIELDS {
                if saved[*key].is_null() {
                    p.settings_config
                        .as_object_mut()
                        .map(|map| map.remove(*key));
                } else {
                    p.settings_config[*key] = saved[*key].clone();
                }
            }
        }
        return Ok(p);
    }
    if *app == AppType::Gemini {
        let mut p = provider.clone();
        if !p.settings_config.is_object() {
            return Err(error("invalidConfig"));
        }
        if !p.settings_config["env"].is_object() {
            p.settings_config["env"] = json!({});
        }
        if models["GEMINI_MODEL"].is_null() {
            p.settings_config["env"]
                .as_object_mut()
                .map(|m| m.remove("GEMINI_MODEL"));
        } else if models["GEMINI_MODEL"].is_string() {
            p.settings_config["env"]["GEMINI_MODEL"] = models["GEMINI_MODEL"].clone();
        } else {
            return Err(error("invalidConfig"));
        }
        if let Some(model) = models.get("model") {
            if !p.settings_config["config"].is_object() {
                p.settings_config["config"] = json!({});
            }
            if model.is_null() {
                p.settings_config["config"]
                    .as_object_mut()
                    .map(|m| m.remove("model"));
            } else {
                p.settings_config["config"]["model"] = model.clone();
            }
        }
        return Ok(p);
    }
    if !matches!(
        app,
        AppType::Claude | AppType::ClaudeDesktop | AppType::Codex
    ) {
        return native::with_provider_models(provider, models);
    }
    let mut updated = provider.clone();
    if *app == AppType::ClaudeDesktop {
        updated
            .meta
            .get_or_insert_with(Default::default)
            .claude_desktop_model_routes = if models["routes"].is_null() {
            Default::default()
        } else {
            serde_json::from_value(models["routes"].clone()).map_err(|_| error("invalidConfig"))?
        };
    } else if *app == AppType::Codex {
        let mut doc = provider.settings_config["config"]
            .as_str()
            .unwrap_or_default()
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| error("invalidConfig"))?;
        for key in CODEX_MODELS {
            if let Some(value) = models[*key].as_str() {
                doc[*key] = toml_edit::value(value);
            } else {
                doc.remove(key);
            }
        }
        updated.settings_config["config"] = json!(doc.to_string());
        if let Some(catalog) = models.get("catalog") {
            if catalog.is_null() {
                updated
                    .settings_config
                    .as_object_mut()
                    .map(|m| m.remove("modelCatalog"));
            } else {
                updated.settings_config["modelCatalog"] = catalog.clone();
            }
        }
    } else {
        if !updated.settings_config["env"].is_object() {
            updated.settings_config["env"] = json!({});
        }
        if let Some(env) = updated.settings_config["env"].as_object_mut() {
            for key in ENV_MODELS {
                if let Some(value) = models[*key].as_str() {
                    env.insert((*key).into(), json!(value));
                } else {
                    env.remove(*key);
                }
            }
        }
    }
    Ok(updated)
}

impl AppSnapshot {
    fn capture(state: &AppState, app: AppType) -> Result<Self, AppError> {
        let mut paths = native::configuration_paths(&app)?;
        if library::instructions(&app) {
            paths.push(crate::prompt_files::prompt_file_path(&app)?);
        }
        if app == AppType::OpenCode {
            let home = crate::config::get_home_dir();
            for name in ["omo.json", "omo.jsonc"] {
                paths.push(home.join(".omo").join(name));
            }
            for name in [
                "oh-my-opencode.json",
                "oh-my-opencode.jsonc",
                "oh-my-opencode-slim.json",
                "oh-my-opencode-slim.jsonc",
            ] {
                paths.push(crate::opencode_config::get_opencode_dir().join(name));
            }
        }
        let mut trees = Vec::new();
        if library::instructions(&app) {
            // Atomic projection replaces a symlink with a file. Undo must
            // restore the link itself as well as the previous content.
            trees.push(files::Tree::capture(
                crate::prompt_files::prompt_file_path(&app)?,
            )?);
        }
        let mut sources = Vec::new();
        if library::skills(&app) {
            let root = crate::services::SkillService::get_app_skills_dir(&app)
                .map_err(|_| error("readFailed"))?;
            for skill in state.db.get_all_installed_skills()?.into_values() {
                if std::path::Path::new(&skill.directory).components().count() != 1 {
                    return Err(error("invalidConfig"));
                }
                trees.push(files::Tree::capture(root.join(&skill.directory))?);
                sources.push(files::Tree::capture(
                    crate::services::SkillService::get_ssot_dir()
                        .map_err(|_| error("readFailed"))?
                        .join(skill.directory),
                )?);
            }
        }
        if app == AppType::Pi {
            trees.push(files::Tree::capture(
                crate::pi_config::get_pi_agent_dir()?.join("prompts"),
            )?);
        }
        let files = paths
            .into_iter()
            .map(|path| {
                let bytes = match std::fs::read(&path) {
                    Ok(bytes) => Some(bytes),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(_) => return Err(error("readFailed")),
                };
                Ok(FileState { path, bytes })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            providers: state
                .db
                .get_all_providers(app.as_str())?
                .into_values()
                .collect(),
            current: state.db.get_current_provider(app.as_str())?,
            local_current: crate::settings::get_current_provider(&app),
            profile_current: state.db.get_current_profile_id(app.as_str())?,
            snippet: state.db.get_config_snippet(app.as_str())?,
            backup: futures::executor::block_on(state.db.get_live_backup(app.as_str()))?
                .map(|b| b.original_config),
            codex: if app == AppType::Codex {
                Some(crate::codex_config::CodexLiveStateSnapshot::capture()?)
            } else {
                None
            },
            library: library::LibraryBefore::capture(state, &app)?,
            library_fingerprint: library::fingerprint(state, &app)?,
            trees,
            sources,
            settings: if app == AppType::Trae {
                vec![(
                    trae::PENDING_KEY.into(),
                    state.db.get_setting(trae::PENDING_KEY)?,
                )]
            } else if crate::services::agent_clients::supported(app.as_str()) {
                [
                    format!("{}_official_model_selection", app.as_str()),
                    crate::services::agent_clients::enabled_key(app.as_str()),
                ]
                .into_iter()
                .map(|key| Ok((key.clone(), state.db.get_setting(&key)?)))
                .collect::<Result<Vec<_>, AppError>>()?
            } else {
                Vec::new()
            },
            app,
            files,
        })
    }

    fn fingerprint(&self) -> Value {
        json!({"app":self.app, "providers":self.providers, "current":self.current,
            "local":self.local_current, "profile":self.profile_current, "snippet":self.snippet, "backup":self.backup,
            "library":self.library_fingerprint,"settings":self.settings,"sources":self.sources.iter().map(files::Tree::fingerprint).collect::<Vec<_>>(),"trees":self.trees.iter().map(files::Tree::fingerprint).collect::<Vec<_>>(),
            "files":self.files.iter().map(|f| json!([f.path, f.bytes.as_ref().map(|b| format!("{:x}", Sha256::digest(b)))])).collect::<Vec<_>>()})
    }

    fn restore(&self, state: &AppState) -> Result<(), AppError> {
        // Restore routing through its existing switch path first; exact file and
        // provider snapshots below undo backfill and preserve external settings.
        let mut failed = false;
        if self.backup.is_some() && self.app != AppType::Trae {
            if let Some(id) = self.local_current.as_ref().or(self.current.as_ref()) {
                failed |=
                    ProviderService::switch_with_lock_held(state, self.app.clone(), id).is_err();
            }
        }
        failed |= self.library.restore(state, &self.app).is_err();
        for provider in &self.providers {
            failed |= state.db.save_provider(self.app.as_str(), provider).is_err();
        }
        failed |= state
            .db
            .set_current_provider(self.app.as_str(), self.current.as_deref().unwrap_or(""))
            .is_err();
        failed |= crate::settings::set_current_provider(&self.app, self.local_current.as_deref())
            .is_err();
        failed |= state
            .db
            .set_current_profile_id(self.app.as_str(), self.profile_current.as_deref())
            .is_err();
        failed |= state
            .db
            .set_config_snippet(self.app.as_str(), self.snippet.clone())
            .is_err();
        failed |= futures::executor::block_on(async {
            match &self.backup {
                Some(backup) => state.db.save_live_backup(self.app.as_str(), backup).await,
                None => state.db.delete_live_backup(self.app.as_str()).await,
            }
        })
        .is_err();
        if let Some(snapshot) = &self.codex {
            failed |= snapshot
                .restore_preserving_newer_same_account_auth()
                .is_err();
        }
        {
            for file in &self.files {
                if self.codex.is_some()
                    && file.path != crate::prompt_files::prompt_file_path(&self.app)?
                {
                    continue;
                }
                let result = match &file.bytes {
                    Some(bytes) => crate::config::atomic_write_private(&file.path, bytes),
                    None => match std::fs::remove_file(&file.path) {
                        Ok(()) => Ok(()),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(_) => Err(error("restoreFailed")),
                    },
                };
                failed |= result.is_err();
            }
        }
        for tree in &self.trees {
            failed |= tree.restore().is_err();
        }
        for (key, value) in &self.settings {
            failed |= match value {
                Some(v) => state.db.set_setting(key, v),
                None => state.db.delete_setting(key),
            }
            .is_err();
        }
        if failed {
            Err(error("restoreFailed"))
        } else {
            Ok(())
        }
    }
}

fn load(state: &AppState, id: &str) -> Result<(Profile, ProfileBundle), AppError> {
    let profile = state.db.get_profile(id)?.ok_or_else(|| error("notFound"))?;
    let payload: ProfilePayload =
        serde_json::from_str(&profile.payload).map_err(|_| error("invalidConfig"))?;
    let bundle = payload.bundle.ok_or_else(|| error("notFound"))?;
    scopes(
        &bundle
            .selections
            .iter()
            .map(|s| s.app.as_str().to_string())
            .collect::<Vec<_>>(),
    )?;
    Ok((profile, bundle))
}

fn snapshots(state: &AppState, bundle: &ProfileBundle) -> Result<Vec<AppSnapshot>, AppError> {
    bundle
        .selections
        .iter()
        .map(|s| AppSnapshot::capture(state, s.app.clone()))
        .collect()
}

fn revision(profile: &Profile, snapshots: &[AppSnapshot]) -> Result<String, AppError> {
    fingerprint_revision(
        profile,
        &snapshots
            .iter()
            .map(AppSnapshot::fingerprint)
            .collect::<Vec<_>>(),
    )
}

fn fingerprint_revision(profile: &Profile, fingerprints: &[Value]) -> Result<String, AppError> {
    let mut value = json!([profile.payload, fingerprints]);
    // Provider metadata contains HashMaps whose iteration order can change between reads.
    value.sort_all_objects();
    let bytes = serde_json::to_vec(&value).map_err(|_| error("invalidConfig"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn lock_apps(state: &AppState, bundle: &ProfileBundle) -> Vec<tokio::sync::OwnedMutexGuard<()>> {
    // Always Claude -> Desktop -> Codex, including profiles loaded from disk.
    AppType::all()
        .filter(|scope| bundle.selections.iter().any(|s| s.app == *scope))
        .map(|scope| {
            futures::executor::block_on(state.proxy_service.lock_switch_for_app(scope.as_str()))
        })
        .collect()
}

fn current_models(state: &AppState, app: &AppType, provider: &Provider) -> Result<Value, AppError> {
    let mut source = native::live_provider(app, provider)?;
    // During takeover the live files contain gateway placeholders; the provider
    // is then the model source. Otherwise include edits made by the agent itself.
    let taken_over = futures::executor::block_on(state.db.get_live_backup(app.as_str()))?.is_some();
    if !taken_over {
        let path = match app {
            AppType::Claude => Some(crate::config::get_claude_settings_path()),
            AppType::Codex => Some(crate::codex_config::get_codex_config_path()),
            AppType::GrokBuild => Some(crate::grok_config::get_grok_config_path()),
            _ => None,
        };
        if let Some(path) = path {
            match std::fs::read_to_string(path) {
                Ok(contents) if matches!(app, AppType::Codex | AppType::GrokBuild) => {
                    source.settings_config["config"] = json!(contents)
                }
                Ok(contents) => {
                    source.settings_config =
                        serde_json::from_str(&contents).map_err(|_| error("invalidConfig"))?
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(error("readFailed")),
            }
        }
    }
    if *app == AppType::Gemini
        && !taken_over
        && crate::gemini_config::get_gemini_env_path().exists()
    {
        source.settings_config["env"] =
            crate::gemini_config::env_to_json(&crate::gemini_config::read_gemini_env()?)["env"]
                .clone();
    }
    if *app == AppType::Gemini
        && !taken_over
        && crate::gemini_config::get_gemini_settings_path().exists()
    {
        source.settings_config["config"] = crate::config::read_json_file::<Value>(
            &crate::gemini_config::get_gemini_settings_path(),
        )?;
    }
    if *app == AppType::Codex && !taken_over {
        if let Some(catalog) = crate::codex_config::read_codex_model_catalog_simplified_from_live()?
        {
            source.settings_config["modelCatalog"] = catalog;
        }
    }
    model_settings(app, &source)
}

fn restore_all(state: &AppState, snapshots: &[AppSnapshot]) -> Result<(), AppError> {
    let mut failed = false;
    for snapshot in snapshots.iter().rev() {
        failed |= snapshot.restore(state).is_err();
    }
    if failed {
        Err(error("restoreFailed"))
    } else {
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileAgent {
    pub app: AppType,
    pub provider: bool,
    pub mcp: bool,
    pub skills: bool,
    pub instructions: bool,
    pub available: bool,
    pub reason: Option<String>,
}
pub fn agents(state: &AppState) -> Result<Vec<ProfileAgent>, AppError> {
    AppType::all()
        .map(|app| {
            let supported = app != AppType::DoubaoWork
                && !(cfg!(target_os = "linux") && app == AppType::ClaudeDesktop);
            let available = supported && !state.db.get_all_providers(app.as_str())?.is_empty();
            let reason = if !supported {
                Some("profiles.bundle.nativeLoginOnly".into())
            } else if !available {
                Some("profiles.bundle.configureFirst".into())
            } else if app == AppType::Trae {
                Some("profiles.bundle.clientSyncHint".into())
            } else {
                None
            };
            Ok(ProfileAgent {
                provider: supported,
                mcp: library::mcp(&app),
                skills: library::skills(&app),
                instructions: library::instructions(&app),
                app,
                available,
                reason,
            })
        })
        .collect()
}

pub struct ProfileBundleService;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileUndo {
    pub id: String,
    pub profile_id: String,
    pub name: String,
    pub apps: Vec<String>,
}
impl ProfileBundleService {
    pub fn clear_current(state: &AppState) -> Result<(), AppError> {
        let _operation = state.profile_bundles.lock_operation();
        if let Some(id) = state.db.get_current_profile_id(BUNDLE_SCOPE)? {
            for app in AppType::all() {
                if state.db.get_current_profile_id(app.as_str())?.as_deref() == Some(&id) {
                    state.db.set_current_profile_id(app.as_str(), None)?;
                }
            }
        }
        state.db.set_current_profile_id(BUNDLE_SCOPE, None)
    }

    pub fn last_undo(state: &AppState) -> Result<Option<ProfileUndo>, AppError> {
        let slot = state.profile_bundles.lock_undo();
        Ok(slot.as_ref().map(|undo| ProfileUndo {
            id: undo.id.clone(),
            profile_id: undo.profile_id.clone(),
            name: state
                .db
                .get_profile(&undo.profile_id)
                .ok()
                .flatten()
                .map(|p| p.name)
                .unwrap_or_default(),
            apps: undo
                .bundle
                .selections
                .iter()
                .map(|s| s.app.as_str().to_owned())
                .collect(),
        }))
    }

    pub fn capture(
        state: &AppState,
        name: &str,
        apps: &[String],
        id: Option<&str>,
    ) -> Result<Profile, AppError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(error("nameRequired"));
        }
        let scopes = scopes(apps)?;
        let skeleton = ProfileBundle {
            selections: scopes
                .iter()
                .map(|app| ProfileSelection {
                    app: app.clone(),
                    provider_id: String::new(),
                    models: Value::Null,
                    providers: None,
                    native_models: None,
                    library: None,
                })
                .collect(),
        };
        let _operation = state.profile_bundles.lock_operation();
        initialize_providers(state, skeleton.selections.iter().map(|s| s.app.clone()))?;
        let _locks = lock_apps(state, &skeleton);
        let mut selections = Vec::new();
        for scope in scopes {
            let app = &scope;
            let current = if captures_current_supplier(app) {
                state.db.get_current_provider(app.as_str())?
            } else {
                crate::settings::get_effective_current_provider(&state.db, app)?
            };
            let all = state.db.get_all_providers(app.as_str())?;
            let id = current
                .or_else(|| {
                    if app.is_additive_mode() {
                        all.keys().next().cloned()
                    } else {
                        None
                    }
                })
                .ok_or_else(|| error("noProvider"))?;
            let provider = state
                .db
                .get_provider_by_id(&id, app.as_str())?
                .ok_or_else(|| error("noProvider"))?;
            selections.push(ProfileSelection {
                app: scope.clone(),
                provider_id: id.clone(),
                models: current_models(state, app, &provider)?,
                providers: if app.is_additive_mode() {
                    let ids = native::enabled(state, app)?;
                    Some(
                        all.values()
                            .filter(|p| {
                                ids.contains(&p.id)
                                    && (!captures_current_supplier(app) || p.id == id)
                            })
                            .map(|p| {
                                Ok(ProviderModels {
                                    id: p.id.clone(),
                                    models: current_models(state, app, p)?,
                                })
                            })
                            .collect::<Result<Vec<_>, AppError>>()?,
                    )
                } else {
                    None
                },
                native_models: Some(native::selection_for_provider(app, &provider)?),
                library: Some(library::LibrarySelection::capture(state, app)?),
            });
        }
        let mut profile = if let Some(id) = id {
            load(state, id)?.0
        } else {
            Profile {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.into(),
                payload: String::new(),
                sort_order: None,
                created_at: Some(chrono::Utc::now().timestamp()),
                updated_at: None,
            }
        };
        let payload = ProfilePayload {
            bundle: Some(ProfileBundle { selections }),
            ..Default::default()
        };
        profile.name = name.into();
        profile.payload = serde_json::to_string(&payload).map_err(|_| error("invalidConfig"))?;
        profile.updated_at = Some(chrono::Utc::now().timestamp());
        state.db.save_profile(&profile)?;
        Ok(profile)
    }

    pub fn preview(state: &AppState, id: &str) -> Result<ProfilePreview, AppError> {
        let _operation = state.profile_bundles.lock_operation();
        let (profile, bundle) = load(state, id)?;
        initialize_providers(state, bundle.selections.iter().map(|s| s.app.clone()))?;
        let _locks = lock_apps(state, &bundle);
        let before = snapshots(state, &bundle)?;
        let mut changes = Vec::new();
        for (selection, snapshot) in bundle.selections.iter().zip(&before) {
            let target = snapshot
                .providers
                .iter()
                .find(|p| p.id == selection.provider_id)
                .ok_or_else(|| error("noProvider"))?;
            with_models(&snapshot.app, target, &selection.models)?;
            if let Some(library) = &selection.library {
                library.validate(state, &snapshot.app)?;
            }
            if let Some(providers) = &selection.providers {
                for saved in providers {
                    let provider = snapshot
                        .providers
                        .iter()
                        .find(|p| p.id == saved.id)
                        .ok_or_else(|| error("missingResource"))?;
                    with_models(&snapshot.app, provider, &saved.models)?;
                }
            }
            let current = if captures_current_supplier(&snapshot.app) {
                snapshot.current.as_ref()
            } else {
                snapshot
                    .local_current
                    .as_ref()
                    .or(snapshot.current.as_ref())
            };
            let enabled_now =
                if selection.providers.is_some() && !captures_current_supplier(&snapshot.app) {
                    let ids = native::enabled(state, &snapshot.app)?;
                    snapshot
                        .providers
                        .iter()
                        .filter(|p| ids.contains(&p.id))
                        .collect::<Vec<_>>()
                } else {
                    current
                        .and_then(|id| snapshot.providers.iter().find(|p| &p.id == id))
                        .into_iter()
                        .collect()
                };
            let enabled_targets = selection.providers.as_ref().map(|ps| {
                ps.iter()
                    .filter_map(|p| snapshot.providers.iter().find(|target| target.id == p.id))
                    .collect::<Vec<_>>()
            });
            changes.push(ProfileChange {
                app: selection.app.as_str().into(),
                client_sync_pending: snapshot.app == AppType::Trae
                    && state.db.get_bool_flag(trae::PENDING_KEY)?,
                current_provider: enabled_now
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" · "),
                target_provider: enabled_targets
                    .as_ref()
                    .map(|ps| {
                        ps.iter()
                            .map(|p| p.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    })
                    .unwrap_or_else(|| target.name.clone()),
                enabled_providers: selection
                    .providers
                    .as_ref()
                    .map(|ps| {
                        ps.iter()
                            .filter_map(|p| {
                                snapshot
                                    .providers
                                    .iter()
                                    .find(|target| target.id == p.id)
                                    .map(|p| p.name.clone())
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                library: selection
                    .library
                    .as_ref()
                    .map(|l| l.preview(state, &snapshot.app))
                    .transpose()?,
                current_library: Some(
                    library::LibrarySelection::capture(state, &snapshot.app)?
                        .preview(state, &snapshot.app)?,
                ),
                current_models: {
                    let mut values = Vec::new();
                    for provider in enabled_now {
                        values.extend(native::model_labels(&current_models(
                            state,
                            &snapshot.app,
                            provider,
                        )?));
                    }
                    values.extend(native::model_labels(&native::selection(&snapshot.app)?));
                    values.sort();
                    values.dedup();
                    values
                },
                models: {
                    let mut values = selection
                        .providers
                        .as_ref()
                        .map(|ps| {
                            ps.iter()
                                .flat_map(|p| native::model_labels(&p.models))
                                .collect()
                        })
                        .unwrap_or_else(|| native::model_labels(&selection.models));
                    if let Some(native) = &selection.native_models {
                        values.extend(native::model_labels(native));
                    }
                    values.sort();
                    values.dedup();
                    values
                },
            });
        }
        Ok(ProfilePreview {
            revision: revision(&profile, &before)?,
            changes,
        })
    }

    pub fn apply(
        state: &AppState,
        id: &str,
        expected: &str,
    ) -> Result<ProfileApplyResult, AppError> {
        Self::apply_using(state, id, expected, |state, app, id| {
            ProviderService::switch_with_lock_held(state, app, id).map(|r| r.warnings)
        })
    }

    fn apply_using<F>(
        state: &AppState,
        id: &str,
        expected: &str,
        mut switch: F,
    ) -> Result<ProfileApplyResult, AppError>
    where
        F: FnMut(&AppState, AppType, &str) -> Result<Vec<String>, AppError>,
    {
        let started = std::time::Instant::now();
        let _operation = state.profile_bundles.lock_operation();
        let (profile, bundle) = load(state, id)?;
        initialize_providers(state, bundle.selections.iter().map(|s| s.app.clone()))?;
        let _locks = lock_apps(state, &bundle);
        let before = snapshots(state, &bundle)?;
        let previous_bundle_id = state.db.get_current_profile_id(BUNDLE_SCOPE)?;
        let fingerprints = before
            .iter()
            .map(AppSnapshot::fingerprint)
            .collect::<Vec<_>>();
        if fingerprint_revision(&profile, &fingerprints)? != expected {
            return Err(error("conflict"));
        }
        // Validate every target before the first write.
        let targets = bundle
            .selections
            .iter()
            .zip(&before)
            .map(|(s, snapshot)| {
                let provider = snapshot
                    .providers
                    .iter()
                    .find(|p| p.id == s.provider_id)
                    .ok_or_else(|| error("noProvider"))?;
                with_models(&snapshot.app, provider, &s.models)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut undo_slot = state.profile_bundles.lock_undo();
        let mut warnings = Vec::new();
        let mut reused = 0;
        for (index, ((selection, snapshot), target)) in bundle
            .selections
            .iter()
            .zip(&before)
            .zip(&targets)
            .enumerate()
        {
            // Reuse only an identical selection whose complete local state
            // still matches the previous successful apply. This includes
            // files, resource definitions, credentials and external edits.
            let unchanged = undo_slot.as_ref().is_some_and(|previous| {
                previous
                    .bundle
                    .selections
                    .iter()
                    .position(|old| old.app == selection.app)
                    .is_some_and(|position| {
                        previous.bundle.selections[position] == *selection
                            && previous.applied.get(position) == Some(&fingerprints[index])
                    })
            });
            let result = (|| {
                if unchanged {
                    state
                        .db
                        .set_current_profile_id(selection.app.as_str(), Some(id))?;
                    reused += 1;
                    return Ok(());
                }
                if snapshot.app == AppType::Trae {
                    trae::select(state, target)?;
                    state
                        .db
                        .set_current_profile_id(selection.app.as_str(), Some(id))?;
                    return Ok(());
                }
                ProviderService::save_profile_models_with_lock_held(state, &snapshot.app, target)?;
                if let Some(providers) = &selection.providers {
                    for saved in providers {
                        let provider = snapshot
                            .providers
                            .iter()
                            .find(|p| p.id == saved.id)
                            .ok_or_else(|| error("missingResource"))?;
                        ProviderService::save_profile_models_with_lock_held(
                            state,
                            &snapshot.app,
                            &with_models(&snapshot.app, provider, &saved.models)?,
                        )?;
                        if snapshot.app == AppType::Pi {
                            crate::pi_config::replace_pi_provider_if_present(
                                &saved.id,
                                &with_models(&snapshot.app, provider, &saved.models)?
                                    .settings_config,
                            )?;
                        }
                        switch(state, snapshot.app.clone(), &saved.id)?;
                    }
                    for old in native::enabled(state, &snapshot.app)? {
                        if !providers.iter().any(|p| p.id == old)
                            && snapshot.providers.iter().any(|p| p.id == old)
                        {
                            if state
                                .db
                                .get_current_provider(snapshot.app.as_str())?
                                .as_deref()
                                == Some(&old)
                            {
                                state.db.set_current_provider(snapshot.app.as_str(), "")?;
                            }
                            ProviderService::remove_from_live_config_with_lock_held(
                                state,
                                snapshot.app.clone(),
                                &old,
                            )?;
                        }
                    }
                }
                let should_switch = selection
                    .providers
                    .as_ref()
                    .is_none_or(|ps| ps.iter().any(|p| p.id == target.id))
                    && snapshot.app != AppType::Pi
                    || target.category.as_deref() == Some("official");
                if should_switch && !switch(state, snapshot.app.clone(), &target.id)?.is_empty() {
                    warnings.push("profiles.bundle.applyWarning".into());
                }
                if let Some(models) = &selection.native_models {
                    native::apply_selection(&snapshot.app, models)?;
                }
                if let Some(library) = &selection.library {
                    library.apply(state, &snapshot.app)?;
                }
                state
                    .db
                    .set_current_profile_id(selection.app.as_str(), Some(id))?;
                Ok::<(), AppError>(())
            })();
            if result.is_err() {
                log::warn!(
                    "Profile bundle apply failed for {}; restoring attempted agents",
                    snapshot.app.as_str()
                );
                let restored = restore_all(state, &before[..=index]);
                // No raw upstream/config errors: they can include credentials.
                return if restored.is_err() {
                    Err(error("restoreFailed"))
                } else {
                    Ok(ProfileApplyResult {
                        success: false,
                        undo_id: None,
                        warnings: vec!["profiles.bundle.applyRolledBack".into()],
                        scopes: bundle.selections.iter().map(|s| s.app.clone()).collect(),
                    })
                };
            }
        }
        // Finish the local transaction and its undo snapshot before touching
        // a client. A local failure must never leave a partially delivered scheme.
        let mut after = match snapshots(state, &bundle) {
            Ok(after) => after,
            Err(_) => {
                restore_all(state, &before)?;
                return Ok(ProfileApplyResult {
                    success: false,
                    undo_id: None,
                    warnings: vec!["profiles.bundle.applyRolledBack".into()],
                    scopes: bundle.selections.iter().map(|s| s.app.clone()).collect(),
                });
            }
        };
        if let Some(trae) = after.iter_mut().find(|s| {
            s.app == AppType::Trae
                && s.settings.iter().any(|(key, value)| {
                    key == trae::PENDING_KEY && value.as_deref() == Some("true")
                })
        }) {
            if trae::sync(state) {
                // Delivery only changes this local status flag. Retain the
                // file snapshots above so external edits during delivery are
                // still detected by undo rather than silently accepted.
                trae.settings = vec![(trae::PENDING_KEY.into(), Some("false".into()))];
            } else {
                warnings.push("profiles.bundle.clientSyncPending".into());
            }
        }
        let applied = after
            .iter()
            .map(AppSnapshot::fingerprint)
            .collect::<Vec<_>>();
        let after_revision = fingerprint_revision(&profile, &applied)?;
        if state
            .db
            .set_current_profile_id(BUNDLE_SCOPE, Some(id))
            .is_err()
        {
            restore_all(state, &before)?;
            return Err(error("operationFailed"));
        }
        let undo_id = uuid::Uuid::new_v4().to_string();
        let scopes: Vec<_> = bundle.selections.iter().map(|s| s.app.clone()).collect();
        let undo = Undo {
            id: undo_id.clone(),
            profile_id: id.into(),
            previous_bundle_id,
            bundle,
            revision: after_revision,
            before,
            applied,
        };
        *undo_slot = Some(undo);
        log::info!(
            "Profile apply completed in {} ms; reused {}/{} agent configurations",
            started.elapsed().as_millis(),
            reused,
            scopes.len()
        );
        Ok(ProfileApplyResult {
            success: true,
            undo_id: Some(undo_id),
            warnings,
            scopes,
        })
    }

    pub fn undo(state: &AppState, id: &str) -> Result<Vec<AppType>, AppError> {
        let _operation = state.profile_bundles.lock_operation();
        let mut slot = state.profile_bundles.lock_undo();
        let undo = slot
            .as_mut()
            .filter(|u| u.id == id)
            .ok_or_else(|| error("undoExpired"))?;
        let _locks = lock_apps(state, &undo.bundle);
        let (profile, _) = load(state, &undo.profile_id)?;
        let after = snapshots(state, &undo.bundle)?;
        if revision(&profile, &after)? != undo.revision {
            return Err(error("conflict"));
        }
        if restore_all(state, &undo.before).is_err() {
            restore_all(state, &after)?;
            undo.revision = revision(&profile, &snapshots(state, &undo.bundle)?)?;
            return Err(error("restoreFailed"));
        }
        if state
            .db
            .set_current_profile_id(BUNDLE_SCOPE, undo.previous_bundle_id.as_deref())
            .is_err()
        {
            restore_all(state, &after)?;
            undo.revision = revision(&profile, &snapshots(state, &undo.bundle)?)?;
            return Err(error("restoreFailed"));
        }
        if undo
            .bundle
            .selections
            .iter()
            .any(|s| s.app == AppType::Trae)
        {
            // Restoring the local scheme also succeeds with the client closed.
            // An unsuccessful delivery remains recorded for the next apply.
            state.db.set_setting(trae::PENDING_KEY, "true")?;
            trae::sync(state);
        }
        let scopes = undo
            .bundle
            .selections
            .iter()
            .map(|s| s.app.clone())
            .collect();
        *slot = None;
        Ok(scopes)
    }
}

#[cfg(test)]
mod revision_tests {
    use super::*;

    #[test]
    fn revision_ignores_nested_object_order_but_detects_changed_values_and_arrays() {
        let profile = Profile {
            id: "synthetic-profile".into(),
            name: "Synthetic profile".into(),
            payload: "{}".into(),
            sort_order: None,
            created_at: None,
            updated_at: None,
        };
        let first: Value = serde_json::from_str(
            r#"{"providers":[{"meta":{"routes":{"a":"model-a","b":"model-b"}},"models":["a","b"]}]}"#,
        ).unwrap();
        let reordered: Value = serde_json::from_str(
            r#"{"providers":[{"models":["a","b"],"meta":{"routes":{"b":"model-b","a":"model-a"}}}]}"#,
        ).unwrap();
        let expected = fingerprint_revision(&profile, &[first]).unwrap();
        assert_eq!(
            expected,
            fingerprint_revision(&profile, &[reordered.clone()]).unwrap()
        );
        let mut changed = reordered.clone();
        changed["providers"][0]["meta"]["routes"]["b"] = json!("model-c");
        assert_ne!(
            expected,
            fingerprint_revision(&profile, &[changed]).unwrap()
        );
        let mut changed = reordered;
        changed["providers"][0]["models"] = json!(["b", "a"]);
        assert_ne!(
            expected,
            fingerprint_revision(&profile, &[changed]).unwrap()
        );
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use std::{
        panic::{catch_unwind, AssertUnwindSafe},
        sync::Arc,
    };

    fn state_with_undo() -> AppState {
        let state = AppState::new(Arc::new(crate::database::Database::memory().unwrap()));
        *state.profile_bundles.undo.lock().unwrap() = Some(Undo {
            id: "old-undo".into(),
            profile_id: "missing-profile".into(),
            previous_bundle_id: None,
            bundle: ProfileBundle { selections: vec![] },
            revision: "old-revision".into(),
            before: vec![],
            applied: vec![],
        });
        state
    }

    #[test]
    fn preview_recovers_after_interrupted_operation_and_expires_old_undo() {
        let state = state_with_undo();
        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            let _operation = state.profile_bundles.operation.lock().unwrap();
            panic!("synthetic interrupted profile preview");
        }));
        assert!(interrupted.is_err());

        for _ in 0..2 {
            let error = ProfileBundleService::preview(&state, "missing-profile").unwrap_err();
            assert!(
                error.to_string().contains("profiles.bundle.notFound"),
                "{error}"
            );
        }
        assert!(!state.profile_bundles.operation.is_poisoned());
        assert!(ProfileBundleService::last_undo(&state).unwrap().is_none());
    }

    #[test]
    fn interrupted_undo_record_is_discarded_instead_of_reporting_busy_forever() {
        let state = state_with_undo();
        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            let _undo = state.profile_bundles.undo.lock().unwrap();
            panic!("synthetic interrupted profile undo");
        }));
        assert!(interrupted.is_err());

        assert!(ProfileBundleService::last_undo(&state).unwrap().is_none());
        assert!(!state.profile_bundles.undo.is_poisoned());
        let error = ProfileBundleService::undo(&state, "old-undo").unwrap_err();
        assert!(
            error.to_string().contains("profiles.bundle.undoExpired"),
            "{error}"
        );
    }
}
