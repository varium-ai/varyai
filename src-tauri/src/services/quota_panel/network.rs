use super::{core::Pending, ResetCard};
use crate::{
    proxy::providers::codex_oauth_auth::CodexOAuthManager,
    services::intent_routing::catalog::ExecutionCredentials,
};
use serde::{Deserialize, Serialize};
const FAILED: &str = "quotaPanel.failed";
const ROOT: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";

#[derive(Serialize)]
pub(crate) struct Outcome {
    pub code: String,
    pub windows: u32,
}
pub(super) async fn client(
    manager: &CodexOAuthManager,
    account: &str,
) -> Result<reqwest::Client, String> {
    if account == "native" {
        crate::proxy::http_client::get_without_redirects().map_err(|_| FAILED.into())
    } else {
        manager
            .account_client(account)
            .await
            .map_err(|_| FAILED.into())
    }
}
async fn request(
    manager: &CodexOAuthManager,
    account: &str,
    context: &ExecutionCredentials,
    pending: Option<&Pending>,
) -> Result<serde_json::Value, String> {
    let token = context.token().await.ok_or("quotaPanel.loginRequired")?;
    let client = client(manager, account).await?;
    send_request(&client, &token, &context.workspace, ROOT, pending).await
}
async fn send_request(
    client: &reqwest::Client,
    token: &str,
    workspace: &str,
    root: &str,
    pending: Option<&Pending>,
) -> Result<serde_json::Value, String> {
    let mut request = match pending {
        Some(pending) => client.post(format!("{root}/consume")).json(
            &serde_json::json!({"credit_id": pending.credit, "redeem_request_id": pending.request}),
        ),
        None => client.get(root),
    };
    request = request
        .bearer_auth(token)
        .header("ChatGPT-Account-Id", workspace)
        .header("Accept", "application/json")
        .header(
            "originator",
            crate::proxy::providers::codex_oauth_auth::CODEX_OAUTH_ORIGINATOR,
        )
        .header(
            "User-Agent",
            crate::codex_client::user_agent(&crate::codex_client::current_version()),
        );
    let mut response = request
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| FAILED)?;
    if response.status().as_u16() == 401 {
        return Err("quotaPanel.loginRequired".into());
    }
    if !response.status().is_success() {
        return Err(FAILED.into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| FAILED)? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(FAILED.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| FAILED.into())
}
fn parse_cards(value: serde_json::Value, now: i64) -> Result<Vec<ResetCard>, String> {
    #[derive(Deserialize)]
    struct Credit {
        id: String,
        status: String,
        expires_at: Option<String>,
    }
    #[derive(Deserialize)]
    struct Wire {
        credits: Vec<Credit>,
    }
    let wire: Wire = serde_json::from_value(value).map_err(|_| FAILED)?;
    if wire.credits.len() > 256 {
        return Err(FAILED.into());
    }
    let mut cards = Vec::new();
    for credit in wire.credits.into_iter().filter(|c| c.status == "available") {
        if credit.id.is_empty() || credit.id.len() > 256 || credit.id.chars().any(char::is_control)
        {
            return Err(FAILED.into());
        }
        let until = credit
            .expires_at
            .map(|at| {
                chrono::DateTime::parse_from_rfc3339(&at)
                    .map(|at| at.timestamp())
                    .map_err(|_| FAILED)
            })
            .transpose()?;
        if until.is_none_or(|at| at > now) {
            cards.push(ResetCard {
                id: credit.id,
                until,
            });
        }
    }
    cards.sort_by_key(|c| c.until.unwrap_or(i64::MAX));
    Ok(cards)
}
pub(super) async fn cards(
    manager: &CodexOAuthManager,
    account: &str,
    context: &ExecutionCredentials,
) -> Result<Vec<ResetCard>, String> {
    parse_cards(
        request(manager, account, context, None).await?,
        chrono::Utc::now().timestamp(),
    )
}
pub(super) async fn consume(
    manager: &CodexOAuthManager,
    account: &str,
    context: &ExecutionCredentials,
    pending: &Pending,
) -> Result<Outcome, String> {
    let value = request(manager, account, context, Some(pending)).await?;
    let code = value["code"]
        .as_str()
        .filter(|code| {
            matches!(
                *code,
                "reset" | "nothing_to_reset" | "no_credit" | "already_redeemed"
            )
        })
        .ok_or(FAILED)?;
    let windows = value["windows_reset"].as_u64().unwrap_or(0).min(32) as u32;
    Ok(Outcome {
        code: code.into(),
        windows,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn resets_are_available_ordered_and_malformed_expiry_is_not_never() {
        let cards = parse_cards(
            serde_json::json!({"credits":[
                {"id":"never","status":"available","expires_at":null},
                {"id":"early","status":"available","expires_at":"2030-01-01T00:00:00Z"},
                {"id":"spent","status":"redeemed","expires_at":null},
                {"id":"expired","status":"available","expires_at":"2000-01-01T00:00:00Z"}
            ]}),
            1_800_000_000,
        )
        .unwrap();
        assert_eq!(
            cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["early", "never"]
        );
        assert!(parse_cards(serde_json::json!({"credits":[{"id":"invalid","status":"available","expires_at":"invalid"}]}), 1000).is_err());
    }
    #[tokio::test]
    async fn reset_transport_keeps_account_and_idempotency_identity_and_bounds_responses() {
        use axum::{
            routing::{get, post},
            Json, Router,
        };
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let copy = captured.clone();
        let app = Router::new()
            .route(
                "/resets",
                get(|| async { Json(serde_json::json!({"credits":[]})) }),
            )
            .route(
                "/resets/consume",
                post(
                    move |headers: http::HeaderMap, Json(body): Json<serde_json::Value>| {
                        let copy = copy.clone();
                        async move {
                            assert_eq!(headers["authorization"], "Bearer synthetic-token");
                            assert_eq!(headers["chatgpt-account-id"], "synthetic-workspace");
                            copy.lock().unwrap().push(body);
                            Json(serde_json::json!({"code":"reset","windows_reset":2}))
                        }
                    },
                ),
            )
            .route("/large", get(|| async { "x".repeat(1024 * 1024 + 1) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let root = format!("http://{address}/resets");
        let pending = Pending {
            credit: "synthetic-credit".into(),
            request: uuid::Uuid::new_v4().to_string(),
        };
        for _ in 0..2 {
            send_request(
                &client,
                "synthetic-token",
                "synthetic-workspace",
                &root,
                Some(&pending),
            )
            .await
            .unwrap();
        }
        let bodies = captured.lock().unwrap().clone();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0], bodies[1]);
        assert_eq!(bodies[0]["credit_id"], "synthetic-credit");
        assert_eq!(
            send_request(
                &client,
                "synthetic-token",
                "synthetic-workspace",
                &format!("http://{address}/large"),
                None
            )
            .await
            .unwrap_err(),
            FAILED
        );
        server.abort();
    }
}
