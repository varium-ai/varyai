//! Real native verifier and staging tests confined to the finalizer issuer.
use super::*;
use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        http_restoration::{test_transport, VerifiedNativeJson},
        payload_buffer::test_support as buffer_events,
        restoration_contract::{
            ClientProtocol, ContinuityScope, EditConstraint, IngressNamespace, SourceClass,
            TextSites,
        },
        restoration_core::{stage_native_json, NativeJsonObservation, StagedNativeJson},
        tokenization::{DlpTokenStore, TokenCategory, TokenFragment},
    },
};
use std::{
    cell::Cell,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const BUDGET_BYTES: usize = 4 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 1024 * 1024;
const SYNTHETIC_TEXT: &str = "Synthetic native finalization text";

struct Fixture {
    store: DlpTokenStore,
    lease: DlpRequestLease,
    budget: PayloadBudget,
    attempt: AttemptProvenance,
    binding: ActualAttemptBinding,
    reference: Option<String>,
}

impl Fixture {
    fn new(changed: bool) -> TestResult<Self> {
        let store = DlpTokenStore::new();
        let mut lease = store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: chat(),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-c5-finalization-app",
            Some("synthetic-c5-finalization-session"),
            1,
        )?;
        let reference = if changed {
            Some(lease.prepare(
                &[TokenFragment {
                    raw: SYNTHETIC_TEXT,
                    whole_span_categories: &[TokenCategory::Phone],
                }],
                &[],
                |tokens| Ok(tokens[0].clone()),
            )?)
        } else {
            None
        };
        let binding = lease.bind_actual_attempt(
            "synthetic-c5-finalization-provider",
            "openai_chat",
            "https://synthetic.invalid/v1/chat/completions",
            Some("synthetic-c5-finalization-principal"),
        )?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let attempt = AttemptProvenance::new(&budget)?;
        Ok(Self {
            store,
            lease,
            budget,
            attempt,
            binding,
            reference,
        })
    }

    fn source(&self) -> Value {
        chat_json(self.reference.as_deref().unwrap_or(SYNTHETIC_TEXT))
    }

    fn proof(&self, source: &Value) -> TestResult<VerifiedNativeJson> {
        futures::executor::block_on(test_transport::native_json_proof(
            &self.attempt,
            &self.binding,
            &self.budget,
            Bytes::from(serde_json::to_vec(source)?),
            source,
        ))
        .map_err(|_| "c5_native_finalization_proof_fixture_rejected".into())
    }

    fn stage<'a>(
        &'a self,
        observation: &'a NativeJsonObservation<'a>,
    ) -> TestResult<StagedNativeJson<'a>> {
        stage_native_json(observation, &self.lease, &self.binding)
            .map_err(|_| "c5_native_finalization_stage_fixture_rejected".into())
    }
}

fn chat() -> ClientProtocol {
    ClientProtocol::new(Protocol::Chat)
}

fn chat_json(text: &str) -> Value {
    json!({
        "id":"chatcmpl_synthetic_c5_finalization",
        "object":"chat.completion",
        "model":"synthetic-native-model",
        "choices":[{
            "index":0,
            "message":{"role":"assistant", "content":text, "refusal":null},
            "finish_reason":"stop"
        }],
        "usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7},
        "synthetic_extension":{"keep":true}
    })
}

fn observation<'a>(fixture: &Fixture, source: &'a Value) -> TestResult<NativeJsonObservation<'a>> {
    let mut sites = TextSites::new(fixture.budget.clone())?;
    sites.observe(
        &source["choices"][0]["message"]["content"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    NativeJsonObservation::new(
        source,
        chat(),
        sites,
        &[&source["choices"][0]["message"]],
        fixture.budget.clone(),
    )
    .map_err(|_| "c5_native_finalization_observation_fixture_rejected".into())
}

fn prepared<'a>(
    fixture: &Fixture,
    stage: StagedNativeJson<'a>,
    source: &Value,
    original: Bytes,
) -> TestResult<PreparedNativeJsonSpec<'a>> {
    prepare_native_json_spec(
        stage,
        fixture.proof(source)?,
        &fixture.attempt,
        original,
        MAX_JSON_BYTES,
    )
    .map_err(|_| "c5_native_finalization_sealed_spec_unavailable".into())
}

