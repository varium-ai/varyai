use super::*;
use axum::{http::HeaderMap, routing::post, Json, Router};

fn account() -> Account {
    serde_json::from_value(json!({
        "id":"fixture-google", "provider":"antigravity", "login":"Fixture",
        "access":"synthetic", "project":"fixture-project"
    }))
    .expect("fixture account")
}

#[tokio::test]
async fn google_observation_identifies_the_subscription_client() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback listener");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let app = Router::new().route(
        "/v1internal:loadCodeAssist",
        post(|headers: HeaderMap| async move {
            if headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with("antigravity/hub/"))
            {
                Ok(Json(
                    json!({"paidTier":{"id":"g1-pro-tier","name":"Google AI Pro"}}),
                ))
            } else {
                Err(axum::http::StatusCode::BAD_REQUEST)
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let result = codeassist(
        &account(),
        &base,
        "loadCodeAssist",
        &json!({
            "metadata":{"ideType":"ANTIGRAVITY"}
        }),
    )
    .await;
    server.abort();
    assert!(
        result.is_ok(),
        "subscription client headers must reach the observation endpoint"
    );
    assert_eq!(
        google_plan(&result.expect("observation")).as_deref(),
        Some("Google AI Pro")
    );
}

#[test]
fn antigravity_summary_keeps_explicit_exhausted_windows() {
    let quota = parse_google_quota(
        SubscriptionProvider::Antigravity,
        &json!({
            "groups":[{"displayName":"Gemini Models", "buckets":[
                {"window":"5h","remainingFraction":1.0},
                {"window":"weekly","remainingFraction":0.968}
            ]}, {"displayName":"Claude and GPT models", "buckets":[
                {"window":"5h","resetTime":"2026-11-09T08:00:00Z"},
                {"window":"weekly","disabled":true}
            ]}]
        }),
    );
    assert_eq!(
        quota.tiers.len(),
        3,
        "an explicit quota window without remainingFraction is exhausted"
    );
    assert_eq!(quota.tiers[2].utilization, 100.0);
    assert!(quota.tiers[0].name.contains("5h"));
}
