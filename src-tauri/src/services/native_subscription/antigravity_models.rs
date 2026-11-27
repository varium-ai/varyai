//! Antigravity client version resolution, entitlement backoff, and model parsing.
//! Protocol reference: Magpie internal/provider/{google,antigravity_models}.go (MIT).

use super::{Account, FAILED};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, RwLock},
    time::{Duration, Instant},
};

pub(super) const DEFAULT_VERSION: &str = "2.35.2";
#[cfg(not(test))]
const VERSION_URL: &str = "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/manifest/latest-arm64-mac.yml";
const VERSION_SUCCESS_TTL: Duration = Duration::from_secs(6 * 3600);
const VERSION_FAILURE_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const TIER_CACHE_TTL: Duration = Duration::from_secs(3600);
const TIER_REFUSED_TTL: Duration = Duration::from_secs(24 * 3600);

const HIDDEN_MODELS: &[&str] = &[
    "chat_20706",
    "chat_23310",
    "tab_flash_lite_preview",
    "tab_jump_flash_lite_preview",
    "gemini-2.5-flash-thinking",
    "gemini-2.5-pro",
];

const EFFORT_WORDS: &[&str] = &[
    "extra-high",
    "extra-low",
    "xhigh",
    "minimal",
    "low",
    "medium",
    "high",
];

#[derive(Default)]
struct VersionCache {
    version: Option<String>,
    #[cfg(not(test))]
    checked: Option<Instant>,
    #[cfg(not(test))]
    succeeded: bool,
}

fn cache_entry_fresh(elapsed: Duration, succeeded: bool) -> bool {
    let ttl = if succeeded {
        VERSION_SUCCESS_TTL
    } else {
        VERSION_FAILURE_TTL
    };
    elapsed < ttl
}

#[cfg(not(test))]
fn version_cache_fresh(cache: &VersionCache) -> bool {
    cache
        .checked
        .is_some_and(|at| cache_entry_fresh(at.elapsed(), cache.succeeded))
}

fn version_cache() -> &'static RwLock<VersionCache> {
    static CACHE: OnceLock<RwLock<VersionCache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

pub(super) fn current_version() -> String {
    version_cache()
        .read()
        .ok()
        .and_then(|cache| cache.version.clone())
        .unwrap_or_else(|| DEFAULT_VERSION.to_owned())
}

pub(super) fn trigger_background_refresh() {
    #[cfg(not(test))]
    {
        let fresh = version_cache()
            .read()
            .ok()
            .is_some_and(|cache| version_cache_fresh(&cache));
        if !fresh && tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async {
                let _ = resolve_version().await;
            });
        }
    }
}

pub(super) fn user_agent(version: &str) -> String {
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        value => value,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        value => value,
    };
    format!("antigravity/hub/{version} {platform}/{arch}")
}

pub(super) async fn resolve_version() -> String {
    #[cfg(test)]
    {
        current_version()
    }
    #[cfg(not(test))]
    {
        resolve_version_from(VERSION_URL).await
    }
}

#[cfg(not(test))]
async fn resolve_version_from(url: &str) -> String {
    if version_cache()
        .read()
        .ok()
        .is_some_and(|cache| version_cache_fresh(&cache))
    {
        return current_version();
    }
    static REFRESH_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = REFRESH_GUARD.lock().await;
    if version_cache()
        .read()
        .ok()
        .is_some_and(|cache| version_cache_fresh(&cache))
    {
        return current_version();
    }
    let resolved = fetch_version_from(url).await;
    if let Ok(mut cache) = version_cache().write() {
        cache.checked = Some(Instant::now());
        cache.succeeded = resolved.is_some();
        if let Some(version) = resolved {
            cache.version = Some(version);
        }
    }
    current_version()
}

#[cfg(not(test))]
async fn fetch_version_from(url: &str) -> Option<String> {
    let client = crate::proxy::http_client::get_without_redirects().ok()?;
    fetch_version_with_client(&client, url).await
}

async fn fetch_version_with_client(client: &reqwest::Client, url: &str) -> Option<String> {
    let mut response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "electron-builder")
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len().saturating_add(chunk.len()) > MAX_MANIFEST_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    parse_manifest_version(text)
}