struct OriginalOwner {
    bytes: Vec<u8>,
    dropped: Arc<AtomicBool>,
}
impl AsRef<[u8]> for OriginalOwner {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}
impl Drop for OriginalOwner {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

fn original_bytes(source: &Value) -> TestResult<(Bytes, Arc<AtomicBool>)> {
    let dropped = Arc::new(AtomicBool::new(false));
    let bytes = Bytes::from_owner(OriginalOwner {
        bytes: serde_json::to_vec_pretty(source)?,
        dropped: Arc::clone(&dropped),
    });
    Ok((bytes, dropped))
}

#[test]
fn c5_native_finalization_strict_real_zero_mapping_proof_reaches_sealed_spec() -> TestResult {
    let fixture = Fixture::new(false)?;
    let source = fixture.source();
    let before = fixture.store.stats()?;
    let observed = observation(&fixture, &source)?;
    let stage = fixture.stage(&observed)?;
    assert!(
        !stage.changed(),
        "c5_native_finalization_unchanged_fixture_changed"
    );
    let (original, _) = original_bytes(&source)?;
    let prepared = prepared(&fixture, stage, &source, original)?;
    let spec = prepared
        .spec()
        .map_err(|_| "c5_native_finalization_spec_issuance_rejected")?;
    let view = spec.view();
    assert!(
        std::ptr::eq(view.binding, prepared.stage.binding()) && view.client == chat(),
        "c5_native_finalization_spec_identity_changed"
    );
    assert!(
        view.delivered == prepared.stage.delivered()
            && view.delivered.as_array().is_some_and(|items| {
                items.len() == 1 && items[0] == source["choices"][0]["message"]
            })
            && view.output_tokens.is_empty(),
        "c5_native_finalization_zero_mapping_spec_changed"
    );
    assert!(
        fixture.lease.is_empty() && fixture.store.stats()? == before,
        "c5_native_finalization_spec_granted_store_authority"
    );
    Ok(())
}

#[test]
fn c5_native_finalization_changed_spec_keeps_exact_tokens_and_native_siblings() -> TestResult {
    let fixture = Fixture::new(true)?;
    let source = fixture.source();
    let unchanged_source = source.clone();
    let before = fixture.store.stats()?;
    let observed = observation(&fixture, &source)?;
    let stage = fixture.stage(&observed)?;
    assert!(
        stage.changed(),
        "c5_native_finalization_changed_fixture_unchanged"
    );
    let (original, _) = original_bytes(&source)?;
    let prepared = prepared(&fixture, stage, &source, original)?;
    let spec = prepared
        .spec()
        .map_err(|_| "c5_native_finalization_changed_spec_issuance_rejected")?;
    let view = spec.view();
    assert!(
        view.output_tokens.len() == 1
            && Some(view.output_tokens[0]) == fixture.reference.as_deref(),
        "c5_native_finalization_output_token_identity_lost"
    );
    assert!(
        view.delivered.as_array().is_some_and(|items| {
            items.len() == 1
                && items[0]["content"].as_str() == Some(SYNTHETIC_TEXT)
                && items[0].get("refusal").is_some_and(Value::is_null)
        }),
        "c5_native_finalization_changed_delivered_snapshot_lost"
    );
    let wire: Value = serde_json::from_slice(&prepared.wire)?;
    let mut expected = source.clone();
    expected["choices"][0]["message"]["content"] = json!(SYNTHETIC_TEXT);
    assert!(
        wire == expected,
        "c5_native_finalization_native_siblings_rewritten"
    );
    assert!(
        source == unchanged_source && fixture.store.stats()? == before,
        "c5_native_finalization_prepare_mutated_source_or_store"
    );
    Ok(())
}

#[test]
fn c5_native_finalization_unchanged_encodes_then_discards_encoding_keeps_original_owner(
) -> TestResult {
    let fixture = Fixture::new(false)?;
    let source = fixture.source();
    let observed = observation(&fixture, &source)?;
    let baseline = fixture.budget.used();
    let stage = fixture.stage(&observed)?;
    let proof = fixture.proof(&source)?;
    let encoding_baseline = fixture.budget.used();
    let (original, dropped) = original_bytes(&source)?;
    let pointer = original.as_ptr();
    let length = original.len();
    let allocations = Rc::new(Cell::new(0));
    let charged_capacity = Rc::new(Cell::new(0));
    let seen = Rc::clone(&allocations);
    let capacity = Rc::clone(&charged_capacity);
    let hook = buffer_events::install(move |event| {
        if event.phase == buffer_events::Phase::BeforeAllocation {
            seen.set(seen.get() + 1);
            capacity.set(event.requested);
            assert!(
                event.requested > 0 && event.used >= encoding_baseline + event.requested,
                "c5_native_finalization_unchanged_encoding_not_precharged"
            );
        }
        buffer_events::Action::Continue
    });
    let result = prepare_native_json_spec(stage, proof, &fixture.attempt, original, MAX_JSON_BYTES);
    drop(hook);
    let prepared = result.map_err(|_| "c5_native_finalization_owner_prepare_rejected")?;
    assert!(
        allocations.get() > 0,
        "c5_native_finalization_unchanged_skipped_encoding"
    );
    assert!(
        prepared.wire.as_ptr() == pointer
            && prepared.wire.len() == length
            && !dropped.load(Ordering::SeqCst),
        "c5_native_finalization_unchanged_original_owner_lost"
    );
    assert!(
        fixture.budget.used() < encoding_baseline + charged_capacity.get(),
        "c5_native_finalization_unchanged_encoding_retained"
    );
    drop(prepared);
    assert!(
        dropped.load(Ordering::SeqCst),
        "c5_native_finalization_original_owner_leaked"
    );
    assert_eq!(
        fixture.budget.used(),
        baseline,
        "c5_native_finalization_prepare_charge_leaked"
    );
    Ok(())
}

#[test]
fn c5_native_finalization_changed_wire_retains_new_charged_owner() -> TestResult {
    let fixture = Fixture::new(true)?;
    let source = fixture.source();
    let observed = observation(&fixture, &source)?;
    let baseline = fixture.budget.used();
    let stage = fixture.stage(&observed)?;
    let stage_baseline = fixture.budget.used();
    let (original, dropped) = original_bytes(&source)?;
    let original_guard = original.clone();
    let prepared = prepared(&fixture, stage, &source, original)?;
    assert!(
        prepared.wire.as_ptr() != original_guard.as_ptr(),
        "c5_native_finalization_changed_reused_original_wire"
    );
    drop(original_guard);
    assert!(
        dropped.load(Ordering::SeqCst),
        "c5_native_finalization_changed_kept_original_owner"
    );
    assert!(
        fixture.budget.used() >= stage_baseline + prepared.wire.len(),
        "c5_native_finalization_changed_wire_not_charged"
    );
    drop(prepared);
    assert_eq!(
        fixture.budget.used(),
        baseline,
        "c5_native_finalization_changed_wire_charge_leaked"
    );
    Ok(())
}

#[test]
fn c5_native_finalization_rejects_real_proof_for_different_source_before_encoding() -> TestResult {
    let fixture = Fixture::new(false)?;
    let source = fixture.source();
    let other = chat_json("Synthetic different source");
    let proof = fixture.proof(&other)?;
    let observed = observation(&fixture, &source)?;
    assert_rejected_before_encoding(&fixture, &observed, proof, &fixture.attempt, &source)
}

#[test]
fn c5_native_finalization_rejects_different_attempt_before_encoding() -> TestResult {
    let fixture = Fixture::new(false)?;
    let source = fixture.source();
    let other = AttemptProvenance::new(&fixture.budget)?;
    let observed = observation(&fixture, &source)?;
    assert_rejected_before_encoding(
        &fixture,
        &observed,
        fixture.proof(&source)?,
        &other,
        &source,
    )
}

#[test]
fn c5_native_finalization_rejects_real_proof_for_other_binding_before_encoding() -> TestResult {
    let fixture = Fixture::new(false)?;
    let other = Fixture::new(false)?;
    let source = fixture.source();
    let observed = observation(&fixture, &source)?;
    // Keep proof provenance correct so that only the store-issued binding differs.
    let proof = futures::executor::block_on(test_transport::native_json_proof(
        &fixture.attempt,
        &other.binding,
        &fixture.budget,
        Bytes::from(serde_json::to_vec(&source)?),
        &source,
    ))
    .map_err(|_| "c5_native_finalization_other_binding_proof_rejected")?;
    assert_rejected_before_encoding(&fixture, &observed, proof, &fixture.attempt, &source)
}

#[test]
fn c5_native_finalization_real_proof_rejects_cross_client_identity() -> TestResult {
    let fixture = Fixture::new(false)?;
    let source = fixture.source();
    for protocol in [Protocol::Responses, Protocol::Anthropic, Protocol::Gemini] {
        assert!(
            fixture
                .proof(&source)?
                .validate(
                    &source,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(protocol)
                )
                .is_err(),
            "c5_native_finalization_cross_client_proof_accepted"
        );
    }
    Ok(())
}

fn assert_rejected_before_encoding(
    fixture: &Fixture,
    observed: &NativeJsonObservation<'_>,
    proof: VerifiedNativeJson,
    attempt: &AttemptProvenance,
    source: &Value,
) -> TestResult {
    let before = fixture.store.stats()?;
    let baseline = fixture.budget.used();
    let stage = fixture.stage(observed)?;
    let allocations = Rc::new(Cell::new(0));
    let seen = Rc::clone(&allocations);
    let hook = buffer_events::install(move |event| {
        if event.phase == buffer_events::Phase::BeforeAllocation {
            seen.set(seen.get() + 1);
        }
        buffer_events::Action::Continue
    });
    let (original, dropped) = original_bytes(source)?;
    let result = prepare_native_json_spec(stage, proof, attempt, original, MAX_JSON_BYTES);
    drop(hook);
    assert!(
        result.is_err(),
        "c5_native_finalization_wrong_proof_issued_spec"
    );
    drop(result);
    assert_eq!(
        allocations.get(),
        0,
        "c5_native_finalization_wrong_proof_reached_encoding"
    );
    assert!(
        fixture.store.stats()? == before
            && fixture.budget.used() == baseline
            && dropped.load(Ordering::SeqCst),
        "c5_native_finalization_wrong_proof_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_native_finalization_rejects_unbound_or_incomplete_original_bytes() -> TestResult {
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = fixture.source();
        let observed = observation(&fixture, &source)?;
        let wrong = serde_json::to_vec(&chat_json("Synthetic unbound original"))?;
        let mut trailing = serde_json::to_vec(&source)?;
        trailing.extend_from_slice(b" {}");
        for invalid in [wrong, trailing, b"{\"choices\":".to_vec()] {
            let baseline = fixture.budget.used();
            let before = fixture.store.stats()?;
            let stage = fixture.stage(&observed)?;
            let result = prepare_native_json_spec(
                stage,
                fixture.proof(&source)?,
                &fixture.attempt,
                Bytes::from(invalid),
                MAX_JSON_BYTES,
            );
            assert!(
                result.is_err(),
                "c5_native_finalization_unbound_original_accepted"
            );
            drop(result);
            assert!(
                fixture.store.stats()? == before && fixture.budget.used() == baseline,
                "c5_native_finalization_original_rejection_not_atomic"
            );
        }
    }
    Ok(())
}

// Characterize the existing strict original-source gate without changing it.
fn assert_original_rejected_before_encoding(
    fixture: &Fixture,
    source: &Value,
    invalid: Vec<u8>,
) -> TestResult {
    let observed = observation(fixture, source)?;
    let before = fixture.store.stats()?;
    let baseline = fixture.budget.used();
    let stage = fixture.stage(&observed)?;
    let proof = fixture.proof(source)?;
    let dropped = Arc::new(AtomicBool::new(false));
    let original = Bytes::from_owner(OriginalOwner {
        bytes: invalid,
        dropped: Arc::clone(&dropped),
    });
    let allocations = Rc::new(Cell::new(0));
    let seen = Rc::clone(&allocations);
    let hook = buffer_events::install(move |event| {
        if event.phase == buffer_events::Phase::BeforeAllocation {
            seen.set(seen.get() + 1);
        }
        buffer_events::Action::Continue
    });
    let result = prepare_native_json_spec(stage, proof, &fixture.attempt, original, MAX_JSON_BYTES);
    drop(hook);
    assert!(
        result.is_err(),
        "c5_native_finalization_invalid_original_issued_spec"
    );
    drop(result);
    assert!(
        allocations.get() == 0
            && fixture.store.stats()? == before
            && fixture.budget.used() == baseline
            && dropped.load(Ordering::SeqCst),
        "c5_native_finalization_invalid_original_encoded_or_retained_authority"
    );
    Ok(())
}

#[test]
fn c5_native_finalization_rejects_duplicate_keys_in_actual_original_bytes() -> TestResult {
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = fixture.source();
        let raw = serde_json::to_vec(&source)?;
        let mut duplicate = b"{\"id\":".to_vec();
        duplicate.extend_from_slice(&serde_json::to_vec(&source["id"])?);
        duplicate.push(b',');
        duplicate.extend_from_slice(
            raw.get(1..)
                .ok_or("c5_native_finalization_object_missing")?,
        );
        assert!(
            serde_json::from_slice::<Value>(&duplicate)? == source,
            "c5_native_finalization_duplicate_fixture_changed_semantics"
        );
        assert_original_rejected_before_encoding(&fixture, &source, duplicate)?;
    }
    Ok(())
}

