//! Raw native Codex access for official models.
//!
//! The intent gateway owns Vary aliases and applies the normal inspection and
//! routing pipeline to them. Official Codex models stay on the native wire
//! protocol: the request body and the upstream response are relayed without
//! protocol conversion.

use axum::{body::Body, response::Response};
use bytes::Bytes;
use futures::StreamExt;
use http::{header, HeaderMap, HeaderValue, StatusCode};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

const OFFICIAL_ORIGIN: &str = "https://chatgpt.com/backend-api/codex";
const CATALOG_LIMIT: usize = 16 * 1024 * 1024;
const CATALOG_WAIT: Duration = Duration::from_secs(3);
const CATALOG_FRESH: Duration = Duration::from_secs(60);
const CATALOG_FETCH: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct CatalogPolicy {
    official_aliases: HashMap<String, String>,
}
impl CatalogPolicy {
    pub(crate) fn for_menu(
        menu: &super::Menu,
        model_map: &std::collections::BTreeMap<String, String>,
    ) -> Self {
        let mut official_aliases = HashMap::new();
        for (alias, session) in &menu.groups {
            if !alias.starts_with("vary-provider/") {
                continue;
            }
            let Some(target) = session.targets.first() else {
                continue;
            };
            if !session
                .targets
                .iter()
                .all(|t| t.official && t.model == target.model)
            {
                continue;
            }
            official_aliases.insert(alias.clone(), target.model.clone());
        }
        for (wire, canonical) in model_map {
            if let Some(model) = official_aliases.get(canonical).cloned() {
                official_aliases.insert(wire.clone(), model);
            }
        }
        Self { official_aliases }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Endpoint {
    Responses,
    Compact,
}

impl Endpoint {
    fn path(self) -> &'static str {
        match self {
            Self::Responses => "/responses",
            Self::Compact => "/responses/compact",
        }
    }
}

/// Vary aliases are local names, even when a caller sends one which is no
/// longer published. Such names must never fall through to the official API.
pub(crate) fn is_vary_alias(model: &str) -> bool {
    model.contains('/')
}

#[derive(Clone)]
enum CatalogReply {
    Models(Value, Option<HeaderValue>),
    Denied(StatusCode, HeaderMap, Bytes),
}
#[derive(Default)]
struct CatalogState {
    reply: Option<CatalogReply>,
    fetched: Option<Instant>,
    pending: Option<tokio::sync::watch::Receiver<bool>>,
}
fn catalogs() -> &'static Mutex<HashMap<String, Arc<Mutex<CatalogState>>>> {
    static CATALOGS: OnceLock<Mutex<HashMap<String, Arc<Mutex<CatalogState>>>>> = OnceLock::new();
    CATALOGS.get_or_init(Mutex::default)
}

