//! Resolve explicitly bound subscription accounts at dispatch time.
use crate::{provider::Provider, services::routing_upstreams::SubscriptionProvider};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::Manager;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Binding {
    pub provider: SubscriptionProvider,
    pub account_id: String,
}
pub(crate) fn binding(provider: &Provider) -> Option<Binding> {
    let binding: Binding =
        serde_json::from_value(provider.settings_config.get("routingSubscription")?.clone())
            .ok()?;
    crate::services::routing_upstreams::valid_identity(&binding.account_id).then_some(binding)
}

pub(crate) async fn resolve(
    provider: &Provider,
    app: Option<&tauri::AppHandle>,
) -> Result<Provider, String> {
    let binding = binding(provider).ok_or("routingUpstreams.invalid")?;
    let (token, endpoint) = match binding.provider {
        SubscriptionProvider::GithubCopilot => {
            let state = app
                .ok_or("routingUpstreams.sourceUnavailable")?
                .try_state::<crate::commands::CopilotAuthState>()
                .ok_or("routingUpstreams.sourceUnavailable")?;
            let manager = state.0.read().await;
            let token = manager
                .get_valid_token_for_account(&binding.account_id)
                .await
                .map_err(|_| "routingUpstreams.sourceUnavailable")?;
            (token, manager.get_api_endpoint(&binding.account_id).await)
        }
        SubscriptionProvider::XaiOauth => {
            let state = app
                .ok_or("routingUpstreams.sourceUnavailable")?
                .try_state::<crate::commands::XaiOAuthState>()
                .ok_or("routingUpstreams.sourceUnavailable")?;
            let manager = state.0.read().await;
            (
                manager
                    .get_valid_token_for_account(&binding.account_id)
                    .await
                    .map_err(|_| "routingUpstreams.sourceUnavailable")?,
                binding.provider.endpoint().into(),
            )
        }
        service => {
            let account = super::native_subscription::resolve(service, &binding.account_id).await?;
            // Specialized execution owns these credentials; this placeholder is never sent.
            let token = if matches!(
                service,
                SubscriptionProvider::Claude | SubscriptionProvider::Cursor
            ) {
                "subscription-cli".to_owned()
            } else {
                account.token().to_string()
            };
            (token, service.endpoint().into())
        }
    };
    if !crate::services::routing_upstreams::valid_url(&endpoint) {
        return Err("routingUpstreams.sourceUnavailable".into());
    }
    Ok(with_credentials(
        provider,
        binding.provider,
        &endpoint,
        &token,
    ))
}

fn with_credentials(
    provider: &Provider,
    service: SubscriptionProvider,
    endpoint: &str,
    token: &str,
) -> Provider {
    let mut provider = provider.clone();
    provider.settings_config["env"]["ANTHROPIC_BASE_URL"] = json!(endpoint);
    provider.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!(token);
    provider.settings_config["auth"]["OPENAI_API_KEY"] = json!(token);
    // Account resolution supplies bearer credentials to the exact protocol bridge.
    provider.settings_config["routingSubscriptionResolved"] = json!(true);
    if service == SubscriptionProvider::GithubCopilot {
        use crate::proxy::providers::copilot_auth::*;
        let headers = [
            ("editor-version", COPILOT_EDITOR_VERSION),
            ("editor-plugin-version", COPILOT_PLUGIN_VERSION),
            ("copilot-integration-id", COPILOT_INTEGRATION_ID),
            ("user-agent", COPILOT_USER_AGENT),
            ("x-github-api-version", COPILOT_API_VERSION),
            ("openai-intent", "conversation-agent"),
            ("x-initiator", "user"),
            ("x-interaction-type", "conversation-agent"),
        ]
        .into_iter()
        .map(|(name, value)| (name.into(), value.into()))
        .collect();
        provider
            .meta
            .get_or_insert_with(Default::default)
            .local_proxy_request_overrides = Some(crate::provider::LocalProxyRequestOverrides {
            headers,
            ..Default::default()
        });
    }
    provider
}

