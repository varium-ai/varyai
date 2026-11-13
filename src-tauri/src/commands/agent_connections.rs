//! The public Agent API never asks for an Agent-owned provider identity.
use crate::services::trae_bridge::shared::Edition;
use crate::{
    services::agent_connections::{self, RecoveryReason, Status, FAILED},
    store::AppState,
};
use tauri::State;

#[cfg(test)]
#[path = "agent_connections/codex_status_tests.rs"]
mod codex_status_tests;

fn codex_connection_status(
    status: crate::services::intent_routing::client::Status,
    selected: Option<String>,
    desired: Option<Option<String>>,
) -> Status {
    Status {
        app: "codex".into(),
        // Desired intent and recovery receipts are not proof of live ownership.
        // A stopped gateway keeps its switch on only while its configuration
        // still belongs to VaryAI; an external takeover must show it as off.
        enabled: status.configuration_owned && status.configuration_issue.is_none(),
        configured: status.enabled && !status.recovery_pending,
        recovery_pending: status.recovery_pending || (desired.is_some() && !status.enabled),
        recovery_reason: if status.orphaned_config {
            Some(RecoveryReason::Orphaned)
        } else if status.recovery_pending
            && (!status.configuration_owned || status.configuration_issue.is_some())
        {
            Some(RecoveryReason::ConfigurationChanged)
        } else if !status.enabled && (status.recovery_pending || desired.is_some()) {
            Some(RecoveryReason::ConnectionInactive)
        } else {
            None
        },
        model: selected.or(desired.flatten()),
        restart_required: false,
        reload_state: None,
        desktop_state: None,
        native_compatibility: Some(status.native_compatibility),
        configuration_issue: status.configuration_issue,
    }
}

fn with_remembered_model(mut status: Status, remembered: Option<String>) -> Status {
    status.model = if status.enabled {
        status.model.or(remembered)
    } else {
        remembered.or(status.model)
    };
    status
}

fn with_codex_reload(
    mut status: Status,
    reload: Option<agent_connections::ReloadState>,
    desktop_state: Option<agent_connections::desktop::State>,
) -> Status {
    let reload_restart = !status.recovery_pending
        && matches!(
            reload,
            Some(
                agent_connections::ReloadState::StaleCatalog
                    | agent_connections::ReloadState::Failed
            )
        );
    let desktop_restart = !status.recovery_pending
        && (desktop_state == Some(agent_connections::desktop::State::RestartRequired)
            || reload_restart && desktop_state == Some(agent_connections::desktop::State::Current));
    status.desktop_state = if desktop_restart {
        Some(agent_connections::desktop::State::RestartRequired)
    } else {
        desktop_state
    };
    status.restart_required = reload_restart || desktop_restart;
    // Prefer a verified desktop-app restart over background app-server reload
    // when both are pending, so the UI offers one clear action.
    status.reload_state = if desktop_restart { None } else { reload };
    status
}

/// Diagnose only the currently owned, active Codex route; never repair it here.
#[tauri::command]
pub(crate) async fn diagnose_codex_proxy(
    state: State<'_, AppState>,
    force: Option<bool>,
) -> Result<Option<crate::services::codex_proxy_diagnostic::ProxyIssue>, String> {
    let gateway = state.proxy_service.get_status().await?;
    if !gateway.running {
        return Ok(None);
    }
    let client = state.proxy_service.intent_client.clone();
    let port = gateway.port;
    let Some(snapshot) =
        tokio::task::spawn_blocking(move || client.proxy_diagnostic_snapshot(port))
            .await
            .map_err(|_| FAILED)??
    else {
        return Ok(None);
    };
    if !snapshot.matches_listener(&gateway.address, gateway.port) {
        return Ok(None);
    }
    let issue =
        crate::services::codex_proxy_diagnostic::check(snapshot.clone(), force.unwrap_or(false))
            .await;
    if issue.is_none() {
        return Ok(None);
    }
    // Ownership and listener settings can change while discovery/probes are in flight.
    let current = state.proxy_service.get_status().await?;
    if !current.running || current.port != gateway.port || current.address != gateway.address {
        return Ok(None);
    }
    let client = state.proxy_service.intent_client.clone();
    let after = tokio::task::spawn_blocking(move || client.proxy_diagnostic_snapshot(port))
        .await
        .map_err(|_| FAILED)??;
    Ok(if after.as_ref() == Some(&snapshot) {
        issue
    } else {
        None
    })
}

#[tauri::command]
pub(crate) async fn get_agent_native_models(
) -> Result<Vec<agent_connections::native::NativeModel>, String> {
    tokio::task::spawn_blocking(agent_connections::native::list)
        .await
        .map_err(|_| FAILED.to_owned())
}
/// Read one Agent without waiting for other Agents' configuration or repair work.
#[tauri::command]
pub(crate) async fn get_agent_connection(
    state: State<'_, AppState>,
    app: String,
) -> Result<Status, String> {
    if app == "doubaowork" {
        let (active, waiting) = state.proxy_service.doubaowork_connection_status();
        let manager = state.proxy_service.agent_connections.clone();
        let desktop_state = tokio::task::spawn_blocking(move || {
            manager.doubaowork_desktop_state(active || waiting)
        })
        .await
        .map_err(|_| FAILED)??;
        let desktop_restart =
            desktop_state == Some(agent_connections::desktop::State::RestartRequired);
        return Ok(Status {
            app,
            enabled: active || waiting,
            configured: active,
            recovery_pending: false,
            recovery_reason: None,
            model: None,
            restart_required: waiting || desktop_restart,
            reload_state: None,
            desktop_state,
            native_compatibility: None,
            configuration_issue: None,
        });
    }
    if app == "codex" {
        state.proxy_service.sync_codex_configuration().await;
        let client = state.proxy_service.intent_client.clone();
        let manager = state.proxy_service.agent_connections.clone();
        let reload = state.proxy_service.codex_reload_state();
        return tokio::task::spawn_blocking(move || {
            let status = codex_connection_status(
                client.status()?,
                client.selected_model()?,
                manager.codex_intent()?,
            );
            let status = with_remembered_model(status, manager.remembered_model("codex"));
            let desktop_state = manager.codex_desktop_state(&status)?;
            Ok(with_codex_reload(status, reload, desktop_state))
        })
        .await
        .map_err(|_| FAILED)?;
    }
    if Edition::parse(&app).is_none() && !agent_connections::APPS.contains(&app.as_str()) {
        return Err(FAILED.into());
    }
    let manager = state.proxy_service.agent_connections.clone();
    let app_id = app.clone();
    let mut status = tokio::task::spawn_blocking(move || {
        if let Some(edition) = Edition::parse(&app_id) {
            manager.bridge_for(edition).bridge_status()
        } else {
            manager.status(&app_id)
        }
    })
    .await
    .map_err(|_| FAILED)??;
    if status.configured && !state.proxy_service.intent_runtime.connections.active(&app) {
        if app == "trae-intl" {
            state.proxy_service.sync_shared_trae_intl(false).await;
        } else if app == "trae" {
            state.proxy_service.sync_shared_trae(false).await;
        } else {
            state.proxy_service.sync_shared_agent_catalog(&app).await;
        }
    }
    let active = state.proxy_service.intent_runtime.connections.active(&app);
    if Edition::parse(&app).is_some() {
        status.configured &= active;
        if status.enabled && !status.configured {
            status.recovery_pending = true;
            status
                .recovery_reason
                .get_or_insert(RecoveryReason::BridgePending);
        }
    } else if status.configured && !active {
        status.configured = false;
        status.recovery_pending = true;
        status.recovery_reason = Some(RecoveryReason::ConnectionInactive);
    }
    Ok(status)
}

