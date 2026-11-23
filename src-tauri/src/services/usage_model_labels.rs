//! Safe analytics labels for opaque direct-model aliases. No credentials or live discovery.
use crate::{database::Database, provider::Provider};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const CACHE_KEY: &str = "usage_model_labels_v1";
const FAILED: &str = "usage.modelLabelsUnavailable";
const MAX_HISTORY: usize = 4096;
const MAX_HISTORY_BYTES: usize = 2 * 1024 * 1024;
const MAX_REQUEST_IDS: usize = 512;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UsageModelLabel {
    pub id: String,
    pub model: String,
    pub display_name: String,
    pub provider_name: String,
}

/// Normalize only the exact generated namespace; custom model names stay exact.
pub(crate) fn canonical_id(value: &str) -> Option<&str> {
    let bare = value.strip_prefix("vary-provider/").unwrap_or(value);
    let digest = bare.strip_prefix("model-")?;
    (digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    .then_some(bare)
}

/// Normalize a routing group UUID alias (bare or prefixed with `vary-auto/` / `vary-provider/`).
fn routing_group_id(value: &str) -> Option<String> {
    let bare = value
        .strip_prefix("vary-auto/")
        .or_else(|| value.strip_prefix("vary-provider/"))
        .unwrap_or(value);
    let parsed = uuid::Uuid::parse_str(bare).ok()?;
    Some(parsed.hyphenated().to_string())
}

fn valid_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && !text.chars().any(char::is_control)
}

fn valid_label(label: &UsageModelLabel) -> bool {
    canonical_id(&label.id) == Some(label.id.as_str())
        && valid_text(&label.model, 200)
        && valid_text(&label.display_name, 200)
        && valid_text(&label.provider_name, 120)
}

/// Project a prepared or local provider catalog without serializing its settings.
pub(crate) fn labels_for_providers(providers: &[Provider]) -> Vec<UsageModelLabel> {
    let mut labels = BTreeMap::new();
    for provider in providers {
        let entries = provider.settings_config["modelCatalog"]["models"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut models = entries
            .iter()
            .filter_map(|entry| {
                let model = entry["model"].as_str()?;
                let display =
                    super::intent_routing::model_preferences::name(entry).unwrap_or(model);
                Some((model.to_owned(), display.to_owned()))
            })
            .collect::<BTreeMap<_, _>>();
        if let Some(model) = provider.settings_config["config"]
            .as_str()
            .and_then(|text| text.parse::<toml::Value>().ok())
            .and_then(|config| config.get("model")?.as_str().map(str::to_owned))
        {
            models.entry(model.clone()).or_insert(model);
        }
        for (model, display_name) in models {
            let label = UsageModelLabel {
                id: super::intent_routing::menu::direct_id(
                    super::routing_upstreams::execution_identity(provider),
                    &model,
                ),
                model,
                display_name,
                provider_name: provider.name.clone(),
            };
            if valid_label(&label) {
                labels.insert(label.id.clone(), label);
            }
        }
    }
    labels.into_values().collect()
}

fn read_history(conn: &Connection) -> Result<Vec<UsageModelLabel>, String> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM intent_routing_preferences WHERE key=?1",
            [CACHE_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| FAILED)?;
    let Some(text) = text else {
        return Ok(Vec::new());
    };
    if text.len() > MAX_HISTORY_BYTES {
        return Err(FAILED.into());
    }
    let labels: Vec<UsageModelLabel> = serde_json::from_str(&text).map_err(|_| FAILED)?;
    if labels.len() > MAX_HISTORY || labels.iter().any(|label| !valid_label(label)) {
        return Err(FAILED.into());
    }
    Ok(labels)
}

impl Database {
    /// Called only during control-plane catalog preparation, on a blocking worker.
    /// Failure is auxiliary and must never prevent routing publication.
    pub(crate) fn remember_usage_model_labels(
        &self,
        labels: &[UsageModelLabel],
    ) -> Result<(), String> {
        if labels.iter().any(|label| !valid_label(label)) {
            return Err(FAILED.into());
        }
        let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
        let mut history = read_history(&conn)?;
        if labels.is_empty() {
            return Ok(());
        }
        let incoming = labels
            .iter()
            .map(|label| &label.id)
            .collect::<BTreeSet<_>>();
        history.retain(|label| !incoming.contains(&label.id));
        history.extend_from_slice(labels);
        if history.len() > MAX_HISTORY {
            history.drain(..history.len() - MAX_HISTORY);
        }
        let encoded = loop {
            let encoded = serde_json::to_string(&history).map_err(|_| FAILED)?;
            if encoded.len() <= MAX_HISTORY_BYTES {
                break encoded;
            }
            history.drain(..(history.len() / 4).max(1));
        };
        conn.execute(
            "INSERT INTO intent_routing_preferences(key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE value<>excluded.value",
            [CACHE_KEY, &encoded],
        )
        .map_err(|_| FAILED)?;
        Ok(())
    }

    /// Resolve labels from local data only, independently of date/provider filters.
    pub(crate) fn get_usage_model_labels(
        &self,
        ids: &[String],
    ) -> Result<Vec<UsageModelLabel>, String> {
        if ids.len() > MAX_REQUEST_IDS {
            return Err(FAILED.into());
        }
        let requested = ids
            .iter()
            .filter_map(|id| canonical_id(id))
            .collect::<BTreeSet<_>>();
        let requested_groups = ids
            .iter()
            .filter_map(|id| routing_group_id(id))
            .collect::<BTreeSet<_>>();
        if requested.is_empty() && requested_groups.is_empty() {
            return Ok(Vec::new());
        }
        let mut resolved = Vec::new();
        if !requested.is_empty() {
            let history = {
                let conn = self.extension_connection().lock().map_err(|_| FAILED)?;
                read_history(&conn).unwrap_or_default()
            };
            let mut labels = history
                .into_iter()
                .map(|label| (label.id.clone(), label))
                .collect::<BTreeMap<_, _>>();
            // Local names take precedence over retained names after a provider rename.
            // Disabled/deleted models can still occur in historical usage records.
            if let Ok(providers) = self.routing_catalog_providers(true) {
                for label in labels_for_providers(&providers) {
                    labels.insert(label.id.clone(), label);
                }
            }
            resolved.extend(
                labels
                    .into_values()
                    .filter(|label| requested.contains(label.id.as_str())),
            );
        }
        if !requested_groups.is_empty() {
            if let Ok(groups) = self.list_intent_routing_groups() {
                for group in groups {
                    let group_id = group.id.to_ascii_lowercase();
                    if requested_groups.contains(group_id.as_str()) && valid_text(&group.name, 200)
                    {
                        resolved.push(UsageModelLabel {
                            id: group_id,
                            model: group.name.clone(),
                            display_name: group.name,
                            provider_name: String::new(),
                        });
                    }
                }
            }
        }
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests;
