//! Public model facts are refreshed off the forwarding path and never select an account.
use super::context::Metadata;
use crate::provider::Provider;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[cfg(not(test))]
const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
struct Catalog {
    fetched_at: i64,
    windows: BTreeMap<String, u64>,
    #[serde(default)]
    facts: BTreeMap<String, TemplateFacts>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TemplateFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    output_price: Option<f64>,
    maker: bool,
    released: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<bool>,
}

fn released(value: &Value) -> String {
    value
        .as_str()
        .filter(|s| s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok())
        .unwrap_or_default()
        .into()
}

fn parse(value: &Value) -> Catalog {
    let mut counts: BTreeMap<String, BTreeMap<u64, usize>> = BTreeMap::new();
    let mut facts = BTreeMap::new();
    for (pid, provider) in value.as_object().into_iter().flatten() {
        for (id, model) in provider
            .get("models")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let maker = model["canonical_model_id"]
                .as_str()
                .and_then(|id| id.split_once('/'))
                .map(|(maker, _)| maker.to_ascii_lowercase());
            let own = maker.is_some_and(|maker| {
                let pid = pid.to_ascii_lowercase();
                (pid == maker || pid.starts_with(&format!("{maker}-"))) && !pid.contains("-plan")
            });
            let output = model["cost"]["output"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0);
            if own && output.is_some_and(|n| n > 0.0) && id.len() <= 256 {
                let fact = TemplateFacts {
                    output_price: output,
                    maker: true,
                    released: released(&model["release_date"]),
                    images: model["modalities"]["input"]
                        .as_array()
                        .filter(|m| !m.is_empty())
                        .map(|m| m.iter().any(|v| v.as_str() == Some("image"))),
                    reasoning: model["reasoning"].as_bool(),
                };
                for name in [Some(id.as_str()), model["canonical_model_id"].as_str()]
                    .into_iter()
                    .flatten()
                {
                    let bare = name.rsplit('/').next().unwrap_or(name);
                    facts.entry(bare.to_owned()).or_insert_with(|| fact.clone());
                }
            }
            let limit = &model["limit"];
            let input = Metadata::parse(&json!({"context_window": limit["input"]})).context_window;
            let context =
                Metadata::parse(&json!({"context_window": limit["context"]})).context_window;
            let window = match (input, context) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            if let Some(n) = window.filter(|_| id.len() <= 256) {
                let bare = id.rsplit('/').next().unwrap_or(id);
                *counts
                    .entry(bare.to_owned())
                    .or_default()
                    .entry(n)
                    .or_default() += 1;
            }
        }
    }
    Catalog {
        fetched_at: chrono::Utc::now().timestamp(),
        facts,
        windows: counts
            .into_iter()
            .filter_map(|(id, counts)| {
                counts
                    .into_iter()
                    .max_by_key(|(n, count)| (*count, *n))
                    .map(|(n, _)| (id, n))
            })
            .collect(),
    }
}

impl Catalog {
    fn facts(&self, model: &str) -> Option<&TemplateFacts> {
        let bare = model.rsplit('/').next().unwrap_or(model);
        self.facts.get(bare).or_else(|| {
            let normalized = super::discovery::same_model(bare);
            self.facts
                .iter()
                .find(|(id, _)| super::discovery::same_model(id) == normalized)
                .map(|(_, facts)| facts)
        })
    }
    fn window(&self, model: &str) -> Option<u64> {
        self.windows.get(model).copied().or_else(|| {
            let leaf = model.rsplit('/').next()?;
            self.windows.get(leaf).copied().or_else(|| {
                for (open, close) in [('(', ')'), ('[', ']')] {
                    if let Some((base, suffix)) =
                        leaf.strip_suffix(close).and_then(|s| s.rsplit_once(open))
                    {
                        let base_window = self.windows.get(base).copied();
                        if base_window.is_some() {
                            let suffix = suffix.to_ascii_lowercase();
                            if let Some(n) = suffix
                                .strip_suffix('m')
                                .and_then(|s| s.parse::<u64>().ok())
                                .and_then(|n| n.checked_mul(1_000_000))
                                .or_else(|| {
                                    suffix
                                        .strip_suffix('k')
                                        .and_then(|s| s.parse::<u64>().ok())
                                        .and_then(|n| n.checked_mul(1000))
                                })
                            {
                                return Metadata::parse(&json!({"context_window":n}))
                                    .context_window;
                            }
                            if matches!(
                                suffix.as_str(),
                                "none"
                                    | "minimal"
                                    | "low"
                                    | "medium"
                                    | "high"
                                    | "xhigh"
                                    | "max"
                                    | "ultra"
                            ) {
                                return base_window;
                            }
                        }
                    }
                }
                let normalized = super::discovery::same_model(leaf);
                self.windows
                    .iter()
                    .filter(|(id, _)| super::discovery::same_model(id) == normalized)
                    .map(|(_, n)| *n)
                    .min()
            })
        })
    }
}

