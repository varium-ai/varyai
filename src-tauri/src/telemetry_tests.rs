use std::fs;

use tempfile::tempdir;
use uuid::Uuid;

use crate::telemetry::{report_due, TelemetryManager};

#[test]
fn fresh_installation_automatically_persists_an_identifier_and_reports() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("installation-analytics.json");
    let state = TelemetryManager::load(Some(path.clone())).snapshot();
    assert!(state.enabled);
    let id = state.installation_id.as_deref().expect("installation ID");
    assert_eq!(
        Uuid::parse_str(id).expect("UUID").get_version(),
        Some(uuid::Version::Random)
    );
    assert!(report_due(&state, "2026-10-22", "0.2.4"));
    assert!(path.exists());
    assert_eq!(
        TelemetryManager::load(Some(path))
            .snapshot()
            .installation_id,
        state.installation_id
    );
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
fn legacy_automatic_state_preserves_identity_and_explicit_disabled_choices() {
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
        let expected_enabled = legacy_enabled != Some(false);
        let state = manager.snapshot();
        assert_eq!(state.enabled, expected_enabled);
        assert_eq!(
            state.installation_id.as_deref(),
            expected_enabled.then_some("12f57d58-7019-4e84-8ed7-f9ce4731e3cb")
        );
        assert_eq!(report_due(&state, "2026-10-22", "0.2.4"), expected_enabled);
        assert_eq!(
            TelemetryManager::load(Some(path)).snapshot().enabled,
            expected_enabled
        );
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

#[test]
fn a_disabled_report_does_not_create_a_dispatch_job() {
    let manager = TelemetryManager::load(None);
    assert!(manager
        .begin_report(&manager.snapshot(), "2026-11-13", "0.3.0")
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
fn automatic_reports_are_deduplicated_by_day_and_version() {
    let directory = tempdir().expect("temporary directory");
    let manager = TelemetryManager::load(Some(directory.path().join("preferences.json")));
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

#[test]
fn default_destination_is_the_public_google_measurement_id() {
    if option_env!("VARYAI_GA4_MEASUREMENT_ID").is_none() {
        assert_eq!(super::measurement_id(), Some("G-2R2MZ76G1E"));
    }
}

#[test]
fn dispatch_jobs_are_token_scoped_and_revoked_by_opt_out() {
    let directory = tempdir().expect("temporary directory");
    let manager = TelemetryManager::load(Some(directory.path().join("preferences.json")));
    let (report, mut receiver) = manager
        .begin_report(&manager.snapshot(), "2026-11-13", "0.3.0")
        .expect("job");
    assert_eq!(
        report.events,
        vec!["desktop_install_observed", "desktop_daily_active"]
    );
    assert!(!manager
        .complete_report("unrelated-token", true)
        .expect("ignored token"));
    assert!(receiver.try_recv().is_err());
    manager.set_enabled(false).expect("disable");
    assert!(receiver.try_recv().is_err());
    assert!(!manager
        .complete_report(&report.token, true)
        .expect("stale token"));
    assert!(manager.snapshot().last_observed_date.is_none());
}

#[test]
fn confirmed_dispatch_records_only_the_local_day_and_expected_version() {
    let directory = tempdir().expect("temporary directory");
    let manager = TelemetryManager::load(Some(directory.path().join("preferences.json")));
    let (report, mut receiver) = manager
        .begin_report(&manager.snapshot(), "2026-11-13", "0.3.0")
        .expect("job");
    let value = serde_json::to_value(&report).expect("report");
    assert_eq!(value.as_object().expect("object").len(), 8);
    assert_eq!(value["os"], std::env::consts::OS);
    assert!(!manager
        .complete_report("stale", true)
        .expect("ignored token"));
    assert!(manager
        .complete_report(&report.token, true)
        .expect("dispatch"));
    assert_eq!(receiver.try_recv(), Ok(true));
    let state = manager.snapshot();
    assert!(!report_due(&state, "2026-11-13", "0.3.0"));
    assert!(super::report_events(&state, "2026-11-13", "0.3.0").is_empty());
    assert_eq!(
        super::report_events(&state, "2026-11-14", "0.3.0"),
        vec!["desktop_daily_active"]
    );
    assert_eq!(
        super::report_events(&state, "2026-11-13", "0.3.1"),
        vec!["desktop_version_changed"]
    );
}

#[test]
fn legacy_backend_markers_do_not_suppress_the_first_google_event() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("preferences.json");
    fs::write(&path, r#"{"schemaVersion":2,"enabled":true,"installationId":"12f57d58-7019-4e84-8ed7-f9ce4731e3cb","lastObservedDate":"2026-11-13","lastReportedVersion":"0.3.0"}"#).expect("legacy");
    let state = TelemetryManager::load(Some(path)).snapshot();
    assert_eq!(
        state.installation_id.as_deref(),
        Some("12f57d58-7019-4e84-8ed7-f9ce4731e3cb")
    );
    assert!(report_due(&state, "2026-11-13", "0.3.0"));
    assert_eq!(
        super::report_events(&state, "2026-11-13", "0.3.0"),
        vec!["desktop_install_observed", "desktop_daily_active"]
    );
}

#[test]
fn failed_dispatch_and_idempotent_settings_do_not_create_success_markers() {
    let directory = tempdir().expect("temporary directory");
    let manager = TelemetryManager::load(Some(directory.path().join("preferences.json")));
    let (report, mut receiver) = manager
        .begin_report(&manager.snapshot(), "2026-11-13", "0.3.0")
        .expect("job");
    manager.set_enabled(true).expect("already enabled");
    assert!(manager.pending_report().is_some());
    assert!(!manager
        .complete_report(&report.token, false)
        .expect("failed dispatch"));
    assert_eq!(receiver.try_recv(), Ok(false));
    assert!(manager.snapshot().last_observed_date.is_none());
}

#[test]
fn a_dispatch_crossing_midnight_rechecks_the_current_statistics_day() {
    let before = chrono::DateTime::parse_from_rfc3339("2026-11-13T15:59:59Z")
        .expect("before midnight")
        .with_timezone(&chrono::Utc);
    let after = chrono::DateTime::parse_from_rfc3339("2026-11-13T16:00:01Z")
        .expect("after midnight")
        .with_timezone(&chrono::Utc);
    assert_eq!(
        super::until_next_statistics_day_at(before, "2026-11-13"),
        std::time::Duration::from_secs(1)
    );
    assert_eq!(
        super::until_next_statistics_day_at(after, "2026-11-13"),
        std::time::Duration::ZERO
    );
    assert_eq!(
        super::until_next_statistics_day_at(after, "2026-11-14"),
        std::time::Duration::from_secs(86399)
    );
}
