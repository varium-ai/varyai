use std::fs;
use std::sync::Arc;

use tempfile::tempdir;
use uuid::Uuid;

use crate::telemetry::{report_due, TelemetryManager};

#[test]
fn fresh_installation_does_not_create_an_identifier_or_report() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("installation-analytics.json");
    let state = TelemetryManager::load(Some(path.clone())).snapshot();
    assert!(!state.enabled);
    assert!(state.installation_id.is_none());
    assert!(!report_due(&state, "2026-10-22", "0.2.4"));
    assert!(!path.exists());
}

#[test]
fn explicit_choice_survives_restart_and_opt_out_forgets_the_identifier() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("installation-analytics.json");
    let manager = TelemetryManager::load(Some(path.clone()));
    manager.set_enabled(true).expect("enable statistics");
    let opted_in = manager.snapshot();
    let id = opted_in
        .installation_id
        .as_deref()
        .expect("installation ID");
    assert_eq!(
        Uuid::parse_str(id).expect("UUID").get_version(),
        Some(uuid::Version::Random)
    );
    let loaded = TelemetryManager::load(Some(path.clone()));
    assert!(loaded.snapshot().enabled);
    assert_eq!(loaded.snapshot().installation_id.as_deref(), Some(id));
    loaded.set_enabled(false).expect("disable statistics");
    let opted_out = TelemetryManager::load(Some(path.clone())).snapshot();
    assert!(!opted_out.enabled);
    assert!(opted_out.installation_id.is_none());
    assert!(!fs::read_to_string(&path)
        .expect("saved preferences")
        .contains(id));
    loaded.set_enabled(true).expect("enable again");
    assert_ne!(loaded.snapshot().installation_id, opted_in.installation_id);
}

#[test]
fn legacy_automatic_and_disabled_files_do_not_count_as_consent() {
    for legacy_enabled in [None, Some(false), Some(true)] {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("installation-analytics.json");
        let mut legacy = serde_json::json!({
            "schemaVersion": 1,
            "installationId": "12f57d58-7019-4e84-8ed7-f9ce4731e3cb"
        });
        if let Some(enabled) = legacy_enabled {
            legacy["enabled"] = enabled.into();
        }
        fs::write(&path, serde_json::to_vec(&legacy).expect("JSON")).expect("legacy file");
        let manager = TelemetryManager::load(Some(path.clone()));
        assert!(!manager.snapshot().enabled);
        assert!(manager.snapshot().installation_id.is_none());
        assert!(!report_due(&manager.snapshot(), "2026-10-22", "0.2.4"));
        assert!(!TelemetryManager::load(Some(path)).snapshot().enabled);
    }
}

#[test]
fn corrupted_or_future_preferences_fail_closed() {
    for content in [
        "not-json",
        r#"{"schemaVersion":255,"enabled":true}"#,
        r#"{"enabled":true,"installationId":"invalid"}"#,
    ] {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("installation-analytics.json");
        fs::write(&path, content).expect("state");
        let state = TelemetryManager::load(Some(path)).snapshot();
        assert!(!state.enabled);
        assert!(state.installation_id.is_none());
    }
}

#[test]
fn consent_is_not_enabled_when_storage_fails() {
    let manager = TelemetryManager::load(None);
    assert!(manager.set_enabled(true).is_err());
    assert!(!manager.snapshot().enabled);
    let directory = tempdir().expect("temporary directory");
    let blocked = directory.path().join("blocked");
    fs::write(&blocked, "not a directory").expect("blocked parent");
    let manager = TelemetryManager::load(Some(blocked.join("preferences.json")));
    assert!(manager.set_enabled(true).is_err());
    assert!(manager.snapshot().installation_id.is_none());
}

#[test]
fn opt_out_revokes_pending_reports_and_late_success_cannot_restore_state() {
    let directory = tempdir().expect("temporary directory");
    let manager = TelemetryManager::load(Some(directory.path().join("preferences.json")));
    manager.set_enabled(true).expect("enable");
    let expected = manager.snapshot();
    assert!(manager.permits_report(&expected));
    manager.set_enabled(false).expect("disable");
    assert!(!manager.permits_report(&expected));
    assert!(!manager
        .record_success(&expected, "2026-10-22".into(), "0.2.4")
        .expect("late result"));
    manager.set_enabled(true).expect("enable again");
    assert!(!manager.permits_report(&expected));
    assert!(!manager
        .record_success(&expected, "2026-10-22".into(), "0.2.4")
        .expect("stale result"));
}

#[tokio::test]
async fn a_disabled_report_exits_without_a_destination_or_network_request() {
    let manager = Arc::new(TelemetryManager::load(None));
    let mut changes = manager.consent_changed.subscribe();
    assert!(super::send_round(
        &Arc::downgrade(&manager),
        &manager.snapshot(),
        "0.2.4",
        &mut changes
    )
    .await
    .is_none());
}

#[test]
fn frontend_preferences_never_include_the_identifier() {
    let directory = tempdir().expect("temporary directory");
    let manager = TelemetryManager::load(Some(directory.path().join("preferences.json")));
    manager.set_enabled(true).expect("enable");
    let value = serde_json::to_value(manager.preferences()).expect("preferences");
    assert_eq!(value.as_object().expect("object").len(), 2);
    assert!(value.get("installationId").is_none());
}

#[test]
fn reports_require_consent_and_are_deduplicated_by_day_and_version() {
    let directory = tempdir().expect("temporary directory");
    let manager = TelemetryManager::load(Some(directory.path().join("preferences.json")));
    assert!(!report_due(&manager.snapshot(), "2026-10-22", "0.2.4"));
    manager.set_enabled(true).expect("enable");
    let before = manager.snapshot();
    assert!(report_due(&before, "2026-10-22", "0.2.4"));
    assert!(manager
        .record_success(&before, "2026-10-22".into(), "0.2.4")
        .expect("success"));
    let after = manager.snapshot();
    assert!(!report_due(&after, "2026-10-22", "0.2.4"));
    assert!(report_due(&after, "2026-10-23", "0.2.4"));
    assert!(report_due(&after, "2026-10-22", "0.2.5"));
}