#[cfg(not(test))]
async fn catalog(refresh: bool) -> Catalog {
    let path = crate::config::get_app_config_dir().join("routing-model-context.json");
    let read_path = path.clone();
    let cached = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let file = std::fs::File::open(read_path).ok()?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > MAX_BYTES {
            return None;
        }
        let cached: Catalog = serde_json::from_slice(&bytes).ok()?;
        if cached.windows.len() > 100_000
            || cached.facts.len() > 100_000
            || cached.facts.iter().any(|(id, fact)| {
                id.len() > 256
                    || id.chars().any(char::is_control)
                    || fact.output_price.is_some_and(|n| !n.is_finite() || n < 0.0)
                    || (!fact.released.is_empty() && released(&json!(fact.released)).is_empty())
            })
            || cached.windows.iter().any(|(id, n)| {
                id.len() > 256
                    || Metadata::parse(&json!({"context_window":n}))
                        .context_window
                        .is_none()
            })
        {
            return None;
        }
        Some(cached)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let age = chrono::Utc::now()
        .timestamp()
        .saturating_sub(cached.fetched_at);
    if !refresh || (0..86400).contains(&age) {
        return cached;
    }
    let fetch = async {
        let client = crate::proxy::http_client::get_without_redirects().ok()?;
        let mut response = client
            .get("https://models.dev/api.json")
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice::<Value>(&bytes).ok()?;
        let catalog = parse(&value);
        (!catalog.windows.is_empty()).then_some(catalog)
    };
    let Some(updated) = tokio::time::timeout(std::time::Duration::from_secs(20), fetch)
        .await
        .ok()
        .flatten()
    else {
        return cached;
    };
    if let Ok(bytes) = serde_json::to_vec(&updated) {
        let _ =
            tokio::task::spawn_blocking(move || crate::config::atomic_write_private(&path, &bytes))
                .await;
    }
    updated
}

fn enrich(providers: &mut [Provider], catalog: &Catalog) {
    for provider in providers {
        let facts = provider
            .settings_config
            .pointer("/modelCatalog/models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let id = entry["model"].as_str()?;
                let vendor = catalog.facts(id).or_else(|| {
                    super::model_preferences::same_as(entry).and_then(|id| catalog.facts(id))
                });
                let price = entry
                    .get("pricing")
                    .or_else(|| entry.get("price"))
                    .or_else(|| entry.get("cost"));
                let own_price = price
                    .and_then(|p| {
                        p["output"]
                            .as_f64()
                            .or_else(|| p["output"].as_str()?.parse::<f64>().ok())
                    })
                    .filter(|n| n.is_finite() && *n >= 0.0);
                let capability = super::effort::Capability::parse(entry);
                let fact = TemplateFacts {
                    output_price: vendor.and_then(|f| f.output_price).or(own_price),
                    maker: vendor.is_some_and(|f| f.maker),
                    released: vendor
                        .map(|f| f.released.clone())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| released(&entry["release_date"])),
                    images: capability.images.or_else(|| vendor.and_then(|f| f.images)),
                    reasoning: capability
                        .quiet
                        .then_some(false)
                        .or_else(|| entry["reasoning"].as_bool())
                        .or_else(|| (!capability.levels.is_empty()).then_some(!capability.quiet))
                        .or_else(|| vendor.and_then(|f| f.reasoning)),
                };
                Some((id.to_owned(), json!(fact)))
            })
            .collect::<serde_json::Map<_, _>>();
        provider.settings_config["routingModelFacts"] = json!(facts);
        // ChatGPT capacities are account/plan-scoped; API catalog guesses cannot enlarge them.
        if super::catalog::official_source(provider) {
            continue;
        }
        let mut models = provider
            .settings_config
            .pointer("/modelCatalog/models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| e["model"].as_str())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if let Some(model) = provider.settings_config["config"]
            .as_str()
            .and_then(|s| s.parse::<toml::Value>().ok())
            .and_then(|v| v.get("model")?.as_str().map(str::to_owned))
        {
            models.push(model);
        }
        let windows = models
            .iter()
            .filter_map(|model| {
                catalog
                    .window(model)
                    .or_else(|| {
                        provider
                            .settings_config
                            .pointer("/modelCatalog/models")?
                            .as_array()?
                            .iter()
                            .find(|entry| entry["model"].as_str() == Some(model))
                            .and_then(super::model_preferences::same_as)
                            .and_then(|id| catalog.window(id))
                    })
                    .map(|n| (model.clone(), json!({"context_window":n})))
            })
            .collect::<serde_json::Map<_, _>>();
        if !provider.settings_config["routingContext"].is_object() {
            provider.settings_config["routingContext"] = json!({});
        }
        provider.settings_config["routingContext"]["models"] = json!(windows);
    }
}

