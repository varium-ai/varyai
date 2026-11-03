//! The public Agent API never asks for an Agent-owned provider identity.
use crate::{
    services::agent_connections::{self, RecoveryReason, Status, FAILED},
    store::AppState,
};
use tauri::State;
#[tauri::command]
pub(crate) async fn get_agent_native_models(
) -> Result<Vec<agent_connections::native::NativeModel>, String> {
    tokio::task::spawn_blocking(agent_connections::native::list)
        .await
        .map_err(|_| FAILED.to_owned())
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
    let (status, selected) = tokio::task::spawn_blocking(move || {
        Ok::<_, String>((client.status()?, client.selected_model()?))
    })
    .await
    .map_err(|_| FAILED)??;
    let manager = state.proxy_service.agent_connections.clone();
    let desired = tokio::task::spawn_blocking(move || manager.codex_intent())
        .await
        .map_err(|_| FAILED)??;
    statuses.push(Status {
        app: "codex".into(),
        enabled: status.enabled || status.recovery_pending || desired.is_some(),
        configured: status.enabled && !status.recovery_pending,
        recovery_pending: status.recovery_pending || (desired.is_some() && !status.enabled),
        recovery_reason: if status.orphaned_config {
            Some(RecoveryReason::Orphaned)
        } else if status.recovery_pending && !status.configuration_owned {
            Some(RecoveryReason::ConfigurationChanged)
        } else if !status.enabled && (status.recovery_pending || desired.is_some()) {
            Some(RecoveryReason::ConnectionInactive)
        } else {
            None
        },
        model: selected.or(desired.flatten()),
        restart_required: status.enabled,
        desktop_state: None,
    });
    let manager = state.proxy_service.agent_connections.clone();
    let mut bridge = tokio::task::spawn_blocking(move || manager.bridge_status())
        .await
        .map_err(|_| FAILED)??;
    bridge.configured &= state
        .proxy_service
        .intent_runtime
        .connections
        .active("trae");
    if bridge.enabled && !bridge.configured {
        bridge.recovery_pending = true;
        bridge.recovery_reason = Some(RecoveryReason::BridgePending);
    }
    statuses.push(bridge);
    let (audit_active, audit_waiting) = state.proxy_service.doubaowork_connection_status();
    statuses.push(Status {
        app: "doubaowork".into(),
        enabled: audit_active || audit_waiting,
        configured: audit_active,
        recovery_pending: false,
        recovery_reason: None,
        model: None,
        restart_required: audit_waiting,
        desktop_state: None,
    });
    Ok(statuses)
}
#[tauri::command]
pub(crate) async fn restart_agent_connection(
    state: State<'_, AppState>,
    app: String,
    confirmed: bool,
) -> Result<(), String> {
    if !confirmed {
        return Err("agentConnections.restartConfirmationRequired".into());
    }
    if app != "claude-desktop" {
        return Err("agentConnections.restartUnsupported".into());
    }
    let _guard = state.proxy_service.lock_switch_for_app(&app).await;
    if !state.proxy_service.intent_runtime.connections.active(&app) {
        return Err(agent_connections::CONFLICT.into());
    }
    let manager = state.proxy_service.agent_connections.clone();
    tokio::task::spawn_blocking(move || manager.restart_desktop(&app))
        .await
        .map_err(|_| FAILED)?
}
#[tauri::command]
pub(crate) async fn reapply_agent_connection(
    state: State<'_, AppState>,
    app: String,
) -> Result<(), String> {
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
        state.proxy_service.sync_shared_trae(false).await;
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
) -> Result<Vec<agent_connections::Model>, String> {
    state.proxy_service.ensure_global_routing().await?;
    Ok(state
        .proxy_service
        .intent_runtime
        .global_snapshot()?
        .as_ref()
        .map(|menu| agent_connections::models(menu))
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
    if app == "trae" {
        if !enabled {
            state
                .proxy_service
                .intent_runtime
                .connections
                .revoke("trae");
        }
        if enabled {
            let legacy = state
                .proxy_service
                .get_takeover_status()
                .await
                .map_err(|_| FAILED)?;
            if legacy.trae
                || legacy.trae_waiting
                || state
                    .db
                    .get_bool_flag(crate::services::trae_bridge::selection::PENDING_KEY)
                    .unwrap_or(false)
            {
                return Err(agent_connections::CONFLICT.into());
            }
            super::proxy::ensure_trae_consent(&app_handle, state.inner()).await?;
            state.proxy_service.start().await?;
            state.proxy_service.refresh_global_routing().await?;
            if state
                .proxy_service
                .intent_runtime
                .global_snapshot()?
                .as_ref()
                .is_none_or(|menu| agent_connections::models(menu).is_empty())
            {
                return Err(agent_connections::EMPTY.into());
            }
        }
        {
            let _guard = state.proxy_service.lock_switch_for_app("trae").await;
            let manager = state.proxy_service.agent_connections.clone();
            tokio::task::spawn_blocking(move || manager.bridge_select(enabled))
                .await
                .map_err(|_| FAILED)??;
        }
        state.proxy_service.sync_shared_trae(false).await;
        return Ok(());
    }
    if !agent_connections::APPS.contains(&app.as_str()) {
        return Err(FAILED.into());
    }
    if app == "codex" {
        let desired = model.clone();
        if enabled {
            state.proxy_service.start().await?;
            state
                .proxy_service
                .connect_codex_agent(true, false, model)
                .await?;
        } else {
            state.proxy_service.disable_intent_client().await?;
        }
        let manager = state.proxy_service.agent_connections.clone();
        tokio::task::spawn_blocking(move || manager.set_codex_intent(desired.as_deref(), enabled))
            .await
            .map_err(|_| FAILED)??;
        return Ok(());
    }
    set_native_connection(state.inner(), app, enabled, model).await
}

async fn set_native_connection(
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
    if legacy_enabled
        || state
            .db
            .get_live_backup(&app)
            .await
            .map_err(|_| FAILED)?
            .is_some()
    {
        return Err(agent_connections::CONFLICT.into());
    }
    let info = state.proxy_service.start().await?;
    state.proxy_service.ensure_global_routing().await?;
    let menu = state
        .proxy_service
        .intent_runtime
        .global_snapshot()?
        .ok_or(agent_connections::EMPTY)?;
    let selected = model
        .or_else(|| menu.default_model.clone())
        .or_else(|| {
            agent_connections::models(&menu)
                .first()
                .map(|model| model.id.clone())
        })
        .ok_or(agent_connections::EMPTY)?;
    let origin = format!("http://127.0.0.1:{}", info.port);
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
                let wire = crate::claude_desktop_config::shared_model_name(&ordered[0]);
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
                    "the authenticated model list must use the saved Desktop order too"
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
