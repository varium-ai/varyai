//! Redacted Codex editor observations; opening never renews credentials or probes a vendor.
use crate::{
    services::{
        codex_provider::{self, Settings},
        routing_upstreams::RoutingUpstream,
    },
    store::AppState,
};
use serde::Serialize;
use serde_json::Value;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Editor {
    #[serde(flatten)]
    upstream: RoutingUpstream,
    codex: Option<CodexEditor>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexEditor {
    settings: Settings,
    accounts: Vec<Account>,
    models: Vec<Value>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    id: String,
    login: String,
    plan: Option<String>,
    native: bool,
    ready: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelTest {
    ok: bool,
    elapsed_ms: u64,
}

#[tauri::command]
pub(crate) async fn create_codex_provider(
    state: State<'_, AppState>,
) -> Result<RoutingUpstream, String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.save_routing_upstream(RoutingUpstream {
            transport: None,
            schema_version: 1,
            id: String::new(),
            name: "ChatGPT / Codex".into(),
            revision: 0,
            source: crate::services::routing_upstreams::UpstreamSource::Codex,
            has_api_key: false,
            enabled: true,
            preset_id: None,
            region: None,
            codex_settings: Some(Settings::default()),
        })
    })
    .await
    .map_err(|_| "routingUpstreams.failed")?
}

#[tauri::command]
pub(crate) async fn test_codex_provider_model(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
    account: String,
    model: String,
) -> Result<ModelTest, String> {
    if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err("routingUpstreams.invalid".into());
    }
    let db = state.db.clone();
    let mut provider =
        tokio::task::spawn_blocking(move || db.routing_discovery_provider(&id, revision))
            .await
            .map_err(|_| "routingUpstreams.failed")??;
    if !codex_provider::is_codex(&provider) {
        return Err("routingUpstreams.invalid".into());
    }
    codex_provider::bind(&mut provider, &account);
    let (token, workspace) = crate::services::intent_routing::catalog::execution_credentials(
        &provider,
        &state.codex_oauth_manager,
    )
    .await
    .ok_or("routingUpstreams.sourceUnavailable")?;
    let client = crate::proxy::http_client::get_without_redirects()
        .map_err(|_| "routingUpstreams.failed")?;
    let version = crate::codex_client::resolve_version().await;
    let request = client
        .post("https://chatgpt.com/backend-api/codex/responses")
        .bearer_auth(token)
        .header("chatgpt-account-id", workspace)
        .header(
            "originator",
            crate::proxy::providers::codex_oauth_auth::CODEX_OAUTH_ORIGINATOR,
        )
        .header("version", &version)
        .header("User-Agent", format!("codex_cli_rs/{version}"));
    let start = std::time::Instant::now();
    let ok = crate::services::codex_provider::probe_model(request, &model).await;
    Ok(ModelTest {
        ok,
        elapsed_ms: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
    })
}

pub(crate) async fn editor(state: &AppState, id: String, revision: u32) -> Result<Editor, String> {
    let db = state.db.clone();
    let (upstream, provider) = tokio::task::spawn_blocking(move || {
        Ok::<_, String>((
            db.routing_upstream_editor(&id, revision)?,
            db.routing_discovery_provider(&id, revision)?,
        ))
    })
    .await
    .map_err(|_| "routingUpstreams.failed")??;
    if !codex_provider::is_codex(&provider)
        || !matches!(
            upstream.source,
            crate::services::routing_upstreams::UpstreamSource::Codex
        )
    {
        return Ok(Editor {
            upstream,
            codex: None,
        });
    }
    let mut settings = Settings::from_provider(&provider);
    if provider
        .settings_config
        .get(codex_provider::POLICY)
        .is_none()
        && provider.is_codex_oauth()
        && provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("codex_oauth"))
            .is_none()
    {
        settings.accounts = state
            .codex_oauth_manager
            .default_account_id()
            .await
            .into_iter()
            .collect();
    }
    let native =
        tokio::task::spawn_blocking(crate::services::intent_routing::catalog::native_editor_info)
            .await
            .map_err(|_| "routingUpstreams.failed")?;
    let mut accounts = vec![Account {
        id: "native".into(),
        login: native
            .as_ref()
            .map(|(login, _)| login.clone())
            .unwrap_or_else(|| "Codex".into()),
        plan: native.as_ref().and_then(|(_, plan)| plan.clone()),
        native: true,
        ready: native.is_some(),
    }];
    for account in state.codex_oauth_manager.list_accounts().await {
        let plan = state.codex_oauth_manager.editor_plan(&account.id).await;
        accounts.push(Account {
            id: account.id.clone(),
            login: account.login,
            plan,
            native: false,
            ready: !account.reauth_required,
        });
    }
    // Local cached catalog only. A refresh is a separate explicit action.
    let mut unfiltered = provider.clone();
    unfiltered
        .settings_config
        .as_object_mut()
        .map(|o| o.remove(codex_provider::POLICY));
    let (prepared, _) = crate::services::intent_routing::catalog::prepare_provider(
        state.db.clone(),
        state.codex_oauth_manager.clone(),
        unfiltered,
        false,
    )
    .await?;
    let models = prepared
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(Editor {
        upstream,
        codex: Some(CodexEditor {
            settings,
            accounts,
            models,
        }),
    })
}

