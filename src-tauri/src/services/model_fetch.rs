//! 模型列表获取服务
//!
//! 通过 OpenAI 兼容的 GET /v1/models 端点获取供应商可用模型列表。
//! 主要面向第三方聚合站（硅基流动、OpenRouter 等），以及把 Anthropic
//! 协议挂在兼容子路径上的官方供应商（DeepSeek、Kimi、智谱 GLM 等）。

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// 获取到的模型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedModel {
    pub id: String,
    pub owned_by: Option<String>,
}

/// 模型列表响应的兼容格式。
///
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Option<Vec<ModelEntry>>,
    models: Option<serde_json::Value>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    owned_by: Option<String>,
}

fn catalog_model_ids(models: Option<serde_json::Value>) -> Vec<String> {
    let Some(serde_json::Value::Array(entries)) = models else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            ["slug", "id"]
                .iter()
                .find_map(|key| entry.get(key)?.as_str().filter(|id| !id.is_empty()))
                .or_else(|| {
                    entry
                        .get("name")?
                        .as_str()?
                        .strip_prefix("models/")
                        .filter(|id| !id.is_empty())
                })
        })
        .map(str::to_string)
        .collect()
}

const FETCH_TIMEOUT_SECS: u64 = 15;
const MAX_MODEL_PAGES: usize = 100;
const MAX_REQUEST_HEADERS: usize = 64;
const MAX_HEADER_NAME_BYTES: usize = 256;
const MAX_HEADER_VALUE_BYTES: usize = 16 * 1024;

/// 404/405 响应体截断长度：避免把几十 KB HTML 404 页整页保留到错误串里。
const ERROR_BODY_MAX_CHARS: usize = 512;

/// 已知的「Anthropic 协议兼容子路径」后缀；按长度降序，最长前缀优先匹配。
/// baseURL 命中这些后缀时，候选列表会追加「剥离后缀再拼 /v1/models / /models」的版本。
const KNOWN_COMPAT_SUFFIXES: &[&str] = &[
    "/api/claudecode",
    "/api/anthropic",
    "/apps/anthropic",
    "/api/coding",
    "/claudecode",
    "/anthropic",
    "/step_plan",
    "/coding",
    "/claude",
];