#[test]
fn c5_native_finalization_rejects_invalid_utf8_in_actual_original_bytes() -> TestResult {
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = fixture.source();
        let mut invalid = serde_json::to_vec(&source)?;
        invalid.push(0xff);
        assert_original_rejected_before_encoding(&fixture, &source, invalid)?;
    }
    Ok(())
}

#[test]
fn c5_native_finalization_rejects_reordered_members_in_actual_original_bytes() -> TestResult {
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = fixture.source();
        let reordered = Value::Object(
            source
                .as_object()
                .ok_or("c5_native_finalization_object_missing")?
                .iter()
                .rev()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        let raw = serde_json::to_vec(&reordered)?;
        assert!(
            reordered == source && raw != serde_json::to_vec(&source)?,
            "c5_native_finalization_reordered_fixture_not_semantically_equal_and_byte_distinct"
        );
        assert_original_rejected_before_encoding(&fixture, &source, raw)?;
    }
    Ok(())
}

#[test]
fn c5_native_finalization_encoding_denial_changed_and_unchanged_exposes_nothing() -> TestResult {
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = fixture.source();
        let observed = observation(&fixture, &source)?;
        let baseline = fixture.budget.used();
        let stage = fixture.stage(&observed)?;
        let proof = fixture.proof(&source)?;
        let encoding_baseline = fixture.budget.used();
        let before = fixture.store.stats()?;
        let (original, dropped) = original_bytes(&source)?;
        let denied = Rc::new(Cell::new(false));
        let seen = Rc::clone(&denied);
        let store = fixture.store.clone();
        let hook = buffer_events::install(move |event| {
            if event.phase == buffer_events::Phase::BeforeAllocation {
                seen.set(true);
                assert!(
                    event.used >= encoding_baseline + event.requested,
                    "c5_native_finalization_encoding_not_precharged"
                );
                assert!(
                    store.stats().is_ok_and(|stats| stats == before),
                    "c5_native_finalization_store_changed_before_encoding"
                );
                buffer_events::Action::FailAllocation
            } else {
                buffer_events::Action::Continue
            }
        });
        let result =
            prepare_native_json_spec(stage, proof, &fixture.attempt, original, MAX_JSON_BYTES);
        drop(hook);
        assert!(
            denied.get(),
            "c5_native_finalization_mandatory_encoding_not_reached"
        );
        assert!(
            result.is_err(),
            "c5_native_finalization_encoding_failure_issued_spec"
        );
        drop(result);
        assert!(
            fixture.store.stats()? == before
                && fixture.budget.used() == baseline
                && dropped.load(Ordering::SeqCst),
            "c5_native_finalization_encoding_failure_exposed_output_or_retained_charge"
        );
    }
    Ok(())
}

