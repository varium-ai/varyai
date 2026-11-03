//! Codex OAuth model list service.
//!
//! ChatGPT Codex exposes models through `chatgpt.com/backend-api/codex/models`,
//! which is not an OpenAI-compatible `/v1/models` endpoint.

use crate::proxy::providers::codex_oauth_auth::CODEX_OAUTH_ORIGINATOR;
use crate::services::model_fetch::FetchedModel;
use serde_json::Value;
use std::time::Duration;

const CODEX_OAUTH_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
const CODEX_OAUTH_FETCH_TIMEOUT_SECS: u64 = 15;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub async fn fetch_models_with_token(
    token: &str,
    account_id: &str,
) -> Result<Vec<FetchedModel>, String> {
    Ok(parse_models(
        fetch_catalog_with_token(token, account_id).await?,
    ))
}

pub(crate) async fn fetch_catalog_with_token(
    token: &str,
    account_id: &str,
) -> Result<Value, String> {
    crate::codex_client::resolve_version().await;
    let client = crate::proxy::http_client::get_without_redirects()
        .map_err(|_| "Model discovery client unavailable".to_string())?;
    let response = build_models_request(&client, token, account_id)
        .send()
        .await
        .map_err(|_| "Model discovery request failed".to_string())?;
    read_catalog_response(response).await
}

#[cfg(test)]
async fn read_models_response(response: reqwest::Response) -> Result<Vec<FetchedModel>, String> {
    Ok(parse_models(read_catalog_response(response).await?))
}