/// 获取供应商的可用模型列表
///
/// 使用 OpenAI 兼容的 GET /v1/models 端点，按候选列表顺序尝试。
pub async fn fetch_models(
    base_url: &str,
    api_key: &str,
    is_full_url: bool,
    models_url_override: Option<&str>,
    user_agent: Option<HeaderValue>,
    api_format: Option<&str>,
    request_headers: Option<&BTreeMap<String, String>>,
) -> Result<Vec<FetchedModel>, String> {
    let is_gemini = matches!(api_format, Some("google-generative-ai" | "gemini_native"));
    let candidates = if is_gemini {
        vec![build_gemini_models_url(
            base_url,
            is_full_url,
            models_url_override,
        )?]
    } else {
        build_models_url_candidates(base_url, is_full_url, models_url_override)?
    };
    let headers =
        build_model_fetch_headers(api_key, api_format, user_agent.as_ref(), request_headers)?;
    let client = crate::proxy::http_client::get();
    let mut last_err: Option<String> = None;
    let mut known_secrets = vec![api_key.to_string()];
    if let Some(request_headers) = request_headers {
        known_secrets.extend(request_headers.values().cloned());
    }

    'candidates: for url in &candidates {
        log::debug!(
            "[ModelFetch] Trying endpoint: {}",
            crate::url_for_log_with_secrets(url, &known_secrets)
        );
        let mut models = Vec::new();
        let mut seen_tokens = std::collections::HashSet::new();
        let mut page_url =
            url::Url::parse(url).map_err(|_| "Invalid models endpoint".to_string())?;
        if let Some((_, token)) = page_url.query_pairs().find(|(key, _)| key == "pageToken") {
            seen_tokens.insert(token.into_owned());
        }
        for page in 0..MAX_MODEL_PAGES {
            let request = client
                .get(page_url.clone())
                .headers(headers.clone())
                .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS));
            let response = match request.send().await {
                Ok(r) => r,
                Err(e) => {
                    return Err(redact_model_fetch_error_body(
                        format!("Request failed: {}", e.without_url()),
                        &known_secrets,
                    ));
                }
            };

            let status = response.status();

            if status.is_success() {
                let resp: ModelsResponse = response
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse response: {e}"))?;

                let page_models: Vec<FetchedModel> = if let Some(data) = resp.data {
                    data.into_iter()
                        .map(|m| FetchedModel {
                            id: m.id,
                            owned_by: m.owned_by,
                        })
                        .collect()
                } else {
                    catalog_model_ids(resp.models)
                        .into_iter()
                        .map(|id| FetchedModel { id, owned_by: None })
                        .collect()
                };

                models.extend(page_models);
                if let Some(token) = resp
                    .next_page_token
                    .filter(|token| is_gemini && !token.is_empty())
                {
                    if !seen_tokens.insert(token.clone()) || page + 1 == MAX_MODEL_PAGES {
                        return Err("Model catalog pagination did not terminate".to_string());
                    }
                    // Tokens are opaque query values, never URLs or credentials.
                    let query: Vec<(String, String)> = page_url
                        .query_pairs()
                        .filter(|(key, _)| key != "pageToken")
                        .map(|(key, value)| (key.into_owned(), value.into_owned()))
                        .collect();
                    page_url.set_query(None);
                    page_url
                        .query_pairs_mut()
                        .extend_pairs(query)
                        .append_pair("pageToken", &token);
                    continue;
                }
                models.sort_by(|a, b| a.id.cmp(&b.id));
                if is_gemini {
                    models.dedup_by(|a, b| a.id == b.id);
                }
                return Ok(models);
            }

            if page == 0
                && (status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED)
            {
                let body = redact_model_fetch_error_body(
                    response.text().await.unwrap_or_default(),
                    &known_secrets,
                );
                last_err = Some(format!("HTTP {status}: {body}"));
                continue 'candidates;
            }

            let body = redact_model_fetch_error_body(
                response.text().await.unwrap_or_default(),
                &known_secrets,
            );
            return Err(format!("HTTP {status}: {body}"));
        }
    }

    Err(format!(
        "All candidates failed: {}",
        last_err.unwrap_or_else(|| "no candidates".to_string())
    ))
}

fn build_gemini_models_url(
    base_url: &str,
    is_full_url: bool,
    models_url_override: Option<&str>,
) -> Result<String, String> {
    if let Some(value) = models_url_override.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(value.to_string());
    }
    let mut url =
        url::Url::parse(base_url.trim()).map_err(|_| "Invalid Gemini base URL".to_string())?;
    let path = url.path().trim_end_matches('/');
    let path = if is_full_url {
        let index = path
            .find("/models/")
            .ok_or_else(|| "Cannot derive models endpoint from full URL".to_string())?;
        format!("{}/models", &path[..index])
    } else if path.ends_with("/models") {
        path.to_string()
    } else {
        let prefix = path
            .strip_suffix("/v1beta")
            .or_else(|| path.strip_suffix("/v1"))
            .unwrap_or(path);
        format!("{prefix}/v1beta/models")
    };
    url.set_path(&path);
    url.set_fragment(None);
    Ok(url.into())
}

fn redact_model_fetch_error_body(body: String, known_secrets: &[String]) -> String {
    truncate_body(crate::redact_known_secrets_strict(&body, known_secrets))
}

