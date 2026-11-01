//! Synthetic in-memory coverage of production runtime policy transactions.
use super::{
    restoration_policy::DlpPolicyGate,
    runtime::{reload_with_gate, update_with_gate},
    tokenization::TokenError,
};
use crate::{
    database::{Database, SecurityAuditSettings},
    error::AppError,
};
use std::sync::Arc;

#[test]
fn runtime_reload_publishes_settings_and_policy_as_one_snapshot() -> Result<(), AppError> {
    let db = Database::memory()?;
    let gate = DlpPolicyGate::new();
    reload_with_gate(&db, &gate)?;
    let old = gate
        .admit()
        .map_err(|_| AppError::Message("admission failed".into()))?;
    update_with_gate(&db, &gate, |db| {
        assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
        db.update_security_settings(&SecurityAuditSettings {
            enabled: !old.runtime.enabled,
        })
    })?;
    let current = gate
        .admit()
        .map_err(|_| AppError::Message("admission failed".into()))?;
    assert_ne!(current.runtime.enabled, old.runtime.enabled);
    assert_eq!(
        current.restoration_policy_revision,
        old.restoration_policy_revision + 1
    );
    assert!(!Arc::ptr_eq(&current.runtime, &old.runtime));
    reload_with_gate(&db, &gate)?;
    let reloaded = gate
        .admit()
        .map_err(|_| AppError::Message("admission failed".into()))?;
    assert_eq!(
        reloaded.restoration_policy_revision,
        current.restoration_policy_revision
    );
    assert!(reloaded.runtime.config_revision > current.runtime.config_revision);
    Ok(())
}

#[test]
fn failed_save_after_possible_commit_stays_closed_until_successful_reload() -> Result<(), AppError>
{
    let db = Database::memory()?;
    let gate = DlpPolicyGate::new();
    reload_with_gate(&db, &gate)?;
    let previous = gate
        .admit()
        .map_err(|_| AppError::Message("admission failed".into()))?;
    let failed = update_with_gate(&db, &gate, |db| {
        db.update_security_settings(&SecurityAuditSettings {
            enabled: !previous.runtime.enabled,
        })?;
        Err::<(), _>(AppError::Message(
            "synthetic private persistence detail".into(),
        ))
    });
    assert_eq!(
        failed.err().map(|e| e.to_string()).as_deref(),
        Some("dlp_unavailable")
    );
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    reload_with_gate(&db, &gate)?;
    let next = gate
        .admit()
        .map_err(|_| AppError::Message("admission failed".into()))?;
    assert_ne!(next.runtime.enabled, previous.runtime.enabled);
    assert_eq!(
        next.restoration_policy_revision,
        previous.restoration_policy_revision + 1
    );
    Ok(())
}

#[test]
fn committed_save_with_failed_source_reload_does_not_report_success() -> Result<(), AppError> {
    let db = Database::memory()?;
    let gate = DlpPolicyGate::new();
    reload_with_gate(&db, &gate)?;
    let failed = update_with_gate(&db, &gate, |db| {
        db.update_security_settings(&SecurityAuditSettings { enabled: false })?;
        db.extension_connection()
            .lock()
            .map_err(|_| AppError::Message("fixture lock failed".into()))?
            .execute_batch("DROP TABLE security_custom_rules")
            .map_err(|_| AppError::Message("fixture mutation failed".into()))
    });
    assert_eq!(
        failed.err().map(|e| e.to_string()).as_deref(),
        Some("dlp_unavailable")
    );
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    assert!(!db.get_security_settings()?.enabled);
    assert!(reload_with_gate(&db, &gate).is_err());
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    Ok(())
}

