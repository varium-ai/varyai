//! Native controlled-import characterization after real async EOF and sealed finalization.
//! These fixtures grant no native registration, Responses cursor, or result pairing.
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use serde_json::{json, Value};

use super::{
    completed_context::{ActualAttemptBinding, CompletedContextHandle, CompletedEvidenceIdentity},
    http_restoration::test_transport,
    payload_budget::PayloadBudget,
    response_restoration::{finalize_native_json, FinalizedNativeJson},
    restoration_contract::{
        ClientProtocol, ContinuityScope, EditConstraint, IngressNamespace, SourceClass, TextSites,
    },
    restoration_core::{stage_native_json, AttemptProvenance, NativeJsonObservation},
    token_store_owners::test_support::{self, Kind, Phase},
    tokenization::{
        DlpRequestLease, DlpTokenStore, TokenCategory, TokenError, TokenFragment, TokenStoreLimits,
    },
};
use crate::proxy::protocol_bridge::Protocol;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const APP: &str = "synthetic-c5-import-app";
const SESSION: &str = "synthetic-c5-import-session";
const FIRST: &str = "Synthetic visible native dependency";
const SECOND: &str = "Synthetic retained native dependency";
const EXISTING: &str = "Synthetic current request dependency";

fn chat() -> ClientProtocol {
    ClientProtocol::new(Protocol::Chat)
}

fn admit(store: &DlpTokenStore) -> TestResult<DlpRequestLease> {
    Ok(store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: chat(),
            ingress: IngressNamespace::Direct,
        },
        APP,
        Some(SESSION),
        1,
    )?)
}

fn bind(lease: &DlpRequestLease) -> TestResult<ActualAttemptBinding> {
    Ok(lease.bind_actual_attempt(
        "synthetic-c5-import-provider",
        "openai_chat",
        "https://synthetic.invalid/v1/chat/completions",
        Some("synthetic-c5-import-principal"),
    )?)
}

fn fragment(raw: &str) -> TokenFragment<'_> {
    TokenFragment {
        raw,
        whole_span_categories: &[TokenCategory::Custom],
    }
}

struct Fixture {
    store: DlpTokenStore,
    lease: DlpRequestLease,
    finalized: FinalizedNativeJson,
    delivered: Value,
    references: Vec<String>,
}

impl Fixture {
    fn mapped() -> TestResult<Self> {
        Self::with_store(DlpTokenStore::new())
    }