fn build_model_fetch_headers(
    api_key: &str,
    api_format: Option<&str>,
    user_agent: Option<&HeaderValue>,
    request_headers: Option<&BTreeMap<String, String>>,
) -> Result<HeaderMap, String> {
    let custom_count = request_headers.map_or(0, BTreeMap::len);
    if api_key.is_empty() && custom_count == 0 {
        return Err("API Key or request headers are required to fetch models".to_string());
    }
    if custom_count > MAX_REQUEST_HEADERS {
        return Err(format!(
            "Too many model-fetch request headers (maximum {MAX_REQUEST_HEADERS})"
        ));
    }

    let mut headers = HeaderMap::new();
    if !api_key.is_empty() {
        let (name, value) = match api_format {
            Some("anthropic-messages") => (
                HeaderName::from_static("x-api-key"),
                HeaderValue::from_str(api_key)
                    .map_err(|error| format!("Invalid API Key header value: {error}"))?,
            ),
            Some("google-generative-ai" | "gemini_native") => (
                HeaderName::from_static("x-goog-api-key"),
                HeaderValue::from_str(api_key)
                    .map_err(|error| format!("Invalid API Key header value: {error}"))?,
            ),
            _ => (
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {api_key}"))
                    .map_err(|error| format!("Invalid API Key header value: {error}"))?,
            ),
        };
        headers.insert(name, value);
    }

    if let Some(user_agent) = user_agent {
        headers.insert(USER_AGENT, user_agent.clone());
    }

    if let Some(request_headers) = request_headers {
        for (raw_name, raw_value) in request_headers {
            let name = raw_name.trim();
            if name.is_empty() || name.len() > MAX_HEADER_NAME_BYTES {
                return Err(format!("Invalid model-fetch header name: {raw_name}"));
            }
            if raw_value.len() > MAX_HEADER_VALUE_BYTES {
                return Err(format!("Model-fetch header value is too large: {name}"));
            }
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| format!("Invalid model-fetch header name {name}: {error}"))?;
            let value = HeaderValue::from_str(raw_value)
                .map_err(|error| format!("Invalid model-fetch header value for {name}: {error}"))?;
            headers.insert(name, value);
        }
    }

    Ok(headers)
}

