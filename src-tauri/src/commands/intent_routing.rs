use super::codex_oauth::CodexOAuthState;
use crate::services::intent_routing::catalog::{prepare_providers, CatalogStatus};
use crate::{
    services::intent_routing::{eligible_provider, PlanPreview, RoutingCatalog, RoutingGroup},
    store::AppState,
};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderOption {
    id: String,
    name: String,
    task_eligible: bool,
    classifier_eligible: bool,
}

#[tauri::command]
pub(crate) async fn list_intent_routing_providers(
    state: State<'_, AppState>,
) -> Result<Vec<ProviderOption>, String> {
    let db = state.db.clone();
    let providers = tauri::async_runtime::spawn_blocking(move || db.get_all_providers("codex"))
        .await
        .map_err(|_| "intentRouting.failed")?
        .map_err(|_| "intentRouting.failed")?;
    let mut options = Vec::new();
    for p in providers.values() {
        options.push(ProviderOption {
            id: p.id.clone(),
            name: p.name.clone(),
            task_eligible: crate::services::intent_routing::catalog::task_configured(
                p,
                &state.codex_oauth_manager,
            )
            .await,
            classifier_eligible: eligible_provider(p, true),
        });
    }
    Ok(options)
}

#[tauri::command]
pub(crate) async fn list_intent_routing_groups(
    state: State<'_, AppState>,
    oauth: State<'_, CodexOAuthState>,
) -> Result<RoutingCatalog, String> {
    let db = state.db.clone();
    let (providers, _) = prepare_providers(db.clone(), oauth.0.clone(), false).await?;
    tauri::async_runtime::spawn_blocking(move || {
        db.intent_routing_catalog_with_providers(Some(providers))
    })
    .await
    .map_err(|_| "intentRouting.failed")?
}

#[tauri::command]
pub(crate) async fn set_intent_routing_auto_create(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || db.set_intent_routing_auto_create(enabled))
        .await
        .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn save_intent_routing_group(
    state: State<'_, AppState>,
    group: RoutingGroup,
) -> Result<RoutingGroup, String> {
    let db = state.db.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || db.save_intent_routing_group(group))
        .await
        .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(saved)
}

#[tauri::command]
pub(crate) async fn delete_intent_routing_group(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
    oauth: State<'_, CodexOAuthState>,
) -> Result<(), String> {
    let db = state.db.clone();
    let (providers, _) = prepare_providers(db.clone(), oauth.0.clone(), false).await?;
    tauri::async_runtime::spawn_blocking(move || {
        db.delete_intent_routing_group_with_providers(&id, revision, Some(providers))
    })
    .await
    .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn preview_intent_routing_plan(
    state: State<'_, AppState>,
    group: RoutingGroup,
    classification: String,
) -> Result<PlanPreview, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        group.validate()?;
        db.validate_intent_routing_providers(&group)?;
        if group.routing == crate::services::intent_routing::RoutingStrategy::Manual {
            return group.preview(None).map_err(String::from);
        }
        let intent = group
            .parse_classification(&classification)
            .map_err(|_| "intentRouting.invalidOutput")?;
        group.preview(intent.as_deref()).map_err(String::from)
    })
    .await
    .map_err(|_| "intentRouting.failed")?
}

#[tauri::command]
pub(crate) async fn refresh_intent_routing_catalogs(
    state: State<'_, AppState>,
    oauth: State<'_, CodexOAuthState>,
) -> Result<Vec<CatalogStatus>, String> {
    let (_, statuses) = prepare_providers(state.db.clone(), oauth.0.clone(), true).await?;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(statuses)
}

#[tauri::command]
pub(crate) async fn trial_intent_routing_classification(
    state: State<'_, AppState>,
    group: RoutingGroup,
    text: String,
    consent: bool,
) -> Result<crate::services::intent_routing::classifier::TrialResult, String> {
    if !consent || text.len() > 65536 || text.trim().is_empty() {
        return Err("intentRouting.invalid".into());
    }
    group.validate()?;
    if group.routing == crate::services::intent_routing::RoutingStrategy::Manual {
        return Ok(crate::services::intent_routing::classifier::TrialResult {
            plan: group.preview(None)?,
            intent_id: None,
            outcome: "manual",
        });
    }
    let db = state.db.clone();
    let (group, classifier) = tauri::async_runtime::spawn_blocking(move || {
        let providers = db
            .get_all_providers("codex")
            .map_err(|_| "intentRouting.failed")?;
        if group
            .members
            .iter()
            .any(|member| !providers.contains_key(&member.provider_id))
        {
            return Err("intentRouting.providerUnavailable".to_string());
        }
        let target = group
            .classifier_target
            .as_ref()
            .ok_or("intentRouting.providerUnavailable")?;
        let classifier = providers
            .get(&target.provider_id)
            .cloned()
            .ok_or("intentRouting.providerUnavailable")?;
        Ok((group, classifier))
    })
    .await
    .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::classifier::trial(group, classifier, text).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoutingSession {
    token: String,
    base_url: String,
    model: String,
    model_catalog_path: String,
    expires_in_seconds: u32,
}

/// Explicit temporary execution: no client files or current-provider writes.
#[tauri::command]
pub(crate) async fn start_intent_routing_session(
    state: State<'_, AppState>,
    group: RoutingGroup,
    consent: bool,
) -> Result<RoutingSession, String> {
    if !consent {
        return Err("intentRouting.invalid".into());
    }
    let session = crate::services::intent_routing::execution::prepare(
        state.db.clone(),
        state.codex_oauth_manager.clone(),
        group,
    )
    .await?;
    let model = session.group.alias();
    let status = state.proxy_service.get_status().await?;
    // Starting a temporary session must not implicitly enable the global proxy.
    if !status.running {
        return Err("intentRouting.proxyRequired".into());
    }
    let catalog = crate::services::intent_routing::execution::temporary_catalog(&session).await?;
    let model_catalog_path = catalog.to_str().ok_or("intentRouting.failed")?.to_owned();
    let token = state
        .proxy_service
        .intent_runtime
        .install(session, Some(catalog))?;
    Ok(RoutingSession {
        token,
        base_url: format!("http://127.0.0.1:{}/intent/v1", status.port),
        model,
        model_catalog_path,
        expires_in_seconds: 3600,
    })
}

#[tauri::command]
pub(crate) fn stop_intent_routing_session(state: State<'_, AppState>, token: String) {
    state.proxy_service.intent_runtime.revoke(&token);
}

#[tauri::command]
pub(crate) async fn intent_routing_client_status(
    state: State<'_, AppState>,
) -> Result<crate::services::intent_routing::client::Status, String> {
    let manager = state.proxy_service.intent_client.clone();
    let mut status = tokio::task::spawn_blocking(move || manager.status())
        .await
        .map_err(|_| "intentRouting.clientFailed")??;
    status.provider_enabled = state
        .db
        .get_proxy_config_for_app("codex")
        .await
        .map_err(|_| "intentRouting.clientFailed")?
        .enabled;
    Ok(status)
}

#[tauri::command]
pub(crate) async fn connect_intent_routing_client(
    state: State<'_, AppState>,
    consent: bool,
) -> Result<crate::services::intent_routing::client::Status, String> {
    if !consent {
        return Err("intentRouting.invalid".into());
    }
    state.proxy_service.connect_codex_menu(true, true).await
}

#[tauri::command]
pub(crate) async fn disconnect_intent_routing_client(
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .proxy_service
        .set_takeover_for_app("codex", false)
        .await
}