/// Magpie's account-scoped list cache: fresh for a minute; stale lists return
/// immediately while one background fetch continues for up to thirty seconds.
/// A cold caller waits at most three seconds, including slow or offline backends.
pub(crate) async fn merged_catalog(
    source_headers: &HeaderMap,
    query: Option<&str>,
    vary: Value,
    policy: CatalogPolicy,
) -> Response {
    use axum::response::IntoResponse;
    let url = official_url("/models");
    let mut key = Sha256::new();
    key.update(url.as_bytes());
    key.update(b"\0");
    key.update(
        source_headers
            .get("chatgpt-account-id")
            .or_else(|| source_headers.get(header::AUTHORIZATION))
            .map(|h| h.as_bytes())
            .unwrap_or_default(),
    );
    key.update(b"\0");
    key.update(query.unwrap_or_default().as_bytes());
    let entry = catalogs().lock().ok().map(|mut cache| {
        cache
            .entry(format!("{:x}", key.finalize()))
            .or_default()
            .clone()
    });
    let Some(entry) = entry else {
        return axum::Json(vary).into_response();
    };
    let (stale, mut pending) = {
        let Ok(mut state) = entry.lock() else {
            return axum::Json(vary).into_response();
        };
        let fresh = matches!(state.reply, Some(CatalogReply::Models(..)))
            && state.fetched.is_some_and(|at| at.elapsed() < CATALOG_FRESH);
        if !fresh && state.pending.is_none() {
            if matches!(state.reply, Some(CatalogReply::Denied(..))) {
                state.reply = None;
            }
            let (done, pending) = tokio::sync::watch::channel(false);
            state.pending = Some(pending);
            let background = entry.clone();
            let headers = source_headers.clone();
            let url = url.clone();
            let query = query.map(str::to_owned);
            tokio::spawn(async move {
                let reply = tokio::time::timeout(
                    CATALOG_FETCH,
                    fetch_catalog(url, headers, query.as_deref()),
                )
                .await
                .ok()
                .flatten();
                if let Ok(mut state) = background.lock() {
                    if let Some(reply) = reply {
                        state.fetched = Some(Instant::now());
                        state.reply = Some(reply);
                    }
                    state.pending = None;
                }
                done.send_replace(true);
            });
        }
        (
            matches!(state.reply, Some(CatalogReply::Models(..))),
            state.pending.clone(),
        )
    };
    if !stale {
        if let Some(pending) = pending.as_mut() {
            let _ = tokio::time::timeout(CATALOG_WAIT, pending.wait_for(|done| *done)).await;
        }
    }
    let reply = entry.lock().ok().and_then(|state| state.reply.clone());
    match reply {
        Some(CatalogReply::Models(official, etag)) => {
            let merged = merge_catalogs(official, &vary, &policy).unwrap_or(vary);
            let mut response = axum::Json(merged).into_response();
            if let Some(etag) = etag {
                response.headers_mut().insert(header::ETAG, etag);
            }
            response
        }
        Some(CatalogReply::Denied(status, headers, body)) if !sign_in_revoked(status, &body) => {
            let mut response = Response::new(Body::from(body));
            *response.status_mut() = status;
            if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
                response
                    .headers_mut()
                    .insert(header::CONTENT_TYPE, content_type.clone());
            }
            response
        }
        _ => axum::Json(vary).into_response(),
    }
}

async fn fetch_catalog(
    mut url: String,
    source: HeaderMap,
    query: Option<&str>,
) -> Option<CatalogReply> {
    if let Some(query) = query {
        url.push('?');
        url.push_str(query);
    }
    let mut headers = outbound_headers(&source);
    for name in [
        header::ACCEPT_ENCODING,
        header::IF_NONE_MATCH,
        header::IF_MODIFIED_SINCE,
    ] {
        headers.remove(name);
    }
    let response = client()
        .ok()?
        .get(url)
        .headers(headers)
        .timeout(CATALOG_FETCH)
        .send()
        .await
        .ok()?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if bytes.len().saturating_add(chunk.len()) > CATALOG_LIMIT {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Some(CatalogReply::Denied(status, headers, bytes.into()));
    }
    if !status.is_success() {
        return None;
    }
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    has_model_array(&value).then(|| CatalogReply::Models(value, headers.get(header::ETAG).cloned()))
}

fn sign_in_revoked(status: StatusCode, body: &[u8]) -> bool {
    if status != StatusCode::UNAUTHORIZED {
        return false;
    }
    static WORDS: OnceLock<Option<regex::bytes::Regex>> = OnceLock::new();
    WORDS.get_or_init(|| regex::bytes::Regex::new(r"(?i)\btoken_revoked\b|\btoken_invalidated\b|\brefresh_token_(reused|invalidated|expired)\b|\binvalid_grant\b|invalidated oauth token|(token|sign-in) (has been|was) revoked").ok())
        .as_ref().is_some_and(|words| words.is_match(body))
}

