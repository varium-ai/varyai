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
    pub provider_enabled: bool,
    pub recovery_pending: bool,
    pub group_ids: Vec<String>,
    pub omitted_entries: usize,
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
            model["context_window"] = json!(32768);
            model["max_context_window"] = json!(32768);
            model["input_modalities"] = if images.get(&group.id).copied().unwrap_or(false) {
                json!(["text", "image"])
            } else {
                json!(["text"])
            };
            model["supports_parallel_tool_calls"] = json!(false);
            super::effort::advertise(
                &mut model,
                efforts
                    .get(&group.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                chosen,
            );
            model
        })
        .collect::<Vec<_>>();
    Ok(json!({"models":models}))
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
        if receipt.version != 1 || receipt.config != self.config {
            return Err(CONFLICT.into());
        }
        Ok(Some(receipt))
    }
    pub fn status(&self) -> Result<Status, String> {
        let receipt = self.load()?;
        let live = self.runtime.menu_active();
        let owned = receipt.as_ref().is_some_and(|r| {
            read(&self.config)
                .ok()
                .flatten()
                .is_some_and(|s| projection_owned(&s, &r.installed))
        });
        Ok(Status {
            enabled: live && owned,
            provider_enabled: false,
            omitted_entries: self
                .runtime
                .menu_snapshot()?
                .map_or(0, |m| m.omitted_entries),
            recovery_pending: receipt.is_some() && !(live && owned),
            group_ids: receipt.map(|r| r.group_ids).unwrap_or_default(),
        })
    }
    pub fn active_or_pending(&self) -> bool {
        self.receipt.exists()
    }
    pub(crate) fn config_snapshot(&self) -> Result<Option<String>, String> {
        read(&self.config)
    }
    pub(crate) fn activate_menu(
        &self,
        menu: Arc<crate::proxy::intent::Menu>,
        base: &str,
    ) -> Result<(), String> {
        let mut groups = menu
            .groups
            .values()
            .map(|session| session.group.clone())
            .collect::<Vec<_>>();
        groups.sort_by(|a, b| a.id.cmp(&b.id));
        let token = menu.token.clone();
        let previous = self.runtime.set_menu(menu)?;
        if let Err(error) = self.write_projection(&groups, &token, base) {
            self.runtime.restore_menu(previous);
            return Err(error);
        }
        Ok(())
    }
    pub fn write_projection(
        &self,
        groups: &[RoutingGroup],
        token: &str,
        base: &str,
    ) -> Result<(), String> {
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
        if old.is_none()
            && doc
                .get("model_providers")
                .and_then(|v| v.get(PROVIDER))
                .is_some()
        {
            return Err(CONFLICT.into());
        }
        let efforts = self.runtime.menu_efforts(token)?;
        let images = self.runtime.menu_images(token)?;
        let chosen = doc
            .get("model_reasoning_effort")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let menu = self.runtime.menu_snapshot()?.filter(|m| m.token == token);
        let mut projection = catalog(groups, &efforts, chosen, &images)?;
        if let Some(menu) = &menu {
            if let Some(models) = projection["models"].as_array_mut() {
                for (model, group) in models.iter_mut().zip(groups) {
                    if let Some(entry) = menu.models.get(&group.id) {
                        model["slug"] = json!(entry.alias);
                        model["display_name"] = json!(entry.label);
                    }
                }
            }
        }
        let aliases = projection["models"]
            .as_array()
            .ok_or(FAILED)?
            .iter()
            .filter_map(|model| model["slug"].as_str())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let catalog_bytes = serde_json::to_vec_pretty(&projection).map_err(|_| FAILED)?;
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
        if !aliases.iter().any(|alias| alias == current_model) {
            let default = menu
                .as_ref()
                .and_then(|m| m.default_model.as_ref())
                .filter(|alias| aliases.contains(alias))
                .unwrap_or(&aliases[0]);
            doc["model"] = toml_edit::value(default.clone());
        }
        doc["model_provider"] = toml_edit::value(PROVIDER);
        doc["model_catalog_json"] = toml_edit::value(catalog_path.to_string_lossy().as_ref());
        doc["web_search"] = toml_edit::value("disabled");
        let mut provider = toml_edit::Table::new();
        provider["name"] = toml_edit::value("VaryAI models and routing groups");
        provider["base_url"] = toml_edit::value(base);
        provider["wire_api"] = toml_edit::value("responses");
        provider["requires_openai_auth"] = toml_edit::value(false);
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
            version: 1,
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
        replace_checked(&self.config, current.as_deref(), Some(&receipt.installed))
    }
    pub fn restore(&self) -> Result<(), String> {
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
        // A crash may occur between quarantining config.toml and linking its replacement.
        return Ok(receipt.original.clone());
    };
    if current == receipt.installed || receipt.previous.as_deref() == Some(current) {
        return Ok(receipt.original.clone());
    }
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
            table.remove(PROVIDER);
            if table.is_empty() && original.get("model_providers").is_none() {
                doc.as_table_mut().remove("model_providers");
            }
        }
    }
    Ok(Some(doc.to_string()))
}
#[cfg(test)]
mod tests;
