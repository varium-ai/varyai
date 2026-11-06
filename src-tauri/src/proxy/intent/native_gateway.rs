//! Raw native Codex access for official models.
//!
//! The intent gateway owns Vary aliases and applies the normal inspection and
//! routing pipeline to them. Official Codex models stay on the native wire
//! protocol: the request body and the upstream response are relayed without
//! protocol conversion.

use super::native::Lease;
use axum::{body::Body, response::Response};
use bytes::Bytes;
use futures::StreamExt;
use http::{header, HeaderMap, HeaderValue, StatusCode};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

const OFFICIAL_ORIGIN: &str = "https://chatgpt.com/backend-api/codex";
const CATALOG_LIMIT: usize = 8 * 1024 * 1024;
const CATALOG_WAIT: Duration = Duration::from_secs(10);

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
    ["vary-auto/", "vary-provider/", "auto/"]
        .iter()
        .any(|prefix| model.starts_with(prefix))
}

/// Fetch the official account catalog and merge it with the local Vary
/// projection. Discovery is best effort: the native client must still see
/// the Vary catalog when ChatGPT is unavailable.
pub(crate) async fn merged_catalog(
    lease: &Lease,
    source_headers: &HeaderMap,
    client_version: &str,
    vary: Value,
) -> Value {
    let Some(official) = fetch_catalog(lease, source_headers, client_version).await else {
        return vary;
    };
    merge_catalogs(official, &vary).unwrap_or(vary)
}

async fn fetch_catalog(
    lease: &Lease,
    source_headers: &HeaderMap,
    client_version: &str,
) -> Option<Value> {
    if !lease.valid() {
        return None;
    }
    let url = official_url("/models");
    let client = client().ok()?;
    let mut headers = outbound_headers(source_headers, lease);
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("identity"),
    );
    headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
    let request = client
        .get(url)
        .headers(headers)
        .query(&[("client_version", client_version)])
        .timeout(CATALOG_WAIT);
    let response = tokio::select! {
        biased;
        _ = lease.cancelled() => return None,
        result = super::super::http_client::send_with_header_timeout(
            request,
            CATALOG_WAIT,
        ) => result.ok()?,
    };
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = tokio::select! {
        biased;
        _ = lease.cancelled() => return None,
        chunk = stream.next() => chunk,
    } {
        let chunk = chunk.ok()?;
        if bytes.len().saturating_add(chunk.len()) > CATALOG_LIMIT {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    has_model_array(&value).then_some(value)
}

/// Merge model arrays while preserving the official payload and each source
/// entry's metadata. The key is the model slug/id; malformed catalogs fall
/// back to the local projection.
pub(crate) fn merge_catalogs(official: Value, vary: &Value) -> Option<Value> {
    let vary_models = model_array(vary)?.clone();
    let mut result = official;
    let official_models = model_array_mut(&mut result)?;
    let mut seen = official_models
        .iter()
        .filter_map(model_key)
        .collect::<std::collections::HashSet<_>>();
    for model in vary_models {
        if model_key(&model)
            .as_deref()
            .is_none_or(|key| seen.insert(key.to_owned()))
        {
            official_models.push(model);
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
    lease: Arc<Lease>,
    endpoint: Endpoint,
    source_headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !lease.valid() {
        return super::error(StatusCode::UNAUTHORIZED, "routing_native_unavailable");
    }
    let client = match client() {
        Ok(client) => client,
        Err(_) => {
            return super::error(
                StatusCode::BAD_GATEWAY,
                "routing_upstream_connection_failed",
            )
        }
    };
    let request = client
        .post(official_url(endpoint.path()))
        .headers(outbound_headers(&source_headers, &lease))
        .body(body);
    let upstream = tokio::select! {
        biased;
        _ = lease.cancelled() => {
            return super::error(StatusCode::UNAUTHORIZED, "routing_native_unavailable");
        }
        result = super::super::http_client::send_with_header_timeout(
            request,
            super::super::http_client::UPSTREAM_HEADER_WAIT,
        ) => match result {
            Ok(response) => response,
            Err(_) => {
                return super::error(
                    StatusCode::BAD_GATEWAY,
                    "routing_upstream_connection_failed",
                )
            }
        },
    };
    let status = upstream.status();
    let mut headers = upstream.headers().clone();
    strip_hop_headers(&mut headers);
    let mut stream = upstream.bytes_stream();
    let stream_lease = lease.clone();
    let stream = async_stream::stream! {
        loop {
            let next = tokio::select! {
                biased;
                _ = stream_lease.cancelled() => break,
                next = stream.next() => next,
            };
            match next {
                Some(Ok(bytes)) => yield Ok::<Bytes, std::io::Error>(bytes),
                Some(Err(_)) => {
                    yield Err(std::io::Error::other("native upstream stream failed"));
                    break;
                }
                None => break,
            }
        }
    };
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

fn outbound_headers(source: &HeaderMap, lease: &Lease) -> HeaderMap {
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
    if let Ok(value) = HeaderValue::from_str(lease.workspace()) {
        headers.insert("chatgpt-account-id", value);
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
        let merged = merge_catalogs(official, &vary).expect("valid catalogs");
        assert_eq!(merged["account"], "synthetic");
        assert_eq!(merged["models"].as_array().map(Vec::len), Some(2));
        assert_eq!(merged["models"][1]["slug"], "vary-auto/development");
    }

    #[test]
    fn unknown_vary_names_never_count_as_official_models() {
        for model in ["vary-auto/missing", "vary-provider/missing", "auto/missing"] {
            assert!(is_vary_alias(model));
        }
        assert!(!is_vary_alias("gpt-5"));
    }
}
