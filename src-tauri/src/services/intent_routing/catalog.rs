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
    fn from_catalog(value: Value, fetched_at: i64) -> Option<Self> {
        use crate::services::codex_oauth_models::{parse_models, picker_visible};
        let entries = ["data", "models", "items"]
            .iter()
            .find_map(|key| value.get(*key).and_then(Value::as_array))
            .or_else(|| value.as_array());
        let model_map = value.get("models").and_then(Value::as_object);
        if entries.is_none() && model_map.is_none() {
            return None;
        }
        let models = parse_models(value.clone());
        // Empty lists and all-hidden catalogs are authoritative; malformed visible entries are not.
        if models.is_empty()
            && (entries.is_some_and(|entries| entries.iter().any(picker_visible))
                || model_map.is_some_and(|entries| entries.values().any(picker_visible)))
        {
            return None;
        }
        let cache = Self {
            fetched_at,
            models: models.into_iter().map(|model| model.id).collect(),
            reasoning: discovery_capabilities(&value),
            names: discovery_names(&value),
        };
        cache.valid_at(fetched_at).then_some(cache)
    }

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
                .all(|(id, c)| self.models.contains(id) && c.levels.len() <= 8 && c.context.valid())
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

fn cache_key(source: &str, account: &str, identity: &str) -> String {
    let mut hash = Sha256::new();
    // Older caches lack picker visibility and cannot be safely filtered in place.
    for part in ["picker-visible-v1", source, account, identity] {
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

fn read_native_auth() -> Option<Value> {
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
    let auth = serde_json::from_str::<Value>(&read_bounded(&get_codex_auth_path())?).ok()?;
    native_credentials(&auth)?;
    Some(auth)
}

fn read_native() -> Option<(String, String)> {
    native_credentials(&read_native_auth()?)
}

pub(crate) struct NativeEditorInfo {
    pub login: String,
    pub plan: Option<String>,
    pub identity: Option<(String, String)>,
}

pub(crate) fn native_editor_info() -> Option<NativeEditorInfo> {
    native_editor_observation(&read_native_auth()?)
}

fn native_editor_observation(auth: &Value) -> Option<NativeEditorInfo> {
    use base64::Engine as _;
    let (_, workspace) = native_credentials(auth)?;
    let unknown = || NativeEditorInfo {
        login: "Codex".into(),
        plan: None,
        identity: None,
    };
    let Some(token) = auth.pointer("/tokens/id_token").and_then(Value::as_str) else {
        return Some(unknown());
    };
    let claims = token
        .split('.')
        .nth(1)
        .and_then(|payload| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(payload)
                .ok()
        })
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let Some(claims) = claims else {
        return Some(unknown());
    };
    let login = claims["email"]
        .as_str()
        .filter(|s| s.len() <= 256 && !s.chars().any(char::is_control))
        .unwrap_or("Codex")
        .to_owned();
    let plan = match claims
        .pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type")
        .and_then(Value::as_str)
    {
        Some("plus") => Some("ChatGPT Plus".into()),
        Some("pro") => Some("ChatGPT Pro".into()),
        Some("free") => Some("ChatGPT Free".into()),
        Some("team" | "business") => Some("ChatGPT Business".into()),
        Some("enterprise") => Some("ChatGPT Enterprise".into()),
        _ => None,
    };
    // Equal display names or emails do not prove an equal subscription seat.
    let identity = crate::codex_config::extract_codex_id_token_user_identity(token)
        .filter(|_| {
            claims
                .get("chatgpt_account_id")
                .and_then(Value::as_str)
                .or_else(|| {
                    claims
                        .pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
                        .and_then(Value::as_str)
                })
                == Some(workspace.as_str())
        })
        .map(|user| (user, workspace));
    Some(NativeEditorInfo {
        login,
        plan,
        identity,
    })
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
    let providers = tauri::async_runtime::spawn_blocking(move || {
        read_db.routing_providers().map(|providers| {
            providers
                .into_iter()
                .map(|p| (p.id.clone(), p))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
    })
    .await
    .map_err(|_| FAILED)?
    .map_err(|_| FAILED)?;
    let results = stream::iter(providers.into_values())
        .map(|provider| prepare_editor_pool(db.clone(), manager.clone(), provider, refresh))
        .buffered(4)
        .collect::<Vec<_>>()
        .await;
    let mut providers = Vec::new();
    let mut statuses = Vec::new();
    for result in results {
        let (mut provider, status) = result?;
        crate::services::codex_provider::filter_models(&mut provider);
        crate::services::routing_upstreams::filter_models(&mut provider);
        providers.push(provider);
        if let Some(status) = status {
            statuses.push(status);
        }
    }
    super::model_context::prepare(&mut providers, refresh).await;
    Ok((providers, statuses))
}

/// Discover missing menus only when the routing UI asks; never on forwarding.
pub(crate) async fn prepare_missing_catalogs(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
) -> Result<Vec<CatalogStatus>, String> {
    use futures::{stream, StreamExt};
    let (providers, _) = prepare_providers(db.clone(), manager.clone(), false).await?;
    let missing = providers
        .into_iter()
        .filter(|provider| {
            if provider
                .settings_config
                .get(crate::services::routing_upstreams::MODEL_POLICY)
                .is_some_and(|policy| matches!(policy["mode"].as_str(), Some("picked" | "usage")))
            {
                return false;
            }
            if provider
                .settings_config
                .get(crate::services::codex_provider::POLICY)
                .is_some()
            {
                let policy = crate::services::codex_provider::Settings::from_provider(provider);
                if policy.mode == crate::services::codex_provider::ModelMode::Usage
                    || policy.accounts.is_empty()
                {
                    return false;
                }
            }
            provider
                .settings_config
                .pointer("/modelCatalog/models")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty)
        })
        .map(|provider| provider.id)
        .collect::<std::collections::HashSet<_>>();
    let read_db = db.clone();
    // Cache keys must use the stored connection, not hydrated publication metadata.
    let missing = tokio::task::spawn_blocking(move || read_db.routing_providers())
        .await
        .map_err(|_| FAILED)??
        .into_iter()
        .filter(|provider| missing.contains(&provider.id));
    let results = stream::iter(missing)
        .map(|provider| prepare_editor_pool(db.clone(), manager.clone(), provider, true))
        .buffered(4)
        .collect::<Vec<_>>()
        .await;
    results
        .into_iter()
        .map(|result| result.map(|(_, status)| status))
        .collect::<Result<Vec<_>, _>>()
        .map(|statuses| statuses.into_iter().flatten().collect())
}

async fn prepare_editor_pool(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    mut provider: Provider,
    refresh: bool,
) -> Result<(Provider, Option<CatalogStatus>), String> {
    use crate::services::codex_provider::{self, CATALOGS, POLICY};
    if !official_source(&provider) || provider.settings_config.get(POLICY).is_none() {
        return prepare_provider(db, manager, provider, refresh).await;
    }
    let mut catalogs = serde_json::Map::new();
    let mut models = std::collections::BTreeMap::new();
    let mut latest = None;
    for mut variant in codex_provider::variants(&provider) {
        // Never hydrate another identity from an already prepared account catalog.
        variant
            .settings_config
            .as_object_mut()
            .map(|o| o.remove(CATALOGS));
        let account = variant
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("codex_oauth"))
            .unwrap_or_else(|| "native".into());
        let (prepared, status) =
            prepare_provider(db.clone(), manager.clone(), variant, refresh).await?;
        let rows = prepared
            .settings_config
            .pointer("/modelCatalog/models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if status.as_ref().is_some_and(|s| s.status != "unavailable") {
            catalogs.insert(account, json!(rows));
        }
        for row in rows {
            if let Some(id) = row["model"].as_str() {
                models.entry(id.to_owned()).or_insert(row);
            }
        }
        latest = status;
    }
    if !catalogs.is_empty() {
        provider.settings_config[CATALOGS] = Value::Object(catalogs);
    }
    provider.settings_config["modelCatalog"] =
        json!({"models": models.into_values().collect::<Vec<_>>()});
    Ok((provider, latest))
}

pub(crate) async fn prepare_provider(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    mut provider: Provider,
    refresh: bool,
) -> Result<(Provider, Option<CatalogStatus>), String> {
    if crate::services::subscription_provider::binding(&provider).is_some() {
        let count = provider.settings_config["modelCatalog"]["models"]
            .as_array()
            .map_or(0, Vec::len);
        let status = CatalogStatus {
            provider_id: provider.id.clone(),
            status: "cached",
            model_count: count,
            reason: None,
        };
        return Ok((provider, Some(status)));
    }
    if !official_source(&provider) {
        let status = prepare_static_context(db, &mut provider, refresh).await;
        return Ok((provider, Some(status)));
    }
    let Some(identity) = source(&provider, &manager).await else {
        // Explicit user catalogs remain independent of the discovery cache.
        let status = CatalogStatus {
            provider_id: provider.id.clone(),
            status: "unavailable",
            model_count: 0,
            reason: Some("notSignedIn"),
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
        if let Some(next) =
            result.and_then(|value| ModelCache::from_catalog(value, chrono::Utc::now().timestamp()))
        {
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
        reason: if status == "unavailable" {
            Some("requestFailed")
        } else if model_count == 0 {
            Some("empty")
        } else {
            None
        },
    };
    Ok((provider, Some(status)))
}

async fn prepare_static_context(
    db: Arc<Database>,
    provider: &mut Provider,
    refresh: bool,
) -> CatalogStatus {
    use crate::proxy::providers::{ClaudeAdapter, ProviderAdapter};
    let mut status = CatalogStatus {
        provider_id: provider.id.clone(),
        status: "unavailable",
        model_count: 0,
        reason: Some("requestFailed"),
    };
    if !super::eligible_provider(provider, false) {
        return status;
    }
    let Ok(transport_provider) = crate::services::routing_upstreams::first_enabled(provider) else {
        return status;
    };
    let adapter = ClaudeAdapter::new();
    let (legacy_base, token) =
        transport_provider.resolve_usage_credentials(&crate::app_config::AppType::Codex);
    let base = adapter
        .extract_base_url(&transport_provider)
        .unwrap_or(legacy_base);
    let Ok(identity) = serde_json::to_string(&provider.settings_config) else {
        return status;
    };
    let key = cache_key("static-capabilities-v2", &provider.id, &identity);
    let cache_db = db.clone();
    let read_key = key.clone();
    let now = chrono::Utc::now().timestamp();
    let mut cache = tokio::task::spawn_blocking(move || cache_db.routing_model_cache(&read_key))
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
        .filter(|c| c.valid_at(now));
    if let Some(cache) = &cache {
        status.status = "cached";
        status.model_count = cache.models.len();
        status.reason = None;
    }
    if refresh {
        let format = transport_provider
            .meta
            .as_ref()
            .and_then(|m| m.api_format.as_deref())
            .or_else(|| transport_provider.settings_config["apiFormat"].as_str())
            .or_else(|| transport_provider.settings_config["api_format"].as_str());
        let format = if format == Some("anthropic")
            && (provider.settings_config["authType"] == "api-key"
                || provider.claude_uses_api_key_field())
        {
            Some("anthropic-messages")
        } else {
            format
        };
        let models_url = transport_provider.settings_config["modelsUrl"].as_str();
        let user_agent = transport_provider
            .meta
            .as_ref()
            .and_then(|m| m.custom_user_agent_header().ok().flatten());
        let request = crate::services::model_fetch::fetch_models(
            &base,
            &token,
            transport_provider
                .meta
                .as_ref()
                .and_then(|m| m.is_full_url)
                .unwrap_or(false),
            models_url,
            user_agent,
            format,
            None,
        );
        if let Ok(Ok(models)) =
            tokio::time::timeout(std::time::Duration::from_secs(20), request).await
        {
            let next = ModelCache {
                fetched_at: now,
                models: models.iter().map(|m| m.id.clone()).collect(),
                reasoning: models
                    .into_iter()
                    .map(|m| {
                        let capability = super::effort::Capability::parse(&m.capability_entry());
                        (
                            m.id,
                            super::effort::Capability {
                                context: m.context,
                                ..capability
                            },
                        )
                    })
                    .collect(),
                names: Default::default(),
            };
            if next.valid_at(now) {
                let cache_db = db;
                if let Ok(next) = tokio::task::spawn_blocking(move || {
                    let _ = cache_db.save_routing_model_cache(&key, &next);
                    next
                })
                .await
                {
                    status.status = "updated";
                    status.model_count = next.models.len();
                    status.reason = next.models.is_empty().then_some("empty");
                    cache = Some(next);
                }
            }
        }
    }
    if let Some(cache) = cache {
        provider.settings_config["routingModelCapabilities"] = json!(cache
            .reasoning
            .iter()
            .map(|(id, capability)| (id.clone(), capability.declaration()))
            .collect::<serde_json::Map<_, _>>());
        if provider
            .settings_config
            .get(crate::services::routing_upstreams::MODEL_POLICY)
            .is_some()
        {
            provider.settings_config["routingVendorCatalog"] = json!(cache
                .models
                .iter()
                .map(|id| static_entry(provider, &cache, id))
                .collect::<Vec<_>>());
        }
        // Empty API selections follow the vendor list. Saved nonempty selections
        // remain authoritative; discovery never edits provider storage.
        if provider
            .settings_config
            .pointer("/modelCatalog/models")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            let models = cache
                .models
                .iter()
                .filter(|id| super::valid_target(&provider.id, id))
                .map(|id| static_entry(provider, &cache, id))
                .collect::<Vec<_>>();
            provider.settings_config["modelCatalog"] = json!({"models":models});
        }
        provider.settings_config["routingContext"] = json!({"vendor": cache.reasoning.into_iter()
            .map(|(id, c)| (id, json!(c.context))).collect::<serde_json::Map<_,_>>()});
    }
    status
}

fn static_entry(provider: &Provider, cache: &ModelCache, id: &str) -> Value {
    let mut entry = cache
        .reasoning
        .get(id)
        .map(|capability| capability.declaration())
        .unwrap_or_else(|| json!({}));
    entry["model"] = json!(id);
    let saved = ["/modelCatalog/models", "/routingVendorCatalog"]
        .into_iter()
        .find_map(|path| {
            provider
                .settings_config
                .pointer(path)
                .and_then(Value::as_array)
                .and_then(|rows| rows.iter().find(|row| row["model"] == id))
        });
    if let Some(saved) = saved {
        let mut vendor = saved.clone();
        if let Some(vendor) = vendor.as_object_mut() {
            vendor.remove("modelPreferences");
        }
        if let (Some(entry), Some(fields)) = (
            entry.as_object_mut(),
            super::effort::Capability::parse(&vendor)
                .declaration()
                .as_object(),
        ) {
            for (key, value) in fields {
                entry.entry(key).or_insert_with(|| value.clone());
            }
        }
        if super::model_preferences::valid(saved) {
            if let Some(prefs) = saved.get("modelPreferences") {
                entry["modelPreferences"] = prefs.clone();
            }
        }
        if let Some(default) = super::effort::Capability::parse(saved).default_level {
            entry["defaultReasoningLevel"] = json!(default);
        }
    }
    entry
}

fn apply_cache(provider: &mut Provider, cache: ModelCache) {
    if !provider.settings_config.is_object() {
        return;
    }
    let saved = provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    provider.settings_config["routingContext"] = json!({
        "typed": saved.iter().filter(|row| row["contextSource"] != "vendor" && super::context::Metadata::parse(row).context_window.is_some())
            .filter_map(|row| row["model"].as_str()).collect::<Vec<_>>(),
        "vendor": cache.reasoning.iter().map(|(id, capability)| (id.clone(), json!(capability.context)))
            .collect::<serde_json::Map<_,_>>()
    });
    provider.settings_config["modelCatalog"] = json!({"models": cache.models.into_iter().map(|model| {
        let mut entry = json!({"model":model});
        if let Some(name) = cache.names.get(&model) {
            entry["displayName"] = json!(name);
        }
        if let Some(capability) = cache.reasoning.get(&model) {
            if let (Some(entry), Some(fields)) = (entry.as_object_mut(), capability.declaration().as_object()) {
                entry.extend(fields.clone());
            }
            if let Some(free) = capability.free { entry["free"] = json!(free); }
            capability.context.write(&mut entry);
        }
        // Saved exact-model overrides win only for models still visible in this account.
        if let Some(saved) = saved.iter().find(|e| e["model"].as_str().is_some_and(|id| id.trim() == model)) {
            if let Some(level) = super::effort::Capability::parse(saved).default_level {
                entry["defaultReasoningLevel"] = json!(level);
            }
            super::context::Metadata::parse(saved)
                .with_fallback(super::context::Metadata::parse(&entry)).write(&mut entry);
            if super::model_preferences::valid(saved) {
                if let Some(prefs) = saved.get("modelPreferences") {
                    entry["modelPreferences"] = prefs.clone();
                }
            }
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
        if !crate::services::codex_oauth_models::picker_visible(entry) {
            return;
        }
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
        if !crate::services::codex_oauth_models::picker_visible(entry) {
            return;
        }
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

    #[tokio::test]
    async fn missing_catalog_discovery_reuses_stored_connection_cache_without_changing_selections(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::routing_upstreams::{RoutingUpstream, UpstreamSource};
        use axum::{routing::get, Json, Router};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let requests = Arc::new(AtomicUsize::new(0));
        let observed = requests.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}/v1", listener.local_addr()?);
        let task = tokio::spawn(async move {
            let app = Router::new().route(
                "/v1/models",
                get(move || {
                    let observed = observed.clone();
                    async move {
                        observed.fetch_add(1, Ordering::Relaxed);
                        Json(json!({"data":[{"id":"shared-model"}]}))
                    }
                }),
            );
            let _ = axum::serve(listener, app).await;
        });
        let db = Arc::new(Database::memory()?);
        db.set_intent_routing_auto_create(true)?;
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut saved = Vec::new();
        for models in [vec![], vec![], vec![json!({"model":"selected-model"})]] {
            saved.push(db.save_routing_upstream(RoutingUpstream {
                notes: None,
                icon: None,
                transport: None,
                schema_version: 1,
                id: String::new(),
                name: "Synthetic relay".into(),
                revision: 0,
                enabled: true,
                has_api_key: false,
                preset_id: None,
                region: None,
                codex_settings: None,
                source: UpstreamSource::Api {
                    catalog: vec![],
                    mode: if models.is_empty() {
                        crate::services::codex_provider::ModelMode::Vendor
                    } else {
                        crate::services::codex_provider::ModelMode::Picked
                    },
                    groups_only: false,
                    base_url: base.clone(),
                    api_key: Some("synthetic-test-key".into()),
                    api_format: "openai_responses".into(),
                    models,
                    auth_type: None,
                },
            })?);
        }
        let statuses = prepare_missing_catalogs(db.clone(), manager.clone()).await?;
        assert_eq!(statuses.len(), 2);
        assert!(statuses
            .iter()
            .all(|status| status.status == "updated" && status.model_count == 1));
        assert_eq!(requests.load(Ordering::Relaxed), 2);
        let (providers, _) = prepare_providers(db.clone(), manager.clone(), false).await?;
        let catalog = db.intent_routing_catalog_with_providers(Some(providers))?;
        assert_eq!(catalog.groups.len(), 1);
        assert_eq!(catalog.groups[0].alias(), "auto/shared-model");
        assert!(prepare_missing_catalogs(db.clone(), manager)
            .await?
            .is_empty());
        assert_eq!(requests.load(Ordering::Relaxed), 2);
        for upstream in saved {
            let stored = db.routing_upstream_editor(&upstream.id, upstream.revision)?;
            assert!(serde_json::to_value(&stored)? == serde_json::to_value(upstream)?);
        }
        task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn static_catalog_refresh_populates_missing_models_and_reuses_cache_for_groups(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{routing::get, Json, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}/v1", listener.local_addr()?);
        let task = tokio::spawn(async move {
            let app = Router::new().route(
                "/v1/models",
                get(|| async {
                    Json(
                        json!({"data":[{"id":"shared-model","context_length":1000000,
                        "supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]}]}),
                    )
                }),
            );
            let _ = axum::serve(listener, app).await;
        });
        let db = Arc::new(Database::memory()?);
        let mut providers = Vec::new();
        for id in ["relay-a", "relay-b"] {
            let saved = json!({
                "apiFormat":"openai_responses",
                "env":{"ANTHROPIC_BASE_URL":base,"ANTHROPIC_AUTH_TOKEN":"synthetic-discovery-key"},
                "auth":{"OPENAI_API_KEY":"synthetic-discovery-key"},
                "config":format!("model_provider='fixture'\n[model_providers.fixture]\nbase_url='{base}'\nwire_api='responses'"),
                "modelCatalog":{"models":[]}
            });
            let mut provider =
                Provider::with_id(id.into(), "Synthetic relay".into(), saved.clone(), None);
            db.save_provider("codex", &provider)?;
            prepare_static_context(db.clone(), &mut provider, true).await;
            assert_eq!(
                provider.settings_config["modelCatalog"]["models"][0]["model"],
                "shared-model"
            );
            assert_eq!(db.get_all_providers("codex")?[id].settings_config, saved);
            let capability =
                super::super::effort::Capability::for_provider(&provider, "shared-model");
            assert_eq!(capability.levels, ["low", "high"]);
            providers.push(provider);
        }
        task.abort();
        assert_eq!(super::super::discover_groups(providers.clone()).len(), 1);
        let manager_dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(manager_dir.path().to_owned()));
        let menu = super::super::menu::prepare(manager, providers, vec![], None).await?;
        let published = crate::services::agent_connections::models(&menu);
        assert_eq!(published.len(), 2);
        for model in published {
            assert_eq!(model.reasoning_levels, ["low", "high"]);
            assert!(model.reasoning);
            assert_eq!(
                crate::services::agent_connections::pi_model(&model)["thinkingLevelMap"]["high"],
                "high"
            );
            assert_eq!(
                crate::services::agent_connections::opencode_model(&model)["variants"]["high"]
                    ["reasoningEffort"],
                "high"
            );
            assert_eq!(
                menu.groups[&model.id].model_entry(&model.id)["supported_reasoning_levels"],
                json!([{"effort":"low","description":"low"},{"effort":"high","description":"high"}])
            );
        }
        for (_, mut provider) in db.get_all_providers("codex")? {
            prepare_static_context(db.clone(), &mut provider, false).await;
            assert_eq!(
                provider.settings_config["modelCatalog"]["models"][0]["model"],
                "shared-model"
            );
            assert_eq!(
                super::super::effort::Capability::for_provider(&provider, "shared-model").levels,
                ["low", "high"]
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn static_refresh_caches_capacity_without_replacing_saved_models_or_overrides(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{routing::get, Json, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}/v1", listener.local_addr()?);
        let task = tokio::spawn(async move {
            let app = Router::new().route(
                "/v1/models",
                get(|| async {
                    Json(json!({"data":[{"id":"fixture","context_length":1000000},
                    {"id":"only-upstream","context_length":500000}]}))
                }),
            );
            let _ = axum::serve(listener, app).await;
        });
        let db = Arc::new(Database::memory()?);
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({
                "apiFormat":"openai_responses", "env":{"ANTHROPIC_BASE_URL":base,"ANTHROPIC_AUTH_TOKEN":"synthetic-key"},
                "config":format!("model_provider='fixture'\n[model_providers.fixture]\nbase_url='{base}'\nwire_api='responses'"),
                "auth":{"OPENAI_API_KEY":"synthetic-key"}, "modelCatalog":{"models":[{"model":"fixture"}]}
            }),
            None,
        );
        let saved = provider.settings_config.clone();
        prepare_static_context(db.clone(), &mut provider, true).await;
        assert_eq!(
            super::super::context::Metadata::for_provider(&provider.settings_config, "fixture")
                .context_window,
            Some(1000000)
        );
        assert_eq!(
            provider.settings_config["modelCatalog"],
            saved["modelCatalog"]
        );
        assert!(!super::super::context::Metadata::typed_window(
            &provider.settings_config,
            "fixture"
        ));
        task.abort();
        provider.settings_config = saved.clone();
        prepare_static_context(db, &mut provider, false).await;
        assert_eq!(
            super::super::context::Metadata::for_provider(&provider.settings_config, "fixture")
                .context_window,
            Some(1000000)
        );
        provider.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(1500000);
        assert_eq!(
            super::super::context::Metadata::for_provider(&provider.settings_config, "fixture")
                .context_window,
            Some(1500000)
        );
        Ok(())
    }
    use serde_json::json;

    #[tokio::test]
    async fn frozen_managed_recovery_identity_is_stable_and_account_specific(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        manager
            .add_test_account_with_user_identity("account-a", "synthetic-access-a", "user-a")
            .await?;
        manager
            .add_test_account_with_user_identity("account-b", "synthetic-access-b", "user-b")
            .await?;
        let mut provider =
            Provider::with_id("official".into(), "Synthetic".into(), json!({}), None);
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".into()),
            ..Default::default()
        });
        let first = freeze_execution(&provider, manager.clone())
            .await
            .ok_or("missing credentials")?;
        let original = first
            .continuation_identity()
            .ok_or("missing provenance")?
            .to_owned();
        assert_eq!(
            freeze_execution(&provider, manager.clone())
                .await
                .and_then(|c| c.continuation),
            Some(original.clone())
        );
        manager.set_default_account("account-b").await?;
        let second = freeze_execution(&provider, manager)
            .await
            .ok_or("missing other credentials")?;
        assert_ne!(second.continuation_identity(), Some(original.as_str()));
        assert_eq!(first.continuation_identity(), Some(original.as_str()));
        Ok(())
    }

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
        assert!(task_configured(&api, &manager).await);
        api.settings_config["apiFormat"] = json!("unsupported");
        assert!(!task_configured(&api, &manager).await);
        api.settings_config["apiFormat"] = json!("openai_responses");
        api.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".into()),
            ..Default::default()
        });
        assert!(!task_configured(&api, &manager).await);
    }

    #[test]
    fn official_visibility_filters_cached_metadata_and_preserves_exact_model_ids() {
        let value = json!({"models":[
            {"slug":"vendor/Model_1.2-20261029", "visibility":"list", "supported_in_api":false,
             "display_name":"Visible model", "reasoningLevels":["low","high"], "private_extension":"discard"},
            {"slug":"hidden-model", "visibility":"hide", "display_name":"Hidden model", "reasoningLevels":["high"]},
            {"slug":"unlisted-model", "visibility":"none", "display_name":"Unlisted model", "reasoningLevels":["high"]},
            {"slug":"vendor/Model_1.2-20261029", "visibility":"hide", "display_name":"Hidden duplicate", "reasoningLevels":["xhigh"]}
        ]});
        assert_eq!(discovery_names(&value).len(), 1);
        assert_eq!(discovery_capabilities(&value).len(), 1);
        let cache = ModelCache::from_catalog(value, 100).expect("visible catalog");
        assert_eq!(cache.models, ["vendor/Model_1.2-20261029"]);
        assert_eq!(cache.names[&cache.models[0]], "Visible model");
        assert_eq!(cache.reasoning[&cache.models[0]].levels, ["low", "high"]);
        let serialized = serde_json::to_string(&cache).expect("cache json");
        assert!(!serialized.contains("hidden-model"));
        assert!(!serialized.contains("private_extension"));
        let restored: ModelCache = serde_json::from_str(&serialized).expect("cache round trip");
        assert!(restored.valid_at(101));
    }

    #[test]
    fn official_visibility_empty_refresh_replaces_previous_discovery_without_changing_saved_data() {
        let db = Database::memory().expect("database");
        let mut provider = Provider::with_id(
            "official".into(),
            "Official".into(),
            json!({
                "config":"model='user-selected-model'",
                "modelCatalog":{"models":[{"model":"user-catalog-model"}]}
            }),
            None,
        );
        provider.category = Some("official".into());
        db.save_provider("codex", &provider).expect("save provider");
        db.register_routing_fixture("codex", &provider.id)
            .expect("explicit import");
        let mut saved = super::super::tests::fixture();
        for member in &mut saved.members {
            member.provider_id = provider.id.clone();
        }
        let saved = db.save_intent_routing_group(saved).expect("save group");
        let key = cache_key("native", "fixture-account", "fixture-identity");
        let previous = ModelCache::from_catalog(json!({"models":[{"slug":"old-visible"}]}), 100)
            .expect("old catalog");
        db.save_routing_model_cache(&key, &previous)
            .expect("save old cache");
        for value in [
            json!({"models":[{"slug":"old-visible", "visibility":"hide", "display_name":"Old", "reasoningLevels":["high"]}]}),
            json!({"models":[]}),
            json!({"models":{}}),
        ] {
            let next = ModelCache::from_catalog(value, 101).expect("authoritative empty catalog");
            assert!(next.valid_at(102));
            assert!(next.models.is_empty() && next.names.is_empty() && next.reasoning.is_empty());
            db.save_routing_model_cache(&key, &next)
                .expect("replace cache");
            let cache = db
                .routing_model_cache(&key)
                .expect("read cache")
                .expect("cache");
            apply_cache(&mut provider, cache);
            assert_eq!(
                provider.settings_config["modelCatalog"]["models"],
                json!([])
            );
            assert_eq!(
                provider.settings_config["config"],
                "model='user-selected-model'"
            );
        }
        for value in [
            json!({"error":"synthetic failure"}),
            json!({"models":null}),
            json!({"models":"invalid"}),
            json!({"models":[null, {}, 42]}),
            json!([{}]),
        ] {
            assert!(ModelCache::from_catalog(value, 102).is_none());
        }
        let stored = db.get_all_providers("codex").expect("providers");
        assert_eq!(
            stored["official"].settings_config["modelCatalog"]["models"][0]["model"],
            "user-catalog-model"
        );
        assert_eq!(
            stored["official"].settings_config["config"],
            "model='user-selected-model'"
        );
        assert_eq!(
            db.list_intent_routing_groups().expect("groups"),
            vec![saved]
        );
    }

    #[test]
    fn official_visibility_policy_does_not_reuse_legacy_unfiltered_cache() {
        let db = Database::memory().expect("database");
        let mut legacy = Sha256::new();
        for part in ["native", "fixture-account", "fixture-identity"] {
            legacy.update(part.len().to_be_bytes());
            legacy.update(part.as_bytes());
        }
        let old_key = format!("{:x}", legacy.finalize());
        let old = ModelCache {
            fetched_at: 100,
            models: vec!["previously-hidden".into()],
            reasoning: Default::default(),
            names: Default::default(),
        };
        db.save_routing_model_cache(&old_key, &old)
            .expect("legacy cache");
        let key = cache_key("native", "fixture-account", "fixture-identity");
        assert_ne!(key, old_key);
        assert!(db
            .routing_model_cache(&key)
            .expect("current lookup")
            .is_none());
        assert!(db
            .routing_model_cache(&old_key)
            .expect("legacy retained")
            .is_some());
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
    fn account_refresh_preserves_overrides_only_for_still_visible_models() {
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":[
                {"model":"visible", "modelPreferences":{"name":"Custom", "reasoningLevels":[], "images":false}},
                {"model":"hidden", "modelPreferences":{"name":"Hidden"}}
            ]}}),
            None,
        );
        apply_cache(
            &mut provider,
            ModelCache {
                fetched_at: 100,
                models: vec!["visible".into()],
                reasoning: Default::default(),
                names: Default::default(),
            },
        );
        let entries = provider.settings_config["modelCatalog"]["models"]
            .as_array()
            .expect("fixture");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            super::super::model_preferences::name(&entries[0]),
            Some("Custom")
        );
        assert!(super::super::effort::Capability::parse(&entries[0]).quiet);
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
    fn context_discovery_cache_round_trips_only_supported_fields_and_saved_overrides() {
        for catalog in [
            json!({"models":[{"slug":"exact-model","context_window":1000000,"max_context_window":1200000,
                "effective_context_window_percent":90,"auto_compact_token_limit":200000,"private_extension":"not-persisted"}]}),
            json!({"models":{"exact-model":{"contextWindow":"1000000","maxContextWindow":1200000,
                "effectiveContextWindowPercent":90,"autoCompactTokenLimit":200000,"private_extension":"not-persisted"}}}),
        ] {
            let cache = ModelCache::from_catalog(catalog, 100).expect("cache");
            let serialized = serde_json::to_string(&cache).expect("serialize");
            assert!(!serialized.contains("private_extension"));
            let restored: ModelCache = serde_json::from_str(&serialized).expect("restore");
            assert!(restored.valid_at(101));
            let mut provider =
                Provider::with_id("fixture".into(), "Fixture".into(), json!({}), None);
            apply_cache(&mut provider, restored);
            let metadata = super::super::context::Metadata::for_provider(
                &provider.settings_config,
                "exact-model",
            );
            assert_eq!(metadata.context_window, Some(1000000));
            assert_eq!(metadata.max_context_window, Some(1200000));
            assert_eq!(metadata.effective_context_window_percent, Some(90));
            assert_eq!(metadata.auto_compact_token_limit, Some(200000));
            provider.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(500000);
            provider.settings_config["modelCatalog"]["models"][0]["autoCompactTokenLimit"] =
                json!(100000);
            provider.settings_config["modelCatalog"]["models"][0]["defaultReasoningLevel"] =
                json!("low");
            let restored: ModelCache = serde_json::from_str(&serialized).expect("restore");
            apply_cache(&mut provider, restored);
            let metadata = super::super::context::Metadata::for_provider(
                &provider.settings_config,
                "exact-model",
            );
            assert_eq!(metadata.context_window, Some(500000));
            assert_eq!(metadata.auto_compact_token_limit, Some(100000));
            assert_eq!(metadata.max_context_window, Some(1200000));
            assert_eq!(
                provider.settings_config["modelCatalog"]["models"][0]["defaultReasoningLevel"],
                "low"
            );
            let mut invalid: Value = serde_json::from_str(&serialized).expect("json");
            invalid["reasoning"]["exact-model"]["context"]["effective_context_window_percent"] =
                json!(101);
            assert!(!serde_json::from_value::<ModelCache>(invalid)
                .expect("cache shape")
                .valid_at(101));
            apply_cache(
                &mut provider,
                ModelCache::from_catalog(json!({"models":[]}), 101).expect("empty"),
            );
            assert!(provider.settings_config["modelCatalog"]["models"]
                .as_array()
                .expect("models")
                .is_empty());
        }
    }

    #[test]
    fn refreshed_vendor_models_keep_imported_reasoning_preferences_and_defaults() {
        let cache = ModelCache::from_catalog(
            json!({"models":[{"id":"exact","reasoningLevels":["low","high","max"]}]}),
            100,
        )
        .expect("synthetic cache");
        for saved in [
            json!({"model":"exact","defaultReasoningLevel":"low","modelPreferences":{"reasoningLevels":["low","high"]}}),
            json!({"model":"exact"}),
        ] {
            let mut provider = Provider::with_id(
                "fixture".into(),
                "Synthetic".into(),
                json!({
                    "modelCatalog":{"models":[saved.clone()]},
                    "routingVendorCatalog":[{"model":"exact","defaultReasoningLevel":"max","modelPreferences":{"reasoningLevels":["max"]}}],
                    "routingModelPolicy":{"mode":"vendor","models":[],"groupsOnly":false}
                }),
                None,
            );
            provider.settings_config["routingVendorCatalog"] =
                json!([static_entry(&provider, &cache, "exact")]);
            crate::services::routing_upstreams::filter_models(&mut provider);
            let capability = super::super::effort::Capability::for_provider(&provider, "exact");
            if saved.get("modelPreferences").is_some() {
                assert_eq!(capability.levels, ["low", "high"]);
                assert_eq!(capability.default_level.as_deref(), Some("low"));
            } else {
                assert_eq!(capability.levels, ["low", "high", "max"]);
                assert_eq!(capability.default_level, None);
            }
            assert_eq!(
                provider.settings_config["modelCatalog"]["models"][0]["reasoningLevels"],
                json!(["low", "high", "max"])
            );
        }
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
    fn native_editor_identity_requires_consistent_workspace_and_never_returns_tokens() {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            json!({
                "sub": "fixture-user",
                "email": "person@example.test",
                "https://api.openai.com/auth": {
                    "chatgpt_account_id": "fixture-workspace",
                    "chatgpt_plan_type": "plus"
                }
            })
            .to_string(),
        );
        let mut auth = json!({"auth_mode":"chatgpt", "tokens": {
            "access_token":"synthetic-access",
            "account_id":"fixture-workspace",
            "id_token":format!("{header}.{payload}.")
        }});
        let info = native_editor_observation(&auth).expect("native observation");
        assert_eq!(info.login, "person@example.test");
        assert_eq!(info.plan.as_deref(), Some("ChatGPT Plus"));
        assert_eq!(
            info.identity,
            Some(("sub:fixture-user".into(), "fixture-workspace".into()))
        );
        auth["tokens"]["account_id"] = json!("other-workspace");
        assert!(native_editor_observation(&auth)
            .expect("observation")
            .identity
            .is_none());
        auth["tokens"]["id_token"] = json!("invalid-token");
        let unknown = native_editor_observation(&auth).expect("unknown identity");
        assert_eq!(unknown.login, "Codex");
        assert!(unknown.identity.is_none());
        auth["auth_mode"] = json!("apikey");
        assert!(native_editor_observation(&auth).is_none());
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
    continuation: Option<String>,
}
impl ExecutionCredentials {
    pub(crate) fn serves_model(&self, model: &str) -> bool {
        crate::services::codex_provider::account_serves(&self.provider, model)
    }
    pub(crate) fn provider_name(&self) -> &str {
        &self.provider.name
    }

    pub(crate) fn continuation_identity(&self) -> Option<&str> {
        self.continuation.as_deref()
    }
    pub(crate) fn fingerprint(&self) -> Vec<u8> {
        let mut hash = Sha256::new();
        for value in [&self.key, &self.workspace] {
            hash.update(value.len().to_be_bytes());
            hash.update(value.as_bytes());
        }
        hash.finalize().to_vec()
    }
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
    // Durable ownership is independent of access-token-sensitive discovery caching.
    let continuation = if let Some(id) = identity.managed_id.as_deref() {
        manager.continuation_identity(id).await
    } else {
        tauri::async_runtime::spawn_blocking(|| {
            let auth: Value =
                serde_json::from_str(&read_bounded(&crate::codex_config::get_codex_auth_path())?)
                    .ok()?;
            let user = crate::codex_config::extract_codex_id_token_user_identity(
                auth.pointer("/tokens/id_token")?.as_str()?,
            )?;
            let (_, workspace) = native_credentials(&auth)?;
            let digest = Sha256::digest(
                serde_json::to_vec(&("native-continuation-v1", user, workspace)).ok()?,
            );
            Some(format!("{digest:x}"))
        })
        .await
        .ok()
        .flatten()
    };
    let after = source(provider, &manager).await?;
    if after.key != identity.key || after.workspace != identity.workspace {
        return None;
    }
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
        continuation,
    })
}
