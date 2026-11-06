//! Control-plane operations for global upstream resources. Secret retrieval is explicit.
use crate::{
    proxy::providers::codex_oauth_auth::CodexOAuthManager,
    services::routing_upstreams::{RoutingUpstream, UpstreamSource},
    store::AppState,
};
use tauri::{Manager, State};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderSummary {
    #[serde(flatten)]
    upstream: RoutingUpstream,
    #[serde(skip_serializing_if = "Option::is_none")]
    subscription_plans: Option<Vec<Option<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reauth_required: Option<bool>,
}

async fn subscription_summaries(
    records: Vec<RoutingUpstream>,
    manager: &CodexOAuthManager,
    native_plan: Option<&str>,
    native_ready: bool,
) -> Vec<ProviderSummary> {
    let accounts = manager.list_accounts().await;
    let mut plans = std::collections::HashMap::<String, Option<String>>::new();
    let mut summaries = Vec::with_capacity(records.len());
    for upstream in records {
        let reauth_required = matches!(upstream.source, UpstreamSource::Codex).then(|| {
            let settings = upstream.codex_settings.clone().unwrap_or_default();
            settings.accounts.is_empty()
                || settings.accounts.iter().any(|id| {
                    if id == "native" {
                        !native_ready
                    } else {
                        accounts
                            .iter()
                            .find(|account| &account.id == id)
                            .is_none_or(|account| account.reauth_required)
                    }
                })
        });
        let subscription_plans = if matches!(upstream.source, UpstreamSource::Codex) {
            let settings = upstream.codex_settings.clone().unwrap_or_default();
            let mut selected = Vec::new();
            for account in settings.accounts {
                let plan = if account == "native" {
                    native_plan.map(str::to_owned)
                } else if let Some(plan) = plans.get(&account) {
                    plan.clone()
                } else {
                    let plan = manager.editor_plan(&account).await;
                    plans.insert(account, plan.clone());
                    plan
                };
                if !selected.contains(&plan) {
                    selected.push(plan);
                }
            }
            Some(selected)
        } else {
            None
        };
        summaries.push(ProviderSummary {
            upstream,
            subscription_plans,
            reauth_required,
        });
    }
    summaries
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopySource {
    id: String,
    revision: u32,
}

async fn refresh(state: &AppState) {
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
}

#[tauri::command]
pub(crate) async fn routing_upstream_keys(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
) -> Result<Vec<crate::services::routing_upstreams::KeySummary>, String> {
    let db = state.db.clone();
    let record_id = id.clone();
    let mut keys =
        tokio::task::spawn_blocking(move || db.routing_upstream_keys(&record_id, revision))
            .await
            .map_err(|_| "routingUpstreams.failed")??;
    for key in &mut keys {
        key.running = state
            .proxy_service
            .intent_runtime
            .traces
            .key_running(&id, &key.observation);
    }
    Ok(keys)
}

#[tauri::command]
pub(crate) async fn reveal_routing_upstream_key(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
    key_id: String,
) -> Result<String, String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.reveal_routing_upstream_key(&id, revision, &key_id))
        .await
        .map_err(|_| "routingUpstreams.failed")?
}

#[tauri::command]
pub(crate) async fn fetch_provider_icon(url: String, automatic: bool) -> Result<String, String> {
    crate::services::provider_icons::fetch(&url, automatic).await
}

#[tauri::command]
pub(crate) async fn preview_provider_import(
    state: State<'_, AppState>,
    magpie_path: Option<String>,
) -> Result<Vec<crate::services::provider_import::Source>, String> {
    let db = state.db.clone();
    let paths = crate::services::provider_import::ImportPaths::local(magpie_path);
    crate::services::provider_import::preview_with_accounts(
        db,
        state.codex_oauth_manager.clone(),
        paths,
    )
    .await
}