#[tauri::command]
pub(crate) async fn get_agent_native_model(
    app: String,
) -> Result<agent_connections::native::NativeModel, String> {
    tokio::task::spawn_blocking(move || agent_connections::native::get(&app))
        .await
        .map_err(|_| FAILED)?
}

#[tauri::command]
pub(crate) async fn list_agent_connections(
    state: State<'_, AppState>,
) -> Result<Vec<Status>, String> {
    let manager = state.proxy_service.agent_connections.clone();
    let mut statuses = tokio::task::spawn_blocking(move || {
        agent_connections::APPS
            .iter()
            .filter(|app| **app != "codex")
            .map(|app| manager.status(app))
            .collect::<Result<Vec<_>, _>>()
    })
    .await
    .map_err(|_| FAILED)??;
    state.proxy_service.sync_codex_configuration().await;
    if statuses.iter().any(|status| {
        status.configured
            && !state
                .proxy_service
                .intent_runtime
                .connections
                .active(&status.app)
    }) {
        state.proxy_service.sync_shared_agent_catalogs().await;
    }
    for status in &mut statuses {
        if status.configured
            && !state
                .proxy_service
                .intent_runtime
                .connections
                .active(&status.app)
        {
            status.configured = false;
            status.recovery_pending = true;
            status.recovery_reason = Some(RecoveryReason::ConnectionInactive);
        }
    }
    let client = state.proxy_service.intent_client.clone();
    let manager = state.proxy_service.agent_connections.clone();
    let reload = state.proxy_service.codex_reload_state();
    let status = tokio::task::spawn_blocking(move || {
        let status = codex_connection_status(
            client.status()?,
            client.selected_model()?,
            manager.codex_intent()?,
        );
        let status = with_remembered_model(status, manager.remembered_model("codex"));
        let desktop_state = manager.codex_desktop_state(&status)?;
        Ok::<_, String>(with_codex_reload(status, reload, desktop_state))
    })
    .await
    .map_err(|_| FAILED)??;
    statuses.push(status);
    for edition in [Edition::China, Edition::International] {
        let manager = state.proxy_service.agent_connections.clone();
        let mut bridge =
            tokio::task::spawn_blocking(move || manager.bridge_for(edition).bridge_status())
                .await
                .map_err(|_| FAILED)??;
        bridge.configured &= state
            .proxy_service
            .intent_runtime
            .connections
            .active(edition.app());
        if bridge.enabled && !bridge.configured {
            bridge.recovery_pending = true;
            bridge
                .recovery_reason
                .get_or_insert(RecoveryReason::BridgePending);
        }
        statuses.push(bridge);
    }
    let (audit_active, audit_waiting) = state.proxy_service.doubaowork_connection_status();
    let manager = state.proxy_service.agent_connections.clone();
    let desktop_state = tokio::task::spawn_blocking(move || {
        manager.doubaowork_desktop_state(audit_active || audit_waiting)
    })
    .await
    .map_err(|_| FAILED)??;
    let desktop_restart = desktop_state == Some(agent_connections::desktop::State::RestartRequired);
    statuses.push(Status {
        app: "doubaowork".into(),
        enabled: audit_active || audit_waiting,
        configured: audit_active,
        recovery_pending: false,
        recovery_reason: None,
        model: None,
        restart_required: audit_waiting || desktop_restart,
        reload_state: None,
        desktop_state,
        native_compatibility: None,
        configuration_issue: None,
    });
    Ok(statuses)
}
#[cfg(test)]
mod desktop_restart_tests {
    use super::*;

    #[test]
    fn stale_chatgpt_catalog_requests_a_full_application_restart() {
        let status = Status {
            app: "codex".into(),
            enabled: true,
            configured: true,
            recovery_pending: false,
            recovery_reason: None,
            model: None,
            restart_required: false,
            reload_state: None,
            desktop_state: None,
            native_compatibility: None,
            configuration_issue: None,
        };
        let status = with_codex_reload(
            status,
            Some(agent_connections::ReloadState::StaleCatalog),
            Some(agent_connections::desktop::State::Current),
        );
        assert!(status.restart_required);
        assert_eq!(
            status.desktop_state,
            Some(agent_connections::desktop::State::RestartRequired)
        );
        assert_eq!(status.reload_state, None);
    }

    #[test]
    fn confirmation_is_required_for_every_app_and_only_verified_desktops_are_selected() {
        for app in [
            "claude-desktop",
            "workbuddy",
            "workbuddy-ai",
            "codex",
            "qoder",
            "trae",
            "doubaowork",
            "kimi",
            "zcode",
            "deepseek-harness",
            "claude",
            "unknown",
        ] {
            assert_eq!(
                validate_restart_request(app, false),
                Err("agentConnections.restartConfirmationRequired".into())
            );
        }
        for app in [
            "claude-desktop",
            "workbuddy",
            "workbuddy-ai",
            "codex",
            "qoder",
            "trae",
            "doubaowork",
            "kimi",
            "zcode",
            "deepseek-harness",
        ] {
            assert_eq!(validate_restart_request(app, true), Ok(()));
        }
        for app in ["claude", "gemini", "opencode", "pi", "unknown"] {
            assert_eq!(
                validate_restart_request(app, true),
                Err("agentConnections.restartUnsupported".into())
            );
        }
    }
}