pub(super) async fn prepare(providers: &mut [Provider], refresh: bool) {
    #[cfg(test)]
    let catalog = {
        let _ = refresh;
        Catalog::default()
    };
    #[cfg(not(test))]
    let catalog = catalog(refresh).await;
    enrich(providers, &catalog);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_equivalence_fills_unknown_facts_without_overwriting_wire_ids_or_declared_limits() {
        let catalog = parse(
            &json!({"lab":{"models":{"canonical":{"canonical_model_id":"lab/canonical","cost":{"output":10},"limit":{"context":1000000},"reasoning":true,"modalities":{"input":["text","image"]}}}}}),
        );
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":[
                {"model":"relay-exact","modelPreferences":{"sameAs":"canonical"}},
                {"model":"relay-limited","contextWindow":128000,"modelPreferences":{"sameAs":"canonical","images":false,"reasoningLevels":[]}}
            ]}}),
            None,
        );
        enrich(std::slice::from_mut(&mut provider), &catalog);
        assert_eq!(
            provider.settings_config["modelCatalog"]["models"][0]["model"],
            "relay-exact"
        );
        assert_eq!(
            Metadata::for_provider(&provider.settings_config, "relay-exact").context_window,
            Some(1000000)
        );
        assert_eq!(
            Metadata::for_provider(&provider.settings_config, "relay-limited").context_window,
            Some(128000)
        );
        assert_eq!(
            provider.settings_config["routingModelFacts"]["relay-exact"]["images"],
            true
        );
        assert_eq!(
            provider.settings_config["routingModelFacts"]["relay-limited"]["images"],
            false
        );
        assert_eq!(
            provider.settings_config["routingModelFacts"]["relay-limited"]["reasoning"],
            false
        );
    }
    #[test]
    fn template_facts_choose_the_makers_price_without_using_a_relays_free_tier() {
        let catalog = parse(&json!({
            "relay":{"models":{"lab/large":{"canonical_model_id":"lab/large","cost":{"output":0},"release_date":"2026-01-01"}}},
            "lab":{"models":{"large":{"canonical_model_id":"lab/large","cost":{"input":3,"output":15},"release_date":"2026-01-01","reasoning":true,"modalities":{"input":["text","image"]}}}},
            "lab-plan":{"models":{"large":{"canonical_model_id":"lab/large","cost":{"output":0}}}}
        }));
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":[{"model":"lab/large","pricing":{"output":0}}]}}),
            None,
        );
        enrich(std::slice::from_mut(&mut provider), &catalog);
        let facts = &provider.settings_config["routingModelFacts"]["lab/large"];
        assert_eq!(facts["outputPrice"], 15.0);
        assert_eq!(facts["maker"], true);
        assert_eq!(facts["released"], "2026-01-01");
        assert_eq!(facts["images"], true);
        assert_eq!(facts["reasoning"], true);
        assert!(facts.get("canonical_model_id").is_none());
    }
    #[test]
    fn public_catalog_uses_input_bounded_by_context_and_fills_only_missing_facts() {
        let catalog = parse(&json!({"fixture":{"models":{
            "model-a":{"limit":{"input":922000,"context":1000000}},
            "model-b":{"limit":{"input":272000,"context":128000}},
            "invalid":{"limit":{"context":512}}
        }}}));
        assert_eq!(catalog.window("vendor/model-a"), Some(922000));
        assert_eq!(catalog.window("vendor/model-a(high)"), Some(922000));
        assert_eq!(catalog.window("model-a[1m]"), Some(1000000));
        assert_eq!(catalog.window("unheard-of[1m]"), None);
        assert_eq!(catalog.window("model-b"), Some(128000));
        assert_eq!(catalog.window("invalid"), None);
        assert_eq!(catalog.window("unheard-of"), None);
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({"modelCatalog":{"models":[{"model":"model-a","contextWindow":500000},{"model":"model-b"}]}}),
            None,
        );
        enrich(std::slice::from_mut(&mut provider), &catalog);
        assert_eq!(
            Metadata::for_provider(&provider.settings_config, "model-a").context_window,
            Some(500000)
        );
        assert_eq!(
            Metadata::for_provider(&provider.settings_config, "model-b").context_window,
            Some(128000)
        );
        assert!(provider.settings_config["modelCatalog"]["models"][1]
            .get("contextWindow")
            .is_none());
    }
}