    fn with_store(store: DlpTokenStore) -> TestResult<Self> {
        let mut lease = admit(&store)?;
        let references = lease.prepare(&[fragment(FIRST), fragment(SECOND)], &[], |tokens| {
            Ok(tokens.to_vec())
        })?;
        let binding = bind(&lease)?;
        let source = json!({
            "id":"chatcmpl_synthetic_c5_import",
            "object":"chat.completion",
            "choices":[{
                "index":0,
                "message":{"role":"assistant","content":references[0]},
                "finish_reason":"stop"
            }]
        });
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let attempt = AttemptProvenance::new(&budget)?;
        let message = &source["choices"][0]["message"];
        let mut sites = TextSites::new(budget.clone())?;
        sites.observe(
            &message["content"],
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        let observation =
            NativeJsonObservation::new(&source, chat(), sites, &[message], budget.clone())
                .map_err(|_| "c5_native_import_observation_rejected")?;
        let stage = stage_native_json(&observation, &lease, &binding)
            .map_err(|_| "c5_native_import_stage_rejected")?;
        assert!(
            stage.output_tokens().len() == 1,
            "c5_native_import_fixture_requires_unobserved_dependency"
        );
        let delivered = stage.delivered().clone();
        let original = Bytes::from(serde_json::to_vec(&source)?);
        let proof = futures::executor::block_on(test_transport::native_json_proof(
            &attempt,
            &binding,
            &budget,
            original.clone(),
            &source,
        ))
        .map_err(|_| "c5_native_import_real_async_proof_rejected")?;
        let finalized = finalize_native_json(stage, &lease, proof, &attempt, original, 1024 * 1024)
            .map_err(|_| "c5_native_import_sealed_finalization_rejected")?;
        Ok(Self {
            store,
            lease,
            finalized,
            delivered,
            references,
        })
    }

    fn next(
        &self,
    ) -> TestResult<(
        DlpRequestLease,
        ActualAttemptBinding,
        CompletedContextHandle,
    )> {
        let lease = admit(&self.store)?;
        let binding = bind(&lease)?;
        let handle = lease.verify_native_delivered(&binding, chat(), &self.delivered)?;
        Ok((lease, binding, handle))
    }
}

#[test]
fn c5_native_import_complete_dependencies_overlap_and_idempotence() -> TestResult {
    let fixture = Fixture::mapped()?;
    let (mut next, binding, handle) = fixture.next()?;
    next.prepare(&[fragment(FIRST), fragment(EXISTING)], &[], |_| Ok(()))?;
    let before = fixture.store.stats()?;
    next.import_native_completed_context(&binding, chat(), &handle)?;
    assert!(
        next.resolve_if_authorized(&fixture.references[0])? == Some(FIRST)
            && next.resolve_if_authorized(&fixture.references[1])? == Some(SECOND)
            && next.authorized_tokens()?.len() == 3,
        "c5_native_import_incomplete_immutable_dependency_set"
    );
    let after = fixture.store.stats()?;
    assert!(
        after.entries == before.entries
            && after.plaintext_bytes == before.plaintext_bytes
            && after.metadata_bytes > before.metadata_bytes,
        "c5_native_import_wrong_persistent_charge"
    );
    let ledger = fixture.store.allocation_ledger_for_test()?;
    let used = ledger.used();
    let hook = test_support::install(|event| {
        assert!(
            event.phase != Phase::BeforeAllocation,
            "c5_native_import_idempotent_import_allocated"
        );
        Ok(())
    });
    next.import_native_completed_context(&binding, chat(), &handle)?;
    drop(hook);
    assert!(
        fixture.store.stats()? == after && ledger.used() == used,
        "c5_native_import_idempotent_import_changed_charge"
    );
    Ok(())
}

#[test]
fn c5_native_import_late_allocation_failure_is_atomic_and_retryable() -> TestResult {
    for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
        for seeded in [false, true] {
            let fixture = Fixture::mapped()?;
            let (mut next, binding, handle) = fixture.next()?;
            let existing = if seeded {
                next.prepare(&[fragment(EXISTING)], &[], |tokens| Ok(tokens.to_vec()))?
            } else {
                Vec::new()
            };
            let before = fixture.store.stats()?;
            let capacity = next.authority_capacity_for_test();
            let ledger = fixture.store.allocation_ledger_for_test()?;
            let used = ledger.used();
            let hits = Rc::new(Cell::new(0usize));
            let observed = Rc::clone(&hits);
            let store = fixture.store.clone();
            let hook = test_support::install(move |event| {
                if event.kind == Kind::Vector && event.phase == phase {
                    observed.set(observed.get() + 1);
                    // A successful stats lock also proves allocation is outside the store lock.
                    assert!(
                        store.stats()? == before && event.used.metadata > used.metadata,
                        "c5_native_import_staging_was_not_precharged"
                    );
                    return Err(TokenError::Unavailable);
                }
                Ok(())
            });
            let result = next.import_native_completed_context(&binding, chat(), &handle);
            drop(hook);
            assert!(
                matches!(result, Err(TokenError::Unavailable)) && hits.get() == 1,
                "c5_native_import_allocation_failure_not_exercised"
            );
            assert!(
                fixture.store.stats()? == before
                    && ledger.used() == used
                    && next.authority_capacity_for_test() == capacity
                    && next.authorized_tokens()?.len() == usize::from(seeded),
                "c5_native_import_allocation_failure_retained_partial_authority_or_charge"
            );
            for reference in &fixture.references {
                assert!(
                    next.resolve_if_authorized(reference)?.is_none(),
                    "c5_native_import_failed_transaction_authorized_dependency"
                );
            }
            for reference in &existing {
                assert!(
                    next.resolve_if_authorized(reference)? == Some(EXISTING),
                    "c5_native_import_failed_transaction_lost_existing_authority"
                );
            }
            next.import_native_completed_context(&binding, chat(), &handle)?;
            assert!(
                next.authorized_tokens()?.len() == 2 + usize::from(seeded),
                "c5_native_import_retry_lost_dependencies"
            );
        }
    }
    Ok(())
}