/// 构造「模型列表端点」的候选 URL 列表
///
/// 候选顺序：
/// 1. `models_url_override` 非空 → 只返回它
/// 2. baseURL 拼 `/v1/models`；若已以版本段 `/v{N}` 结尾（`/v1`、智谱
///    `/api/coding/paas/v4` 等），版本号已在路径里，改拼 `/models`
/// 3. 版本段非 `/v1`（如 `/v4`）时再追加 `/v1/models` 作为兜底次候选
/// 4. 若 baseURL 命中 [`KNOWN_COMPAT_SUFFIXES`]，剥离后缀再拼 `/v1/models`、`/models`
///
/// 结果已去重且保持首次出现顺序。
pub fn build_models_url_candidates(
    base_url: &str,
    is_full_url: bool,
    models_url_override: Option<&str>,
) -> Result<Vec<String>, String> {
    if let Some(raw) = models_url_override {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(vec![trimmed.to_string()]);
        }
    }

    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Base URL is empty".to_string());
    }

    let mut candidates: Vec<String> = Vec::new();

    if is_full_url {
        let mut parsed =
            url::Url::parse(trimmed).map_err(|_| "Invalid models endpoint".to_string())?;
        let prefix = [
            "/chat/completions",
            "/responses",
            "/messages",
            "/completions",
        ]
        .iter()
        .find_map(|suffix| parsed.path().strip_suffix(suffix))
        .map(str::to_string);
        if let Some(prefix) = prefix {
            let query = parsed.query().map(str::to_string);
            parsed.set_path(&prefix);
            parsed.set_query(None);
            parsed.set_fragment(None);
            let mut candidates = build_models_url_candidates(parsed.as_str(), false, None)?;
            if !ends_with_version_segment(&prefix) {
                parsed.set_path(&format!("{prefix}/models"));
                let direct = parsed.to_string();
                if !candidates.contains(&direct) {
                    candidates.push(direct);
                }
            }
            for candidate in &mut candidates {
                let mut endpoint = url::Url::parse(candidate)
                    .map_err(|_| "Invalid models endpoint".to_string())?;
                endpoint.set_query(query.as_deref());
                *candidate = endpoint.into();
            }
            return Ok(candidates);
        }
        if let Some(idx) = trimmed.find("/v1/") {
            candidates.push(format!("{}/v1/models", &trimmed[..idx]));
        } else if let Some(idx) = trimmed.rfind('/') {
            let root = &trimmed[..idx];
            if root.contains("://") && root.len() > root.find("://").unwrap() + 3 {
                candidates.push(format!("{root}/v1/models"));
            }
        }
        if candidates.is_empty() {
            return Err("Cannot derive models endpoint from full URL".to_string());
        }
        return Ok(candidates);
    }

    // baseURL 已以版本段 /v{N} 结尾时（如 `/v1`、智谱 `/api/coding/paas/v4`），
    // OpenAI 惯例的模型端点是 `{base}/models`，不能再补 `/v1`
    // （否则 .../coding/paas/v4/v1/models → 404）。
    if ends_with_version_segment(trimmed) {
        candidates.push(format!("{trimmed}/models"));
        // 版本段非 /v1 时，保留旧的 /v1/models 作为兜底次候选（正确路径已在前）。
        if !trimmed.ends_with("/v1") {
            candidates.push(format!("{trimmed}/v1/models"));
        }
    } else {
        candidates.push(format!("{trimmed}/v1/models"));
    }

    if let Some(stripped) = strip_compat_suffix(trimmed) {
        let root = stripped.trim_end_matches('/');
        if !root.is_empty() && root.contains("://") {
            candidates.push(format!("{root}/v1/models"));
            candidates.push(format!("{root}/models"));
        }
    }

    // 候选最多 3 条，线性去重即可，不值得上 HashSet。
    let mut unique: Vec<String> = Vec::with_capacity(candidates.len());
    for url in candidates {
        if !unique.iter().any(|u| u == &url) {
            unique.push(url);
        }
    }

    Ok(unique)
}

/// 截断响应体到 [`ERROR_BODY_MAX_CHARS`] 字符，避免 HTML 404 页占用错误串。
fn truncate_body(body: String) -> String {
    if body.chars().count() <= ERROR_BODY_MAX_CHARS {
        body
    } else {
        let mut s: String = body.chars().take(ERROR_BODY_MAX_CHARS).collect();
        s.push('…');
        s
    }
}

/// 若 baseURL 以任一已知兼容子路径结尾，返回剥离后的剩余部分；否则 `None`。
///
/// 依赖 [`KNOWN_COMPAT_SUFFIXES`] 按长度降序排列，确保最长前缀优先命中
/// （否则 `/anthropic` 会提前匹配掉 `/api/anthropic` 的场景）。
fn strip_compat_suffix(base_url: &str) -> Option<&str> {
    for suffix in KNOWN_COMPAT_SUFFIXES {
        if base_url.ends_with(*suffix) {
            return Some(&base_url[..base_url.len() - suffix.len()]);
        }
    }
    None
}

