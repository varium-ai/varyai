use crate::provider::Provider;
use http::{HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Protocol {
    Chat,
    Responses,
    Anthropic,
    Gemini,
}
impl Protocol {
    pub fn parse(api: &str) -> Option<Self> {
        match api {
            "openai-completions" => Some(Self::Chat),
            "openai-responses" => Some(Self::Responses),
            "anthropic-messages" => Some(Self::Anthropic),
            "google-generative-ai" => Some(Self::Gemini),
            _ => None,
        }
    }
    pub fn api(self) -> &'static str {
        match self {
            Self::Chat => "openai-completions",
            Self::Responses => "openai-responses",
            Self::Anthropic => "anthropic-messages",
            Self::Gemini => "google-generative-ai",
        }
    }
    pub fn local_base(self, origin: &str) -> String {
        format!(
            "{}/pi{}",
            origin.trim_end_matches('/'),
            match self {
                Self::Chat | Self::Responses => "/v1",
                Self::Gemini => "/v1beta",
                Self::Anthropic => "",
            }
        )
    }
}

// Deliberately no Debug or Serialize: target headers contain upstream credentials.
pub(crate) struct Target {
    pub provider_id: String,
    pub model_id: String,
    pub protocol: Protocol,
    pub base_url: Url,
    pub headers: HeaderMap,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Issue {
    pub provider_id: String,
    pub model_id: Option<String>,
    pub code: String,
}

pub(crate) struct Catalog {
    pub targets: BTreeMap<String, Target>,
    pub node: Value,
    pub providers: BTreeMap<String, Value>,
    pub issues: Vec<Issue>,
}

pub(crate) fn alias(provider: &str, model: &str) -> String {
    let mut digest = Sha256::new();
    digest.update((provider.len() as u64).to_be_bytes());
    digest.update(provider.as_bytes());
    digest.update(model.as_bytes());
    let hint: String = model
        .chars()
        .take(96)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("vary_{:x}_{hint}", digest.finalize())
}

// Custom local URLs hide vendor detection from Pi. Materialize the public Chat
// compatibility fields using the original identity, then apply explicit overrides.
fn chat_compat(provider: &str, base: &Url, model: &str) -> serde_json::Map<String, Value> {
    let url = base.as_str().to_ascii_lowercase();
    let has = |id: &str, domain: &str| provider == id || url.contains(domain);
    let deepseek = has("deepseek", "deepseek.com");
    let zai = has("zai", "api.z.ai")
        || has("zai-coding-cn", "open.bigmodel.cn")
        || url.contains("api.zhipu.ai");
    let moonshot = has("moonshotai", "api.moonshot.") || provider == "moonshotai-cn";
    let together = has("together", "api.together.ai") || url.contains("api.together.xyz");
    let cloudflare_gateway = has("cloudflare-ai-gateway", "gateway.ai.cloudflare.com");
    let cloudflare = has("cloudflare-workers-ai", "api.cloudflare.com");
    let nvidia = has("nvidia", "integrate.api.nvidia.com");
    let ling = has("ant-ling", "api.ant-ling.com");
    let grok = has("xai", "api.x.ai");
    let openrouter = has("openrouter", "openrouter.ai");
    let chutes = url.contains("chutes.ai");
    let nonstandard = nvidia
        || has("cerebras", "cerebras.ai")
        || grok
        || together
        || chutes
        || deepseek
        || zai
        || moonshot
        || has("opencode", "opencode.ai")
        || cloudflare
        || cloudflare_gateway
        || ling;
    let max_tokens =
        chutes || deepseek || moonshot || cloudflare_gateway || together || nvidia || ling || zai;
    let mut compat = json!({
        "supportsStore":!nonstandard,
        "supportsDeveloperRole":(openrouter && (model.starts_with("anthropic/") || model.starts_with("openai/"))) || (!nonstandard && !openrouter),
        "supportsReasoningEffort":!(grok || zai || moonshot || together || cloudflare_gateway || nvidia || ling),
        "maxTokensField":if max_tokens {"max_tokens"} else {"max_completion_tokens"},
        "requiresReasoningContentOnAssistantMessages":deepseek,
        "thinkingFormat":if deepseek {"deepseek"} else if zai {"zai"} else if together {"together"} else if ling {"ant-ling"} else if openrouter {"openrouter"} else {"openai"},
        "sendSessionAffinityHeaders":openrouter,
        "sessionAffinityFormat":if openrouter {"openrouter"} else {"openai"},
        "supportsLongCacheRetention":!(together || cloudflare || cloudflare_gateway || nvidia || ling)
    }).as_object().cloned().unwrap_or_default();
    if provider == "openrouter" && model.starts_with("anthropic/") {
        compat.insert("cacheControlFormat".into(), json!("anthropic"));
    }
    compat
}

fn static_value(value: &str) -> bool {
    !value.trim_start().starts_with(['!', '$'])
        && !value.contains("${")
        && !(value.len() > 2
            && value
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
}

pub(crate) fn is_local_projection(config: &Value) -> bool {
    let local_url = |node: &Value| {
        node.get("baseUrl")
            .and_then(Value::as_str)
            .and_then(|s| Url::parse(s).ok())
            .is_some_and(|u| {
                matches!(u.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
                    && (u.path() == "/pi" || u.path().starts_with("/pi/"))
            })
    };
    local_url(config)
        || config
            .get("models")
            .and_then(Value::as_array)
            .is_some_and(|models| models.iter().any(local_url))
}

impl Catalog {
    pub fn build(
        sources: &[Provider],
        selected: &[String],
        origin: &str,
        token: &str,
    ) -> Result<Self, String> {
        let local = Url::parse(origin).map_err(|_| "piRouting.invalidListener")?;
        if local.scheme() != "http"
            || !matches!(local.host_str(), Some("127.0.0.1" | "[::1]"))
            || local.port().is_none()
        {
            return Err("piRouting.invalidListener".into());
        }
        let mut targets = BTreeMap::new();
        let mut models = Vec::new();
        let mut issues = Vec::new();
        for p in sources.iter().filter(|p| selected.contains(&p.id)) {
            let config = &p.settings_config;
            if p.id != "vary" && is_local_projection(config) {
                issues.push(Issue {
                    provider_id: p.id.clone(),
                    model_id: None,
                    code: "piRouting.sourceRecoveryRequired".into(),
                });
                continue;
            }
            if p.id == "vary"
                || config.get("modelOverrides").is_some()
                || config.get("oauth").is_some()
                || is_local_projection(config)
            {
                issues.push(Issue {
                    provider_id: p.id.clone(),
                    model_id: None,
                    code: "piRouting.unsupportedConfig".into(),
                });
                continue;
            }
            let Some(entries) = config
                .get("models")
                .and_then(Value::as_array)
                .filter(|m| !m.is_empty())
            else {
                issues.push(Issue {
                    provider_id: p.id.clone(),
                    model_id: None,
                    code: "piRouting.noModels".into(),
                });
                continue;
            };
            for m in entries {
                let model_id = m
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty());
                let result = (|| -> Result<(String, Target, Value), &'static str> {
                    let id = model_id.ok_or("piRouting.invalidModel")?;
                    let api = m
                        .get("api")
                        .or_else(|| config.get("api"))
                        .and_then(Value::as_str)
                        .ok_or("piRouting.unsupportedProtocol")?;
                    let protocol = Protocol::parse(api).ok_or("piRouting.unsupportedProtocol")?;
                    let url = m
                        .get("baseUrl")
                        .or_else(|| config.get("baseUrl"))
                        .and_then(Value::as_str)
                        .ok_or("piRouting.invalidEndpoint")?;
                    let base = Url::parse(url).map_err(|_| "piRouting.invalidEndpoint")?;
                    let loopback =
                        matches!(base.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"));
                    if !matches!(base.scheme(), "http" | "https")
                        || base.host_str().is_none()
                        || !base.username().is_empty()
                        || base.password().is_some()
                        || base.fragment().is_some()
                        || (loopback
                            && (base.port_or_known_default() == local.port_or_known_default()
                                || base.path() == "/pi"
                                || base.path().starts_with("/pi/")))
                    {
                        return Err("piRouting.invalidEndpoint");
                    }
                    let key = m
                        .get("apiKey")
                        .or_else(|| config.get("apiKey"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !static_value(key) {
                        return Err("piRouting.dynamicCredentials");
                    }
                    let mut headers = HeaderMap::new();
                    if !key.is_empty() {
                        let (name, value) = match protocol {
                            Protocol::Anthropic => ("x-api-key", key.to_string()),
                            Protocol::Gemini => ("x-goog-api-key", key.to_string()),
                            _ => ("authorization", format!("Bearer {key}")),
                        };
                        headers.insert(
                            HeaderName::from_static(name),
                            HeaderValue::from_str(&value)
                                .map_err(|_| "piRouting.invalidCredentials")?,
                        );
                    }
                    if config.get("authHeader").and_then(Value::as_bool) == Some(true)
                        && !key.is_empty()
                    {
                        headers.insert(
                            "authorization",
                            HeaderValue::from_str(&format!("Bearer {key}"))
                                .map_err(|_| "piRouting.invalidCredentials")?,
                        );
                    }
                    for value in [config.get("headers"), m.get("headers")]
                        .into_iter()
                        .flatten()
                    {
                        let object = value.as_object().ok_or("piRouting.unsupportedConfig")?;
                        for (name, raw) in object {
                            let text = raw
                                .as_str()
                                .filter(|s| static_value(s))
                                .ok_or("piRouting.dynamicCredentials")?;
                            if name.to_ascii_lowercase().starts_with("x-vary-")
                                || matches!(
                                    name.to_ascii_lowercase().as_str(),
                                    "host" | "content-length" | "connection" | "transfer-encoding"
                                )
                            {
                                return Err("piRouting.unsupportedConfig");
                            }
                            headers.insert(
                                HeaderName::from_bytes(name.as_bytes())
                                    .map_err(|_| "piRouting.unsupportedConfig")?,
                                HeaderValue::from_str(text)
                                    .map_err(|_| "piRouting.unsupportedConfig")?,
                            );
                        }
                    }
                    if !loopback
                        && !["authorization", "x-api-key", "x-goog-api-key"]
                            .iter()
                            .any(|k| headers.contains_key(*k))
                    {
                        return Err("piRouting.missingCredentials");
                    }
                    let model_alias = alias(&p.id, id);
                    if targets.contains_key(&model_alias) {
                        return Err("piRouting.duplicateModel");
                    }
                    let allowed = [
                        "id",
                        "name",
                        "reasoning",
                        "thinkingLevelMap",
                        "input",
                        "inputLimits",
                        "cost",
                        "promptCache",
                        "contextWindow",
                        "maxTokens",
                        "samplingParams",
                        "samplingParamsByThinkingLevel",
                        "compat",
                    ];
                    let mut exported = m
                        .as_object()
                        .ok_or("piRouting.invalidModel")?
                        .iter()
                        .filter(|(k, _)| allowed.contains(&k.as_str()))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<serde_json::Map<_, _>>();
                    for name in ["apiKey", "headers", "baseUrl", "authHeader", "oauth"] {
                        exported.remove(name);
                    }
                    let mut compat = if protocol == Protocol::Chat {
                        chat_compat(&p.id, &base, id)
                    } else {
                        serde_json::Map::new()
                    };
                    if let Some(overrides) = config.get("compat").and_then(Value::as_object) {
                        compat.extend(overrides.clone());
                    }
                    if let Some(overrides) = m.get("compat").and_then(Value::as_object) {
                        compat.extend(overrides.clone());
                    }
                    if !compat.is_empty() {
                        exported.insert("compat".into(), Value::Object(compat));
                    }
                    exported.insert("id".into(), json!(model_alias));
                    exported.insert(
                        "name".into(),
                        json!(format!(
                            "{} · {}",
                            p.name,
                            m.get("name").and_then(Value::as_str).unwrap_or(id)
                        )),
                    );
                    exported.insert("api".into(), json!(protocol.api()));
                    exported.insert("baseUrl".into(), json!(protocol.local_base(origin)));
                    Ok((
                        model_alias,
                        Target {
                            provider_id: p.id.clone(),
                            model_id: id.into(),
                            protocol,
                            base_url: base,
                            headers,
                        },
                        Value::Object(exported),
                    ))
                })();
                match result {
                    Ok((id, target, model)) => {
                        targets.insert(id, target);
                        models.push(model);
                    }
                    Err(code) => issues.push(Issue {
                        provider_id: p.id.clone(),
                        model_id: model_id.map(str::to_string),
                        code: code.into(),
                    }),
                }
            }
        }
        // Keep native provider/model identities so existing selections and sessions
        // load the routed endpoint without introducing a second provider.
        let mut providers = BTreeMap::new();
        for p in sources.iter().filter(|p| selected.contains(&p.id)) {
            let mut exported_models = Vec::new();
            for m in &models {
                let Some(target) = m["id"].as_str().and_then(|id| targets.get(id)) else {
                    continue;
                };
                if target.provider_id != p.id {
                    continue;
                }
                let mut exported = m.clone();
                exported["id"] = json!(target.model_id);
                if let Some(original) = p.settings_config["models"]
                    .as_array()
                    .and_then(|ms| ms.iter().find(|v| v["id"] == target.model_id))
                {
                    if let Some(name) = original.get("name") {
                        exported["name"] = name.clone();
                    } else if let Some(obj) = exported.as_object_mut() {
                        obj.remove("name");
                    }
                }
                exported["baseUrl"] = json!(format!(
                    "{origin}/pi/p/{}{}",
                    alias(&p.id, ""),
                    target.protocol.local_base("").trim_start_matches("/pi")
                ));
                exported_models.push(exported);
            }
            if !exported_models.is_empty() {
                let mut node = json!({"api":"openai-completions", "baseUrl":format!("{origin}/pi/p/{}/v1", alias(&p.id, "")), "apiKey":token, "models":exported_models});
                if let Some(name) = p.settings_config.get("name") {
                    node["name"] = name.clone();
                }
                providers.insert(p.id.clone(), node);
            }
        }
        Ok(Self {
            targets,
            providers,
            issues,
            node: json!({"name":"Vary AI", "api":"openai-completions", "baseUrl":format!("{origin}/pi/v1"), "apiKey":token, "models":models}),
        })
    }
}