/// Magpie's CodexListedBeside, codexMerged and FitOwn: the native list owns
/// official slugs it already contains; hidden jobs follow the visible entries.
pub(crate) fn merge_catalogs(
    official: Value,
    vary: &Value,
    policy: &CatalogPolicy,
) -> Option<Value> {
    let vary_models = model_array(vary)?.clone();
    let mut result = official;
    let official_models = model_array_mut(&mut result)?;
    let backend = official_models
        .iter()
        .filter_map(model_key)
        .collect::<std::collections::HashSet<_>>();
    let mut seen = backend.clone();
    let after = official_models.len().saturating_add(100);
    for (index, mut model) in vary_models.into_iter().enumerate() {
        if model_key(&model)
            .and_then(|key| policy.official_aliases.get(&key))
            .is_some_and(|slug| backend.contains(slug))
        {
            continue;
        }
        if model_key(&model)
            .as_deref()
            .is_none_or(|key| seen.insert(key.to_owned()))
        {
            if let Some(fields) = model.as_object_mut() {
                fields.insert("priority".into(), after.saturating_add(index + 1).into());
            }
            official_models.push(model);
        }
    }
    official_models.sort_by_key(|model| {
        (
            model.get("visibility").and_then(Value::as_str) == Some("hide"),
            model
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        )
    });
    // Codex's desktop app reads only the first 100. Keep the account's
    // own visible models there, even when their original priorities are high.
    let listed = official_models
        .iter()
        .take_while(|model| model["visibility"] != "hide")
        .count();
    if listed > 100 {
        let own = |model: &Value| {
            model_key(model).is_some_and(|key| {
                backend.contains(&key) || policy.official_aliases.contains_key(&key)
            })
        };
        let mut promoted = 0;
        let promote = official_models[..listed]
            .iter()
            .map(|model| {
                let keep = promoted < 100 && own(model);
                promoted += usize::from(keep);
                keep
            })
            .collect::<Vec<_>>();
        let mut room = 100 - promoted;
        let mut first = Vec::new();
        let mut rest = Vec::new();
        for (model, promoted) in official_models.drain(..listed).zip(promote) {
            if promoted || room > 0 {
                room -= usize::from(!promoted);
                first.push(model);
            } else {
                rest.push(model);
            }
        }
        first.extend(rest);
        first.append(official_models);
        *official_models = first;
    }
    for (index, model) in official_models.iter_mut().enumerate() {
        if let Some(fields) = model.as_object_mut() {
            fields.insert("priority".into(), (index + 1).into());
        }
    }
    Some(result)
}

fn has_model_array(value: &Value) -> bool {
    model_array(value).is_some()
}

