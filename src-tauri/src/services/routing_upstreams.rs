//! Native global providers, independent of source application identities.
use crate::provider::{Provider, ProviderMeta};
use crate::services::codex_provider::{ModelMode, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) const INVALID: &str = "routingUpstreams.invalid";
pub(crate) const UNAVAILABLE: &str = "routingUpstreams.sourceUnavailable";
pub(crate) const MAX_UPSTREAMS: usize = 256;
pub(crate) const MAX_CONFIG_BYTES: usize = 512 * 1024;
pub(crate) const MODEL_POLICY: &str = "routingModelPolicy";
pub(crate) const KEY_LABEL: &str = "routingKeyLabel";
const KEY_MODELS: &str = "routingKeyModels";
pub(crate) const EXECUTION_IDENTITY: &str = "routingExecutionIdentity";

pub(crate) fn execution_identity(provider: &Provider) -> &str {
    provider
        .settings_config
        .get(EXECUTION_IDENTITY)
        .and_then(Value::as_str)
        .filter(|id| valid_identity(id))
        .unwrap_or(&provider.id)
}

pub(crate) fn execution_group<'a>(
    group: &mut crate::services::intent_routing::RoutingGroup,
    providers: impl Iterator<Item = &'a Provider>,
) {
    let identities: std::collections::HashMap<_, _> = providers
        .map(|p| (p.id.as_str(), execution_identity(p)))
        .collect();
    for member in &mut group.members {
        if let Some(id) = identities.get(member.provider_id.as_str()) {
            member.provider_id = (*id).into();
        }
    }
    if let Some(target) = &mut group.classifier_target {
        if let Some(id) = identities.get(target.provider_id.as_str()) {
            target.provider_id = (*id).into();
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RoutingUpstream {
    pub schema_version: u8,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub revision: u32,
    pub source: UpstreamSource,
    #[serde(default)]
    pub has_api_key: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_settings: Option<Settings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<ImportedTransport>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImportedTransport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_name: Option<String>,
    #[serde(default)]
    pub primary_off: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_models: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_protocol: Option<String>,
    #[serde(default)]
    pub endpoints: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub keys: Vec<ImportedKey>,
    #[serde(default)]
    pub full_url: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImportedKey {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
}
fn default_enabled() -> bool {
    true
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeySummary {
    pub id: String,
    pub label: String,
    pub running: Option<usize>,
    #[serde(skip)]
    pub observation: String,
}

pub(crate) fn key_observation(key: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(key.as_bytes()))
}

pub(crate) fn masked_key(value: &str) -> String {
    let chars: Vec<_> = value.chars().collect();
    if chars.len() < 12 {
        return "••••••••".into();
    }
    format!(
        "{}…{}",
        chars[..4].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum UpstreamSource {
    Api {
        base_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
        api_format: String,
        #[serde(default)]
        models: Vec<Value>,
        #[serde(default)]
        catalog: Vec<Value>,
        #[serde(default)]
        mode: ModelMode,
        #[serde(default)]
        groups_only: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_type: Option<String>,
    },
    Codex,
    Subscription {
        provider: SubscriptionProvider,
        account_id: String,
        #[serde(default)]
        models: Vec<Value>,
        #[serde(default)]
        catalog: Vec<Value>,
        #[serde(default)]
        mode: ModelMode,
        #[serde(default)]
        groups_only: bool,
    },
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SubscriptionProvider {
    GithubCopilot,
    XaiOauth,
    Claude,
    ChatgptApi,
    GeminiCli,
    Antigravity,
    Cursor,
}
impl SubscriptionProvider {
    pub(crate) fn endpoint(self) -> &'static str {
        match self {
            Self::GithubCopilot => "https://api.githubcopilot.com",
            Self::Cursor => "https://agentn.global.api5.cursor.sh",
            Self::XaiOauth => super::grok_subscription::BASE,
            Self::Claude => "https://api.anthropic.com",
            Self::ChatgptApi => "https://api.openai.com/v1",
            Self::GeminiCli => "https://cloudcode-pa.googleapis.com",
            Self::Antigravity => "https://daily-cloudcode-pa.googleapis.com",
        }
    }
    pub(crate) fn format(self) -> &'static str {
        match self {
            Self::GithubCopilot | Self::Cursor => "openai_chat",
            Self::XaiOauth | Self::ChatgptApi => "openai_responses",
            Self::Claude => "anthropic",
            Self::GeminiCli | Self::Antigravity => "gemini_native",
        }
    }
}

// Debug output must never expose credentials or private configuration.
impl std::fmt::Debug for RoutingUpstream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingUpstream")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}
pub(crate) fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

fn valid_name(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 120 && !value.chars().any(char::is_control)
}

pub(crate) fn valid_format(format: &str) -> bool {
    matches!(
        format,
        "anthropic" | "openai_chat" | "openai_responses" | "gemini_native"
    )
}

fn valid_key(key: &str) -> bool {
    !key.trim().is_empty() && key.len() <= 16384 && !key.chars().any(char::is_control)
}

pub(crate) fn valid_url(base_url: &str) -> bool {
    if base_url.len() > 2048
        || base_url != base_url.trim()
        || base_url.chars().any(char::is_control)
    {
        return false;
    }
    let Ok(url) = url::Url::parse(base_url) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && !base_url
            .split('/')
            .nth(2)
            .is_some_and(|authority| authority.contains('@'))
}

fn valid_models(models: &[Value]) -> bool {
    let mut ids = std::collections::HashSet::new();
    models.len() <= 4096
        && models.iter().all(|row| {
            crate::services::intent_routing::model_preferences::valid(row)
                && model_id(row).is_some_and(|id| {
                    !id.is_empty()
                        && id.len() <= 200
                        && id == id.trim()
                        && !id.chars().any(char::is_control)
                        && ids.insert(id)
                        && !id.starts_with("vary-auto/")
                        && !id.starts_with("vary-provider/")
                })
        })
}
pub(crate) fn model_id(row: &Value) -> Option<&str> {
    row.as_str()
        .or_else(|| row.get("model").and_then(Value::as_str))
        .or_else(|| row.get("id").and_then(Value::as_str))
}
impl RoutingUpstream {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !valid_identity(&self.id)
            || !valid_name(&self.name)
            || self
                .icon
                .as_deref()
                .is_some_and(|icon| !crate::services::provider_icons::valid(icon))
            || self
                .preset_id
                .as_ref()
                .is_some_and(|id| !valid_identity(id))
            || self.region.as_ref().is_some_and(|id| !valid_identity(id))
            || self
                .codex_settings
                .as_ref()
                .is_some_and(|s| !s.valid() || !matches!(self.source, UpstreamSource::Codex))
        {
            return Err(INVALID.into());
        }
        if let UpstreamSource::Api {
            base_url,
            api_key,
            api_format,
            models,
            catalog,
            ..
        } = &self.source
        {
            if matches!(&self.source, UpstreamSource::Api { auth_type: Some(auth), .. } if !matches!(auth.as_str(), "api-key" | "bearer" | "both"))
            {
                return Err(INVALID.into());
            }
            if !valid_url(base_url)
                || !api_key.as_deref().is_some_and(valid_key)
                || !valid_format(api_format)
                || models.len() > 256
                || !valid_models(models)
                || !valid_models(catalog)
            {
                return Err(INVALID.into());
            }
        }
        if let UpstreamSource::Subscription {
            account_id,
            models,
            catalog,
            ..
        } = &self.source
        {
            if !valid_identity(account_id)
                || models.len() > 256
                || !valid_models(models)
                || !valid_models(catalog)
            {
                return Err(INVALID.into());
            }
        }
        if let Some(transport) = &self.transport {
            let mut ids = std::collections::HashSet::new();
            if !matches!(self.source, UpstreamSource::Api { .. })
                || transport.primary_models.as_ref().is_some_and(|models| !valid_key_models(models))
                || transport.keys.iter().any(|key| key.models.as_ref().is_some_and(|models| !valid_key_models(models)))
                || transport.endpoints.iter().any(|(format, url)| !valid_format(format) || !valid_url(url))
                || transport.primary_name.as_ref().is_some_and(|name| name.chars().count() > 120 || name.chars().any(char::is_control))
                || transport.keys.len() > 64
                || transport.preferred_key.as_ref().is_some_and(|id| !transport.keys.iter().any(|key| &key.id == id))
                || transport.keys.iter().any(|key| key.name.chars().count() > 120 || key.name.chars().any(char::is_control))
                || transport.primary_protocol.as_deref().is_some_and(|f| !valid_format(f) || !matches!(&self.source, UpstreamSource::Api { api_format, .. } if api_format == f))
                || transport.keys.iter().any(|k| !valid_identity(&k.id) || !ids.insert(&k.id) || !k.key.as_deref().is_some_and(valid_key) || k.api_format.as_deref().is_some_and(|f| !valid_format(f) || !transport.endpoints.contains_key(f)))
                || transport.headers.as_ref().is_some_and(|headers| headers.len() > 64 || headers.iter().any(|(name, value)| {
                    http::HeaderName::from_bytes(name.as_bytes()).is_err()
                        || http::HeaderValue::from_str(value).is_err()
                        || http::HeaderName::from_bytes(name.as_bytes()).is_ok_and(|name| crate::proxy::is_protected_local_proxy_override_header(&name))
                        || name.eq_ignore_ascii_case("cookie")
                        || value.contains("${sessionId}") || value.contains("${threadId}")
                })) { return Err(INVALID.into()); }
        }
        if serde_json::to_vec(self).map_err(|_| INVALID)?.len() > MAX_CONFIG_BYTES {
            return Err(INVALID.into());
        }
        Ok(())
    }
    pub(crate) fn has_enabled_key(&self) -> bool {
        self.transport.as_ref().is_none_or(|transport| {
            !transport.primary_off || transport.keys.iter().any(|key| key.enabled)
        })
    }
    pub(crate) fn redacted(mut self) -> Self {
        self.has_api_key = false;
        if let UpstreamSource::Api { api_key, .. } = &mut self.source {
            self.has_api_key = api_key.as_deref().is_some_and(valid_key);
            *api_key = None;
        }
        if let Some(transport) = &mut self.transport {
            transport.headers = None;
            for key in &mut transport.keys {
                key.key = None;
            }
        }
        self
    }
}
fn executable_settings(
    base_url: &str,
    key: &str,
    format: &str,
    auth_type: Option<&str>,
    models: Vec<Value>,
) -> Value {
    let models = models
        .into_iter()
        .map(|mut model| {
            if let Some(id) = model.as_str() {
                return json!({"model":id});
            }
            if model.get("model").is_none() {
                if let Some(id) = model.get("id").cloned() {
                    model["model"] = id;
                }
            }
            model
        })
        .collect::<Vec<_>>();
    let key_field = if auth_type == Some("api-key") {
        "ANTHROPIC_API_KEY"
    } else {
        "ANTHROPIC_AUTH_TOKEN"
    };
    let base_url_toml = toml::Value::String(base_url.into()).to_string();
    let wire = match format {
        "openai_chat" => "chat",
        "anthropic" => "anthropic",
        "gemini_native" => "gemini",
        _ => "responses",
    };
    json!({
        "env":{"ANTHROPIC_BASE_URL":base_url, (key_field):key, "GOOGLE_GEMINI_BASE_URL":base_url, "GEMINI_API_KEY":key},
        "auth":{"OPENAI_API_KEY":key},
        "apiFormat":format,
        "authType":auth_type.unwrap_or("bearer"),
        "config":format!("model_provider = \"routing\"\n[model_providers.routing]\nname = \"Routing upstream\"\nbase_url = {base_url_toml}\nwire_api = \"{wire}\"\n"),
        "modelCatalog":{"models":models}
    })
}

pub(crate) fn resolve_provider(record: &RoutingUpstream) -> Result<Provider, String> {
    record.validate()?;
    match &record.source {
        UpstreamSource::Api {
            base_url,
            api_key,
            api_format,
            models,
            catalog,
            mode,
            groups_only,
            auth_type,
        } => {
            let settings = executable_settings(
                base_url,
                api_key.as_deref().ok_or(UNAVAILABLE)?,
                api_format,
                auth_type.as_deref(),
                catalog.clone(),
            );
            let mut provider =
                Provider::with_id(record.id.clone(), record.name.clone(), settings, None);
            provider.icon = record.icon.clone();
            provider.meta = Some(ProviderMeta {
                api_format: Some(api_format.clone()),
                api_key_field: (auth_type.as_deref() == Some("api-key"))
                    .then(|| "ANTHROPIC_API_KEY".into()),
                ..Default::default()
            });
            if let Some(transport) = &record.transport {
                provider.settings_config["routingImportedTransport"] = json!(transport);
                if let Some(meta) = &mut provider.meta {
                    meta.is_full_url = Some(transport.full_url);
                    meta.local_proxy_request_overrides = transport.headers.clone().map(|headers| {
                        crate::provider::LocalProxyRequestOverrides {
                            headers: headers.into_iter().collect(),
                            ..Default::default()
                        }
                    });
                }
            }
            provider.settings_config[MODEL_POLICY] =
                json!({"mode":mode,"models":models,"groupsOnly":groups_only});
            // Routing discovery is synchronous; prepare the stored list immediately.
            filter_models(&mut provider);
            Ok(provider)
        }
        UpstreamSource::Subscription {
            provider: service,
            account_id,
            models,
            catalog,
            mode,
            groups_only,
        } => {
            let mut provider = Provider::with_id(
                record.id.clone(),
                record.name.clone(),
                executable_settings(
                    service.endpoint(),
                    "subscription-pending",
                    service.format(),
                    None,
                    catalog.clone(),
                ),
                None,
            );
            provider.icon = record.icon.clone();
            provider.meta = Some(ProviderMeta {
                api_format: Some(service.format().into()),
                ..Default::default()
            });
            provider.settings_config["routingSubscription"] =
                json!({"provider":service,"accountId":account_id});
            provider.settings_config[MODEL_POLICY] =
                json!({"mode":mode,"models":models,"groupsOnly":groups_only});
            filter_models(&mut provider);
            Ok(provider)
        }
        UpstreamSource::Codex => {
            let mut provider = Provider::with_id(
                record.id.clone(),
                record.name.clone(),
                json!({"auth":{},"config":"","apiFormat":"openai_responses"}),
                None,
            );
            provider.category = Some("official".into());
            provider.icon = record.icon.clone().or_else(|| Some("openai".into()));
            provider.meta = Some(ProviderMeta {
                api_format: Some("openai_responses".into()),
                ..Default::default()
            });
            crate::services::codex_provider::attach(
                &mut provider,
                &record.codex_settings.clone().unwrap_or_default(),
            )?;
            crate::services::codex_provider::filter_models(&mut provider);
            Ok(provider)
        }
    }
}

pub(crate) fn variants(provider: &Provider) -> Vec<Provider> {
    let mut variants = crate::services::codex_provider::variants(provider);
    let Some(transport) = provider
        .settings_config
        .get("routingImportedTransport")
        .and_then(|v| serde_json::from_value::<ImportedTransport>(v.clone()).ok())
    else {
        return variants;
    };
    let (_, primary) = provider.resolve_usage_credentials(&crate::app_config::AppType::Codex);
    let mut enabled_keys = std::collections::BTreeSet::new();
    if !transport.primary_off && !primary.is_empty() {
        enabled_keys.insert(primary.as_str());
    }
    for key in transport.keys.iter().filter(|key| key.enabled) {
        if let Some(token) = key.key.as_deref().filter(|key| !key.is_empty()) {
            enabled_keys.insert(token);
        }
    }
    let shared = enabled_keys.len() > 1;
    let key_label = |key: &str, name: Option<&str>| {
        name.filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| masked_key(key))
    };
    // Each execution snapshot contains just its own credential, never the pool.
    for variant in &mut variants {
        if let Some(settings) = variant.settings_config.as_object_mut() {
            settings.remove("routingImportedTransport");
            settings.remove(KEY_LABEL);
            if let Some(models) = &transport.primary_models {
                settings.insert(KEY_MODELS.into(), json!(models));
            }
            if shared {
                settings.insert(
                    KEY_LABEL.into(),
                    json!(key_label(&primary, transport.primary_name.as_deref())),
                );
            }
        }
    }
    if transport.primary_off {
        variants.clear();
    }
    let (base, _) = provider.resolve_usage_credentials(&crate::app_config::AppType::Codex);
    let primary_format = provider.settings_config["apiFormat"]
        .as_str()
        .unwrap_or("openai_chat");
    for key in transport.keys.iter().filter(|key| key.enabled) {
        let Some(token) = &key.key else {
            continue;
        };
        let mut variant = provider.clone();
        let format = key.api_format.as_deref().unwrap_or(primary_format);
        let endpoint = transport
            .endpoints
            .get(format)
            .map(String::as_str)
            .unwrap_or(&base);
        let settings = executable_settings(
            endpoint,
            token,
            format,
            provider.settings_config["authType"].as_str(),
            vec![],
        );
        if let (Some(next), Some(source)) = (
            variant.settings_config.as_object_mut(),
            settings.as_object(),
        ) {
            next.remove("routingImportedTransport");
            next.remove(KEY_LABEL);
            next.remove(KEY_MODELS);
            if let Some(models) = &key.models {
                next.insert(KEY_MODELS.into(), json!(models));
            }
            if shared {
                next.insert(KEY_LABEL.into(), json!(key_label(token, Some(&key.name))));
            }
            for field in ["env", "auth", "apiFormat", "authType", "config"] {
                next.insert(field.into(), source[field].clone());
            }
        }
        variant.meta.get_or_insert_with(Default::default).api_format = Some(format.into());
        if transport.preferred_key.as_ref() == Some(&key.id) {
            variants.insert(0, variant);
        } else {
            variants.push(variant);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    variants.retain(|variant| {
        let (base, key) = variant.resolve_usage_credentials(&crate::app_config::AppType::Codex);
        // Display labels are not part of transport identity. Distinct formats,
        // endpoints and account credentials must remain separate destinations.
        seen.insert((
            base,
            key,
            variant.settings_config["apiFormat"].to_string(),
            variant.settings_config["authType"].to_string(),
            variant.settings_config["headers"].to_string(),
            variant.settings_config["config"].to_string(),
            variant.settings_config[KEY_MODELS].to_string(),
        ))
    });
    variants
}

pub(crate) fn first_enabled(provider: &Provider) -> Result<Provider, String> {
    variants(provider)
        .into_iter()
        .next()
        .ok_or_else(|| UNAVAILABLE.into())
}

fn valid_key_models(models: &[String]) -> bool {
    valid_models(&models.iter().map(|id| json!(id)).collect::<Vec<_>>())
}

pub(crate) fn variants_for_model(provider: &Provider, model: &str) -> Vec<Provider> {
    variants(provider)
        .into_iter()
        .filter(|variant| key_serves(variant, model))
        .collect()
}

pub(crate) fn key_serves(provider: &Provider, model: &str) -> bool {
    provider
        .settings_config
        .get(KEY_MODELS)
        .and_then(Value::as_array)
        .is_none_or(|models| models.iter().any(|id| id.as_str() == Some(model)))
}

pub(crate) fn strip_key_model_scope(provider: &mut Provider) {
    if let Some(settings) = provider.settings_config.as_object_mut() {
        settings.remove(KEY_MODELS);
    }
}

pub(crate) fn filter_models(provider: &mut Provider) {
    let Some(policy) = provider.settings_config.get(MODEL_POLICY).cloned() else {
        return;
    };
    let picks = policy["models"].as_array().cloned().unwrap_or_default();
    let catalog = provider
        .settings_config
        .get("routingVendorCatalog")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| {
            provider.settings_config["modelCatalog"]["models"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        });
    provider.settings_config["routingVendorCatalog"] = json!(catalog);
    let chat_catalog: Vec<_> = catalog
        .iter()
        .filter(|row| !crate::model_capabilities::is_image_generation_model(row))
        .cloned()
        .collect();
    let mut rows = match policy["mode"].as_str() {
        Some("usage") => vec![],
        Some("picked") => picks
            .iter()
            .map(|pick| {
                let Some(id) = model_id(pick) else {
                    return pick.clone();
                };
                let mut row = catalog
                    .iter()
                    .find(|row| model_id(row) == Some(id))
                    .cloned()
                    .unwrap_or_else(|| pick.clone());
                if let Some(fields) = pick.as_object() {
                    if let Some(row) = row.as_object_mut() {
                        // Fresh vendor facts remain authoritative; explicit preferences
                        // carry user overrides without replacing capability observations.
                        for (key, value) in fields {
                            if key == "modelPreferences" || !row.contains_key(key) {
                                row.insert(key.clone(), value.clone());
                            }
                        }
                    }
                }
                row
            })
            .collect(),
        Some("best") => crate::services::codex_provider::best_models(chat_catalog),
        _ => chat_catalog.into_iter().take(24).collect(),
    };
    for row in &mut rows {
        if let Some(id) = row.as_str() {
            *row = json!({"model":id});
        }
        if row.get("model").is_none() {
            row["model"] = row["id"].clone();
        }
        if let Some(picked) = picks.iter().find(|p| model_id(p) == model_id(row)) {
            if let Some(prefs) = picked.get("modelPreferences") {
                row["modelPreferences"] = prefs.clone();
            }
        }
    }
    rows.retain(|row| !crate::model_capabilities::is_image_generation_model(row));
    provider.settings_config["modelCatalog"] = json!({"models":rows});
}

pub(crate) fn serves(provider: &Provider, model: &str) -> bool {
    provider
        .settings_config
        .get(MODEL_POLICY)
        .is_none_or(|policy| {
            policy["mode"] != "usage"
                && (policy["mode"] != "picked"
                    || policy["models"]
                        .as_array()
                        .is_some_and(|rows| rows.iter().any(|row| model_id(row) == Some(model))))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn key_model_scopes_never_send_an_unlisted_model_to_a_key() -> Result<(), String> {
        let record: RoutingUpstream = serde_json::from_value(json!({
            "schemaVersion":1,"id":"fixture","name":"Fixture","revision":1,
            "source":{"kind":"api","baseUrl":"https://fixture.example/v1","apiKey":"synthetic-primary-key","apiFormat":"openai_chat","mode":"picked","models":[{"model":"chat-a"},{"model":"chat-b"}],"catalog":[]},
            "transport":{"endpoints":{},"fullUrl":false,"primaryModels":["chat-a"],"keys":[{"id":"backup","name":"Backup","enabled":true,"key":"synthetic-backup-key","models":["chat-b"]}]}
        })).map_err(|_| INVALID)?;
        record.validate()?;
        let provider = resolve_provider(&record)?;
        for (model, key) in [
            ("chat-a", "synthetic-primary-key"),
            ("chat-b", "synthetic-backup-key"),
        ] {
            let choices = variants_for_model(&provider, model);
            assert_eq!(choices.len(), 1);
            assert!(
                choices[0]
                    .resolve_usage_credentials(&crate::app_config::AppType::Codex)
                    .1
                    == key
            );
        }
        assert!(variants_for_model(&provider, "other").is_empty());
        let db = crate::database::Database::memory().map_err(|_| INVALID)?;
        let mut persisted = record.clone();
        persisted.revision = 0;
        let saved = db.save_routing_upstream(persisted)?;
        let mut edited = db.routing_upstream_editor(&saved.id, saved.revision)?;
        let transport = edited.transport.as_mut().ok_or(INVALID)?;
        assert_eq!(transport.primary_models, Some(vec!["chat-a".to_owned()]));
        assert_eq!(transport.keys[0].models, Some(vec!["chat-b".to_owned()]));
        transport.primary_models = Some(vec![]);
        transport.keys[0].models = None;
        let saved = db.save_routing_upstream(edited)?;
        let updated = db.routing_discovery_provider(&saved.id, saved.revision)?;
        let choices = variants_for_model(&updated, "chat-a");
        assert_eq!(choices.len(), 1);
        assert!(
            choices[0]
                .resolve_usage_credentials(&crate::app_config::AppType::Codex)
                .1
                == "synthetic-backup-key"
        );
        let directory = tempfile::tempdir().map_err(|_| INVALID)?;
        let manager = std::sync::Arc::new(
            crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(
                directory.path().to_owned(),
            ),
        );
        let menu =
            crate::services::intent_routing::menu::prepare(manager, vec![provider], vec![], None)
                .await?;
        assert_eq!(menu.groups.len(), 2);
        for session in menu.groups.values() {
            assert_eq!(session.targets.len(), 1);
            let target = &session.targets[0];
            let expected = if target.model == "chat-a" {
                "synthetic-primary-key"
            } else {
                "synthetic-backup-key"
            };
            let upstream = target.upstream.as_ref().ok_or(INVALID)?;
            assert!(
                upstream
                    .resolve_usage_credentials(&crate::app_config::AppType::Codex)
                    .1
                    == expected
            );
            assert!(upstream.settings_config.get(KEY_MODELS).is_none());
        }
        Ok(())
    }
    use crate::app_config::AppType;

    #[test]
    fn image_models_stay_in_the_editor_catalog_but_leave_agent_chat_lists() -> Result<(), String> {
        let catalog = json!([
            {"model":"gpt-image-2"},
            {"model":"vendor-drawing", "kind":"image"},
            {"model":"vendor-output", "outputModalities":["image"]},
            {"model":"chat-fixture", "inputModalities":["text","image"]}
        ]);
        for mode in ["vendor", "picked", "best", "usage"] {
            let record: RoutingUpstream = serde_json::from_value(json!({
                "schemaVersion":1,"id":"fixture","name":"Fixture","revision":1,
                "source":{"kind":"api","baseUrl":"https://fixture.example/v1",
                    "apiFormat":"openai_chat","apiKey":"synthetic-fixture-key","mode":mode,"models":catalog,"catalog":catalog}
            })).map_err(|_| INVALID)?;
            let provider = resolve_provider(&record)?;
            assert_eq!(provider.settings_config["routingVendorCatalog"], catalog);
            let rows = provider.settings_config["modelCatalog"]["models"]
                .as_array()
                .ok_or(INVALID)?;
            assert_eq!(rows.len(), usize::from(mode != "usage"), "{mode}");
            if mode != "usage" {
                assert_eq!(rows[0]["model"], "chat-fixture", "{mode}");
            }
        }
        Ok(())
    }

    #[test]
    fn selected_subscription_models_keep_refreshed_capabilities_and_preferences(
    ) -> Result<(), String> {
        let record: RoutingUpstream=serde_json::from_value(json!({"schemaVersion":1,"id":"synthetic","name":"Synthetic","revision":1,"source":{"kind":"subscription","provider":"github_copilot","accountId":"fixture","mode":"picked","models":[{"model":"fixture-model","modelPreferences":{"reasoningLevels":["high"]}}],"catalog":[{"model":"fixture-model","contextWindow":200000,"supported_endpoints":["/responses"],"inputModalities":["text","image"]}]}})).map_err(|_|INVALID)?;
        let provider = resolve_provider(&record)?;
        let row = &provider.settings_config["modelCatalog"]["models"][0];
        assert_eq!(row["contextWindow"], 200000);
        assert_eq!(row["supported_endpoints"], json!(["/responses"]));
        assert_eq!(row["modelPreferences"]["reasoningLevels"], json!(["high"]));
        let selected =
            crate::services::subscription_provider::for_model(&provider, "fixture-model")?;
        assert_eq!(
            selected.meta.as_ref().and_then(|m| m.api_format.as_deref()),
            Some("openai_responses")
        );
        let (url, _) = crate::services::intent_routing::execution::static_transport(
            &selected,
            "fixture-model",
        )?;
        assert!(url.ends_with("/responses"));
        Ok(())
    }
    #[test]
    fn parity_duplicate_keys_share_one_execution_destination() -> Result<(), String> {
        let record: RoutingUpstream = serde_json::from_value(json!({
            "schemaVersion":1,"id":"synthetic","name":"Synthetic","revision":1,
            "source":{"kind":"api","baseUrl":"https://synthetic.invalid/v1","apiKey":"synthetic-only","apiFormat":"openai_chat"},
            "transport":{"keys":[{"id":"backup","name":"","key":"synthetic-only","enabled":true}]}
        })).map_err(|_| INVALID)?;
        assert_eq!(variants(&resolve_provider(&record)?).len(), 1);
        Ok(())
    }
    #[test]
    fn subscription_sources_bind_accounts_without_storing_tokens() {
        for service in [
            "github_copilot",
            "xai_oauth",
            "claude",
            "chatgpt_api",
            "gemini_cli",
            "antigravity",
        ] {
            let record: RoutingUpstream = serde_json::from_value(json!({
                "schemaVersion":1,"id":"fixture","name":"Subscription","revision":1,
                "source":{"kind":"subscription","provider":service,"accountId":"fixture-account",
                    "models":[{"model":"fixture-model"}],"catalog":[]}
            }))
            .expect("subscription source");
            let provider = resolve_provider(&record).expect("resolved subscription");
            assert_eq!(
                provider.settings_config["routingSubscription"]["accountId"],
                "fixture-account"
            );
            assert!(crate::services::intent_routing::eligible_provider(
                &provider, false
            ));
            assert!(!crate::services::intent_routing::eligible_provider(
                &provider, true
            ));
            assert!(!record.redacted().has_api_key);
        }
        for service in ["unknown", "codex_oauth"] {
            assert!(serde_json::from_value::<RoutingUpstream>(json!({
                "schemaVersion":1,"id":"fixture","name":"Subscription","revision":1,
                "source":{"kind":"subscription","provider":service,"accountId":"fixture-account","models":[],"catalog":[]}
            })).is_err());
        }
    }
    #[test]
    fn all_protocols_preserve_exact_models_and_support_both_credential_views() {
        for format in [
            "anthropic",
            "openai_chat",
            "openai_responses",
            "gemini_native",
        ] {
            for auth in [None, Some("api-key"), Some("bearer")] {
                let models = vec![
                    json!({"model":"exact-model", "modalities":["text"], "custom":{"unmodified":true}}),
                ];
                let record = RoutingUpstream {
                    notes: None,
                    icon: None,
                    transport: None,
                    schema_version: 1,
                    preset_id: None,
                    region: None,
                    codex_settings: None,
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "Fixture".into(),
                    revision: 1,
                    has_api_key: false,
                    enabled: true,
                    source: UpstreamSource::Api {
                        catalog: vec![],
                        mode: crate::services::codex_provider::ModelMode::Picked,
                        groups_only: false,
                        base_url: "https://synthetic.example/v1".into(),
                        api_key: Some("synthetic-fixture-key".into()),
                        api_format: format.into(),
                        models: models.clone(),
                        auth_type: auth.map(str::to_owned),
                    },
                };
                let provider = resolve_provider(&record).unwrap();
                let (base_url, key) = provider.resolve_usage_credentials(&AppType::Codex);
                assert_eq!(base_url, "https://synthetic.example/v1");
                assert!(key == "synthetic-fixture-key");
                assert_eq!(
                    provider.settings_config["modelCatalog"]["models"],
                    json!(models)
                );
                assert_eq!(
                    provider.meta.as_ref().unwrap().api_format.as_deref(),
                    Some(format)
                );
            }
        }
    }
    #[test]
    fn saved_model_preferences_must_be_validated_before_publication() {
        let mut record: RoutingUpstream = serde_json::from_value(json!({
            "schemaVersion":1, "id":"synthetic", "name":"Synthetic", "revision":0,
            "hasApiKey":false, "source":{"kind":"api","baseUrl":"https://synthetic.invalid/v1", "apiFormat":"openai_chat","apiKey":"synthetic-key", "models":[{"model":"exact", "modelPreferences":{"images":false,"reasoningLevels":[]}}]}
        })).expect("synthetic record");
        assert!(record.validate().is_ok());
        if let UpstreamSource::Api { models, .. } = &mut record.source {
            models[0]["modelPreferences"]["images"] = json!("invalid");
        }
        assert_eq!(record.validate(), Err(INVALID.into()));
    }
}