/// 判断 baseURL 是否以 OpenAI 风格的版本段 `/v{N}` 结尾（`N` 为一个或多个数字），
/// 例如 `/v1`、`.../paas/v4`。这类 URL 版本号已在路径中，模型端点应为
/// `{base}/models`，不能再补 `/v1`（智谱 Coding Plan 即 `.../coding/paas/v4`）。
fn ends_with_version_segment(url: &str) -> bool {
    let last = url.rsplit('/').next().unwrap_or("");
    last.strip_prefix('v')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn full_inference_endpoint_discovers_models_under_the_same_version_prefix() {
        use axum::{routing::get, Json, Router};
        let app = Router::new().route(
            "/proxy/api/v4/models",
            get(|headers: HeaderMap| async move {
                assert_eq!(headers[AUTHORIZATION], "Bearer synthetic-key");
                Json(serde_json::json!({"data":[{"id":"synthetic-model"}]}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/proxy/api/v4/chat/completions",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = fetch_models(&endpoint, "synthetic-key", true, None, None, None, None).await;
        server.abort();
        assert_eq!(result.unwrap()[0].id, "synthetic-model");
    }

    #[test]
    fn full_model_endpoint_candidates_keep_prefix_query_and_supported_suffixes() {
        for suffix in ["chat/completions", "responses", "messages", "completions"] {
            let endpoint = format!("https://fixture.example/proxy/v4/{suffix}?region=synthetic");
            assert_eq!(
                build_models_url_candidates(&endpoint, true, None).unwrap()[0],
                "https://fixture.example/proxy/v4/models?region=synthetic"
            );
        }
        assert_eq!(
            build_models_url_candidates(
                "https://fixture.example/proxy/chat/completions",
                true,
                None
            )
            .unwrap(),
            vec![
                "https://fixture.example/proxy/v1/models",
                "https://fixture.example/proxy/models"
            ]
        );
    }

    #[test]
    fn gemini_catalog_reads_resource_names() {
        assert_eq!(
            catalog_model_ids(Some(serde_json::json!([
                {"name":"models/gemini-fixture"}, {"name":"models/"},
                {"name":"unrelated"}, {"slug":"legacy", "name":"models/ignored"}
            ]))),
            vec!["gemini-fixture", "legacy"]
        );
    }

    #[test]
    fn gemini_endpoints_preserve_prefixes_and_explicit_overrides() {
        for (base, full, expected) in [
            (
                "https://fixture.example/proxy/v1beta/",
                false,
                "https://fixture.example/proxy/v1beta/models",
            ),
            (
                "https://fixture.example/v1beta/models/a:generateContent",
                true,
                "https://fixture.example/v1beta/models",
            ),
            (
                "https://fixture.example/v1beta/models",
                false,
                "https://fixture.example/v1beta/models",
            ),
        ] {
            assert_eq!(build_gemini_models_url(base, full, None).unwrap(), expected);
        }
        assert_eq!(
            build_gemini_models_url("https://fixture.example", false, Some("  ")).unwrap(),
            "https://fixture.example/v1beta/models"
        );
        assert_eq!(
            build_gemini_models_url("invalid", true, Some("https://fixture.example/catalog"))
                .unwrap(),
            "https://fixture.example/catalog"
        );
        assert!(build_gemini_models_url("https://fixture.example/opaque", true, None).is_err());
        let headers =
            build_model_fetch_headers("synthetic-key", Some("gemini_native"), None, None).unwrap();
        assert_eq!(headers["x-goog-api-key"], "synthetic-key");
        assert!(!headers.contains_key(AUTHORIZATION));
    }

    #[tokio::test]
    async fn gemini_catalog_rejects_repeated_page_tokens() {
        use axum::{routing::get, Json, Router};
        let app = Router::new().route(
            "/v1beta/models",
            get(|| async { Json(serde_json::json!({"models":[],"nextPageToken":"repeat"})) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = fetch_models(
            &base,
            "synthetic-key",
            false,
            None,
            None,
            Some("google-generative-ai"),
            None,
        )
        .await;
        server.abort();
        assert_eq!(
            result.unwrap_err(),
            "Model catalog pagination did not terminate"
        );
    }

    #[tokio::test]
    async fn gemini_catalog_uses_header_and_collects_pages() {
        use axum::{extract::Query, routing::get, Json, Router};
        let app = Router::new().route("/v1beta/models", get(
            |headers: HeaderMap, Query(query): Query<BTreeMap<String, String>>| async move {
                assert_eq!(headers["x-goog-api-key"], "synthetic-key");
                assert!(!headers.contains_key(AUTHORIZATION));
                assert!(!query.contains_key("key"));
                if query.get("pageToken").is_some_and(|token| token == "page+two/=") {
                    Json(serde_json::json!({"models":[{"name":"models/a"},{"name":"models/z"}]}))
                } else {
                    assert!(!query.contains_key("pageToken"));
                    Json(serde_json::json!({"models":[{"name":"models/z"}],"nextPageToken":"page+two/="}))
                }
            }
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for suffix in ["", "/v1", "/v1beta"] {
            let result = fetch_models(
                &format!("{base}{suffix}"),
                "synthetic-key",
                false,
                None,
                None,
                Some("google-generative-ai"),
                None,
            )
            .await
            .unwrap();
            assert_eq!(
                result.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
                vec!["a", "z"]
            );
        }
        server.abort();
    }

    #[test]
    fn model_fetch_headers_follow_pi_api_format() {
        let anthropic =
            build_model_fetch_headers("anthropic-key", Some("anthropic-messages"), None, None)
                .unwrap();
        assert_eq!(anthropic["x-api-key"], "anthropic-key");
        assert!(!anthropic.contains_key(AUTHORIZATION));

        let google =
            build_model_fetch_headers("google-key", Some("google-generative-ai"), None, None)
                .unwrap();
        assert_eq!(google["x-goog-api-key"], "google-key");
        assert!(!google.contains_key(AUTHORIZATION));

        let openai =
            build_model_fetch_headers("openai-key", Some("openai-responses"), None, None).unwrap();
        assert_eq!(openai[AUTHORIZATION], "Bearer openai-key");
    }

    #[test]
    fn model_fetch_headers_allow_validated_header_only_auth_and_overrides() {
        let custom = BTreeMap::from([
            ("Authorization".to_string(), "Token literal".to_string()),
            ("X-Tenant".to_string(), "tenant-a".to_string()),
        ]);
        let headers =
            build_model_fetch_headers("", Some("openai-completions"), None, Some(&custom)).unwrap();
        assert_eq!(headers[AUTHORIZATION], "Token literal");
        assert_eq!(headers["x-tenant"], "tenant-a");

        let override_default =
            BTreeMap::from([("x-api-key".to_string(), "header-managed-key".to_string())]);
        let headers = build_model_fetch_headers(
            "provider-key",
            Some("anthropic-messages"),
            None,
            Some(&override_default),
        )
        .unwrap();
        assert_eq!(headers["x-api-key"], "header-managed-key");
    }

    #[test]
    fn model_fetch_headers_reject_invalid_or_missing_credentials() {
        assert!(build_model_fetch_headers("", None, None, None).is_err());
        let invalid = BTreeMap::from([("bad header".to_string(), "literal-value".to_string())]);
        assert!(build_model_fetch_headers("", None, None, Some(&invalid)).is_err());
    }

    #[test]
    fn model_fetch_error_body_redacts_known_header_credentials() {
        let secrets = vec![
            "short".to_string(),
            "Bearer literal-header-secret".to_string(),
        ];
        let body = redact_model_fetch_error_body(
            "invalid short / Bearer literal-header-secret".to_string(),
            &secrets,
        );
        assert_eq!(body, "invalid [REDACTED] / [REDACTED]");
    }

    #[test]
    fn test_candidates_plain_root() {
        let c = build_models_url_candidates("https://api.siliconflow.cn", false, None).unwrap();
        assert_eq!(c, vec!["https://api.siliconflow.cn/v1/models"]);
    }

    #[test]
    fn test_candidates_trailing_slash() {
        let c = build_models_url_candidates("https://api.example.com/", false, None).unwrap();
        assert_eq!(c, vec!["https://api.example.com/v1/models"]);
    }

    #[test]
    fn test_candidates_with_v1() {
        let c = build_models_url_candidates("https://api.example.com/v1", false, None).unwrap();
        assert_eq!(c, vec!["https://api.example.com/v1/models"]);
    }

    #[test]
    fn test_candidates_zhipu_coding_paas_v4() {
        // 智谱 Coding Plan 端点以 /v4 版本段结尾：模型端点是 {base}/models，
        // 正确路径必须排在 .../v4/v1/models（404）之前。
        let c =
            build_models_url_candidates("https://open.bigmodel.cn/api/coding/paas/v4", false, None)
                .unwrap();
        assert_eq!(
            c,
            vec![
                "https://open.bigmodel.cn/api/coding/paas/v4/models",
                "https://open.bigmodel.cn/api/coding/paas/v4/v1/models",
            ]
        );
    }

    #[test]
    fn test_candidates_zai_coding_paas_v4() {
        let c = build_models_url_candidates("https://api.z.ai/api/coding/paas/v4", false, None)
            .unwrap();
        assert_eq!(
            c,
            vec![
                "https://api.z.ai/api/coding/paas/v4/models",
                "https://api.z.ai/api/coding/paas/v4/v1/models",
            ]
        );
    }

    #[test]
    fn test_ends_with_version_segment() {
        assert!(ends_with_version_segment("https://x.com/v1"));
        assert!(ends_with_version_segment(
            "https://open.bigmodel.cn/api/coding/paas/v4"
        ));
        assert!(ends_with_version_segment("https://x.com/v10"));
        assert!(!ends_with_version_segment("https://x.com/api"));
        assert!(!ends_with_version_segment("https://x.com/vX"));
        assert!(!ends_with_version_segment("https://x.com/models"));
        assert!(!ends_with_version_segment("https://api.siliconflow.cn"));
    }

    #[test]
    fn test_candidates_full_url() {
        let c = build_models_url_candidates(
            "https://proxy.example.com/v1/chat/completions",
            true,
            None,
        )
        .unwrap();
        assert_eq!(c, vec!["https://proxy.example.com/v1/models"]);
    }

    #[test]
    fn test_candidates_empty() {
        assert!(build_models_url_candidates("", false, None).is_err());
    }

    #[test]
    fn test_candidates_override_returns_single() {
        let c = build_models_url_candidates(
            "https://api.deepseek.com/anthropic",
            false,
            Some("https://api.deepseek.com/models"),
        )
        .unwrap();
        assert_eq!(c, vec!["https://api.deepseek.com/models"]);
    }

    #[test]
    fn test_candidates_override_empty_falls_through() {
        let c =
            build_models_url_candidates("https://api.siliconflow.cn", false, Some("   ")).unwrap();
        assert_eq!(c, vec!["https://api.siliconflow.cn/v1/models"]);
    }

    #[test]
    fn test_candidates_deepseek_strip_anthropic() {
        let c =
            build_models_url_candidates("https://api.deepseek.com/anthropic", false, None).unwrap();
        assert_eq!(
            c,
            vec![
                "https://api.deepseek.com/anthropic/v1/models",
                "https://api.deepseek.com/v1/models",
                "https://api.deepseek.com/models",
            ]
        );
    }

    #[test]
    fn test_candidates_zhipu_strip_api_anthropic() {
        let c = build_models_url_candidates("https://open.bigmodel.cn/api/anthropic", false, None)
            .unwrap();
        assert_eq!(
            c,
            vec![
                "https://open.bigmodel.cn/api/anthropic/v1/models",
                "https://open.bigmodel.cn/v1/models",
                "https://open.bigmodel.cn/models",
            ]
        );
    }

    #[test]
    fn test_candidates_bailian_strip_apps_anthropic() {
        let c = build_models_url_candidates(
            "https://dashscope.aliyuncs.com/apps/anthropic",
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            c,
            vec![
                "https://dashscope.aliyuncs.com/apps/anthropic/v1/models",
                "https://dashscope.aliyuncs.com/v1/models",
                "https://dashscope.aliyuncs.com/models",
            ]
        );
    }

    #[test]
    fn test_candidates_stepfun_strip_step_plan() {
        let c =
            build_models_url_candidates("https://api.stepfun.com/step_plan", false, None).unwrap();
        assert_eq!(
            c,
            vec![
                "https://api.stepfun.com/step_plan/v1/models",
                "https://api.stepfun.com/v1/models",
                "https://api.stepfun.com/models",
            ]
        );
    }

    #[test]
    fn test_candidates_doubao_strip_api_coding() {
        let c = build_models_url_candidates(
            "https://ark.cn-beijing.volces.com/api/coding",
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            c,
            vec![
                "https://ark.cn-beijing.volces.com/api/coding/v1/models",
                "https://ark.cn-beijing.volces.com/v1/models",
                "https://ark.cn-beijing.volces.com/models",
            ]
        );
    }

    #[test]
    fn test_candidates_rightcode_strip_claude() {
        let c = build_models_url_candidates("https://www.right.codes/claude", false, None).unwrap();
        assert_eq!(
            c,
            vec![
                "https://www.right.codes/claude/v1/models",
                "https://www.right.codes/v1/models",
                "https://www.right.codes/models",
            ]
        );
    }

    #[test]
    fn test_candidates_longer_suffix_wins() {
        // baseURL 以 /api/anthropic 结尾时，应剥离整个 /api/anthropic，
        // 而不是只剥离 /anthropic（那样会得到残缺的 https://.../api 根）。
        let c = build_models_url_candidates("https://api.z.ai/api/anthropic", false, None).unwrap();
        assert_eq!(
            c,
            vec![
                "https://api.z.ai/api/anthropic/v1/models",
                "https://api.z.ai/v1/models",
                "https://api.z.ai/models",
            ]
        );
    }

    #[test]
    fn test_candidates_no_suffix_no_strip() {
        let c = build_models_url_candidates("https://openrouter.ai/api", false, None).unwrap();
        assert_eq!(c, vec!["https://openrouter.ai/api/v1/models"]);
    }

    #[test]
    fn test_candidates_deduplicate() {
        // 虚构 case：baseURL 就是 "scheme://host"，剥不出子路径，应只有一个候选。
        let c = build_models_url_candidates("https://host.example.com", false, None).unwrap();
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn test_parse_response() {
        let json = r#"{"object":"list","data":[{"id":"gpt-4","object":"model","owned_by":"openai"},{"id":"claude-3-sonnet","object":"model","owned_by":"anthropic"}]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        let data = resp.data.unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].id, "gpt-4");
        assert_eq!(data[0].owned_by.as_deref(), Some("openai"));
        assert_eq!(data[1].id, "claude-3-sonnet");
    }

    #[test]
    fn test_parse_response_no_owned_by() {
        let json = r#"{"object":"list","data":[{"id":"my-model","object":"model"}]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        let data = resp.data.unwrap();
        assert_eq!(data[0].id, "my-model");
        assert!(data[0].owned_by.is_none());
    }

    #[test]
    fn test_parse_response_empty_data() {
        let json = r#"{"object":"list","data":[]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.unwrap().is_empty());
    }

    #[test]
    fn model_response_ignores_unrelated_models_shapes() {
        for models in [
            r#"[{"id":"other"}]"#,
            r#"{"count":1}"#,
            "2",
            r#"["not a model"]"#,
            r#"[{"slug":1}]"#,
        ] {
            let json = format!(r#"{{"data":[{{"id":"model-a"}}],"models":{models}}}"#);
            let resp: ModelsResponse = serde_json::from_str(&json).unwrap();
            assert_eq!(resp.data.unwrap()[0].id, "model-a");
        }
    }

    #[test]
    fn catalog_models_prefer_slug_then_id_and_skip_invalid() {
        let resp: ModelsResponse = serde_json::from_str(
            r#"{"models":[
            {"slug":"glm","id":"fallback"}, {"id":"other"},
            {"slug":"","id":"valid"}, {"slug":1}, {}, 3
        ]}"#,
        )
        .unwrap();
        assert_eq!(
            catalog_model_ids(resp.models),
            vec!["glm", "other", "valid"]
        );
    }
}