fn model_array(value: &Value) -> Option<&Vec<Value>> {
    value
        .get("models")
        .and_then(Value::as_array)
        .or_else(|| value.get("data").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
}

fn model_array_mut(value: &mut Value) -> Option<&mut Vec<Value>> {
    let key = if value.get("models").and_then(Value::as_array).is_some() {
        "models"
    } else if value.get("data").and_then(Value::as_array).is_some() {
        "data"
    } else if value.get("items").and_then(Value::as_array).is_some() {
        "items"
    } else {
        return None;
    };
    value.get_mut(key).and_then(Value::as_array_mut)
}

fn model_key(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned).or_else(|| {
        value
            .as_object()?
            .get("slug")
            .or_else(|| value.get("id"))
            .or_else(|| value.get("model"))
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

/// Relay an official native endpoint. Only transport framing headers are
/// removed; application headers and the response's status, headers and body
/// remain intact.
pub(crate) async fn forward(
    endpoint: Endpoint,
    source_headers: HeaderMap,
    body: Bytes,
    query: Option<&str>,
) -> Response {
    let client = match client() {
        Ok(client) => client,
        Err(_) => {
            return super::error(
                StatusCode::BAD_GATEWAY,
                "routing_upstream_connection_failed",
            )
        }
    };
    let mut url = official_url(endpoint.path());
    if source_headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.starts_with("Bearer sk-"))
        && url.starts_with(OFFICIAL_ORIGIN)
    {
        url = format!("https://api.openai.com/v1{}", endpoint.path());
    }
    if let Some(query) = query {
        url.push('?');
        url.push_str(query);
    }
    let request = client
        .post(url)
        .headers(outbound_headers(&source_headers))
        .body(body);
    let upstream = match super::super::http_client::send_with_header_timeout(
        request,
        super::super::http_client::UPSTREAM_HEADER_WAIT,
    )
    .await
    {
        Ok(response) => response,
        Err(_) => {
            return super::error(
                StatusCode::BAD_GATEWAY,
                "routing_upstream_connection_failed",
            )
        }
    };
    let status = upstream.status();
    let mut headers = upstream.headers().clone();
    strip_hop_headers(&mut headers);
    let stream = upstream
        .bytes_stream()
        .map(|chunk| chunk.map_err(|_| std::io::Error::other("native upstream stream failed")));
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

fn outbound_headers(source: &HeaderMap) -> HeaderMap {
    let mut headers = source.clone();
    strip_hop_headers(&mut headers);
    for name in [
        header::HOST,
        header::CONTENT_LENGTH,
        header::TRANSFER_ENCODING,
        header::CONNECTION,
        header::UPGRADE,
        header::ORIGIN,
        header::COOKIE,
    ] {
        headers.remove(name);
    }
    for name in [
        "proxy-connection",
        "proxy-authenticate",
        "proxy-authorization",
        "sec-fetch-site",
    ] {
        headers.remove(name);
    }
    headers
}

fn strip_hop_headers(headers: &mut HeaderMap) {
    let nominated = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(',').map(str::trim))
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for name in nominated {
        headers.remove(name);
    }
    for name in [
        header::CONNECTION,
        header::TRANSFER_ENCODING,
        header::UPGRADE,
    ] {
        headers.remove(name);
    }
    for name in [
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
    ] {
        headers.remove(name);
    }
}

fn official_url(path: &str) -> String {
    #[cfg(test)]
    if let Ok(origin) = test_origin().read() {
        if let Some(origin) = origin.as_deref() {
            return format!("{origin}{path}");
        }
    }
    format!("{OFFICIAL_ORIGIN}{path}")
}

fn client() -> Result<reqwest::Client, String> {
    #[cfg(test)]
    {
        if test_origin()
            .read()
            .map_err(|_| "client unavailable".to_owned())?
            .is_none()
        {
            return Err("test origin not configured".to_owned());
        }
        return reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| "client unavailable".to_owned());
    }
    #[cfg(not(test))]
    super::super::http_client::get_forwarding_without_redirects()
}

#[cfg(test)]
fn test_origin() -> &'static std::sync::RwLock<Option<String>> {
    static ORIGIN: std::sync::OnceLock<std::sync::RwLock<Option<String>>> =
        std::sync::OnceLock::new();
    ORIGIN.get_or_init(|| std::sync::RwLock::new(None))
}

#[cfg(test)]
pub(crate) struct TestOriginGuard(Option<String>);

#[cfg(test)]
pub(crate) fn set_test_origin(origin: impl Into<String>) -> TestOriginGuard {
    let mut slot = test_origin()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = slot.replace(origin.into());
    TestOriginGuard(previous)
}

