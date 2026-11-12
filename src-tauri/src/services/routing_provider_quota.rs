//! Read-only allowance observations for relay API keys.

use crate::services::subscription::QuotaTier;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Ready,
    Unsupported,
    Unauthorized,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Allowance {
    pub status: Status,
    pub balance: Option<f64>,
    pub tiers: Vec<QuotaTier>,
}
impl Allowance {
    fn empty(status: Status) -> Self {
        Self {
            status,
            balance: None,
            tiers: vec![],
        }
    }
}

const FAILED: &str = "routingUpstreams.failed";

fn number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|value| value.as_f64().or_else(|| value.as_str()?.parse().ok()))
        .filter(|value| value.is_finite())
}

fn reset(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(text).ok()?;
    Some(text.into())
}

fn parse_sub2api(body: &Value) -> Option<Allowance> {
    if !matches!(
        body["mode"].as_str(),
        Some("unrestricted" | "quota_limited" | "subscription")
    ) {
        return None;
    }
    if body["isValid"] == false {
        return Some(Allowance::empty(Status::Unauthorized));
    }
    let mut tiers = vec![];
    for (window, name) in [("5h", "five_hour"), ("1d", "daily"), ("7d", "seven_day")] {
        if let Some(limits) = body["rate_limits"].as_array() {
            for row in limits {
                if row["window"].as_str().map(str::trim) != Some(window) {
                    continue;
                }
                let Some(limit) = number(row.get("limit")).filter(|value| *value > 0.0) else {
                    continue;
                };
                let Some(used) = number(row.get("used")) else {
                    continue;
                };
                tiers.push(QuotaTier {
                    display: None,
                    name: name.into(),
                    utilization: (used / limit * 100.0).clamp(0.0, 100.0),
                    resets_at: reset(row.get("reset_at")),
                    used_value_usd: Some(used.max(0.0)),
                    max_value_usd: Some(limit),
                });
                break;
            }
        }
    }
    let balance = number(body.get("remaining"));
    if balance.is_none() && tiers.is_empty() {
        return None;
    }
    Some(Allowance {
        status: Status::Ready,
        balance,
        tiers,
    })
}

fn parse_new_api(body: &Value) -> Option<Allowance> {
    if body["success"] == false || body["code"] == false || body["data"]["unlimited_quota"] == true
    {
        return None;
    }
    let balance = number(body["data"].get("total_available"))? / 500_000.0;
    Some(Allowance {
        status: Status::Ready,
        balance: Some(balance),
        tiers: vec![],
    })
}

fn origin(base: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base).map_err(|_| FAILED.to_owned())?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(FAILED.into());
    }
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

