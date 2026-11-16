use super::*;
use crate::{proxy::intent::Runtime, services::intent_routing::client};
use std::{fs, sync::Arc};

#[test]
fn receiptless_codex_configuration_is_recovery_work_instead_of_enablement(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let before = "model_provider='varyai'\nmodel='synthetic-model'\n[model_providers.varyai]\nname='VaryAI models and routing groups'\n";
    fs::write(&config, before)?;
    let manager = client::Manager::new(
        config.clone(),
        dir.path().join("receipt.json"),
        Arc::new(Runtime::default()),
    );
    let pending = codex_connection_status(manager.status()?, None, None);
    assert!(!pending.enabled);
    assert!(!pending.configured);
    assert!(pending.recovery_pending);
    assert!(matches!(
        pending.recovery_reason,
        Some(RecoveryReason::Orphaned)
    ));

    let requested = codex_connection_status(
        manager.status()?,
        None,
        Some(Some("vary-auto/synthetic".into())),
    );
    assert!(!requested.enabled);
    manager.restore()?;
    let disconnected = codex_connection_status(manager.status()?, None, None);
    assert!(!disconnected.enabled);
    assert!(disconnected.recovery_pending);
    assert_eq!(fs::read_to_string(config)?, before);
    Ok(())
}

#[test]
fn disconnected_codex_uses_the_last_choice_instead_of_its_restored_native_model() {
    let status = client::Status {
        enabled: false,
        configuration_owned: false,
        provider_enabled: false,
        recovery_pending: true,
        orphaned_config: false,
        orphan_cleanup_available: false,
        group_ids: vec![],
        omitted_entries: 0,
        native_compatibility: "unsupported".into(),
        configuration_issue: None,
    };
    let restored = codex_connection_status(status, Some("vary-auto/first".into()), None);
    let remembered = with_remembered_model(restored, Some("vary-auto/second".into()));
    assert_eq!(remembered.model.as_deref(), Some("vary-auto/second"));
    assert!(!remembered.enabled);
    assert!(remembered.recovery_pending);
}

#[test]
fn codex_with_an_unowned_recovery_receipt_shows_disabled() {
    let status = client::Status {
        enabled: false,
        configuration_owned: false,
        provider_enabled: false,
        recovery_pending: true,
        orphaned_config: false,
        orphan_cleanup_available: false,
        group_ids: vec![],
        omitted_entries: 0,
        native_compatibility: "unsupported".into(),
        configuration_issue: None,
    };
    let pending = codex_connection_status(status, Some("vary-auto/synthetic".into()), None);
    assert!(!pending.enabled);
    assert!(pending.recovery_pending);
    assert_eq!(pending.model.as_deref(), Some("vary-auto/synthetic"));
    assert!(matches!(
        pending.recovery_reason,
        Some(RecoveryReason::ConfigurationChanged)
    ));
}

#[test]
fn foreign_native_disconnect_clears_recovery_after_saved_intent_is_removed(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    let config = dir.path().join("config.toml");
    let before = format!("model_provider='magpie'\nmodel='external-model'\nopenai_base_url='{origin}/backend-api/codex'\n[model_providers.magpie]\nbase_url='{origin}/v1'\nwire_api='responses'\n[model_providers.varyai]\nname='VaryAI models and routing groups'\nbase_url='{origin}/v1'\nwire_api='responses'\nrequires_openai_auth=false\nsupports_websockets=false\nsupports_standalone_web_search=false\nexperimental_bearer_token='fixture-foreign-credential-123456'\n");
    fs::write(&config, &before)?;
    let manager = client::Manager::new(
        config.clone(),
        dir.path().join("receipt.json"),
        Arc::default(),
    );
    let pending = codex_connection_status(
        manager.status()?,
        None,
        Some(Some("vary-auto/fixture".into())),
    );
    assert!(!pending.enabled && !pending.configured && pending.recovery_pending);
    assert!(matches!(
        pending.recovery_reason,
        Some(RecoveryReason::ConnectionInactive)
    ));
    manager.disconnect()?;
    let disconnected = codex_connection_status(manager.status()?, None, None);
    assert!(!disconnected.enabled && !disconnected.configured && !disconnected.recovery_pending);
    assert!(disconnected.recovery_reason.is_none());
    assert_eq!(fs::read_to_string(config)?, before);
    Ok(())
}

