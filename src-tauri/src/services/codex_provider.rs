//! Subscription editor policy. Credentials remain in their original account store.
use crate::{
    provider::{AuthBinding, AuthBindingSource, Provider},
    services::routing_upstreams::INVALID,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(crate) const POLICY: &str = "routingCodexPolicy";
pub(crate) const CATALOGS: &str = "routingCodexCatalogs";

pub(crate) async fn probe_model(request: reqwest::RequestBuilder, model: &str) -> bool {
    let run = async {
        let mut response = request.json(&json!({
            "model": model, "instructions": "Reply with exactly VARYAI_PROTOCOL_OK.",
            "input": [{"role":"user","content":[{"type":"input_text","text":"Reply with exactly VARYAI_PROTOCOL_OK."}]}],
            "store": false, "stream": true,
        })).send().await.ok()?;
        if !response.status().is_success()
            || !response
                .headers()
                .get("content-type")
                .and_then(|h| h.to_str().ok())
                .is_some_and(|v| v.starts_with("text/event-stream"))
        {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len() + chunk.len() > 256 * 1024 {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        std::str::from_utf8(&bytes).ok().map(|body| {
            crate::services::provider_protocol_detection::valid_stream("openai_responses", body)
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(30), run)
        .await
        .ok()
        .flatten()
        == Some(true)
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelMode {
    #[default]
    Vendor,
    Picked,
    Best,
    Usage,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    pub accounts: Vec<String>,
    pub mode: ModelMode,
    pub models: Vec<Value>,
    pub groups_only: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            accounts: vec!["native".into()],
            mode: ModelMode::Vendor,
            models: vec![],
            groups_only: false,
        }
    }
}
impl Settings {
    pub(crate) fn valid(&self) -> bool {
        let mut accounts = std::collections::HashSet::new();
        let mut models = std::collections::HashSet::new();
        self.accounts.len() <= 32
            && self.models.len() <= 256
            && self.accounts.iter().all(|id| {
                crate::services::routing_upstreams::valid_identity(id) && accounts.insert(id)
            })
            && self.models.iter().all(|row| {
                row.get("model").and_then(Value::as_str).is_some_and(|id| {
                    !id.is_empty()
                        && id.len() <= 200
                        && id == id.trim()
                        && !id.chars().any(char::is_control)
                        && models.insert(id)
                }) && crate::services::intent_routing::model_preferences::valid(row)
            })
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 256 * 1024)
    }
    pub(crate) fn from_provider(provider: &Provider) -> Self {
        if let Some(value) = provider.settings_config.get(POLICY) {
            if let Ok(policy) = serde_json::from_value::<Self>(value.clone()) {
                if policy.valid() {
                    return policy;
                }
            }
        }
        let account = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("codex_oauth"));
        let models = provider
            .settings_config
            .pointer("/modelCatalog/models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Self {
            accounts: vec![account.unwrap_or_else(|| "native".into())],
            mode: if models.is_empty() {
                ModelMode::Vendor
            } else {
                ModelMode::Picked
            },
            models,
            groups_only: false,
        }
    }
}

pub(crate) fn is_codex(provider: &Provider) -> bool {
    crate::services::intent_routing::catalog::official_source(provider)
        && !crate::proxy::providers::has_explicit_codex_third_party_upstream(provider)
        && (provider.is_codex_oauth()
            || provider
                .meta
                .as_ref()
                .and_then(|m| m.managed_account_id_for("codex_oauth"))
                .is_some()
            || provider
                .settings_config
                .pointer("/auth/OPENAI_API_KEY")
                .and_then(Value::as_str)
                .is_none_or(|key| key.trim().is_empty()))
}

pub(crate) fn bind(provider: &mut Provider, account: &str) {
    let meta = provider.meta.get_or_insert_with(Default::default);
    if account == "native" {
        meta.auth_binding = None;
        meta.provider_type = None;
        provider.category = Some("official".into());
    } else {
        meta.auth_binding = Some(AuthBinding {
            source: AuthBindingSource::ManagedAccount,
            auth_provider: Some("codex_oauth".into()),
            account_id: Some(account.into()),
        });
        meta.provider_type = Some("codex_oauth".into());
    }
}

pub(crate) fn variants(provider: &Provider) -> Vec<Provider> {
    if !is_codex(provider) || provider.settings_config.get(POLICY).is_none() {
        return vec![provider.clone()];
    }
    let policy = Settings::from_provider(provider);
    if policy.mode == ModelMode::Usage {
        return vec![];
    }
    policy
        .accounts
        .iter()
        .map(|id| {
            let mut p = provider.clone();
            bind(&mut p, id);
            if let Some(models) = provider.settings_config[CATALOGS].get(id) {
                p.settings_config["modelCatalog"] = json!({"models": models});
            }
            p
        })
        .collect()
}

pub(crate) fn account_serves(provider: &Provider, model: &str) -> bool {
    let policy = Settings::from_provider(provider);
    let account = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("codex_oauth"))
        .unwrap_or_else(|| "native".into());
    provider.settings_config[CATALOGS]
        .get(&account)
        .and_then(Value::as_array)
        .is_none_or(|rows| rows.iter().any(|row| row["model"].as_str() == Some(model)))
        && (policy.mode != ModelMode::Usage)
}

pub(crate) fn serves(provider: &Provider, model: &str) -> bool {
    if provider.settings_config.get(POLICY).is_none() {
        return true;
    }
    let policy = Settings::from_provider(provider);
    policy.mode != ModelMode::Usage
        && !policy.accounts.is_empty()
        && (policy.mode != ModelMode::Picked
            || policy
                .models
                .iter()
                .any(|row| row["model"].as_str() == Some(model)))
}

pub(crate) fn attach(provider: &mut Provider, policy: &Settings) -> Result<(), String> {
    if !is_codex(provider) || !policy.valid() {
        return Err(INVALID.into());
    }
    provider.settings_config[POLICY] = serde_json::to_value(policy).map_err(|_| INVALID)?;
    if let Some(first) = policy.accounts.first() {
        bind(provider, first);
    }
    Ok(())
}

pub(crate) fn filter_models(provider: &mut Provider) {
    if provider.settings_config.get(POLICY).is_none() {
        return;
    }
    let policy = Settings::from_provider(provider);
    let available = provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let chat_models: Vec<_> = available
        .iter()
        .filter(|row| !crate::model_capabilities::is_image_generation_model(row))
        .cloned()
        .collect();
    let mut models = match policy.mode {
        ModelMode::Usage => vec![],
        ModelMode::Picked => policy
            .models
            .iter()
            .filter(|picked| {
                provider.settings_config.get(CATALOGS).is_none()
                    || available.iter().any(|row| row["model"] == picked["model"])
            })
            .map(|picked| {
                let mut row = available
                    .iter()
                    .find(|row| row["model"] == picked["model"])
                    .cloned()
                    .unwrap_or_else(|| picked.clone());
                if let Some(prefs) = picked.get("modelPreferences") {
                    row["modelPreferences"] = prefs.clone();
                }
                if let Some(level) =
                    crate::services::intent_routing::effort::Capability::parse(picked).default_level
                {
                    row["defaultReasoningLevel"] = json!(level);
                }
                row
            })
            .collect(),
        ModelMode::Vendor => chat_models.into_iter().take(24).collect(),
        ModelMode::Best => best_models(chat_models),
    };
    for row in &mut models {
        if let Some(prefs) = policy
            .models
            .iter()
            .find(|p| p["model"] == row["model"])
            .and_then(|p| p.get("modelPreferences"))
        {
            row["modelPreferences"] = prefs.clone();
        }
    }
    // A legacy default model must not reappear after deselection.
    models.retain(|row| !crate::model_capabilities::is_image_generation_model(row));
    provider.settings_config["config"] = json!("");
    provider.settings_config["modelCatalog"] = json!({"models": models});
}

// Adapted from the reference implementation internal/catalog/family.go (MIT).
#[derive(Default)]
struct Family {
    name: String,
    version: Vec<u64>,
    snapshot: String,
    preview: bool,
}
fn family(id: &str) -> Family {
    use std::sync::OnceLock;
    static SEP: OnceLock<Option<regex::Regex>> = OnceLock::new();
    static NUM: OnceLock<Option<regex::Regex>> = OnceLock::new();
    static SIZE: OnceLock<Option<regex::Regex>> = OnceLock::new();
    static TAIL: OnceLock<Option<regex::Regex>> = OnceLock::new();
    static HEAD: OnceLock<Option<regex::Regex>> = OnceLock::new();
    let Some(sep) = SEP.get_or_init(|| regex::Regex::new(r"[-_/:@\[\]\s]+").ok()) else {
        return Family {
            name: id.into(),
            ..Default::default()
        };
    };
    let Some(num) = NUM.get_or_init(|| regex::Regex::new(r"^\d+(?:\.\d+)*$").ok()) else {
        return Family::default();
    };
    let Some(size) = SIZE.get_or_init(|| {
        regex::Regex::new(r"^(?:[a-z]?\d+(?:\.\d+)?|\d+x\d+(?:\.\d+)?)[bmkte]$").ok()
    }) else {
        return Family::default();
    };
    let Some(tail) =
        TAIL.get_or_init(|| regex::Regex::new(r"^([a-z][a-z.]*?)(\d+(?:\.\d+)*)$").ok())
    else {
        return Family::default();
    };
    let Some(head) = HEAD.get_or_init(|| regex::Regex::new(r"^(\d+(?:\.\d+)*)([a-z]+)$").ok())
    else {
        return Family::default();
    };
    let lower = id.trim().to_lowercase();
    let words = sep.split(&lower).collect::<Vec<_>>();
    let seps = sep
        .find_iter(&lower)
        .map(|s| s.as_str())
        .collect::<Vec<_>>();
    let mut snaps = words
        .iter()
        .map(|w| {
            num.is_match(w)
                && !w.contains('.')
                && (w.len() >= 4 || (w.len() >= 2 && w.starts_with('0')))
        })
        .collect::<Vec<_>>();
    for i in 0..words.len() {
        if !snaps[i]
            && words[i].len() == 2
            && num.is_match(words[i])
            && ((i > 0 && snaps[i - 1])
                || words
                    .get(i + 1)
                    .is_some_and(|w| w.len() == 4 && (w.starts_with("19") || w.starts_with("20"))))
        {
            snaps[i] = true;
        }
    }
    let mut f = Family::default();
    for (i, word) in words.iter().enumerate() {
        let mut keep = None;
        let mut version = None;
        if word.is_empty() {
            continue;
        }
        if snaps[i] {
            if !f.snapshot.is_empty() {
                f.snapshot.push('-');
            }
            f.snapshot.push_str(word);
            continue;
        }
        match *word {
            "preview" | "exp" | "experimental" | "beta" | "alpha" => {
                f.preview = true;
                continue;
            }
            "latest" => continue,
            _ => {}
        }
        if num.is_match(word) {
            version = Some((*word).to_owned());
        } else if size.is_match(word) {
            keep = Some((*word).to_owned());
        } else if let Some(c) = tail.captures(word) {
            if &c[1] != "v" {
                keep = Some(c[1].to_owned());
            }
            version = Some(c[2].to_owned());
        } else if let Some(c) = head.captures(word) {
            version = Some(c[1].to_owned());
            keep = Some(c[2].to_owned());
        } else {
            keep = Some((*word).to_owned());
        }
        if let Some(v) = version {
            f.version
                .extend(v.split('.').map(|n| n.parse::<u64>().unwrap_or(0)));
        }
        if let Some(w) = keep {
            if !f.name.is_empty() && i > 0 {
                f.name.push_str(seps[i - 1]);
            }
            f.name.push_str(&w);
        }
    }
    if f.name.is_empty() {
        f.name = lower;
    }
    f
}
pub(crate) fn best_models(rows: Vec<Value>) -> Vec<Value> {
    let families = rows
        .iter()
        .map(|row| family(row["model"].as_str().unwrap_or("")))
        .collect::<Vec<_>>();
    let mut best = std::collections::HashMap::<&str, usize>::new();
    for (i, f) in families.iter().enumerate() {
        let newer = best.get(f.name.as_str()).is_none_or(|j| {
            let old = &families[*j];
            (
                &f.version,
                !f.preview,
                f.snapshot.is_empty(),
                rows[i]["released"].as_str().unwrap_or(""),
                &f.snapshot,
            ) > (
                &old.version,
                !old.preview,
                old.snapshot.is_empty(),
                rows[*j]["released"].as_str().unwrap_or(""),
                &old.snapshot,
            )
        });
        if newer {
            best.insert(&f.name, i);
        }
    }
    rows.iter()
        .enumerate()
        .filter(|(i, _)| best.get(families[*i].name.as_str()) == Some(i))
        .take(24)
        .map(|(_, row)| row.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_models_are_not_offered_as_codex_chat_models() {
        let rows = json!([{"model":"gpt-image-2"}, {"model":"chat-fixture", "inputModalities":["text","image"]}]);
        for mode in ["vendor", "picked", "best", "usage"] {
            let mut provider = Provider::with_id(
                "fixture".into(),
                "Fixture".into(),
                json!({"modelCatalog":{"models":rows}}),
                None,
            );
            provider.settings_config[POLICY] =
                json!({"mode":mode,"models":rows,"accounts":["fixture"],"groupsOnly":false});
            assert_eq!(
                serde_json::to_value(Settings::from_provider(&provider).mode).unwrap_or_default(),
                mode
            );
            filter_models(&mut provider);
            assert_eq!(
                provider.settings_config["modelCatalog"]["models"],
                if mode == "usage" {
                    json!([])
                } else {
                    json!([{"model":"chat-fixture", "inputModalities":["text","image"]}])
                },
                "{mode}"
            );
        }
    }
    #[tokio::test]
    async fn codex_model_probe_requires_verified_content_without_exposing_vendor_errors(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{response::IntoResponse, routing::post, Json, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, Router::new().route("/responses", post(|Json(body): Json<Value>| async move {
                assert_eq!(body["store"], false);
                assert_eq!(body["stream"], true);
                assert!(body.get("max_output_tokens").is_none());
                if body["model"] == "bad" {
                    return (axum::http::StatusCode::UNAUTHORIZED, "synthetic-private-vendor-error").into_response();
                }
                ([ ("content-type", "text/event-stream") ], "data: {\"type\":\"response.output_text.delta\",\"delta\":\"VARYAI_PROTOCOL_OK\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n").into_response()
            }))).await;
        });
        let client = reqwest::Client::builder().no_proxy().build()?;
        assert!(probe_model(client.post(format!("http://{address}/responses")), "good").await);
        assert!(!probe_model(client.post(format!("http://{address}/responses")), "bad").await);
        task.abort();
        Ok(())
    }

    #[test]
    fn codex_account_catalogs_prevent_cross_identity_model_support() {
        let mut p = Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":[{"model":"only-one"},{"model":"only-two"}]}}),
            None,
        );
        p.category = Some("official".into());
        attach(
            &mut p,
            &Settings {
                accounts: vec!["one".into(), "two".into()],
                ..Default::default()
            },
        )
        .expect("policy");
        p.settings_config[CATALOGS] =
            json!({"one":[{"model":"only-one"}],"two":[{"model":"only-two"}]});
        let pool = variants(&p);
        assert!(account_serves(&pool[0], "only-one"));
        assert!(!account_serves(&pool[0], "only-two"));
        assert!(account_serves(&pool[1], "only-two"));
        assert!(!account_serves(&pool[1], "only-one"));
    }
    #[test]
    fn settings_filter_exact_models_and_preserve_auth() {
        let mut p = Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({"auth":{"untouched":"synthetic-secret"},"modelCatalog":{"models":[{"model":"model-a"},{"model":"model-b"}]},"config":"model = 'model-b'"}),
            None,
        );
        p.category = Some("official".into());
        let policy = Settings {
            mode: ModelMode::Picked,
            models: vec![json!({"model":"model-a","defaultReasoningLevel":"low"})],
            ..Default::default()
        };
        attach(&mut p, &policy).expect("attach");
        filter_models(&mut p);
        assert_eq!(
            p.settings_config["modelCatalog"]["models"],
            json!([{"model":"model-a","defaultReasoningLevel":"low"}])
        );
        assert_eq!(p.settings_config["auth"]["untouched"], "synthetic-secret");
        assert_eq!(variants(&p).len(), 1);
        let usage = Settings {
            mode: ModelMode::Usage,
            ..policy
        };
        attach(&mut p, &usage).expect("usage");
        filter_models(&mut p);
        assert!(variants(&p).is_empty());
    }
    #[test]
    fn best_is_natural_and_settings_are_bounded() {
        assert_eq!(
            best_models(vec![
                json!({"model":"model-9"}),
                json!({"model":"model-10"})
            ]),
            vec![json!({"model":"model-10"})]
        );
        assert!(!Settings {
            accounts: vec!["native".into(), "native".into()],
            ..Default::default()
        }
        .valid());
    }

    #[test]
    fn model_families_keep_sizes_and_prefer_release_aliases_over_snapshots() {
        let rows = vec![
            json!({"model":"qwen3-8b"}),
            json!({"model":"qwen3-32b"}),
            json!({"model":"gpt-6.1-preview"}),
            json!({"model":"gpt-6.1-20261001"}),
            json!({"model":"gpt-6.1"}),
            json!({"model":"o3-mini"}),
            json!({"model":"o4-mini"}),
        ];
        let ids = best_models(rows)
            .into_iter()
            .map(|r| r["model"].as_str().unwrap_or_default().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["qwen3-8b", "qwen3-32b", "gpt-6.1", "o4-mini"]);
    }

    #[test]
    fn codex_editor_policy_round_trips_in_extension_storage_without_editing_the_source(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::{database::Database, services::routing_upstreams::UpstreamSource};
        let db = Database::memory()?;
        let mut p = Provider::with_id(
            "source".into(),
            "Synthetic".into(),
            json!({"auth":{"untouched":"synthetic-credential"},"modelCatalog":{"models":[{"model":"model-a"},{"model":"model-b"}]}}),
            None,
        );
        p.category = Some("official".into());
        db.save_provider("codex", &p)?;
        let mut record = db.register_routing_fixture("codex", "source")?;
        record.codex_settings = Some(Settings {
            mode: ModelMode::Picked,
            models: vec![json!({"model":"model-a"})],
            ..Default::default()
        });
        let saved = db.save_routing_upstream(record)?;
        assert!(matches!(saved.source, UpstreamSource::Codex));
        assert_eq!(
            db.get_provider_by_id("source", "codex")?
                .ok_or("source")?
                .settings_config,
            p.settings_config
        );
        let resolved = db.routing_discovery_provider(&saved.id, saved.revision)?;
        assert!(serves(&resolved, "model-a"));
        assert!(!serves(&resolved, "model-b"));
        assert!(!serde_json::to_string(&saved)?.contains("synthetic-credential"));
        let mut stale = saved.clone();
        stale.revision -= 1;
        assert!(db.save_routing_upstream(stale).is_err());
        Ok(())
    }
}