#[test]
fn c5_native_finalization_max_cannot_bypass_mandatory_final_encoding() -> TestResult {
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = fixture.source();
        let observed = observation(&fixture, &source)?;
        let baseline = fixture.budget.used();
        let stage = fixture.stage(&observed)?;
        let before = fixture.store.stats()?;
        let (original, dropped) = original_bytes(&source)?;
        let result = prepare_native_json_spec(
            stage,
            fixture.proof(&source)?,
            &fixture.attempt,
            original,
            1,
        );
        assert!(
            result.is_err(),
            "c5_native_finalization_final_encoding_limit_bypassed"
        );
        drop(result);
        assert!(
            fixture.store.stats()? == before
                && fixture.budget.used() == baseline
                && dropped.load(Ordering::SeqCst),
            "c5_native_finalization_final_limit_failure_not_atomic"
        );
    }
    Ok(())
}

#[test]
fn c5_native_finalization_exhausted_stage_budget_cannot_be_replaced() -> TestResult {
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = fixture.source();
        let observed = observation(&fixture, &source)?;
        let baseline = fixture.budget.used();
        let stage = fixture.stage(&observed)?;
        let proof = fixture.proof(&source)?;
        let encoding_baseline = fixture.budget.used();
        let before = fixture.store.stats()?;
        let exhaust = fixture
            .budget
            .reserve(fixture.budget.limit() - encoding_baseline)?;
        let (original, dropped) = original_bytes(&source)?;
        let result = prepare_native_json_spec(stage, proof, &fixture.attempt, original, usize::MAX);
        assert!(
            result.is_err(),
            "c5_native_finalization_stage_budget_bypassed"
        );
        drop(result);
        assert!(
            fixture.store.stats()? == before && dropped.load(Ordering::SeqCst),
            "c5_native_finalization_budget_failure_exposed_output"
        );
        drop(exhaust);
        assert_eq!(
            fixture.budget.used(),
            baseline,
            "c5_native_finalization_budget_failure_leaked"
        );
    }
    Ok(())
}