#[tauri::command]
pub(crate) async fn import_codex_provider_account(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let value = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|_| "routingUpstreams.invalid")?;
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "routingUpstreams.invalid")?;
        if bytes.len() > 1024 * 1024 {
            return Err("routingUpstreams.invalid");
        }
        serde_json::from_slice(&bytes).map_err(|_| "routingUpstreams.invalid")
    })
    .await
    .map_err(|_| "routingUpstreams.failed")??;
    state
        .codex_oauth_manager
        .import_editor_account(value)
        .await
        .map_err(|e| match e {
            crate::proxy::providers::codex_oauth_auth::CodexOAuthError::DuplicateAccount => {
                "codex_oauth_duplicate_account"
            }
            _ => "routingUpstreams.invalid",
        })?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn save_codex_provider(
    state: State<'_, AppState>,
    upstream: RoutingUpstream,
    settings: Settings,
) -> Result<RoutingUpstream, String> {
    if !settings.valid() {
        return Err("routingUpstreams.invalid".into());
    }
    let available = state.codex_oauth_manager.list_accounts().await;
    if settings
        .accounts
        .iter()
        .any(|id| id != "native" && !available.iter().any(|a| &a.id == id && !a.reauth_required))
    {
        return Err("routingUpstreams.sourceUnavailable".into());
    }
    let db = state.db.clone();
    let saved = tokio::task::spawn_blocking(move || {
        let provider = db.routing_discovery_provider(&upstream.id, upstream.revision)?;
        if !codex_provider::is_codex(&provider) {
            return Err("routingUpstreams.invalid".into());
        }
        let mut upstream = upstream;
        upstream.codex_settings = Some(settings);
        db.save_routing_upstream(upstream)
    })
    .await
    .map_err(|_| "routingUpstreams.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(saved)
}

#[tauri::command]
pub(crate) async fn codex_provider_models(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
    account: String,
) -> Result<Vec<Value>, String> {
    let db = state.db.clone();
    let mut provider =
        tokio::task::spawn_blocking(move || db.routing_discovery_provider(&id, revision))
            .await
            .map_err(|_| "routingUpstreams.failed")??;
    if !codex_provider::is_codex(&provider) {
        return Err("routingUpstreams.invalid".into());
    }
    codex_provider::bind(&mut provider, &account);
    provider
        .settings_config
        .as_object_mut()
        .map(|o| o.remove(codex_provider::POLICY));
    let (provider, status) = crate::services::intent_routing::catalog::prepare_provider(
        state.db.clone(),
        state.codex_oauth_manager.clone(),
        provider,
        true,
    )
    .await?;
    if !status.is_some_and(|s| s.status == "updated") {
        return Err("routingUpstreams.sourceUnavailable".into());
    }
    Ok(provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

#[tauri::command]
pub(crate) async fn codex_provider_quota(
    state: State<'_, AppState>,
    account: String,
) -> Result<crate::services::subscription::SubscriptionQuota, String> {
    use crate::services::subscription::{query_codex_quota, SubscriptionQuota};
    let result = if account == "native" {
        crate::services::subscription::get_subscription_quota("codex").await
    } else {
        let manager = &state.codex_oauth_manager;
        let token = manager
            .get_valid_token_for_account(&account)
            .await
            .map_err(|_| "routingUpstreams.sourceUnavailable")?;
        let workspace = manager
            .chatgpt_account_id_for_account(&account)
            .await
            .map_err(|_| "routingUpstreams.sourceUnavailable")?;
        query_codex_quota(
            &token,
            Some(&workspace),
            "codex_oauth",
            "Account authorization unavailable",
        )
        .await
    };
    let mut quota: SubscriptionQuota = result.map_err(|_| "routingUpstreams.failed")?;
    // Vendor text and authentication errors never enter this editor.
    quota.error = None;
    quota.credential_message = None;
    Ok(quota)
}
