//! Account-scoped discovery caches; never project them into client configuration.

use crate::{
    database::Database, provider::Provider, proxy::providers::codex_oauth_auth::CodexOAuthManager,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Read, sync::Arc};

const CACHE_TTL: i64 = 24 * 60 * 60;
const FAILED: &str = "intentRouting.failed";

#[derive(Serialize, Deserialize)]
pub(crate) struct ModelCache {
    pub fetched_at: i64,
    pub models: Vec<String>,
    #[serde(default)]
    pub reasoning: std::collections::BTreeMap<String, super::effort::Capability>,
    #[serde(default)]
    pub names: std::collections::BTreeMap<String, String>,
}
impl ModelCache {
    fn valid_at(&self, now: i64) -> bool {
        now.checked_sub(self.fetched_at)
            .is_some_and(|age| (0..=CACHE_TTL).contains(&age))
            && self.models.len() <= 2048
            && self.reasoning.len() <= 2048
            && self.names.len() <= 2048
            && self
                .names
                .iter()
                .all(|(id, name)| self.models.contains(id) && super::valid_text(name, 80))
            && self
                .reasoning
                .iter()
                .all(|(id, c)| self.models.contains(id) && c.levels.len() <= 8)
            && self
                .models
                .iter()
                .all(|m| !m.trim().is_empty() && m.len() <= 256)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogStatus {
    pub provider_id: String,
    pub status: &'static str,
    pub model_count: usize,
}

fn cache_key(source: &str, account: &str, identity: &str) -> String {
    let mut hash = Sha256::new();
    for part in [source, account, identity] {
        hash.update(part.len().to_be_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn native_credentials(auth: &Value) -> Option<(String, String)> {
    let mode = auth.get("auth_mode").filter(|v| !v.is_null());
    if let Some(mode) = mode {
        if !matches!(mode.as_str(), Some("chatgpt" | "chatgptAuthTokens")) {
            return None;
        }
    } else if [
        "OPENAI_API_KEY",
        "personal_access_token",
        "bedrock_api_key",
        "bedrock_access_keys",
    ]
    .iter()
    .any(|k| auth.get(k).is_some_and(|v| !v.is_null()))
    {
        return None;
    }
    let field = |key| {
        auth.pointer(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    Some((field("/tokens/access_token")?, field("/tokens/account_id")?))
}

fn read_bounded(path: &std::path::Path) -> Option<String> {
    let mut value = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut value)
        .ok()?;
    (value.len() <= 1024 * 1024).then_some(value)
}

fn read_native() -> Option<(String, String)> {
    use crate::codex_config::*;
    let path = get_codex_config_path();
    let config = if path.exists() {
        read_bounded(&path)?
    } else {
        String::new()
    };
    // Keyring/auto/ephemeral may have a different active identity than auth.json.
    if codex_config_auth_store_mode(&config) != CodexAuthStoreMode::File {
        return None;
    }
    native_credentials(&serde_json::from_str::<Value>(&read_bounded(&get_codex_auth_path())?).ok()?)
}

pub(crate) fn official_source(provider: &Provider) -> bool {
    provider.category.as_deref() == Some("official")
        || provider.is_codex_oauth()
        || uses_managed_codex(provider)
}

fn uses_managed_codex(provider: &Provider) -> bool {
    provider.is_codex_oauth()
        || provider
            .meta
            .as_ref()
            .and_then(|m| m.auth_binding.as_ref())
            .is_some_and(|b| {
                b.source == crate::provider::AuthBindingSource::ManagedAccount
                    && b.auth_provider.as_deref() == Some("codex_oauth")
            })
}

struct Source {
    key: String,
    workspace: String,
    native_token: Option<String>,
    managed_id: Option<String>,
}
async fn source(provider: &Provider, manager: &CodexOAuthManager) -> Option<Source> {
    let binding = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("codex_oauth"));
    // Unbound official cards follow native Codex, never the manager's default.
    let managed_id = if binding.is_some() {
        binding
    } else if uses_managed_codex(provider) {
        manager.default_account_id().await
    } else {
        None
    };
    if let Some(id) = managed_id {
        let account = manager
            .list_accounts()
            .await
            .into_iter()
            .find(|a| a.id == id && !a.reauth_required)?;
        let workspace = manager.chatgpt_account_id_for_account(&id).await.ok()?;
        let key = cache_key(
            "managed",
            &id,
            &format!("{}:{}", account.authenticated_at, workspace),
        );
        Some(Source {
            key,
            workspace,
            native_token: None,
            managed_id: Some(id),
        })
    } else if uses_managed_codex(provider) {
        None
    } else {
        let (token, workspace) = tauri::async_runtime::spawn_blocking(read_native)
            .await
            .ok()??;
        let key = cache_key("native", &workspace, &token);
        Some(Source {
            key,
            workspace,
            native_token: Some(token),
            managed_id: None,
        })
    }
}

pub(crate) async fn task_configured(provider: &Provider, manager: &CodexOAuthManager) -> bool {
    super::eligible_provider(provider, false)
        || (official_source(provider) && source(provider, manager).await.is_some())
}

pub(crate) async fn prepare_providers(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    refresh: bool,
) -> Result<(Vec<Provider>, Vec<CatalogStatus>), String> {
    use futures::{stream, StreamExt};
    let read_db = db.clone();
    let providers =
        tauri::async_runtime::spawn_blocking(move || read_db.get_all_providers("codex"))
            .await
            .map_err(|_| FAILED)?
            .map_err(|_| FAILED)?;
    let results = stream::iter(providers.into_values())
        .map(|provider| prepare_provider(db.clone(), manager.clone(), provider, refresh))
        .buffered(4)
        .collect::<Vec<_>>()
        .await;
    let mut providers = Vec::new();
    let mut statuses = Vec::new();
    for result in results {
        let (provider, status) = result?;
        providers.push(provider);
        if let Some(status) = status {
            statuses.push(status);
        }
    }
    Ok((providers, statuses))
}

async fn prepare_provider(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    mut provider: Provider,
    refresh: bool,
) -> Result<(Provider, Option<CatalogStatus>), String> {
    if !official_source(&provider) {
        return Ok((provider, None));
    }
    let Some(identity) = source(&provider, &manager).await else {
        // Explicit user catalogs remain independent of the discovery cache.
        let status = CatalogStatus {
            provider_id: provider.id.clone(),
            status: "unavailable",
            model_count: 0,
        };
        return Ok((provider, Some(status)));
    };
    let cache_db = db.clone();
    let key = identity.key.clone();
    let mut cache =
        tauri::async_runtime::spawn_blocking(move || cache_db.routing_model_cache(&key))
            .await
            .map_err(|_| FAILED)??
            .filter(|c| c.valid_at(chrono::Utc::now().timestamp()));
    let mut status = if cache.is_some() {
        "cached"
    } else {
        "unavailable"
    };
    if refresh {
        let fetch = async {
            let token = if let Some(id) = &identity.managed_id {
                manager.get_valid_token_for_account(id).await.ok()?
            } else {
                identity.native_token.clone()?
            };
            crate::services::codex_oauth_models::fetch_catalog_with_token(
                &token,
                &identity.workspace,
            )
            .await
            .ok()
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(30), fetch)
            .await
            .ok()
            .flatten();
        let result = result.map(|value| {
            let models = crate::services::codex_oauth_models::parse_models(value.clone());
            (
                models,
                discovery_capabilities(&value),
                discovery_names(&value),
            )
        });
        if let Some((models, reasoning, names)) = result.filter(|(m, _, _)| {
            !m.is_empty() && m.len() <= 2048 && m.iter().all(|m| m.id.len() <= 256)
        }) {
            let next = ModelCache {
                fetched_at: chrono::Utc::now().timestamp(),
                reasoning,
                names,
                models: models.into_iter().map(|m| m.id).collect(),
            };
            // Persist only to the resolved identity, never to the mutable provider configuration.
            let write_db = db.clone();
            let key = identity.key.clone();
            cache = Some(
                tauri::async_runtime::spawn_blocking(move || {
                    write_db.save_routing_model_cache(&key, &next)?;
                    Ok::<_, String>(next)
                })
                .await
                .map_err(|_| FAILED)??,
            );
            status = "updated";
        }
    }
    // Check on both success and failure, including a native login rotated during the request.
    if !source(&provider, &manager)
        .await
        .is_some_and(|s| s.key == identity.key)
    {
        cache = None;
        status = "unavailable";
    }
    let model_count = cache.as_ref().map_or(0, |c| c.models.len());
    if let Some(cache) = cache {
        apply_cache(&mut provider, cache);
    }
    let status = CatalogStatus {
        provider_id: provider.id.clone(),
        status,
        model_count,
    };
    Ok((provider, Some(status)))
}

fn apply_cache(provider: &mut Provider, cache: ModelCache) {
    if !provider.settings_config.is_object() {
        return;
    }
    provider.settings_config["modelCatalog"] = json!({"models": cache.models.into_iter().map(|model| {
        let mut entry = json!({"model":model});
        if let Some(name) = cache.names.get(&model) {
            entry["displayName"] = json!(name);
        }
        if let Some(capability) = cache.reasoning.get(&model) {
            entry["reasoningLevels"] = json!(capability.levels);
            if let Some(images) = capability.images { entry["input_modalities"] = if images { json!(["text", "image"]) } else { json!(["text"]) }; }
            if capability.quiet { entry["reasoning"] = json!(false); }
            if let Some(free) = capability.free { entry["free"] = json!(free); }
        }
        entry
    }).collect::<Vec<_>>()});
}

fn discovery_capabilities(
    value: &Value,
) -> std::collections::BTreeMap<String, super::effort::Capability> {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .or_else(|| value.as_array());
    let mut result = std::collections::BTreeMap::new();
    let mut add = |entry: &Value, fallback: Option<&str>| {
        let id = ["slug", "id", "model", "name"]
            .iter()
            .filter_map(|key| entry.get(*key).and_then(Value::as_str))
            .map(str::trim)
            .find(|s| !s.is_empty())
            .or(fallback);
        if let Some(id) = id.filter(|id| !id.is_empty() && id.len() <= 256) {
            let capability = super::effort::Capability::parse(entry);
            if capability != Default::default() {
                result.insert(id.to_owned(), capability);
            }
        }
    };
    if let Some(entries) = entries {
        for entry in entries {
            add(entry, None);
        }
    }
    if let Some(map) = value.get("models").and_then(Value::as_object) {
        for (id, entry) in map {
            add(entry, Some(id.trim()));
        }
    }
    result
}

fn discovery_names(value: &Value) -> std::collections::BTreeMap<String, String> {
    let mut names = std::collections::BTreeMap::new();
    let mut add = |entry: &Value, fallback: Option<&str>| {
        let id = ["slug", "id", "model", "name"]
            .iter()
            .find_map(|k| entry.get(*k).and_then(Value::as_str))
            .or(fallback)
            .map(str::trim);
        let name = ["display_name", "displayName", "name"]
            .iter()
            .find_map(|k| {
                entry
                    .get(*k)
                    .and_then(Value::as_str)
                    .filter(|s| super::valid_text(s, 80))
            });
        if let (Some(id), Some(name)) = (id.filter(|id| !id.is_empty() && id.len() <= 256), name) {
            if id != name {
                names.insert(id.into(), name.into());
            }
        }
    };
    if let Some(entries) = value.as_array().or_else(|| {
        ["models", "data", "items"]
            .iter()
            .find_map(|k| value[*k].as_array())
    }) {
        for entry in entries {
            add(entry, None);
        }
    }
    if let Some(map) = value.get("models").and_then(Value::as_object) {
        for (id, entry) in map {
            add(entry, Some(id));
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn task_status_distinguishes_supported_api_config_from_missing_managed_identity() {
        let dir = tempfile::tempdir().expect("dir");
        let manager = CodexOAuthManager::new(dir.path().to_owned());
        let mut api = Provider::with_id(
            "api".into(),
            "API".into(),
            json!({
                "auth":{"OPENAI_API_KEY":"synthetic"},
                "apiFormat":"openai_responses",
                "config":"model_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.example/v1'\nwire_api='responses'"
            }),
            None,
        );
        assert!(task_configured(&api, &manager).await);
        api.settings_config["apiFormat"] = json!("openai_chat");
        assert!(!task_configured(&api, &manager).await);
        api.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".into()),
            ..Default::default()
        });
        assert!(!task_configured(&api, &manager).await);
    }

    #[test]
    fn friendly_names_survive_account_cache_without_retaining_other_metadata() {
        let value = json!({"models":[{"slug":"synthetic-model","display_name":"Synthetic Model","private_extension":"discard"}]});
        let names = discovery_names(&value);
        assert_eq!(names["synthetic-model"], "Synthetic Model");
        let cache = ModelCache {
            fetched_at: 100,
            models: vec!["synthetic-model".into()],
            reasoning: Default::default(),
            names,
        };
        let mut provider = Provider::with_id("fixture".into(), "Fixture".into(), json!({}), None);
        apply_cache(&mut provider, cache);
        assert_eq!(
            provider.settings_config["modelCatalog"]["models"][0]["displayName"],
            "Synthetic Model"
        );
        assert!(!provider
            .settings_config
            .to_string()
            .contains("private_extension"));
    }

    #[test]
    fn discovery_retains_only_normalized_reasoning_capabilities() {
        for value in [
            json!({"models":[{"slug":"synthetic-model", "supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"}], "input_modalities":["text","image"], "private_extension":"not-persisted"}]}),
            json!({"models":{"synthetic-model":{"reasoningLevels":["xhigh","low","medium","high","unknown"],"inputModalities":["text","image"]}}}),
        ] {
            let reasoning = discovery_capabilities(&value);
            assert_eq!(
                reasoning["synthetic-model"].levels,
                ["low", "medium", "high", "xhigh"]
            );
            assert_eq!(reasoning["synthetic-model"].images, Some(true));
            let cache = ModelCache {
                fetched_at: 100,
                models: vec!["synthetic-model".into()],
                reasoning,
                names: Default::default(),
            };
            let serialized = serde_json::to_string(&cache).unwrap();
            assert!(!serialized.contains("private_extension"));
            let restored: ModelCache = serde_json::from_str(&serialized).unwrap();
            let mut provider =
                Provider::with_id("fixture".into(), "Fixture".into(), json!({}), None);
            apply_cache(&mut provider, restored);
            let entry = &provider.settings_config["modelCatalog"]["models"][0];
            assert_eq!(
                super::super::effort::Capability::parse(entry).images,
                Some(true)
            );
            assert_eq!(
                super::super::effort::Capability::parse(entry).levels.len(),
                4
            );
        }
        let old: ModelCache =
            serde_json::from_value(json!({"fetched_at":100,"models":["synthetic-model"]})).unwrap();
        assert!(old.reasoning.is_empty());
        assert!(old.valid_at(101));
    }

    #[test]
    fn discovered_models_create_deletable_groups_without_mutating_provider_storage() {
        let db = Database::memory().unwrap();
        let mut official = Provider::with_id("official".into(), "Official".into(), json!({}), None);
        official.category = Some("official".into());
        let api = Provider::with_id(
            "api".into(),
            "API".into(),
            json!({"modelCatalog":{"models":[{"model":"shared-model"}]}}),
            None,
        );
        db.save_provider("codex", &official).unwrap();
        db.save_provider("codex", &api).unwrap();
        db.set_intent_routing_auto_create(true).unwrap();
        apply_cache(
            &mut official,
            ModelCache {
                fetched_at: 100,
                models: vec!["shared-model".into()],
                reasoning: Default::default(),
                names: Default::default(),
            },
        );
        let providers = vec![official, api];
        let catalog = db
            .intent_routing_catalog_with_providers(Some(providers.clone()))
            .unwrap();
        assert_eq!(catalog.groups.len(), 1);
        assert!(db.get_all_providers("codex").unwrap()["official"]
            .settings_config
            .get("modelCatalog")
            .is_none());
        db.delete_intent_routing_group_with_providers(
            &catalog.groups[0].id,
            0,
            Some(providers.clone()),
        )
        .unwrap();
        assert!(db
            .intent_routing_catalog_with_providers(Some(providers))
            .unwrap()
            .groups
            .is_empty());
    }

    #[test]
    fn refreshed_official_catalog_adds_gpt61_groups_without_inventing_account_support() {
        let db = Database::memory().expect("database");
        db.set_intent_routing_auto_create(true)
            .expect("enable discovery");
        let api = Provider::with_id(
            "api".into(),
            "Synthetic relay".into(),
            json!({"modelCatalog":{"models":[{"model":"gpt-6.1-sol"},{"model":"gpt-6.1-terra"}]}}),
            None,
        );
        let mut official = Provider::with_id(
            "official".into(),
            "Synthetic official".into(),
            json!({}),
            None,
        );
        apply_cache(
            &mut official,
            ModelCache {
                fetched_at: 100,
                models: vec!["gpt-6-sol".into()],
                reasoning: Default::default(),
                names: Default::default(),
            },
        );
        assert!(db
            .intent_routing_catalog_with_providers(Some(vec![api.clone(), official.clone()]))
            .expect("old catalog")
            .groups
            .is_empty());
        apply_cache(
            &mut official,
            ModelCache {
                fetched_at: 101,
                models: vec!["gpt-6.1-sol".into(), "gpt-6.1-terra".into()],
                reasoning: Default::default(),
                names: Default::default(),
            },
        );
        let catalog = db
            .intent_routing_catalog_with_providers(Some(vec![api, official]))
            .expect("refreshed catalog");
        let aliases = catalog.groups.iter().map(|g| g.alias()).collect::<Vec<_>>();
        assert_eq!(aliases, ["auto/gpt-6.1-sol", "auto/gpt-6.1-terra"]);
        assert!(catalog.groups.iter().all(|g| g.members.len() == 2));
    }

    #[tokio::test]
    async fn missing_managed_binding_never_falls_back_to_native_login() {
        let dir = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(dir.path().to_owned());
        let mut provider = Provider::with_id("official".into(), "Official".into(), json!({}), None);
        provider.category = Some("official".into());
        provider.meta = Some(serde_json::from_value(json!({"authBinding":{"source":"managed_account", "authProvider":"codex_oauth", "accountId":"missing"}})).unwrap());
        assert!(source(&provider, &manager).await.is_none());
        provider
            .meta
            .as_mut()
            .unwrap()
            .auth_binding
            .as_mut()
            .unwrap()
            .account_id = None;
        assert!(uses_managed_codex(&provider));
        assert!(source(&provider, &manager).await.is_none());
    }

    #[test]
    fn native_credentials_require_chatgpt_mode_and_complete_tokens() {
        let auth = json!({"tokens":{"access_token":"synthetic", "account_id":"workspace"}});
        assert!(native_credentials(&auth).is_some());
        let mut api_key = auth.clone();
        api_key["OPENAI_API_KEY"] = json!("synthetic-key");
        assert!(native_credentials(&api_key).is_none());
        api_key["auth_mode"] = json!("chatgpt");
        assert!(native_credentials(&api_key).is_some());
        assert!(native_credentials(&json!({"tokens":{"access_token":"synthetic"}})).is_none());
        assert!(
            native_credentials(&json!({"auth_mode":"headers", "tokens":auth["tokens"]})).is_none()
        );
    }

    #[test]
    fn cache_is_scoped_and_expires_without_accepting_future_timestamps() {
        assert_ne!(
            cache_key("native", "a", "token"),
            cache_key("native", "b", "token")
        );
        assert_ne!(
            cache_key("native", "a", "token"),
            cache_key("managed", "a", "token")
        );
        let cache = ModelCache {
            fetched_at: 100,
            models: vec!["synthetic-model".into()],
            reasoning: Default::default(),
            names: Default::default(),
        };
        assert!(cache.valid_at(101));
        assert!(!cache.valid_at(99));
        assert!(!cache.valid_at(100 + CACHE_TTL + 1));
        let db = crate::database::Database::memory().unwrap();
        db.save_routing_model_cache("fixture", &cache).unwrap();
        assert_eq!(
            db.routing_model_cache("fixture").unwrap().unwrap().models,
            cache.models
        );
        assert!(db
            .routing_model_cache("different-account")
            .unwrap()
            .is_none());
        for index in 0..80 {
            db.save_routing_model_cache(&format!("fixture-{index}"), &cache)
                .unwrap();
        }
        let conn = db.extension_connection().lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM intent_routing_preferences WHERE key LIKE 'model_cache:%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 64);
        Database::normalize_imported_intent_routing(&conn).unwrap();
        drop(conn);
        assert!(db.routing_model_cache("fixture-79").unwrap().is_none());
    }
}

/// Freeze the selected account for an explicitly authorized, short-lived session.
/// No later request can silently follow a changed default or native login.
pub(crate) async fn execution_credentials(
    provider: &Provider,
    manager: &CodexOAuthManager,
) -> Option<(String, String)> {
    let identity = source(provider, manager).await?;
    let token = if let Some(id) = identity.managed_id {
        let token = manager.get_valid_token_for_account(&id).await.ok()?;
        if manager.chatgpt_account_id_for_account(&id).await.ok()? != identity.workspace {
            return None;
        }
        token
    } else {
        identity.native_token?
    };
    Some((token, identity.workspace))
}

#[derive(Clone)]
pub(crate) struct ExecutionCredentials {
    manager: Arc<CodexOAuthManager>,
    provider: Provider,
    key: String,
    pub workspace: String,
}
impl ExecutionCredentials {
    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        self.key == other.key && self.workspace == other.workspace
    }
    pub async fn token(&self) -> Option<String> {
        let identity = source(&self.provider, &self.manager).await?;
        if identity.key != self.key || identity.workspace != self.workspace {
            return None;
        }
        let token = execution_credentials(&self.provider, &self.manager)
            .await?
            .0;
        let after = source(&self.provider, &self.manager).await?;
        (after.key == self.key && after.workspace == self.workspace).then_some(token)
    }
}
pub(crate) async fn freeze_execution(
    provider: &Provider,
    manager: Arc<CodexOAuthManager>,
) -> Option<ExecutionCredentials> {
    let identity = source(provider, &manager).await?;
    let mut provider = provider.clone();
    if let Some(id) = identity.managed_id {
        provider
            .meta
            .get_or_insert_with(Default::default)
            .auth_binding = Some(crate::provider::AuthBinding {
            source: crate::provider::AuthBindingSource::ManagedAccount,
            auth_provider: Some("codex_oauth".into()),
            account_id: Some(id),
        });
    }
    Some(ExecutionCredentials {
        manager,
        provider,
        key: identity.key,
        workspace: identity.workspace,
    })
}