/// Only the key's own queries are probed; panel login tokens are never requested.
pub(crate) async fn query(base: &str, key: &str) -> Result<Allowance, String> {
    if key.trim().is_empty() {
        return Ok(Allowance::empty(Status::Unauthorized));
    }
    let origin = origin(base)?;
    let client =
        crate::proxy::http_client::get_without_redirects().map_err(|_| FAILED.to_owned())?;
    let mut failed = false;
    let mut rejected = false;
    for path in ["/v1/usage", "/api/usage/token"] {
        let url = origin.join(path).map_err(|_| FAILED.to_owned())?;
        let response = client
            .get(url)
            .bearer_auth(key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(8))
            .send()
            .await;
        let mut response = match response {
            Ok(response) => response,
            Err(_) => {
                failed = true;
                continue;
            }
        };
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            rejected = true;
            continue;
        }
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            failed = true;
            continue;
        }
        if !status.is_success() {
            continue;
        }
        let mut bytes = vec![];
        let mut read_failed = false;
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if bytes.len() + chunk.len() <= 1024 * 1024 => {
                    bytes.extend_from_slice(&chunk)
                }
                Ok(Some(_)) | Err(_) => {
                    failed = true;
                    read_failed = true;
                    break;
                }
                Ok(None) => break,
            }
        }
        if read_failed {
            continue;
        }
        let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let reading = if path == "/v1/usage" {
            parse_sub2api(&body)
        } else {
            parse_new_api(&body)
        };
        if let Some(reading) = reading {
            return Ok(reading);
        }
    }
    if failed {
        return Err(FAILED.into());
    }
    Ok(Allowance::empty(if rejected {
        Status::Unauthorized
    } else {
        Status::Unsupported
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sub2api_wallet_and_limited_windows_are_distinct() -> Result<(), Box<dyn std::error::Error>> {
        let wallet = parse_sub2api(
            &json!({"mode":"unrestricted","isValid":true,"remaining":993.86,"unit":"USD"}),
        )
        .ok_or("wallet missing")?;
        assert_eq!(wallet.balance, Some(993.86));
        assert!(wallet.tiers.is_empty());
        let windows = parse_sub2api(
            &json!({"mode":"quota_limited","isValid":true,"rate_limits":[
                {"window":"7d","used":720,"limit":800,"reset_at":"2026-11-17T00:00:00Z"},
                {"window":"1d","used":3,"limit":300,"reset_at":null},
                {"window":"5h","used":12,"limit":0}
            ]}),
        )
        .ok_or("windows missing")?;
        assert!(windows.balance.is_none());
        assert_eq!(windows.tiers.len(), 2);
        assert_eq!(windows.tiers[0].name, "daily");
        assert_eq!(windows.tiers[0].utilization, 1.0);
        assert_eq!(windows.tiers[1].utilization, 90.0);
        assert_eq!(
            windows.tiers[1].resets_at.as_deref(),
            Some("2026-11-17T00:00:00Z")
        );
        Ok(())
    }

    #[test]
    fn unsupported_errors_and_unknown_amounts_do_not_become_zero() {
        for value in [
            json!({}),
            json!({"error":{"message":"synthetic-private-error"}}),
            json!({"mode":"unrestricted"}),
            json!({"remaining":12}),
            json!({"mode":"quota_limited","rate_limits":[{"window":"1d","limit":100}]}),
        ] {
            assert!(parse_sub2api(&value).is_none());
        }
        let rejected =
            parse_sub2api(&json!({"mode":"unrestricted","isValid":false,"remaining":100}));
        assert!(rejected.is_some_and(|q| q.status == Status::Unauthorized && q.balance.is_none()));
    }

    #[test]
    fn new_api_units_are_converted_without_inventing_unlimited_balances(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let quota = parse_new_api(&json!({"code":true,"data":{"total_available":"1500000","total_granted":2000000,"total_used":500000,"unlimited_quota":false}})).ok_or("balance missing")?;
        assert_eq!(quota.balance, Some(3.0));
        assert!(
            parse_new_api(&json!({"data":{"unlimited_quota":true,"total_available":0}})).is_none()
        );
        assert!(
            parse_new_api(&json!({"success":false,"data":{"total_available":1500000}})).is_none()
        );
        Ok(())
    }

    #[test]
    fn probe_urls_keep_only_the_configured_origin() -> Result<(), Box<dyn std::error::Error>> {
        let url =
            origin("https://relay.example/proxy/v1/responses?token=synthetic-secret#fragment")?;
        assert_eq!(
            url.join("/v1/usage")?.as_str(),
            "https://relay.example/v1/usage"
        );
        assert!(origin("file:///private/example").is_err());
        assert!(origin("https://synthetic-user:synthetic-secret@relay.example").is_err());
        Ok(())
    }

    #[tokio::test]
    async fn probes_use_the_saved_key_and_fall_back_to_new_api(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{routing::get, Json, Router};
        let app = Router::new()
            .route(
                "/v1/usage",
                get(|| async { axum::http::StatusCode::NOT_FOUND }),
            )
            .route(
                "/api/usage/token",
                get(|headers: axum::http::HeaderMap| async move {
                    if headers.get("authorization").and_then(|v| v.to_str().ok())
                        != Some("Bearer synthetic-key")
                    {
                        return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({})));
                    }
                    (
                        axum::http::StatusCode::OK,
                        Json(json!({"code":true,"data":{"total_available":1500000}})),
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let result = query(&format!("http://{addr}/v1"), "synthetic-key").await;
        server.abort();
        let quota = result?;
        assert_eq!(quota.balance, Some(3.0));
        Ok(())
    }
}
