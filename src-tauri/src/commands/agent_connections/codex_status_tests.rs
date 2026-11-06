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