#[cfg(test)]
impl Drop for TestOriginGuard {
    fn drop(&mut self) {
        let mut slot = test_origin()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = self.0.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn revoked_sign_in_vocabulary_matches_magpie_without_hiding_refreshable_errors() {
        for body in [
            "token_revoked",
            "token_invalidated",
            "refresh_token_reused",
            "refresh_token_invalidated",
            "refresh_token_expired",
            "invalid_grant",
            "Encountered invalidated oauth token for user",
            "sign-in was revoked",
            "token has been revoked",
        ] {
            assert!(sign_in_revoked(StatusCode::UNAUTHORIZED, body.as_bytes()));
            assert!(!sign_in_revoked(StatusCode::FORBIDDEN, body.as_bytes()));
        }
        for body in [
            "token_expired",
            "invalid_api_key",
            "synthetic_token_revoked_suffix",
            "synthetic error",
        ] {
            assert!(!sign_in_revoked(StatusCode::UNAUTHORIZED, body.as_bytes()));
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn catalog_deadline_keeps_fetch_running_and_stale_lists_return_immediately(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let task = tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                axum::Router::new().fallback(move || {
                    observed.fetch_add(1, Ordering::Relaxed);
                    async {
                        tokio::time::sleep(Duration::from_secs(4)).await;
                        axum::Json(json!({"models":[{"slug":"synthetic-native"}]}))
                    }
                }),
            )
            .await;
        });
        let _origin = set_test_origin(format!("http://{address}/backend-api/codex"));
        let headers = HeaderMap::new();
        let vary = json!({"models":[{"slug":"vary-auto/synthetic"}]});
        let first = tokio::time::timeout(
            Duration::from_millis(3500),
            merged_catalog(&headers, None, vary.clone(), CatalogPolicy::default()),
        )
        .await?;
        let body: Value =
            serde_json::from_slice(&axum::body::to_bytes(first.into_body(), CATALOG_LIMIT).await?)?;
        assert_eq!(body, vary);
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let cached = merged_catalog(&headers, None, vary.clone(), CatalogPolicy::default()).await;
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(cached.into_body(), CATALOG_LIMIT).await?,
        )?;
        assert_eq!(body["models"][0]["slug"], "synthetic-native");
        assert_eq!(count.load(Ordering::Relaxed), 1);
        if let Ok(cache) = catalogs().lock() {
            for entry in cache.values() {
                if let Ok(mut state) = entry.lock() {
                    state.fetched = Some(Instant::now() - CATALOG_FRESH);
                }
            }
        }
        let stale = tokio::time::timeout(
            Duration::from_millis(500),
            merged_catalog(&headers, None, vary, CatalogPolicy::default()),
        )
        .await?;
        let body: Value =
            serde_json::from_slice(&axum::body::to_bytes(stale.into_body(), CATALOG_LIMIT).await?)?;
        assert_eq!(body["models"][0]["slug"], "synthetic-native");
        task.abort();
        Ok(())
    }

    #[test]
    fn merge_preserves_official_entries_and_adds_vary_aliases() {
        let official = json!({
            "models": [{"slug": "gpt-5", "display_name": "Official"}],
            "account": "synthetic"
        });
        let vary = json!({
            "models": [
                {"slug": "vary-auto/development", "display_name": "Vary"},
                {"slug": "gpt-5", "display_name": "duplicate"}
            ]
        });
        let merged =
            merge_catalogs(official, &vary, &CatalogPolicy::default()).expect("valid catalogs");
        assert_eq!(merged["account"], "synthetic");
        assert_eq!(merged["models"].as_array().map(Vec::len), Some(2));
        assert_eq!(merged["models"][1]["slug"], "vary-auto/development");
    }

    #[test]
    fn magpie_beside_list_omits_only_official_direct_models_already_in_backend() {
        let mut menu = super::super::global_tests::menu("http://127.0.0.1:1/responses");
        let mut direct = menu
            .groups
            .values()
            .next()
            .expect("fixture")
            .as_ref()
            .clone();
        direct.targets[0].official = true;
        let canonical = "vary-provider/synthetic-direct";
        menu.groups
            .insert(canonical.into(), Arc::new(direct.clone()));
        // An actual routing group remains listed even when its sole target is official.
        let group = direct.group.alias();
        menu.groups.insert(group.clone(), Arc::new(direct.clone()));
        direct.targets[0].model = "synthetic-team-model".into();
        menu.groups
            .insert("vary-provider/team".into(), Arc::new(direct.clone()));
        direct.targets[0].model = "exact-model".into();
        direct.targets[0].official = false;
        menu.groups
            .insert("vary-provider/relay".into(), Arc::new(direct));
        let policy = CatalogPolicy::for_menu(
            &menu,
            &std::collections::BTreeMap::from([
                ("Readable-official".into(), canonical.into()),
                ("Historical-official".into(), canonical.into()),
            ]),
        );
        let official = json!({"models":[{"slug":"exact-model","priority":1}]});
        let vary = json!({"models":[
            {"slug":"Readable-official","display_name":"Identical label","priority":0},
            {"slug":canonical,"visibility":"hide"},
            {"slug":"Historical-official","visibility":"hide"},
            {"slug":group,"display_name":"Identical label","priority":1},
            {"slug":"vary-provider/team","display_name":"Team model","priority":2},
            {"slug":"vary-provider/relay","display_name":"Identical label","priority":3}
        ]});
        let merged = merge_catalogs(official, &vary, &policy).expect("catalog");
        let slugs = merged["models"]
            .as_array()
            .expect("models")
            .iter()
            .filter_map(model_key)
            .collect::<Vec<_>>();
        assert_eq!(
            slugs,
            [
                "exact-model",
                group.as_str(),
                "vary-provider/team",
                "vary-provider/relay"
            ]
        );
        // No native list: models available through another account are still offered.
        let offline = merge_catalogs(json!({"models":[]}), &vary, &policy).expect("catalog");
        assert_eq!(offline["models"].as_array().map(Vec::len), Some(6));
    }

    #[test]
    fn magpie_fit_keeps_native_models_in_the_first_desktop_page() {
        let official = json!({"models":[
            {"slug":"synthetic-native","priority":1000,"context_window":128000},
            {"slug":"synthetic-hidden","priority":0,"visibility":"hide"}
        ]});
        let vary = json!({"models":(0..110).map(|i| json!({
            "slug":format!("synthetic-relay-{i}"),"priority":i
        })).collect::<Vec<_>>()});
        let merged = merge_catalogs(official, &vary, &CatalogPolicy::default()).expect("catalog");
        let models = merged["models"].as_array().expect("models");
        assert_eq!(models[99]["slug"], "synthetic-native");
        assert_eq!(models[99]["context_window"], 128000);
        assert_eq!(models.last().expect("hidden")["slug"], "synthetic-hidden");
    }

    #[test]
    fn merged_picker_follows_magpie_visible_order_and_renumbers_hidden_entries_last() {
        let official = json!({
            "models": [
                {"slug":"synthetic-native-history","visibility":"hide","priority":0},
                {"slug":"synthetic-native-a","display_name":"Synthetic A","priority":1,"context_window":128000},
                {"slug":"synthetic-native-b","display_name":"Synthetic B","priority":2}
            ]
        });
        let vary = json!({
            "models": [
                {"slug":"synthetic-managed-a","display_name":"Synthetic A · Managed","priority":0,"supported_reasoning_levels":["high"]},
                {"slug":"synthetic-managed-b","display_name":"Synthetic B · Managed","priority":1},
                {"slug":"synthetic-history","visibility":"hide","priority":0}
            ]
        });
        let merged = merge_catalogs(official.clone(), &vary, &CatalogPolicy::default())
            .expect("valid catalogs");
        let models = merged["models"].as_array().expect("model array");
        assert_eq!(models[0]["slug"], "synthetic-native-a");
        assert_eq!(models[0]["context_window"], 128000);
        assert_eq!(models[2]["priority"], 3);
        assert_eq!(models[3]["priority"], 4);
        assert_eq!(models[2]["slug"], vary["models"][0]["slug"]);
        assert_eq!(
            models[2]["supported_reasoning_levels"],
            vary["models"][0]["supported_reasoning_levels"]
        );
        assert_eq!(models[4]["slug"], "synthetic-native-history");
        assert_eq!(models[5]["slug"], "synthetic-history");
        for (index, model) in models.iter().enumerate() {
            assert_eq!(model["priority"], index + 1);
        }
        let mut picker = models
            .iter()
            .filter(|model| model["visibility"] != "hide")
            .collect::<Vec<_>>();
        picker.sort_by_key(|model| model["priority"].as_i64().unwrap_or_default());
        assert_eq!(
            picker
                .iter()
                .map(|model| model["slug"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "synthetic-native-a",
                "synthetic-native-b",
                "synthetic-managed-a",
                "synthetic-managed-b"
            ]
        );
    }

    #[test]
    fn unknown_vary_names_never_count_as_official_models() {
        for model in ["vary-auto/missing", "vary-provider/missing", "auto/missing"] {
            assert!(is_vary_alias(model));
        }
        assert!(!is_vary_alias("gpt-5"));
    }
}