#[test]
fn c5_native_finalization_sealed_spec_preparation_remains_unpublished() -> TestResult {
    let fixture = Fixture::new(true)?;
    let source = fixture.source();
    let observed = observation(&fixture, &source)?;
    let baseline = fixture.budget.used();
    let stage = fixture.stage(&observed)?;
    let before = fixture.store.stats()?;
    let (original, _) = original_bytes(&source)?;
    let prepared = prepared(&fixture, stage, &source, original)?;
    let spec = prepared
        .spec()
        .map_err(|_| "c5_native_finalization_store_boundary_spec_unavailable")?;
    assert!(
        matches!(
            fixture.lease.verify_native_delivered(
                &fixture.binding,
                chat(),
                prepared.stage.delivered()
            ),
            Err(TokenError::ContextUnavailable)
        ) && fixture.store.stats()? == before,
        "c5_native_finalization_sealed_spec_published_context"
    );
    drop(spec);
    drop(prepared);
    assert_eq!(
        fixture.budget.used(),
        baseline,
        "c5_native_finalization_unpublished_spec_charge_leaked"
    );
    Ok(())
}

#[test]
fn c5_native_store_prepare_accepts_sealed_spec_but_drop_never_publishes() -> TestResult {
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = fixture.source();
        let mut expected = json!([source["choices"][0]["message"].clone()]);
        expected[0]["content"] = json!(SYNTHETIC_TEXT);
        let observed = observation(&fixture, &source)?;
        let baseline = fixture.budget.used();
        let stage = fixture.stage(&observed)?;
        let before = fixture.store.stats()?;
        let (original, _) = original_bytes(&source)?;
        let prepared = prepared(&fixture, stage, &source, original)?;
        let spec = prepared
            .spec()
            .map_err(|_| "native_store_spec_fixture_rejected")?;
        let warmup = fixture
            .lease
            .prepare_native_publication(spec)
            .map_err(|_| "native_store_prepare_rejected")?;
        drop(warmup);
        let retained = fixture.store.stats()?;
        assert!(
            retained.entries == before.entries
                && retained.plaintext_bytes == before.plaintext_bytes
                && retained.metadata_bytes >= before.metadata_bytes,
            "native_store_first_cancel_changed_logical_occupancy"
        );
        // Empty slot backing retains its charged high-water mark. Repeat after
        // warmup to distinguish that backing from record/owner/pin leakage.
        let pending = fixture
            .lease
            .prepare_native_publication(
                prepared
                    .spec()
                    .map_err(|_| "native_store_repeat_spec_rejected")?,
            )
            .map_err(|_| "native_store_repeat_prepare_rejected")?;
        assert!(
            matches!(
                fixture.lease.verify_native_delivered(
                    &fixture.binding,
                    chat(),
                    prepared.stage.delivered()
                ),
                Err(TokenError::ContextUnavailable)
            ),
            "native_store_pending_visible_before_activation"
        );
        // No test activation issuer: dropping a pending owner is the only action here.
        drop(pending);
        drop(prepared);
        assert!(
            matches!(
                fixture
                    .lease
                    .verify_native_delivered(&fixture.binding, chat(), &expected),
                Err(TokenError::ContextUnavailable)
            ) && fixture.store.stats()? == retained
                && fixture.budget.used() == baseline,
            "native_store_dropped_pending_published_or_retained_charge"
        );
    }
    Ok(())
}

