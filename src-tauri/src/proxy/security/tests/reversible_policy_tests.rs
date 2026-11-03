//! Synthetic-only tests for atomic restoration policy admission and publication.
use super::{
    restoration_policy::{DlpPolicyGate, RestorationPolicy},
    runtime::{CompiledCustomRule, SecurityRuntimeConfig},
    tokenization::TokenError,
    types::{AuditAction, RiskLevel},
};
use crate::database::{CustomSecurityRule, SecurityAuditSettings};
use std::collections::HashMap;
use std::sync::Arc;

fn rule() -> CustomSecurityRule {
    CustomSecurityRule {
        id: "synthetic-rule".into(),
        name: "Synthetic rule".into(),
        rule_type: "keyword".into(),
        pattern: "synthetic-fixture".into(),
        risk_level: "HIGH".into(),
        action: "redact".into(),
        enabled: true,
        description: None,
        updated_at: 1,
    }
}

fn policy(
    enabled: bool,
    rules: &[CustomSecurityRule],
    overrides: &HashMap<String, AuditAction>,
    catalog: u64,
) -> RestorationPolicy {
    RestorationPolicy::from_sources(
        &SecurityAuditSettings { enabled },
        rules,
        overrides,
        catalog,
    )
}

fn runtime(enabled: bool) -> Arc<SecurityRuntimeConfig> {
    Arc::new(SecurityRuntimeConfig {
        enabled,
        ..Default::default()
    })
}

#[test]
fn uninitialized_gate_is_closed_and_first_publication_admits_one_atomic_snapshot(
) -> Result<(), TokenError> {
    let gate = DlpPolicyGate::new();
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    let published = runtime(true);
    gate.begin_update()?
        .publish(published.clone(), policy(true, &[], &HashMap::new(), 1))?;
    let admitted = gate.admit()?;
    assert!(Arc::ptr_eq(&admitted.runtime, &published));
    assert_eq!(admitted.restoration_policy_revision, 1);
    Ok(())
}

#[test]
fn write_closes_admission_before_persistence_without_invalidating_admitted_requests(
) -> Result<(), TokenError> {
    let gate = DlpPolicyGate::new();
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &HashMap::new(), 1))?;
    let admitted = gate.admit()?;
    let update = gate.begin_update()?;
    // A persistence callback can read the gate without a publication lock deadlock.
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    assert!(admitted.runtime.enabled);
    assert_eq!(admitted.restoration_policy_revision, 1);
    update.publish(runtime(false), policy(false, &[], &HashMap::new(), 1))?;
    let next = gate.admit()?;
    assert!(!next.runtime.enabled);
    assert_eq!(next.restoration_policy_revision, 2);
    assert!(admitted.runtime.enabled);
    assert_eq!(admitted.restoration_policy_revision, 1);
    Ok(())
}

#[test]
fn abandoned_or_unwound_save_stays_closed_until_successful_republication() -> Result<(), TokenError>
{
    let gate = DlpPolicyGate::new();
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &HashMap::new(), 1))?;
    drop(gate.begin_update()?);
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    let cancelled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _update = gate.begin_update()?;
        // Simulate a save which may have committed before the caller unwinds.
        panic!("synthetic publication cancellation");
        #[allow(unreachable_code)]
        Ok::<_, TokenError>(())
    }));
    assert!(cancelled.is_err());
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &HashMap::new(), 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 1);
    Ok(())
}

#[test]
fn policy_comparison_excludes_presentation_and_order_but_includes_disabled_rule_semantics(
) -> Result<(), TokenError> {
    let gate = DlpPolicyGate::new();
    let initial = rule();
    let mut other = rule();
    other.id = "second-synthetic-rule".into();
    other.enabled = false;
    let mut rules = vec![initial, other];
    let mut overrides = HashMap::from([("chinese_phone_number".into(), AuditAction::Redact)]);
    gate.begin_update()?
        .publish(runtime(true), policy(true, &rules, &overrides, 1))?;
    rules.reverse();
    for rule in &mut rules {
        rule.name = "Renamed synthetic rule".into();
        rule.description = Some("Presentation only".into());
        rule.updated_at += 1;
        rule.risk_level = "LOW".into();
    }
    let mut presentation_runtime = SecurityRuntimeConfig::default();
    presentation_runtime.custom_rules.push(CompiledCustomRule {
        id: "synthetic-rule".into(),
        name: "Renamed synthetic rule".into(),
        rule_type: "keyword".into(),
        pattern: "synthetic-fixture".into(),
        regex: None,
        risk_level: RiskLevel::Low,
        action: AuditAction::Redact,
        enabled: true,
    });
    gate.begin_update()?.publish(
        Arc::new(presentation_runtime),
        policy(true, &rules, &overrides, 1),
    )?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 1);
    rules[0].pattern.push_str("-changed");
    gate.begin_update()?
        .publish(runtime(true), policy(true, &rules, &overrides, 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 2);
    rules[0].enabled = true;
    gate.begin_update()?
        .publish(runtime(true), policy(true, &rules, &overrides, 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 3);
    rules[0].action = "audit".into();
    gate.begin_update()?
        .publish(runtime(true), policy(true, &rules, &overrides, 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 4);
    overrides.insert("chinese_phone_number".into(), AuditAction::Audit);
    gate.begin_update()?
        .publish(runtime(true), policy(true, &rules, &overrides, 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 5);
    gate.begin_update()?
        .publish(runtime(true), policy(true, &rules, &overrides, 2))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 6);
    Ok(())
}

#[test]
fn revision_overflow_and_poisoned_publication_fail_closed() -> Result<(), TokenError> {
    let gate = DlpPolicyGate::new();
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &HashMap::new(), 1))?;
    gate.set_revision_for_test(u64::MAX)?;
    assert!(matches!(
        gate.begin_update()?
            .publish(runtime(false), policy(false, &[], &HashMap::new(), 1)),
        Err(TokenError::Unavailable)
    ));
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    let poisoned = DlpPolicyGate::new();
    poisoned.poison_publication_for_test();
    assert!(matches!(poisoned.admit(), Err(TokenError::Unavailable)));
    assert!(matches!(
        poisoned.begin_update(),
        Err(TokenError::Unavailable)
    ));
    Ok(())
}