#[test]
fn runtime_transaction_cancellation_and_publication_poison_fail_closed() -> Result<(), AppError> {
    let db = Database::memory()?;
    let gate = DlpPolicyGate::new();
    reload_with_gate(&db, &gate)?;
    let cancelled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        update_with_gate(&db, &gate, |_| -> Result<(), AppError> {
            panic!("synthetic runtime save cancellation");
        })
    }));
    assert!(cancelled.is_err());
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    reload_with_gate(&db, &gate)?;
    gate.poison_publication_for_test();
    let mut saved = false;
    assert!(update_with_gate(&db, &gate, |_| {
        saved = true;
        Ok(())
    })
    .is_err());
    assert!(
        !saved,
        "poisoned authorization must fail before persistence"
    );
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn http_admission_rejects_closed_policy_instead_of_using_stale_runtime(
) -> Result<(), Box<dyn std::error::Error>> {
    struct Restore(super::runtime::SecurityRuntimeConfig);
    impl Drop for Restore {
        fn drop(&mut self) {
            super::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _restore = Restore((*super::runtime::get_security_runtime()).clone());
    super::runtime::set_security_runtime_for_test(Default::default());
    let db = Database::memory()?;
    let mut captured = None;
    let result = super::runtime::update_security_runtime(&db, |_| {
        let mut body = serde_json::json!({ "input": "synthetic harmless fixture" });
        captured = super::inspect_and_intercept_mut(&crate::app_config::AppType::Codex, &mut body)
            .map_err(|_| AppError::Message("fixture inspection failed".into()))?;
        Err::<(), _>(AppError::Message("synthetic save failure".into()))
    });
    assert!(result.is_err());
    let response = captured.ok_or("closed HTTP policy must return a fail-closed response")?;
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    let bytes = axum::body::to_bytes(response.into_body(), 4096).await?;
    let error: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(error["error"]["code"], "dlp_unavailable");
    assert_eq!(error["error"]["message"], "dlp_unavailable");
    let mut body = serde_json::json!({ "input": "synthetic harmless fixture" });
    let retry = super::inspect_and_intercept_mut(&crate::app_config::AppType::Codex, &mut body)?;
    assert_eq!(
        retry.ok_or("uncertain save must remain closed")?.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    Ok(())
}

#[test]
fn invalid_regex_runtime_diagnostics_do_not_echo_private_rule_fields(
) -> Result<(), Box<dyn std::error::Error>> {
    const CHILD: &str = "VARYAI_SYNTHETIC_REGEX_DIAGNOSTIC_TEST";
    if std::env::var_os(CHILD).is_none() {
        // Isolate the logger from unrelated tests using the process-global log API.
        let output = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", "proxy::security::reversible_runtime_tests::invalid_regex_runtime_diagnostics_do_not_echo_private_rule_fields", "--nocapture"])
            .env(CHILD, "1")
            .output()?;
        assert!(
            output.status.success(),
            "isolated diagnostic regression failed; private fixture output suppressed"
        );
        return Ok(());
    }
    use std::sync::atomic::{AtomicBool, Ordering};
    static PRIVATE_DIAGNOSTIC: AtomicBool = AtomicBool::new(false);
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            if record.args().to_string().contains("SYNTHETIC_PRIVATE") {
                PRIVATE_DIAGNOSTIC.store(true, Ordering::SeqCst);
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Logger = Logger;
    log::set_logger(&LOGGER).map_err(|_| "isolated logger unavailable")?;
    log::set_max_level(log::LevelFilter::Warn);
    let db = Database::memory()?;
    db.save_custom_security_rule(&crate::database::CustomSecurityRule {
        id: "synthetic-regex-runtime".into(),
        name: "SYNTHETIC_PRIVATE_RULE_NAME".into(),
        rule_type: "regex".into(),
        pattern: "SYNTHETIC_PRIVATE_PATTERN(".into(),
        risk_level: "HIGH".into(),
        action: "redact".into(),
        enabled: true,
        description: None,
        updated_at: 1,
    })?;
    let gate = DlpPolicyGate::new();
    reload_with_gate(&db, &gate)?;
    assert!(
        !PRIVATE_DIAGNOSTIC.load(Ordering::SeqCst),
        "runtime diagnostics must not expose private rule fields"
    );
    assert!(
        gate.admit()?.runtime.custom_rules.is_empty(),
        "invalid regex retains legacy disabled behavior"
    );
    Ok(())
}

#[test]
fn keyword_runtime_snapshot_precompiles_unicode_literal_matchers(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let gate = DlpPolicyGate::new();
    for (id, pattern) in [
        ("synthetic-unicode-keyword", "K.Σ"),
        ("synthetic-empty-keyword", ""),
    ] {
        db.save_custom_security_rule(&crate::database::CustomSecurityRule {
            id: id.into(),
            name: "Synthetic keyword".into(),
            rule_type: "keyword".into(),
            pattern: pattern.into(),
            risk_level: "MEDIUM".into(),
            action: "redact".into(),
            enabled: true,
            description: None,
            updated_at: 1,
        })?;
    }
    reload_with_gate(&db, &gate)?;
    let snapshot = gate.admit()?;
    let rule = snapshot
        .runtime
        .custom_rules
        .iter()
        .find(|rule| rule.id == "synthetic-unicode-keyword")
        .ok_or("keyword rule missing")?;
    let compiled = rule.regex.as_ref();
    assert!(
        compiled.is_some(),
        "published keyword snapshot lacks its compiled matcher"
    );
    let compiled = compiled.ok_or("compiled keyword missing")?;
    assert!(compiled.is_match("synthetic k.ς fixture"));
    assert!(!compiled.is_match("synthetic kXς fixture"));
    let empty = snapshot
        .runtime
        .custom_rules
        .iter()
        .find(|rule| rule.id == "synthetic-empty-keyword")
        .ok_or("empty keyword missing")?;
    assert!(empty.regex.is_none(), "empty keyword must remain inactive");
    Ok(())
}

#[test]
fn keyword_reload_preserves_captured_matcher_and_policy_snapshots(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{
        payload_budget::PayloadBudget, request_detection::detect_spans_with_budget,
        request_plan::EffectiveAction,
    };
    let db = Database::memory()?;
    let gate = DlpPolicyGate::new();
    let mut rule = crate::database::CustomSecurityRule {
        id: "synthetic-snapshot-keyword".into(),
        name: "Synthetic keyword".into(),
        rule_type: "keyword".into(),
        pattern: "K.Σ".into(),
        risk_level: "MEDIUM".into(),
        action: "redact".into(),
        enabled: true,
        description: None,
        updated_at: 1,
    };
    update_with_gate(&db, &gate, |db| db.save_custom_security_rule(&rule))?;
    let old = gate.admit()?;
    rule.pattern = "[NEW]+".into();
    rule.action = "audit".into();
    rule.updated_at = 2;
    update_with_gate(&db, &gate, |db| db.save_custom_security_rule(&rule))?;
    let current = gate.admit()?;
    assert!(current.restoration_policy_revision > old.restoration_policy_revision);
    for (snapshot, expected, action) in [
        (&old, "k.ς", EffectiveAction::Redact),
        (&current, "[new]+", EffectiveAction::Audit),
    ] {
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        for _ in 0..3 {
            let found = detect_spans_with_budget(
                "尾 k.ς [new]+",
                false,
                &snapshot.runtime,
                budget.clone(),
            )?;
            let matching: Vec<_> = found
                .as_slice()
                .iter()
                .filter(|span| span.rule_id == rule.id)
                .collect();
            assert_eq!(matching.len(), 1);
            assert_eq!(
                "尾 k.ς [new]+".get(matching[0].range.clone()),
                Some(expected)
            );
            assert_eq!(matching[0].action, action);
            drop(found);
            assert_eq!(budget.used(), 0);
        }
    }
    Ok(())
}

#[test]
fn keyword_actual_compilation_limit_retains_rule_and_private_detection_fails_closed(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{payload_budget::PayloadBudget, request_detection::detect_spans_with_budget};
    let db = Database::memory()?;
    let gate = DlpPolicyGate::new();
    let pattern = "a".repeat(512 * 1024);
    // Exercise the pinned compiler's real error, not a placeholder failure hook.
    assert!(regex::RegexBuilder::new(&regex::escape(&pattern))
        .case_insensitive(true)
        .build()
        .is_err());
    db.save_custom_security_rule(&crate::database::CustomSecurityRule {
        id: "synthetic-oversized-compiled-keyword".into(),
        name: "Synthetic keyword".into(),
        rule_type: "keyword".into(),
        pattern,
        risk_level: "MEDIUM".into(),
        action: "redact".into(),
        enabled: true,
        description: None,
        updated_at: 1,
    })?;
    reload_with_gate(&db, &gate)?;
    let snapshot = gate.admit()?;
    assert_eq!(snapshot.runtime.custom_rules.len(), 1);
    assert!(snapshot.runtime.custom_rules[0].regex.is_none());
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let result = detect_spans_with_budget(
        "synthetic ordinary input",
        false,
        &snapshot.runtime,
        budget.clone(),
    );
    assert_eq!(result.err(), Some(TokenError::RedactionFailed));
    assert_eq!(budget.used(), 0);
    Ok(())
}