fn parse_manifest_version(manifest: &str) -> Option<String> {
    for line in manifest.lines() {
        let Some(raw) = line.strip_prefix("version:") else {
            continue;
        };
        let trimmed = raw.trim().trim_matches(['\'', '"']);
        if trimmed.is_empty() || trimmed.len() > 32 {
            return None;
        }
        let valid = trimmed
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
        return valid.then(|| trimmed.to_owned());
    }
    None
}

type TierCache = Mutex<HashMap<String, (Vec<String>, Instant)>>;

fn tier_cache() -> &'static TierCache {
    static TIERS: OnceLock<TierCache> = OnceLock::new();
    TIERS.get_or_init(Default::default)
}

fn tier_cache_key(account: &Account, project: &str) -> String {
    format!("{}:{}:{project}", account.id, account.generation)
}

pub(super) fn cached_entitlements(account: &Account, project: &str) -> Option<Vec<String>> {
    let mut map = tier_cache().lock().ok()?;
    map.retain(|_, (_, at)| at.elapsed() < TIER_CACHE_TTL);
    map.get(&tier_cache_key(account, project))
        .map(|(tiers, _)| tiers.clone())
}

pub(super) fn store_entitlements(account: &Account, project: &str, tiers: Vec<String>) {
    if let Ok(mut map) = tier_cache().lock() {
        map.insert(tier_cache_key(account, project), (tiers, Instant::now()));
    }
}

fn refused_tiers() -> &'static Mutex<HashMap<String, Instant>> {
    static REFUSED: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    REFUSED.get_or_init(Default::default)
}

fn tier_key(account: &Account, project: &str, tier: &str) -> String {
    format!("{}:{}:{project}:{tier}", account.id, account.generation)
}

pub(super) fn extract_entitlements(load_response: &Value) -> Vec<String> {
    let mut tiers = Vec::new();
    for tier in [
        load_response["paidTier"]["id"].as_str(),
        load_response["currentTier"]["id"].as_str(),
    ]
    .into_iter()
    .flatten()
    {
        if !tier.is_empty()
            && tier != "free-tier"
            && tier != "legacy-tier"
            && !tiers.iter().any(|existing| existing == tier)
        {
            tiers.push(tier.to_owned());
        }
    }
    tiers
}

pub(super) fn filter_tiers(account: &Account, project: &str, tiers: Vec<String>) -> Vec<String> {
    let Ok(mut map) = refused_tiers().lock() else {
        return tiers;
    };
    map.retain(|_, at| at.elapsed() < TIER_REFUSED_TTL);
    tiers
        .into_iter()
        .filter(|tier| !map.contains_key(&tier_key(account, project, tier)))
        .collect()
}

pub(super) fn mark_tier_refused(account: &Account, project: &str, tier: &str) {
    if tier.is_empty() {
        return;
    }
    if let Ok(mut map) = refused_tiers().lock() {
        map.insert(tier_key(account, project, tier), Instant::now());
    }
}

fn is_hidden_or_internal(id: &str) -> bool {
    HIDDEN_MODELS.contains(&id) || id.starts_with("chat_") || id.starts_with("tab_")
}

fn effort_family(id: &str) -> &str {
    for word in EFFORT_WORDS {
        if let Some(prefix) = id
            .strip_suffix(word)
            .and_then(|rest| rest.strip_suffix('-'))
        {
            if !prefix.is_empty() {
                return prefix;
            }
        }
    }
    id
}

fn strip_name_effort(name: &str) -> &str {
    let Some(open) = name.rfind(" (") else {
        return name;
    };
    if !name.ends_with(')') {
        return name;
    }
    let inside = &name[open + 2..name.len() - 1];
    if matches!(
        inside.to_ascii_lowercase().as_str(),
        "minimal" | "extra low" | "low" | "medium" | "high" | "extra high" | "xhigh"
    ) {
        name[..open].trim()
    } else {
        name
    }
}

fn name_spells_id(name: &str, id: &str) -> bool {
    let normalized = strip_name_effort(name)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    !normalized.is_empty() && normalized.eq_ignore_ascii_case(effort_family(id))
}