#[test]
fn c5_native_import_exact_handle_fields_and_evidence_variant_are_required() -> TestResult {
    let fixture = Fixture::mapped()?;
    let (mut next, binding, handle) = fixture.next()?;
    let mut mismatches = Vec::new();
    let mut wrong = handle.clone();
    wrong.store_id ^= 1;
    mismatches.push(wrong);
    let mut wrong = handle.clone();
    wrong.namespace ^= 1;
    mismatches.push(wrong);
    let mut wrong = handle.clone();
    wrong.generation[0] ^= 1;
    mismatches.push(wrong);
    let mut wrong = handle.clone();
    wrong.policy_revision ^= 1;
    mismatches.push(wrong);
    let mut wrong = handle.clone();
    wrong.provider_fingerprint[0] ^= 1;
    mismatches.push(wrong);
    let digest = match handle.evidence {
        CompletedEvidenceIdentity::Chat(digest) => digest,
        _ => return Err("c5_native_import_chat_handle_missing".into()),
    };
    for evidence in [
        CompletedEvidenceIdentity::Responses(digest),
        CompletedEvidenceIdentity::Anthropic(digest),
        CompletedEvidenceIdentity::Gemini(digest),
        CompletedEvidenceIdentity::Chat([0; 32]),
    ] {
        let mut wrong = handle.clone();
        wrong.evidence = evidence;
        mismatches.push(wrong);
    }
    let before = fixture.store.stats()?;
    let ledger = fixture.store.allocation_ledger_for_test()?;
    let used = ledger.used();
    for wrong in mismatches {
        assert!(
            matches!(
                next.import_native_completed_context(&binding, chat(), &wrong),
                Err(TokenError::ContextUnavailable)
            ),
            "c5_native_import_mismatched_handle_accepted"
        );
        assert!(
            next.is_empty() && fixture.store.stats()? == before && ledger.used() == used,
            "c5_native_import_mismatched_handle_mutated_authority_or_charge"
        );
    }
    next.import_native_completed_context(&binding, chat(), &handle)?;
    Ok(())
}

#[test]
fn c5_native_import_expired_handle_cannot_authorize_recreated_namespace() -> TestResult {
    let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
        idle_ttl: Duration::from_secs(1),
        ..Default::default()
    });
    let fixture = Fixture::with_store(store.clone())?;
    let old_binding = bind(&fixture.lease)?;
    let old_handle =
        fixture
            .lease
            .verify_native_delivered(&old_binding, chat(), &fixture.delivered)?;
    let delivered = fixture.delivered.clone();
    let references = fixture.references.clone();
    drop(fixture.finalized.into_delivery_stream());
    drop(fixture.lease);
    drop(fixture.store);
    store.expire_idle_at(Instant::now() + Duration::from_secs(60))?;
    assert!(
        store.stats()?.namespaces == 0,
        "c5_native_import_expiration_fixture_still_live"
    );
    let mut next = admit(&store)?;
    let current_binding = bind(&next)?;
    assert!(
        current_binding.generation != old_binding.generation,
        "c5_native_import_generation_did_not_rotate"
    );
    let before = store.stats()?;
    let ledger = store.allocation_ledger_for_test()?;
    let used = ledger.used();
    for binding in [&old_binding, &current_binding] {
        assert!(
            matches!(
                next.import_native_completed_context(binding, chat(), &old_handle),
                Err(TokenError::ContextUnavailable)
            ),
            "c5_native_import_expired_handle_accepted"
        );
    }
    assert!(
        matches!(
            next.verify_native_delivered(&current_binding, chat(), &delivered),
            Err(TokenError::ContextUnavailable)
        ) && next.is_empty()
            && next.authority_capacity_for_test() == 0
            && store.stats()? == before
            && ledger.used() == used,
        "c5_native_import_stale_handle_changed_authority_or_charge"
    );
    for reference in references {
        assert!(
            next.resolve_if_authorized(&reference)?.is_none(),
            "c5_native_import_stale_handle_inherited_mapping"
        );
    }
    Ok(())
}
