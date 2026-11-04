//! Codex model-menu publication with durable, field-owned configuration recovery.
use super::RoutingGroup;
use crate::proxy::intent::Runtime;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use toml_edit::DocumentMut;

const PROVIDER: &str = "varyai-routing";
const FIELDS: &[&str] = &[
    "model",
    "model_provider",
    "model_catalog_json",
    "web_search",
];
const FAILED: &str = "intentRouting.clientFailed";
const CONFLICT: &str = "intentRouting.clientConflict";

pub(crate) struct Manager {
    config: PathBuf,
    receipt: PathBuf,
    pub runtime: Arc<Runtime>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub enabled: bool,
    pub configuration_owned: bool,
    pub provider_enabled: bool,
    pub recovery_pending: bool,
    pub orphaned_config: bool,
    pub orphan_cleanup_available: bool,
    pub group_ids: Vec<String>,
    pub omitted_entries: usize,
    pub native_compatibility: String,
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    version: u32,
    config: PathBuf,
    original: Option<String>,
    installed: String,
    previous: Option<String>,
    group_ids: Vec<String>,
    catalogs: Vec<PathBuf>,
    #[serde(default)]
    previous_group_ids: Vec<String>,
    #[serde(default)]
    model_aliases: Vec<String>,
    #[serde(default)]
    previous_model_aliases: Vec<String>,
    #[serde(default)]
    native: Option<NativeOwnership>,
    #[serde(default)]
    pre_write: Option<ConfigSnapshot>,
}
#[derive(Clone, Serialize, Deserialize)]
struct NativeOwnership {
    // A present baseline proves explicit acquisition, even when the file was absent.
    #[serde(default)]
    acquisition_baseline: Option<String>,
    #[serde(default)]
    installed: Option<String>,
    #[serde(default)]
    previous_owned: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct ConfigSnapshot {
    text: Option<String>,
}
fn valid_native_url(value: &str) -> bool {
    let Some(authority) = value
        .strip_prefix("http://")
        .and_then(|rest| rest.strip_suffix("/backend-api/codex"))
    else {
        return false;
    };
    if authority.is_empty()
        || authority.contains(['@', '/', '?', '#', '\\'])
        || value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return false;
    }
    url::Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "http"
            && url.username().is_empty()
            && url.password().is_none()
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
            && url.port().is_some()
            && url.path() == "/backend-api/codex"
            && url.query().is_none()
            && url.fragment().is_none()
    })
}
impl Receipt {
    fn native_ownership(&self) -> Option<&NativeOwnership> {
        let native = self.native.as_ref().filter(|_| self.version == 2)?;
        let baseline: toml::Value = native.acquisition_baseline.as_ref()?.parse().ok()?;
        let installed: toml::Value = self.installed.parse().ok()?;
        let url = native.installed.as_deref()?;
        (baseline.get("openai_base_url").is_none()
            && valid_native_url(url)
            && installed
                .get("openai_base_url")
                .and_then(toml::Value::as_str)
                == Some(url)
            && native
                .previous_owned
                .as_deref()
                .is_none_or(valid_native_url))
        .then_some(native)
    }
}
fn read(path: &Path) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() || m.len() > 4 * 1024 * 1024 => Err(FAILED.into()),
        Ok(_) => fs::read_to_string(path)
            .map(Some)
            .map_err(|_| FAILED.into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(FAILED.into()),
    }
}
fn requires_openai_auth(config: &Path, doc: &DocumentMut) -> bool {
    use crate::codex_config::{codex_config_auth_store_mode, CodexAuthStoreMode};

    match codex_config_auth_store_mode(&doc.to_string()) {
        CodexAuthStoreMode::File => config
            .parent()
            .and_then(|parent| read(&parent.join("auth.json")).ok().flatten())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .is_some_and(|auth| {
                auth["OPENAI_API_KEY"].as_str() != Some("PROXY_MANAGED")
                    && crate::codex_config::codex_auth_has_openai_account_material(&auth)
            }),
        CodexAuthStoreMode::Ephemeral => false,
        CodexAuthStoreMode::Keyring | CodexAuthStoreMode::Auto | CodexAuthStoreMode::Unknown => {
            // Disk cannot establish keyring login state; preserve the active provider's policy.
            let provider = doc
                .get("model_provider")
                .and_then(|value| value.as_str())
                .unwrap_or("openai");
            doc.get("model_providers")
                .and_then(|providers| providers.get(provider))
                .and_then(|provider| provider.get("requires_openai_auth"))
                .and_then(|value| value.as_bool())
                .unwrap_or(provider == "openai")
        }
    }
}
/// Install through a no-clobber hard link. A concurrent editor's replacement wins.
/// Quarantined originals are retained on a conflict that prevents safe rollback.
fn replace_checked(
    path: &Path,
    expected: Option<&str>,
    replacement: Option<&str>,
) -> Result<(), String> {
    if read(path)?.as_deref() != expected {
        return Err(CONFLICT.into());
    }
    let parent = path.parent().ok_or(FAILED)?;
    fs::create_dir_all(parent).map_err(|_| FAILED)?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let staged = parent.join(format!(".vary-routing-{suffix}.staged"));
    let held = parent.join(format!(".vary-routing-{suffix}.original"));
    if let Some(text) = replacement {
        crate::config::atomic_write_private(&staged, text.as_bytes()).map_err(|_| FAILED)?;
    }
    let result = (|| {
        if expected.is_some() {
            fs::rename(path, &held).map_err(|_| CONFLICT)?;
            if read(&held)?.as_deref() != expected {
                let _ = fs::hard_link(&held, path).map(|_| fs::remove_file(&held));
                return Err(CONFLICT.into());
            }
        }
        if replacement.is_some() {
            if fs::hard_link(&staged, path).is_err() {
                if expected.is_some() {
                    let _ = fs::hard_link(&held, path).map(|_| fs::remove_file(&held));
                }
                return Err(CONFLICT.into());
            }
        } else if path.exists() {
            return Err(CONFLICT.into());
        }
        if expected.is_some() {
            let _ = fs::remove_file(&held);
        }
        Ok(())
    })();
    if replacement.is_some() {
        let _ = fs::remove_file(&staged);
    }
    result
}
pub(crate) fn catalog(
    groups: &[RoutingGroup],
    efforts: &std::collections::HashMap<String, Vec<String>>,
    chosen: &str,
    images: &std::collections::HashMap<String, bool>,
) -> Result<Value, String> {
    let template: Value = serde_json::from_str(include_str!(
        "../../../resources/codex_native_responses_template.json"
    ))
    .map_err(|_| FAILED)?;
    let models = groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let mut model = template.clone();
            model["slug"] = json!(group.alias());
            model["display_name"] = json!(format!("{} · routing group", group.name));
            model["description"] = json!("VaryAI routing group");
            model["priority"] = json!(index);
            // Never infer capability from an alias or a model-family name.
            super::context::Metadata::shared(std::iter::empty()).write(&mut model);
            model["input_modalities"] = if images.get(&group.id).copied().unwrap_or(false) {
                json!(["text", "image"])
            } else {
                json!(["text"])
            };
            model["supports_parallel_tool_calls"] = json!(false);
            super::effort::advertise(
                &mut model,
                if !group.levels.is_empty() {
                    &group.levels
                } else {
                    efforts
                        .get(&group.id)
                        .map(Vec::as_slice)
                        .unwrap_or_default()
                },
                chosen,
            );
            model
        })
        .collect::<Vec<_>>();
    Ok(json!({"models":models}))
}
pub(crate) fn menu_catalog(
    menu: &crate::proxy::intent::Menu,
    chosen: &str,
) -> Result<Value, String> {
    let mut groups = menu
        .groups
        .values()
        .filter(|session| menu.models.is_empty() || menu.models.contains_key(&session.group.id))
        .map(|session| session.group.clone())
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| {
        (
            menu.models
                .get(&group.id)
                .map_or(usize::MAX, |entry| entry.order),
            group.id.clone(),
        )
    });
    let efforts = menu
        .groups
        .values()
        .map(|session| (session.group.id.clone(), session.offered_levels()))
        .collect();
    let images = menu
        .groups
        .values()
        .map(|session| {
            (
                session.group.id.clone(),
                session
                    .menu_targets()
                    .any(|target| target.effort.images != Some(false)),
            )
        })
        .collect();
    let mut projection = catalog(&groups, &efforts, chosen, &images)?;
    if let Some(models) = projection["models"].as_array_mut() {
        for (model, group) in models.iter_mut().zip(&groups) {
            if let Some(session) = menu.groups.values().find(|s| s.group.id == group.id) {
                if chosen.is_empty() {
                    super::effort::advertise(
                        model,
                        &session.offered_levels(),
                        session.default_reasoning_level().as_deref().unwrap_or(""),
                    );
                }
                let metadata = super::context::Metadata::for_session(session);
                let typed = super::context::Metadata::session_typed_window(session);
                let vendor =
                    !session.targets.is_empty() && session.targets.iter().all(|t| t.official);
                metadata.publish(&menu.context_settings, typed, vendor, model);
            }
            if let Some(entry) = menu.models.get(&group.id) {
                model["slug"] = json!(entry.alias);
                model["display_name"] = json!(entry.label);
                model["description"] = json!(entry.description);
            }
        }
    }
    Ok(projection)
}
impl Manager {
    pub fn new(config: PathBuf, receipt: PathBuf, runtime: Arc<Runtime>) -> Self {
        Self {
            config,
            receipt,
            runtime,
        }
    }
    fn load(&self) -> Result<Option<Receipt>, String> {
        let Some(text) = read(&self.receipt)? else {
            return Ok(None);
        };
        let receipt: Receipt = serde_json::from_str(&text).map_err(|_| FAILED)?;
        if !matches!(receipt.version, 1 | 2) || receipt.config != self.config {
            return Err(CONFLICT.into());
        }
        Ok(Some(receipt))
    }
    pub fn status(&self) -> Result<Status, String> {
        let receipt = self.load()?;
        let live = self.runtime.menu_active();
        let orphan = if receipt.is_none() {
            orphan_state(read(&self.config)?.as_deref())?
        } else {
            (false, false)
        };
        let owned = receipt.as_ref().is_some_and(|r| {
            read(&self.config)
                .ok()
                .flatten()
                .is_some_and(|s| projection_owned(&s, &r.installed))
        });
        Ok(Status {
            native_compatibility: self.runtime.native.status().into(),
            enabled: live && owned,
            configuration_owned: owned,
            provider_enabled: false,
            omitted_entries: self
                .runtime
                .menu_snapshot()?
                .map_or(0, |m| m.omitted_entries),
            recovery_pending: receipt.is_some() && !(live && owned) || orphan.0,
            orphaned_config: orphan.0,
            orphan_cleanup_available: orphan.1 && !live,
            group_ids: receipt.map(|r| r.group_ids).unwrap_or_default(),
        })
    }
    pub fn active_or_pending(&self) -> bool {
        self.receipt.exists()
    }
    pub(crate) fn cleanup_orphan(&self, consent: bool) -> Result<(), String> {
        if !consent {
            return Err("intentRouting.invalid".into());
        }
        if self.runtime.menu_active() || self.load()?.is_some() {
            return Err(CONFLICT.into());
        }
        let current = read(&self.config)?.ok_or(CONFLICT)?;
        if !orphan_state(Some(&current))?.1 {
            return Err(CONFLICT.into());
        }
        let mut doc: DocumentMut = current.parse().map_err(|_| CONFLICT)?;
        doc.get_mut("model_providers")
            .and_then(|v| v.as_table_like_mut())
            .ok_or(CONFLICT)?
            .remove(PROVIDER);
        // With no receipt, keep a private full backup and touch only an inactive
        // recognized provider table. Never guess the original model or account.
        self.backup_orphan(&current)?;
        if self.receipt.exists() {
            return Err(CONFLICT.into());
        }
        replace_checked(&self.config, Some(&current), Some(&doc.to_string()))
    }
    fn backup_orphan(&self, current: &str) -> Result<(), String> {
        let backup = self.receipt.parent().ok_or(FAILED)?.join(format!(
            "vary-routing-orphan-backup-{}.toml",
            uuid::Uuid::new_v4().simple()
        ));
        crate::config::atomic_write_private(&backup, current.as_bytes()).map_err(|_| FAILED.into())
    }
    pub(crate) fn config_path(&self) -> &Path {
        &self.config
    }
    pub(crate) fn config_snapshot(&self) -> Result<Option<String>, String> {
        read(&self.config)
    }
    pub(crate) fn selected_model(&self) -> Result<Option<String>, String> {
        let Some(text) = read(&self.config)? else {
            return Ok(None);
        };
        let doc: toml::Value = text.parse().map_err(|_| FAILED)?;
        let Some(receipt) = self.load()? else {
            return Ok(None);
        };
        Ok(doc
            .get("model")
            .and_then(toml::Value::as_str)
            .filter(|alias| receipt.model_aliases.iter().any(|id| id == alias))
            .map(str::to_owned))
    }
    pub(crate) fn native_owned_url(&self) -> Result<Option<String>, String> {
        let Some(receipt) = self.load()? else {
            return Ok(None);
        };
        let Some(native) = receipt.native_ownership() else {
            return Ok(None);
        };
        let Some(current) = read(&self.config)? else {
            return Ok(None);
        };
        let doc: toml::Value = current.parse().map_err(|_| FAILED)?;
        Ok(native
            .installed
            .as_ref()
            .filter(|url| {
                doc.get("openai_base_url").and_then(toml::Value::as_str) == Some(url.as_str())
            })
            .cloned())
    }
    #[cfg(test)]
    pub(crate) fn activate_menu(
        &self,
        menu: Arc<crate::proxy::intent::Menu>,
        base: &str,
    ) -> Result<(), String> {
        self.activate_menu_with_native(menu, base, None, false)
    }
    #[cfg(test)]
    pub(crate) fn activate_menu_with_native(
        &self,
        menu: Arc<crate::proxy::intent::Menu>,
        base: &str,
        native_url: Option<&str>,
        reconnect: bool,
    ) -> Result<(), String> {
        self.activate_menu_with_selection(menu, base, native_url, reconnect, None)
    }
    pub(crate) fn activate_menu_with_selection(
        &self,
        menu: Arc<crate::proxy::intent::Menu>,
        base: &str,
        native_url: Option<&str>,
        reconnect: bool,
        selected: Option<&str>,
    ) -> Result<(), String> {
        if selected.is_some_and(|alias| !menu.groups.contains_key(alias)) {
            return Err("agentConnections.noModels".into());
        }
        let mut groups = menu
            .groups
            .values()
            .map(|session| session.group.clone())
            .collect::<Vec<_>>();
        groups.sort_by_key(|group| {
            (
                menu.models
                    .get(&group.id)
                    .map_or(usize::MAX, |entry| entry.order),
                group.id.clone(),
            )
        });
        let token = menu.token.clone();
        let previous = self.runtime.set_menu(menu)?;
        if let Err(error) =
            self.write_projection_inner(&groups, &token, base, native_url, reconnect, selected)
        {
            self.runtime.restore_menu(previous);
            return Err(error);
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn write_projection(
        &self,
        groups: &[RoutingGroup],
        token: &str,
        base: &str,
    ) -> Result<(), String> {
        self.write_projection_with_native(groups, token, base, None)
    }
    #[cfg(test)]
    pub(crate) fn write_projection_with_native(
        &self,
        groups: &[RoutingGroup],
        token: &str,
        base: &str,
        native_url: Option<&str>,
    ) -> Result<(), String> {
        self.write_projection_inner(groups, token, base, native_url, false, None)
    }
    fn write_projection_inner(
        &self,
        groups: &[RoutingGroup],
        token: &str,
        base: &str,
        native_url: Option<&str>,
        reconnect: bool,
        selected: Option<&str>,
    ) -> Result<(), String> {
        if native_url.is_some_and(|url| !valid_native_url(url)) {
            return Err("intentRouting.invalid".into());
        }
        if groups.is_empty() || groups.len() > super::menu::MAX_MENU_ENTRIES {
            return Err("intentRouting.invalid".into());
        }
        for group in groups {
            group.validate()?;
        }
        let current = read(&self.config)?;
        let old = self.load()?;
        if let Some(receipt) = &old {
            if !current
                .as_ref()
                .is_some_and(|s| projection_owned(s, &receipt.installed))
            {
                return Err(CONFLICT.into());
            }
        }
        let mut doc: DocumentMut = current
            .as_deref()
            .unwrap_or("")
            .parse()
            .map_err(|_| FAILED)?;
        let orphan = old.is_none()
            && doc
                .get("model_providers")
                .and_then(|v| v.get(PROVIDER))
                .is_some();
        if orphan && !reconnect {
            return Err(CONFLICT.into());
        }
        let previous_native = old
            .as_ref()
            .and_then(Receipt::native_ownership)
            .filter(|native| {
                doc.get("openai_base_url").and_then(|value| value.as_str())
                    == native.installed.as_deref()
            });
        let native = if let Some(url) = native_url {
            if doc.get("openai_base_url").is_some() && previous_native.is_none() {
                return Err("intentRouting.nativeConflict".into());
            }
            let ownership = NativeOwnership {
                acquisition_baseline: Some(
                    previous_native
                        .and_then(|native| native.acquisition_baseline.clone())
                        .unwrap_or_else(|| current.clone().unwrap_or_default()),
                ),
                installed: Some(url.into()),
                previous_owned: previous_native.and_then(|native| native.installed.clone()),
            };
            doc["openai_base_url"] = toml_edit::value(url);
            Some(ownership)
        } else {
            // Menu-only refresh neither acquires a URL nor relinquishes a still-owned one.
            previous_native.cloned()
        };
        let chosen = doc
            .get("model_reasoning_effort")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let menu = self.runtime.menu_snapshot()?.filter(|m| m.token == token);
        let projection = if let Some(menu) = &menu {
            let mut ids = groups.iter().map(|group| &group.id).collect::<Vec<_>>();
            ids.sort();
            let mut menu_ids = menu
                .groups
                .values()
                .map(|session| &session.group.id)
                .collect::<Vec<_>>();
            menu_ids.sort();
            if ids != menu_ids {
                return Err(CONFLICT.into());
            }
            menu_catalog(menu, chosen)?
        } else {
            catalog(groups, &Default::default(), chosen, &Default::default())?
        };
        let aliases = projection["models"]
            .as_array()
            .ok_or(FAILED)?
            .iter()
            .filter_map(|model| model["slug"].as_str())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let first_alias = aliases.first().ok_or("agentConnections.noModels")?;
        let catalog_bytes = serde_json::to_vec_pretty(&projection).map_err(|_| FAILED)?;
        if orphan {
            // Explicit reconnect replaces the reserved section, not the restored baseline.
            // Background refresh must never acquire receiptless configuration.
            self.backup_orphan(current.as_deref().ok_or(CONFLICT)?)?;
        }
        let reusable = old.as_ref().and_then(|r| r.catalogs.last()).filter(|path| {
            path.parent() == self.receipt.parent()
                && read(path)
                    .ok()
                    .flatten()
                    .is_some_and(|text| text.as_bytes() == catalog_bytes)
        });
        let new_catalog = reusable.is_none();
        let catalog_path = if let Some(path) = reusable {
            path.clone()
        } else {
            if old.as_ref().is_some_and(|r| r.catalogs.len() >= 64) {
                return Err("intentRouting.limit".into());
            }
            let path = self.receipt.parent().ok_or(FAILED)?.join(format!(
                "vary-routing-catalog-{}.json",
                uuid::Uuid::new_v4().simple()
            ));
            crate::config::atomic_write_private(&path, &catalog_bytes).map_err(|_| FAILED)?;
            path
        };
        let current_model = doc.get("model").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(selected) = selected {
            if !aliases.iter().any(|alias| alias == selected) {
                return Err("agentConnections.noModels".into());
            }
            doc["model"] = toml_edit::value(selected);
        } else if !aliases.iter().any(|alias| alias == current_model) {
            let stand_in = menu.as_ref().and_then(|m| m.stand_in_direct(current_model));
            let default = stand_in
                .or_else(|| {
                    menu.as_ref()
                        .and_then(|m| m.default_model.as_ref())
                        .filter(|alias| aliases.contains(alias))
                        .map(String::as_str)
                })
                .unwrap_or(first_alias);
            doc["model"] = toml_edit::value(default);
        }
        let keep_login = requires_openai_auth(&self.config, &doc);
        doc["model_provider"] = toml_edit::value(PROVIDER);
        doc["model_catalog_json"] = toml_edit::value(catalog_path.to_string_lossy().as_ref());
        doc["web_search"] = toml_edit::value("disabled");
        let mut provider = toml_edit::Table::new();
        provider["name"] = toml_edit::value("VaryAI models and routing groups");
        provider["base_url"] = toml_edit::value(base);
        provider["wire_api"] = toml_edit::value("responses");
        // Keep account discovery alive; the local bearer token still owns request authentication.
        provider["requires_openai_auth"] = toml_edit::value(keep_login);
        provider["experimental_bearer_token"] = toml_edit::value(token);
        provider["supports_websockets"] = toml_edit::value(false);
        provider["supports_standalone_web_search"] = toml_edit::value(false);
        if doc.get("model_providers").is_none() {
            doc["model_providers"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = doc["model_providers"].as_table_like_mut().ok_or(CONFLICT)?;
        table.insert(PROVIDER, toml_edit::Item::Table(provider));
        let mut catalogs = old.as_ref().map(|r| r.catalogs.clone()).unwrap_or_default();
        if new_catalog {
            catalogs.push(catalog_path);
        }
        let receipt = Receipt {
            version: 2,
            config: self.config.clone(),
            original: old
                .as_ref()
                .map(|r| r.original.clone())
                .unwrap_or_else(|| current.clone()),
            installed: doc.to_string(),
            previous: old.as_ref().map(|r| r.installed.clone()),
            group_ids: groups
                .iter()
                .filter(|g| {
                    menu.as_ref()
                        .and_then(|m| m.models.get(&g.id))
                        .is_none_or(|entry| !entry.alias.starts_with("vary-provider/"))
                })
                .map(|g| g.id.clone())
                .collect(),
            catalogs,
            previous_group_ids: old
                .as_ref()
                .map(|r| r.group_ids.clone())
                .unwrap_or_default(),
            model_aliases: aliases,
            previous_model_aliases: old
                .as_ref()
                .map(|r| r.model_aliases.clone())
                .unwrap_or_default(),
            native,
            pre_write: Some(ConfigSnapshot {
                text: current.clone(),
            }),
        };
        if crate::config::atomic_write_private(
            &self.receipt,
            &serde_json::to_vec(&receipt).map_err(|_| FAILED)?,
        )
        .is_err()
        {
            if let Some(path) = receipt.catalogs.last().filter(|_| new_catalog) {
                let _ = fs::remove_file(path);
            }
            return Err(FAILED.into());
        }
        replace_checked(&self.config, current.as_deref(), Some(&receipt.installed))?;
        if let Some(menu) = menu {
            self.runtime
                .native
                .publish_menu(menu, projection)
                .map_err(|_| FAILED)?;
        }
        Ok(())
    }
    pub(crate) fn disconnect(&self) -> Result<(), String> {
        self.runtime.native.revoke();
        if self.load()?.is_some() {
            self.restore()?;
        }
        self.runtime.clear_menu();
        let Some(current) = read(&self.config)? else {
            return Ok(());
        };
        let Some(disconnected) = disconnect_orphan_text(&current)? else {
            return Ok(());
        };
        // An explicit disconnect can remove recognized routing fields without
        // inventing the lost baseline. Background recovery never takes this path.
        self.backup_orphan(&current)?;
        if self.receipt.exists() {
            return Err(CONFLICT.into());
        }
        replace_checked(&self.config, Some(&current), Some(&disconnected))
    }
    pub fn restore(&self) -> Result<(), String> {
        // Revoke before fallible recovery, including missing or unreadable receipts.
        self.runtime.native.revoke();
        let Some(receipt) = self.load()? else {
            self.runtime.clear_menu();
            return Ok(());
        };
        let current = read(&self.config)?;
        let restored = restore_text(current.as_deref(), &receipt)?;
        replace_checked(&self.config, current.as_deref(), restored.as_deref())?;
        self.runtime.clear_menu();
        fs::remove_file(&self.receipt).map_err(|_| FAILED)?;
        for path in receipt.catalogs {
            if path.parent() == self.receipt.parent()
                && path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .is_some_and(|v| v.starts_with("vary-routing-catalog-") && v.ends_with(".json"))
            {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }
}

fn recognized_orphan_provider(table: &toml::map::Map<String, toml::Value>) -> bool {
    table.keys().all(|key| {
        matches!(
            key.as_str(),
            "name"
                | "wire_api"
                | "requires_openai_auth"
                | "supports_websockets"
                | "supports_standalone_web_search"
                | "experimental_bearer_token"
                | "base_url"
        )
    }) && matches!(
        table.get("name").and_then(toml::Value::as_str),
        Some("VaryAI models and routing groups" | "VaryAI routing groups")
    ) && table.get("wire_api").and_then(toml::Value::as_str) == Some("responses")
        && table
            .get("requires_openai_auth")
            .and_then(toml::Value::as_bool)
            .is_some()
        && ["supports_websockets", "supports_standalone_web_search"]
            .iter()
            .all(|key| {
                table
                    .get(*key)
                    .is_none_or(|value| value.as_bool() == Some(false))
            })
        && table
            .get("experimental_bearer_token")
            .and_then(toml::Value::as_str)
            .is_some_and(|token| {
                token.len() >= 16
                    && token.len() <= 256
                    && token
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            })
        && table
            .get("base_url")
            .and_then(toml::Value::as_str)
            .and_then(|url| url::Url::parse(url).ok())
            .is_some_and(|url| {
                url.scheme() == "http"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
                    && url.port().is_some()
                    && url.path() == "/intent/v1"
                    && url.query().is_none()
                    && url.fragment().is_none()
            })
}

fn disconnect_orphan_fields(table: &mut dyn toml_edit::TableLike) {
    if table.get("model_provider").and_then(|v| v.as_str()) == Some(PROVIDER) {
        table.remove("model_provider");
    }
    if table
        .get("model")
        .and_then(|v| v.as_str())
        .is_some_and(|model| {
            ["vary-auto/", "vary-provider/", "auto/"]
                .iter()
                .any(|prefix| model.starts_with(prefix))
        })
    {
        table.remove("model");
    }
    if table
        .get("model_catalog_json")
        .and_then(|v| v.as_str())
        .and_then(|path| Path::new(path).file_name()?.to_str())
        .and_then(|name| {
            name.strip_prefix("vary-routing-catalog-")?
                .strip_suffix(".json")
        })
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        table.remove("model_catalog_json");
    }
}

fn disconnect_orphan_text(current: &str) -> Result<Option<String>, String> {
    let value: toml::Value = current.parse().map_err(|_| CONFLICT)?;
    let top_routed = value.get("model_provider").and_then(toml::Value::as_str) == Some(PROVIDER);
    let profile = value.get("profile").and_then(toml::Value::as_str);
    let profile_routed = profile
        .and_then(|name| value.get("profiles")?.get(name)?.get("model_provider"))
        .and_then(toml::Value::as_str)
        == Some(PROVIDER);
    let provider = value
        .get("model_providers")
        .and_then(|providers| providers.get(PROVIDER))
        .and_then(toml::Value::as_table);
    let native_routed = value
        .get("openai_base_url")
        .and_then(toml::Value::as_str)
        .filter(|url| valid_native_url(url))
        .and_then(|url| url::Url::parse(url).ok())
        .zip(
            provider
                .and_then(|provider| provider.get("base_url"))
                .and_then(toml::Value::as_str)
                .and_then(|url| url::Url::parse(url).ok()),
        )
        .is_some_and(|(native, menu)| native.origin() == menu.origin());
    if !provider.is_some_and(recognized_orphan_provider) {
        return if top_routed || profile_routed || native_routed {
            Err(CONFLICT.into())
        } else {
            Ok(None)
        };
    }
    let mut doc: DocumentMut = current.parse().map_err(|_| CONFLICT)?;
    if top_routed || native_routed {
        disconnect_orphan_fields(doc.as_table_mut());
    }
    if profile_routed || (top_routed && profile.is_some()) {
        if let Some(table) = profile
            .and_then(|name| doc.get_mut("profiles")?.get_mut(name))
            .and_then(toml_edit::Item::as_table_like_mut)
        {
            let provider = table.get("model_provider").and_then(|v| v.as_str());
            if provider.is_none() || provider == Some(PROVIDER) {
                disconnect_orphan_fields(table);
            }
        }
    }
    if native_routed {
        doc.as_table_mut().remove("openai_base_url");
    }
    // Retain the provider table and inactive profiles for sessions that name them.
    let disconnected = doc.to_string();
    Ok((disconnected != current).then_some(disconnected))
}

fn orphan_state(current: Option<&str>) -> Result<(bool, bool), String> {
    let Some(current) = current else {
        return Ok((false, false));
    };
    let doc: toml::Value = current.parse().map_err(|_| FAILED)?;
    let Some(provider) = doc.get("model_providers").and_then(|p| p.get(PROVIDER)) else {
        return Ok((false, false));
    };
    let known = provider
        .as_table()
        .is_some_and(|table| table.len() == 7 && recognized_orphan_provider(table));
    Ok((true, known && !references_projection(&doc)))
}
fn references_projection(value: &toml::Value) -> bool {
    match value {
        toml::Value::Table(table) => {
            table
                .iter()
                .any(|(key, value)| match (key.as_str(), value.as_str()) {
                    ("model_provider", Some(PROVIDER)) => true,
                    ("model", Some(model))
                        if model.starts_with("vary-auto/")
                            || model.starts_with("vary-provider/")
                            || model.starts_with("auto/") =>
                    {
                        true
                    }
                    ("model_catalog_json", Some(path))
                        if Path::new(path)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .is_some_and(|s| s.starts_with("vary-routing-catalog-")) =>
                    {
                        true
                    }
                    _ => references_projection(value),
                })
        }
        toml::Value::Array(values) => values.iter().any(references_projection),
        _ => false,
    }
}
fn projection_owned(current: &str, installed: &str) -> bool {
    let (Ok(current), Ok(installed)) = (
        current.parse::<toml::Value>(),
        installed.parse::<toml::Value>(),
    ) else {
        return false;
    };
    current.get("model_provider").and_then(toml::Value::as_str) == Some(PROVIDER)
        && current.get("model_catalog_json") == installed.get("model_catalog_json")
        && current.get("model_providers").and_then(|v| v.get(PROVIDER))
            == installed
                .get("model_providers")
                .and_then(|v| v.get(PROVIDER))
}
fn restore_text(current: Option<&str>, receipt: &Receipt) -> Result<Option<String>, String> {
    let Some(current) = current else {
        // Recover the actual pre-write document, not the first takeover's baseline.
        if receipt.version == 2 {
            if let Some(snapshot) = &receipt.pre_write {
                return match snapshot.text.as_deref() {
                    Some(text) => restore_text(Some(text), receipt),
                    None => Ok(None),
                };
            }
        }
        // Legacy journals cannot disambiguate an unowned URL changed during refresh.
        let original: toml::Value = receipt
            .original
            .as_deref()
            .unwrap_or("")
            .parse()
            .map_err(|_| FAILED)?;
        for text in std::iter::once(&receipt.installed).chain(receipt.previous.iter()) {
            let doc: toml::Value = text.parse().map_err(|_| FAILED)?;
            if doc.get("openai_base_url") != original.get("openai_base_url") {
                return Err(CONFLICT.into());
            }
        }
        return Ok(receipt.original.clone());
    };
    // Even an exact installed/previous document can contain an unowned user URL.
    let mut doc: DocumentMut = current.parse().map_err(|_| CONFLICT)?;
    let original: DocumentMut = receipt
        .original
        .as_deref()
        .unwrap_or("")
        .parse()
        .map_err(|_| FAILED)?;
    let installed: toml::Value = receipt.installed.parse().map_err(|_| FAILED)?;
    let previous: Option<toml::Value> = receipt
        .previous
        .as_ref()
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| FAILED)?;
    let current_value: toml::Value = current.parse().map_err(|_| CONFLICT)?;
    for field in FIELDS {
        let value = current_value.get(*field);
        let ours = value == installed.get(*field)
            || previous.as_ref().is_some_and(|p| value == p.get(*field))
            || (*field == "model"
                && value.and_then(|v| v.as_str()).is_some_and(|v| {
                    receipt
                        .model_aliases
                        .iter()
                        .chain(&receipt.previous_model_aliases)
                        .any(|alias| alias == v)
                        || receipt
                            .group_ids
                            .iter()
                            .chain(receipt.previous_group_ids.iter())
                            .any(|id| v == format!("vary-auto/{id}"))
                }));
        if ours {
            if let Some(value) = original.get(field) {
                doc[*field] = value.clone();
            } else {
                doc.as_table_mut().remove(field);
            }
        }
    }
    if let Some(native) = receipt.native_ownership() {
        let current_url = current_value
            .get("openai_base_url")
            .and_then(toml::Value::as_str);
        if current_url.is_some()
            && (current_url == native.installed.as_deref()
                || current_url == native.previous_owned.as_deref())
        {
            // Ownership begins at acquisition, which can be later than takeover.
            let baseline: DocumentMut = native
                .acquisition_baseline
                .as_deref()
                .ok_or(FAILED)?
                .parse()
                .map_err(|_| FAILED)?;
            if let Some(value) = baseline.get("openai_base_url") {
                doc["openai_base_url"] = value.clone();
            } else {
                doc.as_table_mut().remove("openai_base_url");
            }
        }
    }
    let current_provider = current_value
        .get("model_providers")
        .and_then(|v| v.get(PROVIDER));
    if current_provider
        == installed
            .get("model_providers")
            .and_then(|v| v.get(PROVIDER))
        || previous.as_ref().is_some_and(|v| {
            current_provider == v.get("model_providers").and_then(|v| v.get(PROVIDER))
        })
    {
        if let Some(table) = doc
            .get_mut("model_providers")
            .and_then(|v| v.as_table_like_mut())
        {
            if let Some(provider) = original
                .get("model_providers")
                .and_then(|v| v.get(PROVIDER))
            {
                table.insert(PROVIDER, provider.clone());
            } else {
                table.remove(PROVIDER);
            }
            if table.is_empty() && original.get("model_providers").is_none() {
                doc.as_table_mut().remove("model_providers");
            }
        }
    }
    let restored = doc.to_string();
    if receipt.original.is_none() && restored.is_empty() {
        Ok(None)
    } else {
        Ok(Some(restored))
    }
}
#[cfg(test)]
mod tests;
