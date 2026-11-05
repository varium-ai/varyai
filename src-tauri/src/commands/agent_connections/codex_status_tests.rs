use super::*;
use crate::{proxy::intent::Runtime, services::intent_routing::client};
use std::{fs, sync::Arc};

#[test]
fn receiptless_codex_configuration_is_recovery_work_instead_of_enablement(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let before = "model_provider='varyai-routing'\nmodel='synthetic-model'\n[model_providers.varyai-routing]\nname='VaryAI models and routing groups'\n";
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
    assert!(requested.enabled);
    manager.restore()?;
    let disconnected = codex_connection_status(manager.status()?, None, None);
    assert!(!disconnected.enabled);
    assert!(disconnected.recovery_pending);
    assert_eq!(fs::read_to_string(config)?, before);
    Ok(())
}

#[test]
fn codex_with_a_recovery_receipt_remains_enabled_until_explicit_disconnect() {
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
    };
    let pending = codex_connection_status(status, Some("vary-auto/synthetic".into()), None);
    assert!(pending.enabled);
    assert!(pending.recovery_pending);
    assert_eq!(pending.model.as_deref(), Some("vary-auto/synthetic"));
    assert!(matches!(
        pending.recovery_reason,
        Some(RecoveryReason::ConfigurationChanged)
    ));
}