#[test]
fn concurrent_publishers_serialize_the_entire_save_and_publish_transaction(
) -> Result<(), TokenError> {
    let gate = Arc::new(DlpPolicyGate::new());
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &HashMap::new(), 1))?;
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (saving_tx, saving_rx) = std::sync::mpsc::channel();
    let update = gate.begin_update()?;
    let other = gate.clone();
    let thread = std::thread::spawn(move || {
        started_tx.send(()).map_err(|_| TokenError::Unavailable)?;
        let next = other.begin_update()?;
        saving_tx.send(()).map_err(|_| TokenError::Unavailable)?;
        next.publish(runtime(true), policy(true, &[], &HashMap::new(), 3))
    });
    started_rx.recv().map_err(|_| TokenError::Unavailable)?;
    assert!(matches!(
        saving_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert!(matches!(gate.admit(), Err(TokenError::Unavailable)));
    update.publish(runtime(false), policy(false, &[], &HashMap::new(), 2))?;
    thread.join().map_err(|_| TokenError::Unavailable)??;
    assert_eq!(gate.admit()?.restoration_policy_revision, 3);
    Ok(())
}

#[test]
fn explicit_builtin_default_and_reset_preserve_the_effective_policy_generation(
) -> Result<(), TokenError> {
    let gate = DlpPolicyGate::new();
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &HashMap::new(), 1))?;
    let overrides = HashMap::from([("chinese_phone_number".into(), AuditAction::Redact)]);
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &overrides, 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 1);
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &HashMap::new(), 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 1);
    Ok(())
}
#[test]
fn builtin_policy_comparison_uses_catalog_defaults_and_generic_notice_restrictions(
) -> Result<(), TokenError> {
    let gate = DlpPolicyGate::new();
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &HashMap::new(), 1))?;
    let mut overrides: HashMap<_, _> = super::builtin_rules::BUILTIN_RULES
        .iter()
        .map(|rule| (rule.id.to_string(), rule.default_action))
        .collect();
    overrides.insert("chinese_id_card".into(), AuditAction::Redact);
    overrides.insert("chinese_phone_number".into(), AuditAction::Redact);
    overrides.insert("synthetic-unknown-rule".into(), AuditAction::Allow);
    for ineffective in [AuditAction::Block, AuditAction::Redact, AuditAction::Allow] {
        overrides.insert("catalog:generic-sk-key-notice".into(), ineffective);
        gate.begin_update()?
            .publish(runtime(true), policy(true, &[], &overrides, 1))?;
        assert_eq!(gate.admit()?.restoration_policy_revision, 1);
    }
    overrides.insert("catalog:generic-sk-key-notice".into(), AuditAction::Audit);
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &overrides, 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 2);
    Ok(())
}

#[test]
fn rule_type_removal_and_recreation_close_the_previous_generation() -> Result<(), TokenError> {
    let gate = DlpPolicyGate::new();
    let mut rules = vec![rule()];
    gate.begin_update()?
        .publish(runtime(true), policy(true, &rules, &HashMap::new(), 1))?;
    rules[0].rule_type = "regex".into();
    gate.begin_update()?
        .publish(runtime(true), policy(true, &rules, &HashMap::new(), 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 2);
    gate.begin_update()?
        .publish(runtime(true), policy(true, &[], &HashMap::new(), 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 3);
    gate.begin_update()?
        .publish(runtime(true), policy(true, &rules, &HashMap::new(), 1))?;
    assert_eq!(gate.admit()?.restoration_policy_revision, 4);
    Ok(())
}