#[tokio::test]
async fn c5_native_store_finalizer_activates_after_encoding_before_body_poll() -> TestResult {
    use futures::StreamExt;
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = fixture.source();
        let observed = observation(&fixture, &source)?;
        let stage = fixture.stage(&observed)?;
        let delivered = stage.delivered().clone();
        let proof = fixture.proof(&source)?;
        let before = fixture.store.stats()?;
        let encoding_baseline = fixture.budget.used();
        let (original, dropped) = original_bytes(&source)?;
        let original_guard = original.clone();
        let allocations = Rc::new(Cell::new(0));
        let seen = Rc::clone(&allocations);
        let store = fixture.store.clone();
        let hook = buffer_events::install(move |event| {
            if event.phase == buffer_events::Phase::BeforeAllocation {
                seen.set(seen.get() + 1);
                assert!(
                    event.used >= encoding_baseline + event.requested,
                    "native_store_final_encoding_not_precharged"
                );
                assert!(
                    store.stats().is_ok_and(|stats| stats == before),
                    "native_store_activated_before_final_encoding"
                );
            }
            buffer_events::Action::Continue
        });
        let result = finalize_native_json(
            stage,
            &fixture.lease,
            proof,
            &fixture.attempt,
            original,
            MAX_JSON_BYTES,
        );
        drop(hook);
        let finalized = result.map_err(|_| "native_store_finalizer_prepare_rejected")?;
        assert!(
            allocations.get() > 0,
            "native_store_finalizer_skipped_final_encoding"
        );
        assert!(
            fixture
                .lease
                .verify_native_delivered(&fixture.binding, chat(), &delivered)
                .is_ok(),
            "native_store_finalizer_not_active_before_body_poll"
        );
        let mut stream = finalized.into_delivery_stream();
        let frame = stream
            .next()
            .await
            .ok_or("native_store_delivery_chunk_missing")??;
        let mut expected = source.clone();
        if changed {
            expected["choices"][0]["message"]["content"] = json!(SYNTHETIC_TEXT);
            assert!(
                frame.as_ptr() != original_guard.as_ptr(),
                "native_store_changed_wire_reused_original"
            );
        } else {
            assert!(
                frame.as_ptr() == original_guard.as_ptr() && frame.len() == original_guard.len(),
                "native_store_unchanged_wire_lost_original_owner"
            );
        }
        let decoded: Value = serde_json::from_slice(&frame)?;
        assert!(decoded == expected, "native_store_delivery_wire_changed");
        assert!(
            fixture
                .lease
                .verify_native_delivered(&fixture.binding, chat(), &delivered)
                .is_ok(),
            "native_store_delivery_chunk_lost_activation"
        );
        assert!(
            stream.next().await.is_none(),
            "native_store_delivery_emitted_second_terminal"
        );
        assert!(
            fixture
                .lease
                .verify_native_delivered(&fixture.binding, chat(), &delivered)
                .is_ok(),
            "native_store_delivery_completion_lost_activation"
        );
        drop(frame);
        drop(stream);
        drop(original_guard);
        assert!(
            dropped.load(Ordering::SeqCst),
            "native_store_original_owner_retained_after_delivery"
        );
    }
    Ok(())
}

#[tokio::test]
async fn c5_native_store_drop_before_poll_or_after_chunk_preserves_json_activation() -> TestResult {
    use futures::StreamExt;
    for changed in [false, true] {
        for poll_chunk in [false, true] {
            let fixture = Fixture::new(changed)?;
            let source = fixture.source();
            let observed = observation(&fixture, &source)?;
            let baseline = fixture.budget.used();
            let stage = fixture.stage(&observed)?;
            let delivered = stage.delivered().clone();
            let proof = fixture.proof(&source)?;
            let (original, dropped) = original_bytes(&source)?;
            let finalized = finalize_native_json(
                stage,
                &fixture.lease,
                proof,
                &fixture.attempt,
                original,
                MAX_JSON_BYTES,
            )
            .map_err(|_| "native_store_drop_fixture_prepare_rejected")?;
            assert!(
                fixture
                    .lease
                    .verify_native_delivered(&fixture.binding, chat(), &delivered)
                    .is_ok(),
                "native_store_drop_fixture_not_active_before_body_poll"
            );
            let published = fixture.store.stats()?;
            let mut stream = finalized.into_delivery_stream();
            if poll_chunk {
                let frame = stream
                    .next()
                    .await
                    .ok_or("native_store_drop_fixture_chunk_missing")??;
                assert!(
                    fixture
                        .lease
                        .verify_native_delivered(&fixture.binding, chat(), &delivered)
                        .is_ok(),
                    "native_store_drop_fixture_chunk_lost_activation"
                );
                drop(frame);
            }
            drop(stream);
            assert!(
                fixture
                    .lease
                    .verify_native_delivered(&fixture.binding, chat(), &delivered)
                    .is_ok()
                    && fixture.store.stats()? == published
                    && fixture.budget.used() == baseline
                    && dropped.load(Ordering::SeqCst),
                "native_store_dropped_delivery_lost_activation_or_retained_owner"
            );
        }
    }
    Ok(())
}

fn tool_source(fixture: &Fixture, text: &str) -> TestResult<Value> {
    let mut source = fixture.source();
    source["choices"][0]["finish_reason"] = json!("tool_calls");
    source["choices"][0]["message"]["tool_calls"] = json!([{
        "id":"call_synthetic_json_revalidation", "type":"function",
        "function":{"name":"synthetic_noop", "arguments":serde_json::to_string(&json!({
            "label":text, "count":7, "enabled":true, "opaque":[null, 3]
        }))?},
        "synthetic_metadata":null
    }]);
    Ok(source)
}

fn tool_observation<'a>(
    fixture: &Fixture,
    source: &'a Value,
) -> TestResult<NativeJsonObservation<'a>> {
    let mut sites = TextSites::new(fixture.budget.clone())?;
    sites.observe(
        &source["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    NativeJsonObservation::new(
        source,
        chat(),
        sites,
        &[&source["choices"][0]["message"]],
        fixture.budget.clone(),
    )
    .map_err(|_| "native_json_revalidation_observation_rejected".into())
}

