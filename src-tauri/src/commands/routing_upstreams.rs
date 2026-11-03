//! Control-plane operations for global upstream resources. Never return stored credentials.
use crate::{services::routing_upstreams::RoutingUpstream, store::AppState};
use tauri::State;

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
    state: State<'_, AppState>,
) -> Result<Vec<RoutingUpstream>, String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.list_routing_upstreams())
        .await
        .map_err(|_| "routingUpstreams.failed")?
}

#[tauri::command]
pub(crate) async fn save_routing_upstream(
    state: State<'_, AppState>,
    upstream: RoutingUpstream,
    copy_from: Option<CopySource>,
) -> Result<RoutingUpstream, String> {
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
