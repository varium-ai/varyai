//! Grok CLI subscription transport, separate from the general xAI API.
//! Wire reference: @magpie-community/opencode-grok-auth (MIT), pinned in the parity review.
use super::subscription::{CredentialStatus, QuotaTier, SubscriptionQuota};
use serde_json::{json, Value};
pub(crate) const BASE: &str = "https://cli-chat-proxy.grok.com/v1";

pub(crate) fn headers() -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();
    for (name, value) in [
        ("x-grok-client-version", "1.0.67"),
        ("x-xai-token-auth", "xai-grok-cli"),
        ("x-grok-client-identifier", "grok-shell"),
        ("x-grok-client-mode", "headless"),
        ("user-agent", "grok-shell/1.0.67"),
        ("x-grok-doom-loop-check", "1024"),
        ("x-grok-exact-repetition-check", "64"),
    ] {
        headers.insert(
            http::HeaderName::from_static(name),
            http::HeaderValue::from_static(value),
        );
    }
    headers
}
pub(crate) fn shape(body: &mut Value) -> Result<http::HeaderMap, &'static str> {
    let mut headers = headers();
    let asked = body["model"]
        .as_str()
        .ok_or("routingUpstreams.failed")?
        .to_owned();
    let (model, window) = split_model(&asked);
    headers.insert(
        "x-grok-model-override",
        http::HeaderValue::from_str(model).map_err(|_| "routingUpstreams.failed")?,
    );
    if let Some(window) = window {
        headers.insert(
            "x-grok-context-window",
            http::HeaderValue::from_str(&window.to_string())
                .map_err(|_| "routingUpstreams.failed")?,
        );
    }
    if let Some(key) = body["prompt_cache_key"]
        .as_str()
        .filter(|key| !key.is_empty())
    {
        headers.insert(
            "x-grok-conv-id",
            http::HeaderValue::from_str(key).map_err(|_| "routingUpstreams.failed")?,
        );
    }
    body["model"] = json!(model);
    if let Some(items) = body["input"].as_array_mut() {
        for item in items {
            if item["type"] == "reasoning" && item.get("content") == Some(&Value::Null) {
                if let Some(item) = item.as_object_mut() {
                    item.remove("content");
                }
            }
        }
    }
    Ok(headers)
}
fn split_model(id: &str) -> (&str, Option<u64>) {
    if let Some((model, size)) = id.rsplit_once('@') {
        let (digits, multiplier) = size.strip_suffix('k').map_or((size, 1), |s| (s, 1000));
        if let Some(size) = digits
            .parse::<u64>()
            .ok()
            .and_then(|n| n.checked_mul(multiplier))
            .filter(|n| *n > 0)
        {
            return (model, Some(size));
        }
    }
    (id, None)
}
pub(crate) fn models(body: &Value) -> Vec<super::model_fetch::FetchedModel> {
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["api_backend"].as_str().is_none_or(|v| v == "responses"))
        .filter_map(|row| Some((row["id"].as_str()?, row)))
        .flat_map(|(id, row)| {
            let windows: Vec<u64> = row["context_windows"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(number)
                .filter(|n| *n > 0)
                .collect();
            let default = number(&row["context_window"])
                .filter(|n| *n > 0 && (windows.is_empty() || windows.contains(n)))
                .or_else(|| windows.first().copied());
            let max = windows.iter().copied().max().or(default);
            let efforts: Vec<_> = row["reasoning_efforts"]
                .as_array()
                .into_iter()
                .flatten()
                .rev()
                .filter_map(|e| e["value"].as_str())
                .collect();
            let mut facts = row.clone();
            if row["reasoning_efforts"].is_array() {
                facts["reasoningLevels"] = json!(efforts);
            }
            facts["context_window"] = json!(default);
            facts["max_context_window"] = json!(max);
            let mut out = vec![super::model_fetch::FetchedModel::from_entry(
                id.into(),
                None,
                &facts,
            )];
            for window in windows {
                if Some(window) != default {
                    let suffix = if window % 1000 == 0 {
                        format!("{}k", window / 1000)
                    } else {
                        window.to_string()
                    };
                    let alias = format!("{id}@{suffix}");
                    facts["context_window"] = json!(window);
                    facts["display_name"] =
                        json!(format!("{} @ {suffix}", row["name"].as_str().unwrap_or(id)));
                    if !out.iter().any(|m| m.id == alias) {
                        out.push(super::model_fetch::FetchedModel::from_entry(
                            alias, None, &facts,
                        ));
                    }
                }
            }
            out
        })
        .collect()
}
fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
pub(crate) fn billing(body: &Value, tool: &str) -> Option<SubscriptionQuota> {
    let config = body.get("config")?.as_object()?;
    let percent = config
        .get("creditUsagePercent")?
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)?;
    let period = config.get("currentPeriod");
    let resets_at = period
        .and_then(|p| p["end"].as_str())
        .or_else(|| config.get("billingPeriodEnd")?.as_str())
        .filter(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
        .map(str::to_owned);
    let name = match period.and_then(|p| p["type"].as_str()) {
        Some("USAGE_PERIOD_TYPE_WEEKLY" | "WEEKLY") => "weekly_limit",
        Some("USAGE_PERIOD_TYPE_MONTHLY" | "MONTHLY") => "monthly",
        _ => "credits",
    };
    let mut tiers = vec![QuotaTier {
        display: None,
        name: name.into(),
        utilization: percent.clamp(0.0, 100.0),
        resets_at: resets_at.clone(),
        used_value_usd: None,
        max_value_usd: None,
    }];
    if let Some((used, cap)) = config
        .get("onDemandUsed")
        .and_then(|v| v["val"].as_f64())
        .zip(config.get("onDemandCap").and_then(|v| v["val"].as_f64()))
        .filter(|(used, cap)| used.is_finite() && cap.is_finite() && *used >= 0.0 && *cap > 0.0)
    {
        tiers.push(QuotaTier {
            display: None,
            name: "on_demand".into(),
            utilization: (100.0 * used / cap).clamp(0.0, 100.0),
            resets_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }
    Some(SubscriptionQuota {
        tool: tool.into(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        codex: None,
        error: None,
        queried_at: Some(chrono::Utc::now().timestamp_millis()),
    })
}
pub(crate) async fn inference(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    mut body: Value,
) -> Result<reqwest::Response, String> {
    let headers = shape(&mut body)?;
    body["stream"] = json!(true);
    client
        .post(format!("{base}/responses"))
        .headers(headers)
        .bearer_auth(token)
        .header("accept", "text/event-stream")
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(300))
        .json(&body)
        .send()
        .await
        .map_err(|_| "nativeSubscription.failed".into())
}
pub(crate) async fn list_models(
    client: &reqwest::Client,
    base: &str,
    token: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    client
        .get(format!("{base}/models"))
        .headers(headers())
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
}
pub(crate) async fn query(token: &str) -> Result<SubscriptionQuota, String> {
    query_at(&crate::proxy::http_client::get(), BASE, token).await
}
async fn query_at(
    client: &reqwest::Client,
    base: &str,
    token: &str,
) -> Result<SubscriptionQuota, String> {
    let response = client
        .get(format!("{base}/billing?format=credits"))
        .bearer_auth(token)
        .header("accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| "nativeSubscription.failed")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(SubscriptionQuota::error(
            "xai_oauth",
            CredentialStatus::Expired,
            "Authorization unavailable".into(),
        ));
    }
    if !response.status().is_success() {
        return Err("nativeSubscription.failed".into());
    }
    let body = response
        .json::<Value>()
        .await
        .map_err(|_| "nativeSubscription.failed")?;
    billing(&body, "xai_oauth").ok_or_else(|| "nativeSubscription.failed".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn transport_uses_cli_model_inference_and_billing_contracts(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{extract::Request, routing::any, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}/v1", listener.local_addr()?);
        let app=Router::new().fallback(any(|request:Request|async move {
            use axum::response::IntoResponse;
            if request.headers().get("authorization").is_some_and(|h|h=="Bearer synthetic-old") {
                return http::StatusCode::UNAUTHORIZED.into_response();
            }
            assert_eq!(request.headers()["authorization"],"Bearer synthetic-renewed");
            match request.uri().path() {
                "/v1/models"=>{
                    assert_eq!(request.method(),http::Method::GET);
                    assert_eq!(request.headers()["x-xai-token-auth"],"xai-grok-cli");
                    axum::Json(json!({"data":[{"id":"fixture","api_backend":"responses","context_window":100000}]})).into_response()
                }
                "/v1/billing"=>{
                    assert_eq!(request.uri().query(),Some("format=credits"));
                    axum::Json(json!({"config":{"creditUsagePercent":25}})).into_response()
                }
                "/v1/responses"=>{
                    assert_eq!(request.method(),http::Method::POST);
                    assert_eq!(request.headers()["x-grok-model-override"],"fixture");
                    assert_eq!(request.headers()["x-grok-context-window"],"500000");
                    let bytes=axum::body::to_bytes(request.into_body(),1024*1024).await.expect("synthetic request");
                    let body:Value=serde_json::from_slice(&bytes).expect("request JSON");
                    assert_eq!(body["model"],"fixture");
                    assert_eq!(body["stream"],true);
                    assert_eq!(body["input"][0]["arguments"],"{\"n\":1}");
                    ([("content-type","text/event-stream")],"data: {\"type\":\"response.completed\"}\n\n").into_response()
                }
                _=>http::StatusCode::NOT_FOUND.into_response()
            }
        }));
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let client = reqwest::Client::new();
        assert!(matches!(
            query_at(&client, &base, "synthetic-old")
                .await?
                .credential_status,
            CredentialStatus::Expired
        ));
        let quota = query_at(&client, &base, "synthetic-renewed").await?;
        assert_eq!(quota.tiers[0].utilization, 25.0);
        let payload = list_models(&client, &base, "synthetic-renewed")
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        assert_eq!(models(&payload)[0].id, "fixture");
        let response=inference(&client,&base,"synthetic-renewed",json!({"model":"fixture@500k","input":[{"type":"function_call","arguments":"{\"n\":1}"}]})).await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        server.abort();
        Ok(())
    }
    #[test]
    fn subscription_request_uses_cli_headers_and_keeps_function_arguments() {
        let mut body = json!({"model":"fixture@500k","input":[{"type":"reasoning","content":null,"encrypted_content":"synthetic"},{"type":"function_call","arguments":"{\"n\":1}"}],"prompt_cache_key":"fixture-cache"});
        let headers = shape(&mut body).expect("CLI request");
        assert_eq!(headers["x-grok-context-window"], "500000");
        assert_eq!(headers["x-grok-model-override"], "fixture");
        assert_eq!(headers["x-grok-conv-id"], "fixture-cache");
        assert_eq!(body["input"][1]["arguments"], "{\"n\":1}");
        assert!(body["input"][0].get("content").is_none());
    }
    #[test]
    fn billing_and_models_follow_the_cli_contract() {
        let quota=billing(&json!({"config":{"creditUsagePercent":37,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","end":"2026-12-01T00:00:00Z"},"onDemandUsed":{"val":2},"onDemandCap":{"val":10}}}),"xai_oauth").expect("credits");
        assert_eq!(quota.tiers[0].name, "weekly_limit");
        assert_eq!(quota.tiers[1].utilization, 20.0);
        assert!(billing(&json!({"config":{}}), "xai_oauth").is_none());
        assert!(billing(&json!({"config":{"creditUsagePercent":-1}}), "xai_oauth").is_none());
        let models = models(
            &json!({"data":[{"id":"fixture","api_backend":"responses","context_window":100000,"context_windows":[100000,500000],"reasoning_efforts":[{"value":"high"},{"value":"low"}],"input_modalities":["text","image"]},{"id":"chat-only","api_backend":"chat"}]}),
        );
        assert_eq!(models.len(), 2);
        assert_eq!(models[1].id, "fixture@500k");
        assert_eq!(models[1].display_name.as_deref(), Some("fixture @ 500k"));
        assert_eq!(models[1].context.max_context_window, Some(500000));
    }
}