#[test]
fn c5_native_json_revalidation_rejects_invalid_restored_tool_before_publication() -> TestResult {
    use crate::proxy::security::tokenization::publication_test_support;
    let mut fixture = Fixture::new(false)?;
    let reference = fixture.lease.prepare(
        &[TokenFragment {
            raw: "Synthetic \" quote",
            whole_span_categories: &[TokenCategory::Phone],
        }],
        &[],
        |tokens| Ok(tokens[0].clone()),
    )?;
    // Preparing a mapping updates the generation, so bind only the resulting request.
    fixture.binding = fixture.lease.bind_actual_attempt(
        "synthetic-c5-finalization-provider",
        "openai_chat",
        "https://synthetic.invalid/v1/chat/completions",
        Some("synthetic-c5-finalization-principal"),
    )?;
    let source = tool_source(&fixture, &reference)?;
    let proof = fixture.proof(&source)?;
    let observed = tool_observation(&fixture, &source)?;
    let baseline = fixture.budget.used();
    let before = fixture.store.stats()?;
    let stage = fixture.stage(&observed)?;
    assert!(
        stage.changed(),
        "native_json_revalidation_fixture_not_changed"
    );
    let arguments = stage.as_value()["choices"][0]["message"]["tool_calls"][0]["function"]
        ["arguments"]
        .as_str()
        .ok_or("native_json_revalidation_arguments_missing")?;
    assert!(
        serde_json::from_str::<Value>(arguments).is_err(),
        "native_json_revalidation_fixture_still_valid"
    );
    let (original, dropped) = original_bytes(&source)?;
    let result = finalize_native_json(
        stage,
        &fixture.lease,
        proof,
        &fixture.attempt,
        original,
        MAX_JSON_BYTES,
    );
    let rejected = result.is_err();
    drop(result);
    let counts = publication_test_support::counts(&fixture.store)?;
    assert!(rejected, "native_json_invalid_restored_tool_released");
    assert!(
        counts.0 == 0 && counts.1 == 0 && counts.2 == 0,
        "native_json_invalid_restored_tool_published"
    );
    assert!(
        fixture.store.stats()? == before
            && fixture.budget.used() == baseline
            && dropped.load(Ordering::SeqCst),
        "native_json_revalidation_rejection_not_atomic"
    );
    Ok(())
}