#[test]
fn external_takeover_turns_the_switch_off_even_with_saved_intent() {
    let status = client::Status {
        enabled: false,
        configuration_owned: false,
        provider_enabled: false,
        recovery_pending: true,
        orphaned_config: false,
        orphan_cleanup_available: false,
        group_ids: vec![],
        omitted_entries: 0,
        native_compatibility: "unsupported".into(),
        configuration_issue: None,
    };
    let result = codex_connection_status(
        status,
        Some("vary-auto/synthetic".into()),
        Some(Some("vary-auto/synthetic".into())),
    );
    assert!(!result.enabled);
    assert!(!result.configured);
    assert!(result.recovery_pending);
    assert!(matches!(
        result.recovery_reason,
        Some(RecoveryReason::ConfigurationChanged)
    ));
}
#[test]
fn reload_progress_never_implies_configuration_ownership_or_inference_success() {
    use agent_connections::ReloadState;
    for reload in [
        ReloadState::Current,
        ReloadState::StaleCatalog,
        ReloadState::WaitingForIdle,
        ReloadState::Restarting,
        ReloadState::Failed,
        ReloadState::Stopped,
    ] {
        let status = client::Status {
            enabled: false,
            configuration_owned: false,
            provider_enabled: false,
            recovery_pending: true,
            orphaned_config: false,
            orphan_cleanup_available: false,
            group_ids: vec![],
            omitted_entries: 0,
            native_compatibility: "unsupported".into(),
            configuration_issue: None,
        };
        let result = with_codex_reload(
            codex_connection_status(status, None, None),
            Some(reload),
            None,
        );
        assert!(!result.enabled && !result.configured && result.recovery_pending);
        assert!(
            !result.restart_required,
            "configuration must be recovered before reloading"
        );
        assert_eq!(result.reload_state, Some(reload));
    }
}

#[test]
fn stopped_gateway_does_not_look_like_an_external_takeover() {
    let status = client::Status {
        enabled: false,
        configuration_owned: true,
        provider_enabled: false,
        recovery_pending: true,
        orphaned_config: false,
        orphan_cleanup_available: false,
        group_ids: vec![],
        omitted_entries: 0,
        native_compatibility: "unsupported".into(),
        configuration_issue: None,
    };
    let result = codex_connection_status(status, None, None);
    assert!(result.enabled);
    assert!(!result.configured);
    assert!(matches!(
        result.recovery_reason,
        Some(RecoveryReason::ConnectionInactive)
    ));
}

#[test]
fn current_background_server_clears_window_age_without_claiming_inference_success() {
    for reload in [
        agent_connections::ReloadState::Current,
        agent_connections::ReloadState::WaitingForIdle,
        agent_connections::ReloadState::Restarting,
        agent_connections::ReloadState::StaleCatalog,
    ] {
        let status = client::Status {
            enabled: true,
            configuration_owned: true,
            provider_enabled: true,
            recovery_pending: false,
            orphaned_config: false,
            orphan_cleanup_available: false,
            group_ids: vec![],
            omitted_entries: 0,
            native_compatibility: "unsupported".into(),
            configuration_issue: None,
        };
        let result = with_codex_reload(
            codex_connection_status(status, None, None),
            Some(reload),
            Some(agent_connections::desktop::State::RestartRequired),
        );
        assert_eq!(
            result.desktop_state,
            Some(agent_connections::desktop::State::Current)
        );
        assert_eq!(result.reload_state, Some(reload));
        assert_eq!(
            result.restart_required,
            matches!(
                reload,
                agent_connections::ReloadState::StaleCatalog
                    | agent_connections::ReloadState::WaitingForIdle
            )
        );
    }
}

fn codex_restart_fixture(reload: agent_connections::ReloadState) -> Status {
    with_codex_reload(
        Status {
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
        },
        Some(reload),
        Some(agent_connections::desktop::State::Current),
    )
}

#[tokio::test]
async fn confirmed_codex_restart_enters_native_lifecycle_without_queuing_for_idle() {
    use agent_connections::ReloadState;
    for reload in [
        ReloadState::StaleCatalog,
        ReloadState::Failed,
        ReloadState::WaitingForIdle,
    ] {
        let called = std::cell::Cell::new(false);
        let result = restart_confirmed_codex(codex_restart_fixture(reload), async {
            called.set(true);
            Err("agentConnections.restartBlocked".into())
        })
        .await;
        assert!(
            called.get(),
            "confirmed {reload:?} restart must enter the native lifecycle"
        );
        assert_eq!(result, Err("agentConnections.restartBlocked".into()));
    }
}

#[tokio::test]
async fn confirmed_codex_restart_stays_pending_until_native_relaunch_finishes() {
    let (finish, completion) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let restart = restart_confirmed_codex(
        codex_restart_fixture(agent_connections::ReloadState::StaleCatalog),
        async { completion.await.map_err(|_| "synthetic channel closed")? },
    );
    tokio::pin!(restart);
    assert!(futures::poll!(&mut restart).is_pending());
    assert!(finish.send(Ok(())).is_ok());
    assert_eq!(restart.await, Ok(()));
}

#[tokio::test]
async fn confirmed_codex_restart_preserves_conflict_and_changed_state_guards() {
    for reload in [
        agent_connections::ReloadState::StaleCatalog,
        agent_connections::ReloadState::WaitingForIdle,
        agent_connections::ReloadState::Current,
    ] {
        let mut status = codex_restart_fixture(reload);
        status.recovery_pending = reload != agent_connections::ReloadState::Current;
        let expected = if status.recovery_pending {
            agent_connections::CONFLICT
        } else {
            "agentConnections.restartChanged"
        };
        let result = restart_confirmed_codex(status, async {
            panic!("invalid state must not enter the native lifecycle")
        })
        .await;
        assert_eq!(result, Err(expected.into()));
    }
}