#[tauri::command]
pub(crate) async fn import_provider_sources(
    state: State<'_, AppState>,
    picks: Vec<crate::services::provider_import::Pick>,
    magpie_path: Option<String>,
) -> Result<crate::services::provider_import::ImportResult, String> {
    let db = state.db.clone();
    let paths = crate::services::provider_import::ImportPaths::local(magpie_path);
    let result = crate::services::provider_import::import_with_accounts(
        db,
        state.codex_oauth_manager.clone(),
        paths,
        picks,
    )
    .await?;
    refresh(state.inner()).await;
    Ok(result)
}

#[tauri::command]
pub(crate) async fn routing_upstream_editor(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
) -> Result<super::codex_provider::Editor, String> {
    super::codex_provider::editor(state.inner(), id, revision).await
}

#[tauri::command]
pub(crate) async fn list_routing_upstreams(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ProviderSummary>, String> {
    let db = state.db.clone();
    let records = tokio::task::spawn_blocking(move || db.list_routing_upstreams())
        .await
        .map_err(|_| "routingUpstreams.failed")??;
    let native = if records.iter().any(|record| {
        matches!(record.source, UpstreamSource::Codex)
            && record
                .codex_settings
                .as_ref()
                .is_none_or(|settings| settings.accounts.iter().any(|account| account == "native"))
    }) {
        tokio::task::spawn_blocking(|| {
            crate::services::intent_routing::catalog::native_editor_info()
        })
        .await
        .ok()
        .flatten()
    } else {
        None
    };
    let mut summaries = subscription_summaries(
        records,
        &state.codex_oauth_manager,
        native.as_ref().and_then(|info| info.plan.as_deref()),
        native.is_some(),
    )
    .await;
    // Status queries read local records only. A failed status read is unknown, not logged out.
    for summary in &mut summaries {
        if let UpstreamSource::Subscription {
            provider,
            account_id,
            ..
        } = &summary.upstream.source
        {
            use crate::services::routing_upstreams::SubscriptionProvider;
            let observation = match provider {
                SubscriptionProvider::GithubCopilot => {
                    if let Some(auth) = app.try_state::<super::CopilotAuthState>() {
                        let manager = auth.0.read().await;
                        let account = manager.get_account(account_id).await;
                        let plan = manager.editor_plan(account_id).await;
                        Some((
                            account
                                .as_ref()
                                .is_none_or(|account| account.reauth_required),
                            plan,
                        ))
                    } else {
                        None
                    }
                }
                SubscriptionProvider::XaiOauth => {
                    if let Some(auth) = app.try_state::<super::XaiOAuthState>() {
                        let manager = auth.0.read().await;
                        let accounts = manager.list_accounts().await;
                        Some((
                            accounts
                                .iter()
                                .find(|account| &account.id == account_id)
                                .is_none_or(|account| account.requires_reauth),
                            None,
                        ))
                    } else {
                        None
                    }
                }
                provider => crate::services::native_subscription::list(*provider)
                    .await
                    .ok()
                    .map(|accounts| {
                        let account = accounts.iter().find(|account| &account.id == account_id);
                        (
                            account.is_none_or(|account| account.reauth_required),
                            account.and_then(|account| account.plan.clone()),
                        )
                    }),
            };
            if let Some((reauth, plan)) = observation {
                summary.reauth_required = Some(reauth);
                summary.subscription_plans = Some(vec![plan]);
            }
        }
    }
    Ok(summaries)
}

#[tauri::command]
pub(crate) async fn save_routing_upstream(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    upstream: RoutingUpstream,
    copy_from: Option<CopySource>,
) -> Result<RoutingUpstream, String> {
    if let UpstreamSource::Subscription {
        provider,
        account_id,
        ..
    } = &upstream.source
    {
        let available = match provider {
            crate::services::routing_upstreams::SubscriptionProvider::GithubCopilot => {
                let manager = app
                    .try_state::<super::CopilotAuthState>()
                    .ok_or("routingUpstreams.sourceUnavailable")?;
                let manager = manager.0.read().await;
                manager.get_account(account_id).await.is_some()
            }
            crate::services::routing_upstreams::SubscriptionProvider::XaiOauth => {
                let manager = app
                    .try_state::<super::XaiOAuthState>()
                    .ok_or("routingUpstreams.sourceUnavailable")?;
                let manager = manager.0.read().await;
                manager
                    .list_accounts()
                    .await
                    .iter()
                    .any(|account| &account.id == account_id && !account.requires_reauth)
            }
            service => crate::services::native_subscription::exists(*service, account_id).await,
        };
        if !available {
            return Err("routingUpstreams.sourceUnavailable".into());
        }
    }
    let db = state.db.clone();
    let saved = tokio::task::spawn_blocking(move || match copy_from {
        Some(source) => db.save_routing_upstream_copy(upstream, &source.id, source.revision),
        None => db.save_routing_upstream(upstream),
    })
    .await
    .map_err(|_| "routingUpstreams.failed")??;
    refresh(state.inner()).await;
    Ok(saved)
}

#[tauri::command]
pub(crate) async fn probe_routing_upstream(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
    model: String,
) -> Result<Vec<crate::services::provider_protocol_detection::ProtocolDetection>, String> {
    if model.trim().is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err("routingUpstreams.invalid".into());
    }
    let db = state.db.clone();
    let provider =
        tokio::task::spawn_blocking(move || db.routing_discovery_provider(&id, revision))
            .await
            .map_err(|_| "routingUpstreams.failed")??;
    if crate::services::subscription_provider::binding(&provider).is_some() {
        return Err("routingUpstreams.invalid".into());
    }
    let provider = crate::services::routing_upstreams::first_enabled(&provider)?;
    let (base_url, api_key) =
        provider.resolve_usage_credentials(&crate::app_config::AppType::Codex);
    let config = crate::services::provider_protocol_detection::DetectionConfig {
        base_url,
        api_key,
        model: model.trim().into(),
        anthropic_auth_field: Some(
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.api_key_field.clone())
                .unwrap_or_else(|| "ANTHROPIC_AUTH_TOKEN".into()),
        ),
    };
    crate::services::provider_protocol_detection::detect(config)
        .await
        .map_err(|_| "routingUpstreams.failed".into())
}