#[tokio::test]
async fn c5_native_json_revalidation_safe_tool_and_zero_mapping_preserve_wire() -> TestResult {
    use crate::proxy::security::tokenization::publication_test_support;
    use futures::StreamExt;
    for changed in [false, true] {
        let fixture = Fixture::new(changed)?;
        let source = tool_source(
            &fixture,
            fixture.reference.as_deref().unwrap_or(SYNTHETIC_TEXT),
        )?;
        let proof = fixture.proof(&source)?;
        let observed = tool_observation(&fixture, &source)?;
        let baseline = fixture.budget.used();
        let stage = fixture.stage(&observed)?;
        assert!(
            stage.changed() == changed,
            "native_json_safe_tool_mapping_state_changed"
        );
        let delivered = stage.delivered().clone();
        let (original, _) = original_bytes(&source)?;
        let original_guard = original.clone();
        let finalized = finalize_native_json(
            stage,
            &fixture.lease,
            proof,
            &fixture.attempt,
            original,
            MAX_JSON_BYTES,
        )
        .map_err(|_| "native_json_safe_tool_finalization_rejected")?;
        let counts = publication_test_support::counts(&fixture.store)?;
        assert!(
            counts.0 == 1 && counts.1 == 0 && counts.2 == 0,
            "native_json_safe_tool_not_active_before_poll"
        );
        assert!(
            fixture
                .lease
                .verify_native_delivered(&fixture.binding, chat(), &delivered)
                .is_ok(),
            "native_json_safe_tool_missing_delivered_evidence"
        );
        let mut stream = finalized.into_delivery_stream();
        let frame = stream
            .next()
            .await
            .ok_or("native_json_safe_tool_chunk_missing")??;
        let decoded: Value = serde_json::from_slice(&frame)?;
        let args: Value = serde_json::from_str(
            decoded["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .ok_or("native_json_safe_tool_arguments_missing")?,
        )?;
        let mut expected = source.clone();
        expected["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
            json!(serde_json::to_string(
                &json!({"label":SYNTHETIC_TEXT,"count":7,"enabled":true,"opaque":[null,3]})
            )?);
        assert!(
            decoded == expected && args["count"].as_u64() == Some(7) && args["enabled"] == true,
            "native_json_safe_tool_wire_or_types_changed"
        );
        if !changed {
            assert!(
                frame.as_ptr() == original_guard.as_ptr() && frame.len() == original_guard.len(),
                "native_json_zero_mapping_original_owner_changed"
            );
        }
        assert!(
            stream.next().await.is_none(),
            "native_json_safe_tool_extra_terminal"
        );
        drop(frame);
        drop(stream);
        drop(original_guard);
        assert!(
            fixture.budget.used() == baseline,
            "native_json_safe_tool_charge_leaked"
        );
    }
    Ok(())
}

// Characterize restored-wire parser failures without changing production behavior.
#[test]
fn c5_native_json_revalidation_parser_faults_are_atomic() -> TestResult {
    use crate::proxy::security::{
        payload_json::test_support as json_events, tokenization::publication_test_support,
    };
    for changed in [false, true] {
        for target in [2, 3, 4] {
            for phase in [
                json_events::Phase::BeforeParse,
                json_events::Phase::AfterParse,
            ] {
                let fixture = Fixture::new(changed)?;
                let source = tool_source(
                    &fixture,
                    fixture.reference.as_deref().unwrap_or(SYNTHETIC_TEXT),
                )?;
                let proof = fixture.proof(&source)?;
                let observed = tool_observation(&fixture, &source)?;
                let baseline = fixture.budget.used();
                let before = fixture.store.stats()?;
                let stage = fixture.stage(&observed)?;
                let staged = fixture.budget.used();
                let (original, dropped) = original_bytes(&source)?;
                let parses = Rc::new(Cell::new(0));
                let denied = Rc::new(Cell::new(false));
                let seen = Rc::clone(&parses);
                let failed = Rc::clone(&denied);
                let store = fixture.store.clone();
                let hook = json_events::install(move |event| {
                    if event.phase == json_events::Phase::BeforeParse {
                        seen.set(seen.get() + 1);
                    }
                    if seen.get() == target && event.phase == phase {
                        failed.set(true);
                        assert!(
                            event.charged > 0 && event.used >= staged + event.charged,
                            "native_json_restored_parse_not_precharged"
                        );
                        assert!(
                            store.stats().is_ok_and(|stats| stats == before),
                            "native_json_restored_parse_published_early"
                        );
                        return json_events::Action::Fail;
                    }
                    json_events::Action::Continue
                });
                let result = finalize_native_json(
                    stage,
                    &fixture.lease,
                    proof,
                    &fixture.attempt,
                    original,
                    MAX_JSON_BYTES,
                );
                drop(hook);
                assert!(
                    denied.get() && parses.get() == target,
                    "native_json_restored_parse_fault_not_reached"
                );
                assert!(result.is_err(), "native_json_restored_parse_fault_released");
                drop(result);
                let counts = publication_test_support::counts(&fixture.store)?;
                assert!(
                    counts.0 == 0 && counts.1 == 0 && counts.2 == 0,
                    "native_json_restored_parse_fault_retained_publication"
                );
                assert!(
                    fixture.store.stats()? == before
                        && fixture.budget.used() == baseline
                        && dropped.load(Ordering::SeqCst),
                    "native_json_restored_parse_fault_not_atomic"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn c5_native_json_revalidation_real_budget_denial_is_atomic() -> TestResult {
    use crate::proxy::security::{
        payload_json::test_support as json_events, tokenization::publication_test_support,
    };
    use std::cell::RefCell;
    for changed in [false, true] {
        for deny_inner in [false, true] {
            let fixture = Fixture::new(changed)?;
            let mut source = tool_source(
                &fixture,
                fixture.reference.as_deref().unwrap_or(SYNTHETIC_TEXT),
            )?;
            // The inner tree must cost more than the outer parser's released scratch.
            let arguments = source["choices"][0]["message"]["tool_calls"][0]["function"]
                ["arguments"]
                .as_str()
                .ok_or("native_json_budget_arguments_missing")?;
            let mut arguments: Value = serde_json::from_str(arguments)?;
            arguments["bulk"] = json!(vec![0; 128]);
            source["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
                json!(serde_json::to_string(&arguments)?);
            let proof = fixture.proof(&source)?;
            let observed = tool_observation(&fixture, &source)?;
            let baseline = fixture.budget.used();
            let before = fixture.store.stats()?;
            let stage = fixture.stage(&observed)?;
            let (original, dropped) = original_bytes(&source)?;
            let occupied = Rc::new(RefCell::new(None));
            let held = Rc::clone(&occupied);
            let budget = fixture.budget.clone();
            let encoded = Rc::new(Cell::new(false));
            let seen_encoding = Rc::clone(&encoded);
            let buffer_hook = buffer_events::install(move |event| {
                if event.phase == buffer_events::Phase::CapacityObserved {
                    seen_encoding.set(true);
                    if !deny_inner && held.borrow().is_none() {
                        held.replace(Some(
                            budget
                                .reserve(budget.limit() - budget.used())
                                .unwrap_or_else(|_| {
                                    panic!("native_json_budget_fixture_reservation_failed")
                                }),
                        ));
                    }
                }
                buffer_events::Action::Continue
            });
            let parses = Rc::new(Cell::new(0));
            let seen = Rc::clone(&parses);
            let held = Rc::clone(&occupied);
            let budget = fixture.budget.clone();
            let hook = json_events::install(move |event| {
                if event.phase == json_events::Phase::BeforeParse {
                    seen.set(seen.get() + 1);
                }
                if deny_inner && seen.get() == 2 && event.phase == json_events::Phase::AfterParse {
                    held.replace(Some(
                        budget
                            .reserve(budget.limit() - budget.used())
                            .unwrap_or_else(|_| {
                                panic!("native_json_budget_fixture_reservation_failed")
                            }),
                    ));
                }
                json_events::Action::Continue
            });
            let result = finalize_native_json(
                stage,
                &fixture.lease,
                proof,
                &fixture.attempt,
                original,
                MAX_JSON_BYTES,
            );
            drop(hook);
            drop(buffer_hook);
            assert!(
                encoded.get()
                    && parses.get() == if deny_inner { 2 } else { 1 }
                    && occupied.borrow().is_some(),
                "native_json_restored_budget_denial_not_reached"
            );
            assert!(
                result.is_err(),
                "native_json_restored_budget_denial_released"
            );
            drop(result);
            drop(occupied.borrow_mut().take());
            let counts = publication_test_support::counts(&fixture.store)?;
            assert!(
                counts.0 == 0 && counts.1 == 0 && counts.2 == 0,
                "native_json_restored_budget_denial_retained_publication"
            );
            assert!(
                fixture.store.stats()? == before
                    && fixture.budget.used() == baseline
                    && dropped.load(Ordering::SeqCst),
                "native_json_restored_budget_denial_not_atomic"
            );
        }
    }
    Ok(())
}
