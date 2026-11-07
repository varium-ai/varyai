//! Shared open-snapshot and decoded-argument transactions, with synthetic authority only.
use super::*;
use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        payload_text::test_support::{install, Action, Phase},
        restoration_contract::{
            ClientProtocol, ContinuityScope, IngressNamespace, UpstreamProtocol,
        },
        tokenization::{DlpTokenStore, TokenCategory, TokenFragment},
    },
};
use serde_json::{json, Value};
use std::{cell::Cell, rc::Rc};

type TestResult<T = ()> = Result<T, &'static str>;
const PRIVATE: &str = "Synthetic transaction private value";
const ORDINARY: &str = "Synthetic transaction ordinary value";

fn accepted<T>(result: Result<T, RestoreError>) -> TestResult<T> {
    result.map_err(|_| "c3_transaction_expected_success")
}

fn fixture() -> TestResult<(DlpTokenStore, ResponseLifecycle, String, PayloadBudget)> {
    let budget = PayloadBudget::with_limit(2 * 1024 * 1024);
    let store = DlpTokenStore::new();
    let client = ClientProtocol::new(Protocol::Chat);
    let mut lease = store
        .admit_scoped_published_policy(
            ContinuityScope::Native {
                client,
                ingress: IngressNamespace::Direct,
            },
            "synthetic-c3-transaction",
            Some("synthetic-c3-transaction-session"),
            1,
        )
        .map_err(|_| "c3_transaction_admission")?;
    let reference = lease
        .prepare(
            &[TokenFragment {
                raw: PRIVATE,
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |references| Ok(references[0].clone()),
        )
        .map_err(|_| "c3_transaction_reference")?;
    let binding = lease
        .bind_actual_attempt(
            "synthetic-provider",
            "openai_chat",
            "https://synthetic.invalid/chat/completions",
            Some("synthetic-principal"),
        )
        .map_err(|_| "c3_transaction_binding")?;
    let owner = accepted(ResponseLifecycle::begin(
        lease,
        binding,
        budget.clone(),
        ProtocolPair::new(client, UpstreamProtocol::new(Protocol::Chat)),
        AttemptProvenance::new(&budget).map_err(|_| "c3_transaction_attempt")?,
        LifecycleLimits {
            max_text_bytes: 64 * 1024,
            max_text_streams: 16,
            max_held_bytes: 64 * 1024,
            max_held_units: 16,
        },
    ))?;
    Ok((store, owner, reference, budget))
}

fn decision_text(decision: &RestoreDecision, raw: &str, expected: &str) -> bool {
    match decision {
        RestoreDecision::Unchanged => raw == expected,
        RestoreDecision::Patch(text) => text.as_str() == expected,
        RestoreDecision::Hold => false,
    }
}

#[test]
fn c3_transaction_snapshot_repeats_reconcile_without_recount() -> TestResult {
    let (_store, mut owner, reference, _budget) = fixture()?;
    let id = accepted(owner.open_text(TextPolicy::Output))?;
    drop(accepted(owner.delta(&id, &reference))?);
    for _ in 0..3 {
        let decision = accepted(owner.snapshot(&id, &reference))?;
        assert!(
            decision_text(&decision, &reference, PRIVATE),
            "c3_transaction_snapshot_content"
        );
        assert!(
            owner.restored_occurrences() == 1 && owner.dependency_tokens().len() == 1,
            "c3_transaction_snapshot_recount"
        );
    }
    drop(accepted(owner.done(&id, &reference))?);
    drop(accepted(owner.finish())?);
    Ok(())
}

#[test]
fn c3_transaction_snapshot_remains_open_for_delta_and_terminal() -> TestResult {
    let (_store, mut owner, reference, _budget) = fixture()?;
    let id = accepted(owner.open_text(TextPolicy::Refusal))?;
    drop(accepted(owner.snapshot(&id, &reference))?);
    drop(accepted(owner.delta(&id, " suffix"))?);
    let raw = format!("{reference} suffix");
    let expected = format!("{PRIVATE} suffix");
    let decision = accepted(owner.snapshot(&id, &raw))?;
    assert!(
        decision_text(&decision, &raw, &expected) && owner.restored_occurrences() == 1,
        "c3_transaction_snapshot_closed_stream"
    );
    drop(accepted(owner.done(&id, &raw))?);
    drop(accepted(owner.finish())?);
    Ok(())
}

#[test]
fn c3_transaction_snapshot_late_fault_is_atomic_sticky() -> TestResult {
    let (_store, mut owner, reference, budget) = fixture()?;
    let prior = accepted(owner.open_text(TextPolicy::Output))?;
    drop(accepted(owner.done(&prior, &reference))?);
    let id = accepted(owner.open_text(TextPolicy::Output))?;
    let baseline = budget.used();
    let ledger = owner.dependency_tokens()[0].as_str().as_ptr();
    let reached = Rc::new(Cell::new(false));
    let hook_reached = Rc::clone(&reached);
    let hook = install(move |observation| {
        if observation.phase == Phase::BeforeCopy && observation.bytes == PRIVATE.len() {
            hook_reached.set(true);
            Action::Fail
        } else {
            Action::Continue
        }
    });
    let result = owner.snapshot(&id, &reference);
    drop(hook);
    assert!(
        reached.get() && result.is_err(),
        "c3_transaction_snapshot_late_fault_not_reached"
    );
    let error = result.err().ok_or("c3_transaction_snapshot_error")?;
    assert!(
        owner.failure() == Some(error)
            && owner.restored_occurrences() == 1
            && owner.dependency_tokens().len() == 1
            && owner.dependency_tokens()[0].as_str().as_ptr() == ledger
            && budget.used() == baseline,
        "c3_transaction_snapshot_partial_commit"
    );
    assert!(
        owner.delta(&id, ORDINARY).err() == Some(error)
            && owner.arguments("{}").err() == Some(error),
        "c3_transaction_snapshot_not_sticky"
    );
    Ok(())
}

#[test]
fn c3_transaction_arguments_decoded_object_leaves_preserve_structure() -> TestResult {
    let (_store, mut owner, reference, _budget) = fixture()?;
    let value = json!({"leaf":reference, "nested":[format!("quoted \" \\ {reference}"),7,true,null],
        "unknown":{"enabled":false}});
    let raw = serde_json::to_string(&value)
        .map_err(|_| "c3_transaction_encode")?
        .replace("rredact", "\\u0072redact");
    let restored = accepted(owner.arguments(&raw))?;
    let actual: Value =
        serde_json::from_str(restored.as_str()).map_err(|_| "c3_transaction_decode")?;
    let expected = json!({"leaf":PRIVATE, "nested":[format!("quoted \" \\ {PRIVATE}"),7,true,null],
        "unknown":{"enabled":false}});
    assert!(
        actual == expected
            && owner.restored_occurrences() == 2
            && owner.dependency_tokens().len() == 1,
        "c3_transaction_argument_leaf_semantics"
    );
    drop(accepted(owner.finish())?);
    Ok(())
}

#[test]
fn c3_transaction_arguments_zero_preserves_exact_and_invalid_is_sticky() -> TestResult {
    for invalid in ["[]", "null", "true", "\"string\"", "{", "{} trailing"] {
        let (_store, mut owner, _reference, _budget) = fixture()?;
        let raw = " { \"untouched\" : [ 7, true, null, \"ordinary\" ] } ";
        let restored = accepted(owner.arguments(raw))?;
        assert!(
            restored.as_str() == raw
                && owner.dependency_tokens().is_empty()
                && owner.restored_occurrences() == 0,
            "c3_transaction_argument_zero_changed"
        );
        assert!(
            owner.arguments(invalid).is_err() && owner.arguments("{}").is_err(),
            "c3_transaction_argument_invalid_accepted"
        );
    }
    Ok(())
}

#[test]
fn c3_transaction_arguments_late_fault_keeps_prior_ledger_atomic() -> TestResult {
    let (_store, mut owner, reference, budget) = fixture()?;
    let id = accepted(owner.open_text(TextPolicy::Output))?;
    drop(accepted(owner.done(&id, &reference))?);
    let raw =
        serde_json::to_string(&json!({"leaf":reference})).map_err(|_| "c3_transaction_encode")?;
    let baseline = budget.used();
    let ledger = owner.dependency_tokens()[0].as_str().as_ptr();
    let reached = Rc::new(Cell::new(false));
    let hook_reached = Rc::clone(&reached);
    let hook = install(move |observation| {
        // The restored argument copy follows leaf restoration and JSON serialization.
        if observation.phase == Phase::BeforeCopy && observation.bytes == PRIVATE.len() + 11 {
            hook_reached.set(true);
            Action::Fail
        } else {
            Action::Continue
        }
    });
    let result = owner.arguments(&raw);
    drop(hook);
    assert!(
        reached.get() && result.is_err(),
        "c3_transaction_argument_late_fault_not_reached"
    );
    let error = result.err().ok_or("c3_transaction_argument_error")?;
    assert!(
        owner.failure() == Some(error)
            && owner.restored_occurrences() == 1
            && owner.dependency_tokens().len() == 1
            && owner.dependency_tokens()[0].as_str().as_ptr() == ledger
            && budget.used() == baseline,
        "c3_transaction_argument_partial_commit"
    );
    assert!(
        owner.arguments("{}").err() == Some(error)
            && owner.open_text(TextPolicy::Reasoning).err() == Some(error),
        "c3_transaction_argument_not_sticky"
    );
    Ok(())
}