fn validate_restart_request(app: &str, confirmed: bool) -> Result<(), String> {
    if !confirmed {
        return Err("agentConnections.restartConfirmationRequired".into());
    }
    if !agent_connections::desktop::supports_restart(app) {
        return Err("agentConnections.restartUnsupported".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn get_agent_model_control(
    state: State<'_, AppState>,
    app: String,
) -> Result<agent_connections::model_control::Status, String> {
    let manager = state.proxy_service.agent_connections.clone();
    tokio::task::spawn_blocking(move || manager.model_control_status(&app))
        .await
        .map_err(|_| FAILED)?
}

#[tauri::command]
pub(crate) async fn authorize_agent_model_control(
    state: State<'_, AppState>,
    app: String,
    confirmed: bool,
) -> Result<(), String> {
    let _guard = state.proxy_service.lock_switch_for_app(&app).await;
    let manager = state.proxy_service.agent_connections.clone();
    tokio::task::spawn_blocking(move || manager.authorize_model_control(&app, confirmed))
        .await
        .map_err(|_| FAILED)?
}

#[tauri::command]
pub(crate) async fn restart_agent_connection(
    state: State<'_, AppState>,
    app: String,
    confirmed: bool,
) -> Result<(), String> {
    validate_restart_request(&app, confirmed)?;
    let _guard = state.proxy_service.lock_switch_for_app(&app).await;
    if let Some(edition) = Edition::parse(&app) {
        let manager = state.proxy_service.agent_connections.clone();
        return tokio::task::spawn_blocking(move || {
            manager.bridge_for(edition).restart_bridge_desktop()
        })
        .await
        .map_err(|_| FAILED)?;
    }
    if app == "doubaowork" {
        let manager = state.proxy_service.agent_connections.clone();
        return tokio::task::spawn_blocking(move || manager.restart_doubaowork_desktop())
            .await
            .map_err(|_| FAILED)?;
    }
    if app == "codex" {
        let client = state.proxy_service.intent_client.clone();
        let manager = state.proxy_service.agent_connections.clone();
        let reload = state.proxy_service.codex_reload_state();
        let (status, desktop_state) = tokio::task::spawn_blocking(move || {
            let status = codex_connection_status(
                client.status()?,
                client.selected_model()?,
                manager.codex_intent()?,
            );
            let status = with_remembered_model(status, manager.remembered_model("codex"));
            let desktop_state = manager.codex_desktop_state(&status)?;
            Ok::<_, String>((status, desktop_state))
        })
        .await
        .map_err(|_| FAILED)??;
        if status.configuration_issue.is_some() || status.recovery_pending {
            return Err(agent_connections::CONFLICT.into());
        }
        let desktop_restart = matches!(
            desktop_state,
            Some(
                agent_connections::desktop::State::RestartRequired
                    | agent_connections::desktop::State::Current
            )
        );
        let reload_restart = matches!(
            reload,
            Some(
                agent_connections::ReloadState::StaleCatalog
                    | agent_connections::ReloadState::Failed
            )
        );
        if desktop_restart {
            let manager = state.proxy_service.agent_connections.clone();
            return tokio::task::spawn_blocking(move || manager.restart_codex_desktop())
                .await
                .map_err(|_| FAILED)?;
        }
        if reload_restart {
            return state.proxy_service.request_codex_reload();
        }
        return Err("agentConnections.restartChanged".into());
    }
    let active = state.proxy_service.intent_runtime.connections.active(&app);
    let manager = state.proxy_service.agent_connections.clone();
    tokio::task::spawn_blocking(move || {
        // Connected restarts need a live gateway. A disconnected restart instead
        // validates the token-free restoration fingerprints in the manager.
        if !active
            && manager
                .load(&app)?
                .is_some_and(|receipt| !receipt.suspended)
        {
            return Err(agent_connections::CONFLICT.into());
        }
        manager.restart_desktop(&app)
    })
    .await
    .map_err(|_| FAILED)?
}
#[tauri::command]
pub(crate) async fn reapply_agent_connection(
    state: State<'_, AppState>,
    app: String,
) -> Result<(), String> {
    if app == "trae-intl" {
        let edition = Edition::International;
        if state
            .proxy_service
            .agent_connections
            .bridge_for(edition)
            .bridge_enabled()
        {
            state.proxy_service.start().await?;
        }
        state.proxy_service.refresh_global_routing().await?;
        {
            let _guard = state.proxy_service.lock_switch_for_app(edition.app()).await;
            state
                .proxy_service
                .agent_connections
                .bridge_for(edition)
                .bridge_retry()?;
        }
        if !state
            .proxy_service
            .agent_connections
            .bridge_for(edition)
            .bridge_enabled()
            || state.proxy_service.prepare_shared_trae(edition).await
        {
            state.proxy_service.sync_shared_trae_intl(false).await;
        }
        return Ok(());
    }
    if app == "trae" {
        if state.proxy_service.agent_connections.bridge_enabled() {
            state.proxy_service.start().await?;
        }
        state.proxy_service.refresh_global_routing().await?;
        {
            let _guard = state.proxy_service.lock_switch_for_app("trae").await;
            let manager = state.proxy_service.agent_connections.clone();
            tokio::task::spawn_blocking(move || manager.bridge_retry())
                .await
                .map_err(|_| FAILED)??;
        }
        if !state.proxy_service.agent_connections.bridge_enabled()
            || state
                .proxy_service
                .prepare_shared_trae(Edition::China)
                .await
        {
            state.proxy_service.sync_shared_trae(false).await;
        }
        return Ok(());
    }
    state
        .proxy_service
        .reapply_agent_connection(&app)
        .await
        .map_err(|error| {
            match error.as_str() {
                agent_connections::EMPTY => agent_connections::EMPTY,
                agent_connections::CONFLICT | "intentRouting.clientConflict" => {
                    agent_connections::CONFLICT
                }
                _ => FAILED,
            }
            .to_owned()
        })
}
#[tauri::command]
pub(crate) async fn get_agent_model_catalog(
    state: State<'_, AppState>,
) -> Result<Vec<agent_connections::PickerModel>, String> {
    state.proxy_service.ensure_global_routing().await?;
    Ok(state
        .proxy_service
        .intent_runtime
        .global_snapshot()?
        .as_ref()
        .map(|menu| agent_connections::picker_models(menu))
        .unwrap_or_default())
}
#[tauri::command]
pub(crate) async fn get_agent_model_visibility(
    state: State<'_, AppState>,
    app: String,
) -> Result<agent_connections::ModelVisibility, String> {
    let manager = state.proxy_service.agent_connections.clone();
    tokio::task::spawn_blocking(move || manager.model_visibility(&app))
        .await
        .map_err(|_| FAILED)?
}
#[tauri::command]
pub(crate) async fn set_agent_model_visibility(
    state: State<'_, AppState>,
    app: String,
    visibility: agent_connections::ModelVisibility,
) -> Result<(), String> {
    if app != "claude-desktop" {
        return Err(FAILED.into());
    }
    state.proxy_service.refresh_global_routing().await?;
    let menu = state
        .proxy_service
        .intent_runtime
        .global_snapshot()?
        .ok_or(agent_connections::EMPTY)?;
    let port = state.proxy_service.get_status().await?.port;
    let _guard = state.proxy_service.lock_switch_for_app(&app).await;
    let manager = state.proxy_service.agent_connections.clone();
    let runtime = state.proxy_service.intent_runtime.clone();
    let running = state.proxy_service.is_running().await;
    tokio::task::spawn_blocking(move || {
        match manager.set_model_visibility(
            &app,
            &visibility,
            &menu,
            &format!("http://127.0.0.1:{port}"),
        ) {
            Ok(Some(receipt)) if running => runtime.connections.install(&receipt),
            Ok(_) => Ok(()),
            Err(error) => {
                if manager.status(&app).map_or(true, |s| s.recovery_pending) {
                    runtime.connections.revoke(&app);
                }
                Err(error)
            }
        }
    })
    .await
    .map_err(|_| FAILED)?
}
#[tauri::command]
pub(crate) async fn set_agent_connection(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    app: String,
    enabled: bool,
    model: Option<String>,
) -> Result<(), String> {
    if app == "doubaowork" {
        return super::proxy::set_proxy_takeover_for_app(app_handle, state, app, enabled).await;
    }
    if app == "trae-intl" {
        let edition = Edition::International;
        if enabled {
            if state
                .db
                .get_all_providers(edition.app())
                .map_err(|_| FAILED)?
                .values()
                .any(|provider| {
                    provider
                        .meta
                        .as_ref()
                        .and_then(|meta| meta.live_config_managed)
                        == Some(true)
                })
            {
                return Err(agent_connections::CONFLICT.into());
            }
            if !cfg!(any(target_os = "macos", target_os = "windows")) {
                return Err("agentConnections.restartUnsupported".into());
            }
            super::proxy::ensure_trae_edition_consent(&app_handle, state.inner(), edition).await?;
            state.proxy_service.start().await?;
            state.proxy_service.refresh_global_routing().await?;
            let catalog = state
                .proxy_service
                .intent_runtime
                .global_snapshot()?
                .as_ref()
                .map(|menu| agent_connections::models(menu))
                .unwrap_or_default();
            if catalog.is_empty()
                || model
                    .as_ref()
                    .is_some_and(|selected| !catalog.iter().any(|entry| &entry.id == selected))
            {
                return Err(agent_connections::EMPTY.into());
            }
        } else {
            state
                .proxy_service
                .intent_runtime
                .connections
                .revoke(edition.app());
        }
        {
            let _guard = state.proxy_service.lock_switch_for_app(edition.app()).await;
            state
                .proxy_service
                .agent_connections
                .bridge_for(edition)
                .bridge_select_model(enabled, model)?;
        }
        if !enabled || state.proxy_service.prepare_shared_trae(edition).await {
            state.proxy_service.sync_shared_trae_intl(false).await;
        }
        return Ok(());
    }
    if app == "trae" {
        if !enabled {
            state
                .proxy_service
                .intent_runtime
                .connections
                .revoke("trae");
        }
        if enabled {
            super::proxy::ensure_trae_consent(&app_handle, state.inner()).await?;
            state.proxy_service.start().await?;
            state.proxy_service.refresh_global_routing().await?;
            let catalog = state
                .proxy_service
                .intent_runtime
                .global_snapshot()?
                .as_ref()
                .map(|menu| agent_connections::models(menu))
                .unwrap_or_default();
            if catalog.is_empty() {
                return Err(agent_connections::EMPTY.into());
            }
            if model
                .as_ref()
                .is_some_and(|selected| !catalog.iter().any(|entry| &entry.id == selected))
            {
                return Err(agent_connections::EMPTY.into());
            }
        }
        state
            .proxy_service
            .select_shared_trae_connection_model(enabled, model)
            .await?;
        if !enabled
            || state
                .proxy_service
                .prepare_shared_trae(Edition::China)
                .await
        {
            state.proxy_service.sync_shared_trae(false).await;
        }
        return Ok(());
    }
    if !agent_connections::APPS.contains(&app.as_str()) {
        return Err(FAILED.into());
    }
    if app == "codex" {
        let desired = if enabled {
            model.clone()
        } else {
            let client = state.proxy_service.intent_client.clone();
            tokio::task::spawn_blocking(move || client.selected_model().ok().flatten())
                .await
                .unwrap_or_default()
                .or_else(|| model.clone())
        };
        if enabled {
            state.proxy_service.start().await?;
            state
                .proxy_service
                .connect_codex_agent(true, false, model)
                .await?;
        } else {
            state
                .proxy_service
                .disconnect_codex_agent()
                .await
                .map_err(|error| {
                    if error == "intentRouting.clientConflict" {
                        agent_connections::CONFLICT.to_owned()
                    } else {
                        FAILED.to_owned()
                    }
                })?;
        }
        let manager = state.proxy_service.agent_connections.clone();
        let client = state.proxy_service.intent_client.clone();
        tokio::task::spawn_blocking(move || {
            let desired = if enabled {
                client.selected_model().ok().flatten().or(desired)
            } else {
                desired
            };
            manager.set_codex_intent(desired.as_deref(), enabled)
        })
        .await
        .map_err(|_| FAILED)??;
        return Ok(());
    }
    set_native_connection(state.inner(), app, enabled, model).await
}

pub(crate) async fn set_native_connection(
    state: &AppState,
    app: String,
    enabled: bool,
    model: Option<String>,
) -> Result<(), String> {
    let _guard = state.proxy_service.lock_switch_for_app(&app).await;
    let manager = state.proxy_service.agent_connections.clone();
    if !enabled {
        state.proxy_service.intent_runtime.connections.revoke(&app);
        return tokio::task::spawn_blocking(move || manager.disconnect(&app))
            .await
            .map_err(|_| FAILED)?;
    }
    let legacy_enabled = state
        .proxy_service
        .legacy_agent_connection_active(&app)
        .await?;
    let legacy = legacy_enabled
        || state
            .db
            .get_live_backup(&app)
            .await
            .map_err(|_| FAILED)?
            .is_some();
    let info = state.proxy_service.start().await?;
    state.proxy_service.ensure_global_routing().await?;
    let menu = state
        .proxy_service
        .intent_runtime
        .global_snapshot()?
        .ok_or(agent_connections::EMPTY)?;
    let catalog = agent_connections::models(&menu);
    let remembered = manager
        .remembered_model(&app)
        .filter(|model| catalog.iter().any(|entry| &entry.id == model));
    let selected = model
        .or(remembered)
        .or_else(|| menu.default_model.clone())
        .or_else(|| catalog.first().map(|model| model.id.clone()))
        .ok_or(agent_connections::EMPTY)?;
    if !catalog.iter().any(|entry| entry.id == selected) {
        return Err(agent_connections::EMPTY.into());
    }
    let origin = format!("http://127.0.0.1:{}", info.port);
    if legacy {
        if manager.owns(&app) {
            return Err(agent_connections::CONFLICT.into());
        }
        {
            let manager = manager.clone();
            let app = app.clone();
            let menu = menu.clone();
            let origin = origin.clone();
            let selected = selected.clone();
            tokio::task::spawn_blocking(move || manager.build(&app, &menu, &origin, &selected))
                .await
                .map_err(|_| FAILED)?
                .map_err(|_| agent_connections::CONFLICT)?;
        }
        state
            .proxy_service
            .release_legacy_agent_connection(&app)
            .await
            .map_err(|_| agent_connections::CONFLICT)?;
    }
    let runtime = state.proxy_service.intent_runtime.clone();
    let result = tokio::task::spawn_blocking(move || {
        if manager.load(&app)?.is_some() {
            let receipt = manager
                .update(&app, &menu, &origin, Some(&selected))?
                .ok_or(FAILED)?;
            return runtime.connections.install(&receipt);
        }
        let receipt = manager.build(&app, &menu, &origin, &selected)?;
        manager.install(receipt.clone())?;
        runtime.connections.install(&receipt)
    })
    .await
    .map_err(|_| FAILED)?;
    // The shared catalog has not changed; other adapters and bridges are unrelated.
    result
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::{
        config::write_json_file,
        database::Database,
        services::routing_upstreams::{RoutingUpstream, UpstreamSource},
    };
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::{fs, path::PathBuf, sync::Arc};

    struct IsolatedHome {
        dir: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }
    impl IsolatedHome {
        fn new() -> Result<Self, String> {
            let dir = tempfile::tempdir().map_err(|_| FAILED)?;
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Ok(Self { dir, previous })
        }
    }
    impl Drop for IsolatedHome {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => std::env::set_var("CC_SWITCH_TEST_HOME", previous),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    async fn upgrade_state() -> Result<AppState, Box<dyn std::error::Error>> {
        let db = Arc::new(Database::memory()?);
        let mut config = db.get_proxy_config().await?;
        config.listen_port = 0;
        db.update_proxy_config(config).await?;
        db.save_routing_upstream(RoutingUpstream {
            notes: None,
            transport: None,
            schema_version: 1,
            preset_id: None,
            region: None,
            codex_settings: None,
            id: uuid::Uuid::new_v4().to_string(),
            name: "Synthetic upgrade provider".into(),
            icon: None,
            revision: 0,
            has_api_key: false,
            enabled: true,
            source: UpstreamSource::Api {
                catalog: vec![],
                mode: crate::services::codex_provider::ModelMode::Picked,
                groups_only: false,
                base_url: "https://synthetic.invalid/v1".into(),
                api_key: Some("synthetic-upgrade-key".into()),
                api_format: "openai_responses".into(),
                models: vec![json!({"model":"synthetic"})],
                auth_type: None,
            },
        })?;
        Ok(AppState::new(db))
    }

    #[tokio::test]
    #[serial]
    async fn t3code_connect_refresh_and_disconnect_through_the_public_service(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _home = IsolatedHome::new()?;
        let state = upgrade_state().await?;
        for app in ["t3code"] {
            assert!(
                !state
                    .proxy_service
                    .legacy_agent_connection_active(app)
                    .await?
            );
            set_native_connection(&state, app.into(), true, None).await?;
            assert!(
                state
                    .proxy_service
                    .agent_connections
                    .status(app)?
                    .configured
            );
            assert!(state.proxy_service.intent_runtime.connections.active(app));
            state.proxy_service.sync_shared_agent_catalog(app).await;
            assert!(
                state
                    .proxy_service
                    .agent_connections
                    .status(app)?
                    .configured
            );
            assert!(state.proxy_service.intent_runtime.connections.active(app));
            let status = state.proxy_service.get_takeover_status().await?;
            assert!(status.t3code);
            set_native_connection(&state, app.into(), false, None).await?;
            assert!(
                !state
                    .proxy_service
                    .agent_connections
                    .status(app)?
                    .configured
            );
            assert!(!state.proxy_service.intent_runtime.connections.active(app));
        }
        state.proxy_service.stop().await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn legacy_takeovers_upgrade_to_shared_connections_and_restore_originals(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::{agent_clients, qoder_proxy, workbuddy_proxy};
        let _home = IsolatedHome::new()?;
        crate::settings::reload_settings()?;
        let state = upgrade_state().await?;
        let db = &state.db;
        let claude_path = crate::config::get_claude_settings_path();
        let claude = json!({"env":{"ANTHROPIC_BASE_URL":"https://synthetic.invalid","ANTHROPIC_AUTH_TOKEN":"synthetic-original"},"custom":true});
        write_json_file(&claude_path, &claude)?;
        db.save_live_backup("claude", &claude.to_string()).await?;
        let mut legacy_claude = claude.clone();
        legacy_claude["env"]["ANTHROPIC_BASE_URL"] = json!("http://127.0.0.1:1");
        legacy_claude["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("PROXY_MANAGED");
        write_json_file(&claude_path, &legacy_claude)?;
        let mut config = db.get_proxy_config_for_app("claude").await?;
        config.enabled = true;
        db.update_proxy_config_for_app(config).await?;

        let gemini = json!({"env":{"GOOGLE_GEMINI_BASE_URL":"https://synthetic.invalid","GEMINI_API_KEY":"synthetic-original","CUSTOM":"keep"}});
        crate::gemini_config::write_gemini_env_atomic(&crate::gemini_config::json_to_env(
            &gemini,
        )?)?;
        db.save_live_backup("gemini", &gemini.to_string()).await?;
        let mut legacy_gemini = gemini.clone();
        legacy_gemini["env"]["GOOGLE_GEMINI_BASE_URL"] = json!("http://127.0.0.1:1");
        crate::gemini_config::write_gemini_env_atomic(&crate::gemini_config::json_to_env(
            &legacy_gemini,
        )?)?;

        let grok = json!({"config":"# keep\ncustom = true\n"});
        crate::grok_config::write_grok_live_settings(&grok)?;
        db.save_live_backup("grokbuild", &grok.to_string()).await?;
        let mut config = db.get_proxy_config_for_app("grokbuild").await?;
        config.enabled = true;
        db.update_proxy_config_for_app(config).await?;

        let workbuddy_dir = crate::workbuddy_config::get_workbuddy_dir();
        let workbuddy_path = workbuddy_dir.join("models.json");
        let workbuddy = json!({"models":[{"id":"native","name":"Native","url":"https://synthetic.invalid/v1","apiKey":"synthetic-native"}],"custom":true});
        write_json_file(&workbuddy_path, &workbuddy)?;
        write_json_file(
            &workbuddy_dir.join("settings.json"),
            &json!({"env":{"CODEBUDDY_BASE_URL":"https://synthetic.invalid"},"custom":true}),
        )?;
        let settings_before = fs::read(workbuddy_dir.join("settings.json"))?;
        let backup =
            workbuddy_proxy::Backup::prepare(&workbuddy_dir, "http://127.0.0.1:1/workbuddy/v1")?;
        db.save_live_backup("workbuddy", &serde_json::to_string(&backup)?)
            .await?;
        backup.apply()?;
        db.set_setting(workbuddy_proxy::ENABLED_KEY, "true")?;

        let qoder_dir = crate::qoder_config::directory();
        let qoder_path = crate::qoder_config::path();
        let qoder = json!({"providers":{"native":{"baseUrl":"https://synthetic.invalid/v1","apiKey":"synthetic-native","models":[{"model":"native"}]}},"custom":true});
        write_json_file(&qoder_path, &qoder)?;
        fs::write(qoder_dir.join(".env"), "CUSTOM=keep\n")?;
        let backup =
            qoder_proxy::Backup::prepare(&qoder_dir, "http://127.0.0.1:1", "https://localhost:1")?;
        db.save_live_backup("qoder", &serde_json::to_string(&backup)?)
            .await?;
        backup.apply()?;
        db.set_setting(qoder_proxy::ENABLED_KEY, "true")?;

        let mut native_baselines = vec![];
        for app in ["kimi", "qwen", "zcode"] {
            let path = agent_clients::config_path(app)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(
                &path,
                if app == "kimi" {
                    "custom = true\n"
                } else {
                    "{}"
                },
            )?;
            let mut doc = agent_clients::Document::read(app, &path)
                .map_err(|_| format!("synthetic {app} read failed"))?;
            doc.apply(app, "legacy", "Synthetic legacy", &json!({"baseUrl":"https://synthetic.invalid/v1","apiKey":"synthetic-legacy","protocol":"openai","models":[{"model":"old-model"}]})).map_err(|_| format!("synthetic {app} apply failed"))?;
            doc.value["custom"] = json!(true);
            let baseline = doc.value.clone();
            let routes = doc
                .route(app, "http://127.0.0.1:1")
                .map_err(|_| format!("synthetic {app} route failed"))?;
            db.save_live_backup(
                app,
                &serde_json::to_string(&agent_clients::Backup {
                    path: path.clone(),
                    routes,
                })?,
            )
            .await?;
            doc.write(&path)
                .map_err(|_| format!("synthetic {app} write failed"))?;
            db.set_setting(&agent_clients::enabled_key(app), "true")?;
            native_baselines.push((app, path, baseline));
        }

        for app in [
            "claude",
            "gemini",
            "grokbuild",
            "workbuddy",
            "qoder",
            "kimi",
            "qwen",
            "zcode",
        ] {
            set_native_connection(&state, app.into(), true, None).await?;
            assert!(
                state
                    .proxy_service
                    .agent_connections
                    .status(app)?
                    .configured,
                "{app}"
            );
            assert!(db.get_live_backup(app).await?.is_none(), "{app}");
            set_native_connection(&state, app.into(), false, None).await?;
        }
        assert_eq!(
            crate::config::read_json_file::<Value>(&claude_path)?,
            claude
        );
        assert_eq!(
            crate::gemini_config::env_to_json(&crate::gemini_config::read_gemini_env()?),
            gemini
        );
        assert_eq!(crate::grok_config::read_grok_live_settings()?, grok);
        assert_eq!(
            crate::config::read_json_file::<Value>(&workbuddy_path)?,
            workbuddy
        );
        assert!(fs::read(workbuddy_dir.join("settings.json"))? == settings_before);
        assert_eq!(crate::config::read_json_file::<Value>(&qoder_path)?, qoder);
        assert_eq!(fs::read_to_string(qoder_dir.join(".env"))?, "CUSTOM=keep\n");
        for (app, path, baseline) in native_baselines {
            assert_eq!(
                agent_clients::Document::read(app, &path)?.value,
                baseline,
                "{app}"
            );
            assert!(!db.get_bool_flag(&agent_clients::enabled_key(app))?);
        }
        assert!(!db.get_proxy_config_for_app("claude").await?.enabled);
        assert!(!db.get_bool_flag(workbuddy_proxy::ENABLED_KEY)?);
        assert!(!db.get_bool_flag(qoder_proxy::ENABLED_KEY)?);
        state.proxy_service.stop().await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn legacy_upgrade_rejects_invalid_models_and_retains_unreadable_recovery(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::workbuddy_proxy;
        let _home = IsolatedHome::new()?;
        crate::settings::reload_settings()?;
        let state = upgrade_state().await?;
        let directory = crate::workbuddy_config::get_workbuddy_dir();
        write_json_file(&directory.join("models.json"), &json!({"models":[]}))?;
        write_json_file(
            &directory.join("settings.json"),
            &json!({"env":{"CODEBUDDY_BASE_URL":"https://synthetic.invalid"}}),
        )?;
        let backup =
            workbuddy_proxy::Backup::prepare(&directory, "http://127.0.0.1:1/workbuddy/v1")?;
        let saved = serde_json::to_string(&backup)?;
        state.db.save_live_backup("workbuddy", &saved).await?;
        backup.apply()?;
        state.db.set_setting(workbuddy_proxy::ENABLED_KEY, "true")?;
        let before = fs::read(directory.join("settings.json"))?;
        assert_eq!(
            set_native_connection(
                &state,
                "workbuddy".into(),
                true,
                Some("missing-model".into())
            )
            .await,
            Err(agent_connections::EMPTY.into())
        );
        assert!(state.db.get_live_backup("workbuddy").await?.is_some());
        assert!(fs::read(directory.join("settings.json"))? == before);
        fs::write(directory.join("models.json"), "invalid synthetic models")?;
        assert_eq!(
            set_native_connection(&state, "workbuddy".into(), true, None).await,
            Err(agent_connections::CONFLICT.into())
        );
        assert!(state.db.get_live_backup("workbuddy").await?.is_some());
        assert!(state.db.get_bool_flag(workbuddy_proxy::ENABLED_KEY)?);
        assert!(fs::read(directory.join("settings.json"))? == before);
        write_json_file(&directory.join("models.json"), &json!({"models":[]}))?;
        state
            .db
            .save_live_backup("workbuddy", "invalid synthetic recovery")
            .await?;
        assert_eq!(
            set_native_connection(&state, "workbuddy".into(), true, None).await,
            Err(agent_connections::CONFLICT.into())
        );
        assert!(state.db.get_live_backup("workbuddy").await?.is_some());
        assert!(state.db.get_bool_flag(workbuddy_proxy::ENABLED_KEY)?);
        assert!(fs::read(directory.join("settings.json"))? == before);
        assert!(!state.proxy_service.agent_connections.owns("workbuddy"));
        state.db.save_live_backup("workbuddy", &saved).await?;
        state.proxy_service.stop().await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn legacy_trae_connection_can_queue_shared_delivery_without_enabling_audit(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::trae_bridge;
        let _home = IsolatedHome::new()?;
        let state = upgrade_state().await?;
        state
            .db
            .update_security_settings(&crate::database::SecurityAuditSettings { enabled: false })?;
        state.db.set_setting(trae_bridge::ENABLED_KEY, "true")?;
        state
            .db
            .set_setting(trae_bridge::selection::PENDING_KEY, "true")?;
        state
            .proxy_service
            .select_shared_trae_connection(true)
            .await?;
        assert!(!state.db.get_bool_flag(trae_bridge::ENABLED_KEY)?);
        assert!(state
            .db
            .get_bool_flag(trae_bridge::selection::PENDING_KEY)?);
        assert!(state
            .db
            .get_bool_flag(trae_bridge::selection::ROUTE_PENDING_KEY)?);
        let bridge = state.proxy_service.agent_connections.bridge_status()?;
        assert!(bridge.enabled && bridge.recovery_pending && !bridge.configured);
        assert!(!state.db.get_security_settings()?.enabled);
        assert!(!state.proxy_service.is_running().await);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn reasoning_catalog_projects_into_native_agent_files_and_restores_them(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _home = IsolatedHome::new()?;
        let provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({
                "apiFormat":"openai_responses",
                "env":{"ANTHROPIC_BASE_URL":"https://synthetic.invalid/v1","ANTHROPIC_AUTH_TOKEN":"synthetic-upstream"},
                "auth":{"OPENAI_API_KEY":"synthetic-upstream"},
                "config":"model_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.invalid/v1'\nwire_api='responses'",
                "modelCatalog":{"models":[{"model":"synthetic","reasoningLevels":["none","low","high","max"],"reasoning":true}]}
            }),
            None,
        );
        let manager = Arc::new(
            crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(
                _home.dir.path().join("accounts"),
            ),
        );
        let menu =
            crate::services::intent_routing::menu::prepare(manager, vec![provider], vec![], None)
                .await?;
        let alias = menu.default_model.as_deref().ok_or("missing model")?;
        let catalog = agent_connections::models(&menu);
        let model = catalog
            .iter()
            .find(|model| model.id == alias)
            .ok_or("missing catalog model")?;
        assert_eq!(model.name, "synthetic");
        assert_eq!(model.group, "Synthetic");
        let connections = agent_connections::Manager {
            root: _home.dir.path().join("receipts"),
        };
        for app in ["claude", "pi", "opencode", "workbuddy"] {
            let receipt = agent_connections::build(app, &menu, "http://127.0.0.1:1", alias)?;
            let files = serde_json::to_value(&receipt)?;
            let installed = &files["files"][0]["installed"];
            match app {
                "claude" => assert_eq!(
                    installed["env"]["CLAUDE_CODE_MODEL_CAPABILITIES"],
                    "synthetic=effort,xhigh_effort,max_effort"
                ),
                "pi" => assert_eq!(
                    installed["providers"]["varyai"]["models"][0]["thinkingLevelMap"]["max"],
                    "max"
                ),
                "opencode" => {
                    let wire = agent_connections::model_wire(
                        &receipt.model_map,
                        receipt.visible_models.as_deref(),
                        alias,
                    );
                    assert_eq!(
                        installed["provider"]["varyai"]["models"][wire]["variants"]["max"]
                            ["reasoningEffort"],
                        "max"
                    );
                }
                "workbuddy" => {
                    let row = if installed.is_array() {
                        &installed[0]
                    } else {
                        &installed["models"][0]
                    };
                    assert_eq!(row["supportsReasoning"], true);
                    assert_eq!(
                        row["reasoning"]["supportedEfforts"],
                        json!(["low", "high", "max"])
                    );
                    assert_eq!(row["reasoning"]["canDisableThinking"], true);
                }
                _ => unreachable!(),
            }
            connections.install(receipt)?;
            assert!(connections.status(app)?.configured);
            connections.disconnect(app)?;
            assert!(!connections.status(app)?.configured);
        }
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn connecting_one_agent_does_not_refresh_or_wait_for_other_agents(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _home = IsolatedHome::new()?;
        let db = Arc::new(Database::memory()?);
        let mut config = db.get_proxy_config().await?;
        config.listen_port = 0;
        db.update_proxy_config(config).await?;
        db.save_routing_upstream(RoutingUpstream {
            notes: None,
            icon: None,
            transport: None,
            schema_version: 1,
            preset_id: None,
            region: None,
            codex_settings: None,
            id: uuid::Uuid::new_v4().to_string(),
            name: "Synthetic isolated provider".into(),
            revision: 0,
            has_api_key: false,
            enabled: true,
            source: UpstreamSource::Api {
                catalog: vec![],
                mode: crate::services::codex_provider::ModelMode::Picked,
                groups_only: false,
                base_url: "https://synthetic.invalid/v1".into(),
                api_key: Some("synthetic-upstream-key".into()),
                api_format: "openai_responses".into(),
                models: vec![json!({"model":"first"})],
                auth_type: None,
            },
        })?;
        let state = AppState::new(db);
        state.proxy_service.start().await?;
        set_native_connection(&state, "claude".into(), true, None).await?;
        let before = state
            .proxy_service
            .intent_runtime
            .global_snapshot()?
            .ok_or("missing menu")?;
        let manager = &state.proxy_service.agent_connections;
        let receipt_path = manager.root.join("claude.json");
        let receipt_before = fs::read(&receipt_path)?;
        let owned: Vec<(PathBuf, Vec<u8>)> =
            serde_json::to_value(manager.load("claude")?.ok_or("missing receipt")?)?["files"]
                .as_array()
                .ok_or("missing files")?
                .iter()
                .map(|file| -> Result<_, Box<dyn std::error::Error>> {
                    let path = PathBuf::from(file["path"].as_str().ok_or("missing path")?);
                    Ok((path.clone(), fs::read(path)?))
                })
                .collect::<Result<_, _>>()?;
        let unrelated = state.proxy_service.lock_switch_for_app("claude").await;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            set_native_connection(&state, "gemini".into(), true, None),
        )
        .await;
        drop(unrelated);
        let after = state
            .proxy_service
            .intent_runtime
            .global_snapshot()?
            .ok_or("missing menu")?;
        let gemini = manager.status("gemini")?;
        let receipt_unchanged = fs::read(&receipt_path)? == receipt_before;
        let fields_unchanged = owned
            .iter()
            .all(|(path, bytes)| fs::read(path).ok().as_ref() == Some(bytes));
        set_native_connection(&state, "gemini".into(), false, None).await?;
        let claude = manager.status("claude")?;
        state.proxy_service.stop().await?;
        result.map_err(|_| "connecting Gemini waited for an unrelated Agent lock")??;
        assert!(
            Arc::ptr_eq(&before, &after),
            "unchanged shared catalogs must not be rebuilt for every switch"
        );
        assert!(gemini.enabled && gemini.configured);
        assert!(claude.enabled && claude.configured);
        assert!(
            receipt_unchanged && fields_unchanged,
            "another Agent's recovery data and configuration must not change"
        );
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn desktop_command_reconnects_after_restoring_an_active_legacy_profile(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let home = IsolatedHome::new()?;
        crate::settings::reload_settings()?;
        let support = home.dir.path().join("Library/Application Support");
        let profile_id = crate::claude_desktop_config::PROFILE_ID;
        let fixtures = [
            (
                support.join("Claude/claude_desktop_config.json"),
                json!({"deploymentMode":"3p","theme":"dark"}),
            ),
            (
                support.join("Claude-3p/claude_desktop_config.json"),
                json!({"deploymentMode":"3p","locale":"en"}),
            ),
            (
                support.join(format!("Claude-3p/configLibrary/{profile_id}.json")),
                json!({"inferenceProvider":"gateway","inferenceGatewayBaseUrl":"https://synthetic.invalid","inferenceGatewayApiKey":concat!("synthetic", "-original"),"customSetting":true}),
            ),
            (
                support.join("Claude-3p/configLibrary/_meta.json"),
                json!({"entries":[{"id":profile_id,"name":"Original"}],"appliedId":profile_id}),
            ),
        ];
        let mut baseline: Vec<(PathBuf, Vec<u8>)> = vec![];
        for (path, value) in fixtures {
            write_json_file(&path, &value)?;
            baseline.push((path.clone(), fs::read(path)?));
        }
        let db = Arc::new(Database::memory()?);
        let mut config = db.get_proxy_config().await?;
        config.listen_port = 0;
        db.update_proxy_config(config).await?;
        db.save_routing_upstream(RoutingUpstream {
            notes: None,
            icon: None,
            transport: None,
            schema_version: 1,
            preset_id: None,
            region: None,
            codex_settings: None,
            id: uuid::Uuid::new_v4().to_string(),
            name: "Synthetic provider".into(),
            revision: 0,
            has_api_key: false,
            enabled: true,
            source: UpstreamSource::Api {
                catalog: vec![],
                mode: crate::services::codex_provider::ModelMode::Picked,
                groups_only: false,
                base_url: "https://synthetic.invalid/v1".into(),
                api_key: Some(concat!("synthetic", "-upstream").into()),
                api_format: "openai_responses".into(),
                models: vec![json!({"model":"first"}), json!({"model":"second"})],
                auth_type: None,
            },
        })?;
        let state = AppState::new(db.clone());
        let client = reqwest::Client::builder().no_proxy().build()?;
        for cycle in 0..2 {
            set_native_connection(&state, "claude-desktop".into(), true, None).await?;
            let manager = &state.proxy_service.agent_connections;
            let receipt = manager.load("claude-desktop")?.ok_or("missing receipt")?;
            assert!(manager.status("claude-desktop")?.configured);
            let port = state.proxy_service.get_status().await?.port;
            let endpoint = format!("http://127.0.0.1:{port}/connections/claude-desktop/v1/models");
            let response = client
                .get(&endpoint)
                .bearer_auth(&receipt.token)
                .send()
                .await?;
            assert!(response.status().is_success());
            let catalog: Value = response.json().await?;
            assert_eq!(
                catalog["data"].as_array().map(Vec::len),
                Some(if cycle == 0 { 2 } else { 1 })
            );
            let profile_path = support.join(format!("Claude-3p/configLibrary/{profile_id}.json"));
            let mut profile: Value = crate::config::read_json_file(&profile_path)?;
            profile["inferenceGatewayApiKey"] = json!("__sealed__:v1:c3ludGhldGljLW9wYXF1ZS1zZWFs");
            write_json_file(&profile_path, &profile)?;
            let sealed_profile = fs::read(&profile_path)?;
            assert!(manager.status("claude-desktop")?.configured);
            let menu = state
                .proxy_service
                .intent_runtime
                .global_snapshot()?
                .ok_or("missing menu")?;
            for _ in 0..2 {
                manager.refresh("claude-desktop", &menu, &format!("http://127.0.0.1:{port}"))?;
                assert!(
                    fs::read(&profile_path)? == sealed_profile,
                    "refresh must retain Desktop's encrypted key without rewriting its file"
                );
            }
            // A rejected plaintext credential in our gateway profile is repaired on sync.
            let mut invalid_key: Value = crate::config::read_json_file(&profile_path)?;
            invalid_key["inferenceGatewayApiKey"] = json!("synthetic-retired-local");
            write_json_file(&profile_path, &invalid_key)?;
            assert!(!manager.status("claude-desktop")?.configured);
            let repaired = manager
                .refresh("claude-desktop", &menu, &format!("http://127.0.0.1:{port}"))?
                .ok_or("missing repaired receipt")?;
            let repaired_profile: Value = crate::config::read_json_file(&profile_path)?;
            assert!(repaired_profile["inferenceGatewayApiKey"].as_str() == Some(&receipt.token));
            assert!(
                repaired.token == receipt.token,
                "repair must preserve the active connection"
            );
            assert_eq!(repaired_profile["customSetting"], true);
            assert!(manager.status("claude-desktop")?.configured);
            assert_eq!(
                client
                    .get(&endpoint)
                    .bearer_auth("synthetic-retired-local")
                    .send()
                    .await?
                    .status(),
                reqwest::StatusCode::UNAUTHORIZED,
                "credential repair must not accept the retired key"
            );
            assert!(client
                .get(&endpoint)
                .bearer_auth(&repaired.token)
                .send()
                .await?
                .status()
                .is_success());
            for field in [
                "inferenceGatewayBaseUrl",
                "inferenceProvider",
                "inferenceGatewayAuthScheme",
            ] {
                let mut external = repaired_profile.clone();
                external[field] = json!("synthetic-external");
                write_json_file(&profile_path, &external)?;
                let before = fs::read(&profile_path)?;
                assert!(matches!(
                    manager.refresh("claude-desktop", &menu, &format!("http://127.0.0.1:{port}")),
                    Err(error) if error == agent_connections::CONFLICT
                ));
                assert!(
                    fs::read(&profile_path)? == before,
                    "credential repair must not overwrite an external gateway change"
                );
                write_json_file(&profile_path, &repaired_profile)?;
            }
            if cycle == 0 {
                let ordered = agent_connections::models(&menu)
                    .into_iter()
                    .rev()
                    .map(|model| model.id)
                    .collect::<Vec<_>>();
                let sorted = manager
                    .set_model_visibility(
                        "claude-desktop",
                        &agent_connections::ModelVisibility {
                            only: false,
                            ids: vec![],
                            order: ordered.clone(),
                        },
                        &menu,
                        &format!("http://127.0.0.1:{port}"),
                    )?
                    .ok_or("missing ordered receipt")?;
                state
                    .proxy_service
                    .intent_runtime
                    .connections
                    .install(&sorted)?;
                let profile: Value = crate::config::read_json_file(&profile_path)?;
                let wire = crate::claude_desktop_config::shared_plain_model_name(&sorted.model);
                assert_eq!(profile["inferenceModels"][0]["name"], wire);
                let published: Value = client
                    .get(&endpoint)
                    .bearer_auth(&receipt.token)
                    .send()
                    .await?
                    .json()
                    .await?;
                assert_eq!(
                    published["data"][0]["id"], wire,
                    "the authenticated model list must publish the selected default first too"
                );
            }
            set_native_connection(&state, "claude-desktop".into(), false, None).await?;
            assert!(!manager.status("claude-desktop")?.enabled);
            assert_eq!(
                client
                    .get(&endpoint)
                    .bearer_auth(&receipt.token)
                    .send()
                    .await?
                    .status(),
                reqwest::StatusCode::UNAUTHORIZED
            );
            for (path, bytes) in &baseline {
                assert!(
                    fs::read(path)? == *bytes,
                    "restore must retain the original document"
                );
            }
            if cycle == 0 {
                let menu = state
                    .proxy_service
                    .intent_runtime
                    .global_snapshot()?
                    .ok_or("missing menu")?;
                let models = agent_connections::models(&menu);
                assert!(manager
                    .set_model_visibility(
                        "claude-desktop",
                        &agent_connections::ModelVisibility {
                            only: true,
                            ids: vec![models[0].id.clone()],
                            order: vec![],
                        },
                        &menu,
                        &format!("http://127.0.0.1:{port}"),
                    )?
                    .is_none());
                assert!(!manager.owns("claude-desktop"));
            }
        }
        db.save_live_backup("claude-desktop", "{}").await?;
        assert_eq!(
            set_native_connection(&state, "claude-desktop".into(), true, None).await,
            Err(agent_connections::CONFLICT.into())
        );
        for (path, bytes) in &baseline {
            assert!(fs::read(path)? == *bytes);
        }
        state.proxy_service.stop().await?;
        Ok(())
    }
}