fn enum_names_id(model_enum: &str, id: &str) -> bool {
    let Some(suffix) = model_enum.strip_prefix("MODEL_GOOGLE_") else {
        return false;
    };
    !suffix.is_empty() && suffix.eq_ignore_ascii_case(&id.replace(['-', '.'], "_"))
}

pub(super) fn parse_models(body: &Value) -> Result<Vec<Value>, String> {
    let models = body["models"].as_object().ok_or(FAILED)?;
    let mut kept: Vec<(&str, &Value)> = models
        .iter()
        .filter(|(id, row)| row["disabled"] != true && !is_hidden_or_internal(id))
        .map(|(id, row)| (id.as_str(), row))
        .collect();

    let mut spelled = HashSet::new();
    for (id, row) in &kept {
        let name = row["displayName"].as_str().map(str::trim).unwrap_or("");
        if !name.is_empty() && name_spells_id(name, id) {
            spelled.insert(name.to_owned());
        }
    }

    let mut rank: HashMap<&str, usize> = HashMap::new();
    for sort_entry in body["agentModelSorts"].as_array().into_iter().flatten() {
        for group in sort_entry["groups"].as_array().into_iter().flatten() {
            for model_id in group["modelIds"].as_array().into_iter().flatten() {
                if let Some(id) = model_id.as_str() {
                    let next = rank.len() + 1;
                    rank.entry(id).or_insert(next);
                }
            }
        }
    }

    kept.sort_by(|(a, _), (b, _)| {
        let rank_a = rank.get(a).copied().unwrap_or(usize::MAX);
        let rank_b = rank.get(b).copied().unwrap_or(usize::MAX);
        rank_a.cmp(&rank_b).then_with(|| a.cmp(b))
    });

    Ok(kept
        .into_iter()
        .map(|(id, row)| {
            let raw_name = row["displayName"].as_str().map(str::trim).unwrap_or("");
            let model_enum = row["model"].as_str().unwrap_or("");
            let display_name = if raw_name.is_empty()
                || (!name_spells_id(raw_name, id)
                    && (spelled.contains(raw_name) || enum_names_id(model_enum, id)))
            {
                id.to_owned()
            } else {
                raw_name.to_owned()
            };
            let mut entry = json!({
                "model": id,
                "displayName": display_name,
                "contextWindow": row["maxTokens"],
                "maxOutputTokens": row["maxOutputTokens"],
            });
            if let Some(supports_images) = row["supportsImages"].as_bool() {
                entry["inputModalities"] = if supports_images {
                    json!(["text", "image"])
                } else {
                    json!(["text"])
                };
            }
            entry
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::routing_upstreams::SubscriptionProvider;
    use axum::{http::HeaderMap, http::StatusCode, routing::get, Router};

    fn fixture_account(generation: &str) -> Account {
        serde_json::from_value(json!({
            "id": "fixture-antigravity",
            "provider": "antigravity",
            "login": "fixture@example.test",
            "generation": generation,
            "access": "synthetic",
        }))
        .expect("fixture account")
    }

    #[test]
    fn parse_manifest_version_accepts_valid_semver_and_rejects_injection() {
        assert_eq!(
            parse_manifest_version("version: 2.35.2\nfiles:\n").as_deref(),
            Some("2.35.2")
        );
        assert_eq!(
            parse_manifest_version("version: \"2.36.0\"\n").as_deref(),
            Some("2.36.0")
        );
        assert_eq!(
            parse_manifest_version("version: '2.35.2'\r\n").as_deref(),
            Some("2.35.2")
        );
        assert_eq!(
            parse_manifest_version("version: 2.35.2\r\nX-Injected: 1").as_deref(),
            Some("2.35.2")
        );
        for invalid in [
            "",
            "other: 2.35.2",
            "version: ",
            "version: 2.35.2-beta",
            "version: 2.35.2\x00",
            "version: 2.35.2\\r\\nX-Injected: 1",
            "version: 2.35.2 X",
            "version: ..",
            &format!("version: {}", "1.".repeat(20)),
        ] {
            assert!(
                parse_manifest_version(invalid).is_none(),
                "expected invalid version for {invalid:?}"
            );
        }
    }

    #[tokio::test]
    async fn fetch_version_with_client_reads_manifest_and_rejects_errors_redirects_and_oversized() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener");
        let addr = listener.local_addr().expect("addr");
        let app = Router::new()
            .route(
                "/ok.yml",
                get(|headers: HeaderMap| async move {
                    if headers
                        .get(reqwest::header::USER_AGENT)
                        .and_then(|v| v.to_str().ok())
                        == Some("electron-builder")
                    {
                        (StatusCode::OK, "version: 2.35.2\npath: Antigravity.zip\n")
                    } else {
                        (StatusCode::BAD_REQUEST, "")
                    }
                }),
            )
            .route(
                "/err.yml",
                get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "version: 9.9.9\n") }),
            )
            .route(
                "/large.yml",
                get(|| async {
                    (
                        StatusCode::OK,
                        format!("version: 2.35.2\n{}", "a".repeat(MAX_MANIFEST_BYTES + 16)),
                    )
                }),
            )
            .route(
                "/redirect.yml",
                get(|| async {
                    (
                        StatusCode::FOUND,
                        [(axum::http::header::LOCATION, "/ok.yml")],
                        "",
                    )
                }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("client");

        assert_eq!(
            fetch_version_with_client(&client, &format!("http://{addr}/ok.yml"))
                .await
                .as_deref(),
            Some("2.35.2")
        );
        assert!(
            fetch_version_with_client(&client, &format!("http://{addr}/err.yml"))
                .await
                .is_none()
        );
        assert!(
            fetch_version_with_client(&client, &format!("http://{addr}/large.yml"))
                .await
                .is_none()
        );
        assert!(
            fetch_version_with_client(&client, &format!("http://{addr}/redirect.yml"))
                .await
                .is_none()
        );
        server.abort();
    }

    #[test]
    fn extract_entitlements_orders_paid_first_skips_free_and_legacy_and_deduplicates() {
        let reply = json!({
            "paidTier": {"id": "g1-ultra-tier"},
            "currentTier": {"id": "standard-tier"}
        });
        assert_eq!(
            extract_entitlements(&reply),
            vec!["g1-ultra-tier", "standard-tier"]
        );

        let same = json!({
            "paidTier": {"id": "g1-pro-tier"},
            "currentTier": {"id": "g1-pro-tier"}
        });
        assert_eq!(extract_entitlements(&same), vec!["g1-pro-tier"]);

        let free_only = json!({
            "paidTier": {"id": "legacy-tier"},
            "currentTier": {"id": "free-tier"}
        });
        assert!(extract_entitlements(&free_only).is_empty());
    }

    #[test]
    fn refused_tiers_are_skipped_for_same_generation_and_reset_after_reauth() {
        let gen1 = fixture_account("gen-1");
        let gen2 = fixture_account("gen-2");
        mark_tier_refused(&gen1, "proj-a", "g1-pro-tier");
        assert_eq!(
            filter_tiers(
                &gen1,
                "proj-a",
                vec!["g1-pro-tier".into(), "standard-tier".into()]
            ),
            vec!["standard-tier"]
        );
        assert_eq!(
            filter_tiers(
                &gen2,
                "proj-a",
                vec!["g1-pro-tier".into(), "standard-tier".into()]
            ),
            vec!["g1-pro-tier", "standard-tier"]
        );
    }

    #[test]
    fn parse_models_filters_internal_repairs_mislabeled_names_and_orders_by_agent_sorts() {
        let body = json!({
            "models": {
                "tab_jump_flash_lite_preview": {"displayName": "Tab Jump"},
                "chat_20706": {"displayName": "Chat"},
                "gemini-2.5-pro": {"displayName": "Gemini 2.5 Pro"},
                "gemini-2.5-flash-thinking": {"displayName": "Gemini 2.5 Flash Thinking"},
                "gemini-4-argon-disabled": {"displayName": "Gemini 4 Argon", "disabled": true},
                "gemini-2.5-flash": {
                    "displayName": "Gemini 3.5 Flash Lite",
                    "model": "MODEL_GOOGLE_GEMINI_2_5_FLASH",
                    "maxTokens": 1000000,
                    "maxOutputTokens": 65536,
                    "supportsImages": true
                },
                "gemini-2.5-flash-lite": {
                    "displayName": "Gemini 3.5 Flash Lite",
                    "maxTokens": 1000000,
                    "maxOutputTokens": 65536
                },
                "gemini-3.5-flash-lite": {
                    "displayName": "Gemini 3.5 Flash Lite",
                    "model": "MODEL_PLACEHOLDER_M198",
                    "maxTokens": 1000000,
                    "maxOutputTokens": 65536,
                    "supportsImages": true
                },
                "gpt-oss-120b-medium": {
                    "displayName": "GPT-OSS 120B (Medium)",
                    "supportsImages": false
                },
                "gemini-4-neon": {
                    "displayName": "Gemini 4 Neon",
                    "maxTokens": 1048576,
                    "maxOutputTokens": 65536,
                    "supportsImages": true
                },
                "gemini-4-argon": {
                    "displayName": "Gemini 4 Argon",
                    "maxTokens": 1048576,
                    "maxOutputTokens": 65536,
                    "supportsImages": true
                },
                "gemini-3.8-flash-high": {
                    "displayName": "Gemini 3.8 Flash (High)",
                    "maxTokens": 1048576,
                    "maxOutputTokens": 65536
                }
            },
            "agentModelSorts": [
                {
                    "groups": [
                        {"modelIds": ["gemini-4-argon", "gemini-4-neon"]},
                        {"modelIds": ["gemini-3.8-flash-high", "gemini-3.5-flash-lite"]}
                    ]
                }
            ]
        });

        let parsed = parse_models(&body).expect("parsed models");
        let ids: Vec<_> = parsed
            .iter()
            .map(|row| row["model"].as_str().expect("id"))
            .collect();
        assert_eq!(
            ids,
            vec![
                "gemini-4-argon",
                "gemini-4-neon",
                "gemini-3.8-flash-high",
                "gemini-3.5-flash-lite",
                "gemini-2.5-flash",
                "gemini-2.5-flash-lite",
                "gpt-oss-120b-medium",
            ]
        );

        assert_eq!(parsed[0]["displayName"], "Gemini 4 Argon");
        assert_eq!(parsed[0]["inputModalities"], json!(["text", "image"]));
        assert_eq!(parsed[2]["displayName"], "Gemini 3.8 Flash (High)");
        assert_eq!(parsed[3]["displayName"], "Gemini 3.5 Flash Lite");
        assert_eq!(parsed[4]["displayName"], "gemini-2.5-flash");
        assert_eq!(parsed[5]["displayName"], "gemini-2.5-flash-lite");
        assert_eq!(parsed[6]["displayName"], "GPT-OSS 120B (Medium)");
        assert_eq!(parsed[6]["inputModalities"], json!(["text"]));

        // Verify cached user_agent formats properly for Antigravity
        assert!(user_agent(DEFAULT_VERSION).starts_with("antigravity/hub/2.35.2 "));
        let _ = SubscriptionProvider::Antigravity;
    }

    #[test]
    fn version_cache_uses_short_backoff_after_failure_and_long_ttl_after_success() {
        assert!(cache_entry_fresh(Duration::from_secs(10 * 60), false));
        assert!(!cache_entry_fresh(Duration::from_secs(30 * 60), false));
        assert!(cache_entry_fresh(Duration::from_secs(30 * 60), true));
        assert!(!cache_entry_fresh(Duration::from_secs(6 * 3600), true));
    }

    #[test]
    fn entitlement_tier_cache_is_scoped_by_account_generation_and_project() {
        let gen1 = fixture_account("tier-gen-1");
        let gen2 = fixture_account("tier-gen-2");
        assert!(cached_entitlements(&gen1, "proj-a").is_none());

        store_entitlements(&gen1, "proj-a", vec!["g1-pro-tier".into()]);
        assert_eq!(
            cached_entitlements(&gen1, "proj-a"),
            Some(vec!["g1-pro-tier".into()])
        );
        assert!(cached_entitlements(&gen1, "proj-b").is_none());
        assert!(cached_entitlements(&gen2, "proj-a").is_none());
    }
}