async fn read_catalog_response(mut response: reqwest::Response) -> Result<Value, String> {
    if !response.status().is_success() {
        return Err(format!(
            "Model discovery HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Model discovery read failed")?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err("Model discovery response too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid model discovery response")?;
    Ok(value)
}

fn build_models_request(
    client: &reqwest::Client,
    token: &str,
    account_id: &str,
) -> reqwest::RequestBuilder {
    let version = crate::codex_client::current_version();
    client
        .get(CODEX_OAUTH_MODELS_URL)
        .query(&[("client_version", &version)])
        .header("Authorization", format!("Bearer {token}"))
        .header("originator", CODEX_OAUTH_ORIGINATOR)
        .header("version", &version)
        .header("User-Agent", format!("codex_cli_rs/{version}"))
        .header("chatgpt-account-id", account_id)
        .timeout(Duration::from_secs(CODEX_OAUTH_FETCH_TIMEOUT_SECS))
}

pub(crate) fn parse_models(value: Value) -> Vec<FetchedModel> {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .or_else(|| value.as_array());

    let mut models = Vec::new();

    if let Some(entries) = entries {
        for entry in entries {
            push_model_entry(&mut models, entry, None);
        }
    }

    if let Some(model_map) = value.get("models").and_then(Value::as_object) {
        for (key, entry) in model_map {
            push_model_entry(&mut models, entry, Some(key));
        }
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

pub(crate) fn picker_visible(entry: &Value) -> bool {
    entry.get("hidden").and_then(Value::as_bool) != Some(true)
        && entry.get("show_in_picker").and_then(Value::as_bool) != Some(false)
        && entry
            .get("visibility")
            .is_none_or(|visibility| visibility.as_str() == Some("list"))
}

fn push_model_entry(models: &mut Vec<FetchedModel>, entry: &Value, fallback_id: Option<&str>) {
    if !picker_visible(entry) {
        return;
    }
    if let Some(id) = entry.as_str().map(str::trim).filter(|id| !id.is_empty()) {
        models.push(FetchedModel::from_entry(
            id.to_string(),
            Some("Codex".to_string()),
            entry,
        ));
        return;
    }

    let Some(obj) = entry.as_object() else {
        if let Some(id) = fallback_id.map(str::trim).filter(|id| !id.is_empty()) {
            models.push(FetchedModel::from_entry(
                id.to_string(),
                Some("Codex".to_string()),
                entry,
            ));
        }
        return;
    };

    let Some(id) = string_field(obj, &["slug", "id", "model", "name"]).or_else(|| {
        fallback_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
    }) else {
        return;
    };
    let owned_by = string_field(
        obj,
        &[
            "owned_by", "ownedBy", "provider", "vendor", "category", "owner",
        ],
    )
    .or_else(|| Some("Codex".to_string()));

    models.push(FetchedModel::from_entry(id, owned_by, entry));
}

fn string_field(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| obj.get(*key))
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn synthetic_response(status: &str, body: String) -> reqwest::Response {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request).await;
            let _ = socket.write_all(response.as_bytes()).await;
        });
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{addr}"))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn discovery_bounds_response_and_never_exposes_error_bodies() {
        let response =
            synthetic_response("401 Unauthorized", "synthetic-private-body".into()).await;
        assert_eq!(
            read_models_response(response).await.unwrap_err(),
            "Model discovery HTTP 401"
        );
        let response = synthetic_response("200 OK", "x".repeat(MAX_RESPONSE_BYTES + 1)).await;
        assert_eq!(
            read_models_response(response).await.unwrap_err(),
            "Model discovery response too large"
        );
        let response = synthetic_response("200 OK", "synthetic-private-invalid-json".into()).await;
        assert_eq!(
            read_models_response(response).await.unwrap_err(),
            "Invalid model discovery response"
        );
        let response = synthetic_response(
            "200 OK",
            r#"{"models":[{"slug":"synthetic-model"}]}"#.into(),
        )
        .await;
        assert_eq!(
            read_models_response(response).await.unwrap()[0].id,
            "synthetic-model"
        );
    }

    #[test]
    fn codex_oauth_model_discovery_uses_current_identity_for_gpt61() {
        let request = build_models_request(&reqwest::Client::new(), "test-token", "test-account")
            .build()
            .unwrap();
        assert_eq!(request.headers()["authorization"], "Bearer test-token");
        assert_eq!(request.headers()["chatgpt-account-id"], "test-account");
        assert_eq!(request.headers()["originator"], "codex_cli_rs");
        let version = request
            .url()
            .query_pairs()
            .find(|(key, _)| key == "client_version")
            .unwrap()
            .1
            .into_owned();
        let parts: Vec<u32> = version
            .split('.')
            .map(|part| part.parse().unwrap())
            .collect();
        // Older discovery identities omit 6.1 even when the account can use it.
        assert!(parts.as_slice() >= [0, 164, 0].as_slice());
        assert_eq!(request.headers()["version"], version);
        assert_eq!(
            request.headers()["user-agent"],
            format!("codex_cli_rs/{version}")
        );
        let models = parse_models(json!({"models": [
            {"slug": "gpt-6.1-sol", "minimal_client_version": "0.164.0"},
            {"slug": "gpt-6.1-terra", "minimal_client_version": "0.164.0"}
        ]}));
        assert_eq!(models[0].id, "gpt-6.1-sol");
        assert_eq!(models[1].id, "gpt-6.1-terra");
    }

    #[test]
    fn parse_codex_oauth_models_accepts_openai_style_data() {
        let models = parse_models(json!({
            "data": [
                { "id": "gpt-5.4", "owned_by": "openai" },
                { "id": "gpt-5.4-mini", "ownedBy": "openai" }
            ]
        }));

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-5.4");
        assert_eq!(models[0].owned_by.as_deref(), Some("openai"));
        assert_eq!(models[1].id, "gpt-5.4-mini");
        assert_eq!(models[1].owned_by.as_deref(), Some("openai"));
    }

    #[test]
    fn parse_codex_oauth_models_accepts_model_list_shape() {
        let models = parse_models(json!({
            "models": [
                { "slug": "gpt-5.3-codex", "display_name": "GPT-5.3 Codex" },
                "gpt-5.5"
            ]
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.3-codex".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parse_codex_oauth_models_deduplicates_ids() {
        let models = parse_models(json!({
            "data": [
                { "id": "gpt-5.4" },
                { "model": "gpt-5.4" }
            ]
        }));

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5.4");
    }

    #[test]
    fn official_catalog_only_lists_picker_visible_models_without_rewriting_ids() {
        let models = parse_models(json!({"models": [
            {"slug":"vendor/Model_1.2-20261029", "visibility":"list", "supported_in_api":false},
            {"slug":"hidden-model", "visibility":"hide"},
            {"slug":"unlisted-model", "visibility":"none"},
            {"slug":"future-visibility", "visibility":"internal"},
            {"slug":"explicitly-hidden", "visibility":"list", "hidden":true},
            {"slug":"not-in-picker", "show_in_picker":false},
            {"slug":"legacy-model"},
            "legacy-string",
            {"slug":"duplicate", "visibility":"hide"},
            {"slug":"duplicate", "visibility":"list"}
        ]}));
        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec![
                "duplicate",
                "legacy-model",
                "legacy-string",
                "vendor/Model_1.2-20261029"
            ]
        );
    }

    #[test]
    fn official_visibility_applies_to_all_supported_catalog_shapes() {
        let entries = json!([
            {"id":"visible", "visibility":"list"},
            {"id":"hidden", "visibility":"hide"},
            {"id":"unlisted", "visibility":"none"}
        ]);
        for value in [
            entries.clone(),
            json!({"data":entries}),
            json!({"items":entries}),
            json!({"models": {
                "visible":{"visibility":"list"},
                "hidden":{"visibility":"hide"},
                "unlisted":{"visibility":"none"}
            }}),
        ] {
            let ids = parse_models(value)
                .into_iter()
                .map(|model| model.id)
                .collect::<Vec<_>>();
            assert_eq!(ids, vec!["visible"]);
        }
        assert!(
            parse_models(json!({"models":[{"slug":"hidden", "visibility":"hide"}]})).is_empty()
        );
    }

    #[test]
    fn parse_codex_oauth_models_accepts_model_map_shape() {
        let models = parse_models(json!({
            "models": {
                "gpt-5.4": { "display_name": "GPT-5.4" },
                "gpt-5.5": { "slug": "gpt-5.5" }
            }
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]
        );
    }
}