/// Model-specific endpoints are part of the account's observed catalog.
pub(crate) fn for_model(provider: &Provider, model: &str) -> Result<Provider, String> {
    let mut provider = provider.clone();
    if !binding(&provider).is_some_and(|b| b.provider == SubscriptionProvider::GithubCopilot) {
        return Ok(provider);
    }
    let row = provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.iter().find(|r| r["model"] == model));
    let Some(endpoints) = row.and_then(|r| r["supported_endpoints"].as_array()) else {
        return Ok(provider);
    };
    let format = [
        ("/chat/completions", "openai_chat"),
        ("/responses", "openai_responses"),
        ("/v1/messages", "anthropic"),
    ]
    .into_iter()
    .find(|(endpoint, _)| endpoints.iter().any(|v| v == endpoint))
    .map(|(_, format)| format)
    .ok_or("routingUpstreams.sourceUnavailable")?;
    provider
        .meta
        .get_or_insert_with(Default::default)
        .api_format = Some(format.into());
    provider.settings_config["apiFormat"] = json!(format);
    Ok(provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn missing_runtime_never_falls_back_to_another_account() {
        let provider = Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({"routingSubscription":{"provider":"github_copilot","accountId":"fixture-account"}}),
            None,
        );
        assert!(resolve(&provider, None).await.is_err());
    }
    #[test]
    fn subscription_registry_round_trip_keeps_catalog_and_binding_without_credentials() {
        let db = crate::database::Database::memory().expect("in-memory registry");
        for service in [
            "github_copilot",
            "xai_oauth",
            "claude",
            "chatgpt_api",
            "gemini_cli",
            "antigravity",
            "cursor",
        ] {
            let draft = serde_json::from_value(json!({
                "schemaVersion":1,"id":"","name":service,"revision":0,
                "source":{"kind":"subscription","provider":service,"accountId":"fixture-account",
                    "models":[{"model":"exact-model"}],"catalog":[{"model":"exact-model"}],"mode":"picked"}
            })).expect("synthetic subscription");
            let saved = db.save_routing_upstream(draft).expect("saved subscription");
            assert_eq!(saved.revision, 1);
            let provider = db
                .routing_discovery_provider(&saved.id, saved.revision)
                .expect("execution snapshot");
            assert_eq!(
                provider.settings_config["modelCatalog"]["models"][0]["model"],
                "exact-model"
            );
            assert_eq!(
                binding(&provider).expect("bound account").account_id,
                "fixture-account"
            );
            let mut updated = saved;
            updated.enabled = false;
            let updated = db
                .save_routing_upstream(updated)
                .expect("disabled subscription");
            assert_eq!(updated.revision, 2);
        }
        let public = serde_json::to_value(db.list_routing_upstreams().expect("public registry"))
            .expect("public JSON");
        for row in public.as_array().expect("public rows") {
            assert!(row["source"].get("apiKey").is_none());
            assert!(row["source"].get("accessToken").is_none());
            assert_eq!(row["source"]["accountId"], "fixture-account");
        }
    }

    #[test]
    fn resolved_credentials_preserve_binding_and_model_without_mutating_source() {
        let source = Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({
                "env":{},"auth":{},"modelCatalog":{"models":[{"model":"exact-model"}]},
                "routingSubscription":{"provider":"github_copilot","accountId":"fixture-account"}
            }),
            None,
        );
        let resolved = with_credentials(
            &source,
            SubscriptionProvider::GithubCopilot,
            "https://copilot.example",
            "synthetic-token",
        );
        assert_eq!(
            resolved.settings_config["modelCatalog"],
            source.settings_config["modelCatalog"]
        );
        assert_eq!(
            resolved.settings_config["routingSubscription"],
            source.settings_config["routingSubscription"]
        );
        assert!(source.settings_config["auth"]
            .get("OPENAI_API_KEY")
            .is_none());
        assert!(resolved
            .meta
            .as_ref()
            .and_then(|m| m.local_proxy_request_overrides.as_ref())
            .is_some());
    }
}