#[tauri::command]
pub(crate) async fn delete_routing_upstream(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
) -> Result<(), String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.delete_routing_upstream(&id, revision))
        .await
        .map_err(|_| "routingUpstreams.failed")??;
    refresh(state.inner()).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn discover_routing_upstream_models(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
) -> Result<Vec<crate::services::model_fetch::FetchedModel>, String> {
    let db = state.db.clone();
    let provider =
        tokio::task::spawn_blocking(move || db.routing_discovery_provider(&id, revision))
            .await
            .map_err(|_| "routingUpstreams.failed")??;
    if crate::services::subscription_provider::binding(&provider).is_some() {
        return Err("routingUpstreams.invalid".into());
    }
    let provider = crate::services::routing_upstreams::first_enabled(&provider)?;
    let (base, key) = provider.resolve_usage_credentials(&crate::app_config::AppType::Codex);
    let format = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .ok_or("routingUpstreams.sourceUnavailable")?;
    let auth = provider.settings_config["authType"].as_str();
    let header = if format == "gemini_native" {
        "x-goog-api-key"
    } else if format == "anthropic" && auth == Some("api-key") {
        "x-api-key"
    } else {
        "Authorization"
    };
    let mut headers: std::collections::BTreeMap<String, String> = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.local_proxy_request_overrides.as_ref())
        .map(|overrides| {
            overrides
                .headers
                .iter()
                .filter(|(name, _)| {
                    http::HeaderName::from_bytes(name.as_bytes()).is_ok_and(|name| {
                        !crate::proxy::is_protected_local_proxy_override_header(&name)
                    })
                })
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    headers.extend([(
        header.into(),
        if header == "Authorization" {
            format!("Bearer {key}")
        } else {
            key
        },
    )]);
    if format == "anthropic" && auth == Some("both") {
        let (_, token) = provider.resolve_usage_credentials(&crate::app_config::AppType::Codex);
        headers.insert("x-api-key".into(), token);
    }
    crate::services::model_fetch::fetch_models(
        &base,
        "",
        false,
        None,
        None,
        Some(format),
        Some(&headers),
    )
    .await
    .map_err(|_| "routingUpstreams.failed".into())
}

#[cfg(test)]
mod subscription_tests {
    use super::*;
    use crate::{
        database::Database,
        proxy::providers::codex_oauth_auth::CodexOAuthManager,
        services::{codex_provider::Settings, routing_upstreams::UpstreamSource},
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    #[tokio::test]
    async fn provider_list_reports_only_selected_local_subscription_plans(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let manager = CodexOAuthManager::new(temp.path().to_owned());
        let mut ids = Vec::new();
        for (user, plan) in [
            ("plus-user", "plus"),
            ("pro-user", "pro"),
            ("unknown-user", "unknown"),
            ("prolite-user", "prolite"),
        ] {
            let claims = serde_json::json!({
                "sub": user,
                "email": format!("{user}@example.test"),
                "https://api.openai.com/auth": {
                    "chatgpt_account_id": format!("workspace-{user}"),
                    "chatgpt_plan_type": plan,
                }
            });
            let token = format!(
                "{}.{}.",
                URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#),
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?)
            );
            let account = manager
                .import_editor_account(serde_json::json!({"tokens": {
                    "id_token": token,
                    "refresh_token": format!("synthetic-refresh-{user}"),
                    "account_id": format!("workspace-{user}"),
                }}))
                .await?;
            ids.push(account.id);
        }
        let db = Database::memory()?;
        let record = |name: &str, accounts: Vec<String>, enabled| RoutingUpstream {
            notes: None,
            schema_version: 1,
            id: String::new(),
            name: name.into(),
            icon: None,
            revision: 0,
            source: UpstreamSource::Codex,
            has_api_key: false,
            enabled,
            preset_id: None,
            region: None,
            transport: None,
            codex_settings: Some(Settings {
                accounts,
                ..Default::default()
            }),
        };
        let plus = db.save_routing_upstream(record("Plus fixture", vec![ids[0].clone()], true))?;
        let prolite =
            db.save_routing_upstream(record("Prolite fixture", vec![ids[3].clone()], true))?;
        let mixed = db.save_routing_upstream(record(
            "Mixed fixture",
            vec![ids[0].clone(), ids[1].clone()],
            false,
        ))?;
        let unknown = db.save_routing_upstream(record(
            "Unknown fixture",
            vec![ids[2].clone(), "missing".into()],
            true,
        ))?;
        let native =
            db.save_routing_upstream(record("Native fixture", vec!["native".into()], true))?;
        let rows = subscription_summaries(
            db.list_routing_upstreams()?,
            &manager,
            Some("ChatGPT Business"),
            true,
        )
        .await;
        let plans = |id: &str| -> Result<&Vec<Option<String>>, Box<dyn std::error::Error>> {
            rows.iter()
                .find(|row| row.upstream.id == id)
                .and_then(|row| row.subscription_plans.as_ref())
                .ok_or_else(|| "missing summary".into())
        };
        assert_eq!(plans(&plus.id)?, &vec![Some("ChatGPT Plus".into())]);
        assert_eq!(plans(&prolite.id)?, &vec![Some("ChatGPT Prolite".into())]);
        assert_eq!(
            plans(&mixed.id)?,
            &vec![Some("ChatGPT Plus".into()), Some("ChatGPT Pro".into())]
        );
        assert_eq!(plans(&unknown.id)?, &vec![None]);
        assert_eq!(plans(&native.id)?, &vec![Some("ChatGPT Business".into())]);
        assert_eq!(
            rows.iter()
                .find(|r| r.upstream.id == plus.id)
                .and_then(|r| r.reauth_required),
            Some(false)
        );
        assert_eq!(
            rows.iter()
                .find(|r| r.upstream.id == unknown.id)
                .and_then(|r| r.reauth_required),
            Some(true)
        );
        let serialized = serde_json::to_string(&rows)?;
        for private in [
            "id_token",
            "refresh_token",
            "synthetic-refresh",
            "example.test",
        ] {
            assert!(!serialized.contains(private));
        }
        assert!(serde_json::to_value(db.list_routing_upstreams()?)?[0]
            .get("subscriptionPlans")
            .is_none());
        Ok(())
    }
}
