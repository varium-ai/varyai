//! Layered native evidence boundaries using real sealed Chat JSON finalization.
//! Selections below are test-only: no native adapter, lane, or registration grant.
//! Ordered assistant/call snapshots attest no tool-result body or result pairing.
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};

use super::{
    completed_context::{ActualAttemptBinding, CompletedCallSpec, CompletedContextHandle},
    http_restoration::test_transport,
    payload_budget::PayloadBudget,
    protocol_adapters::{registration, Registration},
    response_restoration::{finalize_native_json, FinalizedNativeJson},
    restoration_contract::{
        ClientProtocol, ContinuityScope, EditConstraint, Framing, IngressNamespace, Operation,
        ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
    },
    restoration_core::{stage_native_json, AttemptProvenance, NativeJsonObservation},
    tokenization::{
        DlpRequestLease, DlpTokenStore, TokenCategory, TokenError, TokenFragment, TokenStoreLimits,
    },
};
use crate::proxy::protocol_bridge::Protocol;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const APP: &str = "synthetic-c5-evidence-app";
const SESSION: &str = "synthetic-c5-evidence-session";
const PROVIDER: &str = "synthetic-c5-evidence-provider";
const PRINCIPAL: &str = "synthetic-c5-evidence-principal";
const TARGET: &str = "https://synthetic.invalid/v1/chat/completions";
const RESPONSE_ID: &str = "chatcmpl_synthetic_c5_evidence";
const CALL_ID: &str = "synthetic-c5-evidence-call-a";
const SYNTHETIC_TEXT: &str = "Synthetic native evidence message";
const BUDGET_BYTES: usize = 4 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 1024 * 1024;

fn chat() -> ClientProtocol {
    ClientProtocol::new(Protocol::Chat)
}

fn native_scope(client: ClientProtocol, ingress: IngressNamespace) -> ContinuityScope {
    ContinuityScope::Native { client, ingress }
}

fn admit(
    store: &DlpTokenStore,
    scope: ContinuityScope,
    session: &str,
    revision: u64,
) -> TestResult<DlpRequestLease> {
    Ok(store.admit_scoped_published_policy(scope, APP, Some(session), revision)?)
}

fn bind(
    lease: &DlpRequestLease,
    provider: &str,
    principal: Option<&str>,
) -> TestResult<ActualAttemptBinding> {
    Ok(lease.bind_actual_attempt(provider, "openai_chat", TARGET, principal)?)
}

fn unavailable<T>(result: Result<T, TokenError>) -> bool {
    matches!(result, Err(TokenError::ContextUnavailable))
}

fn chat_json(text: &str) -> TestResult<Value> {
    let first_arguments = serde_json::to_string(&json!({"value":text,"ordinal":1}))?;
    let second_arguments = serde_json::to_string(&json!({"value":text,"ordinal":2}))?;
    Ok(json!({
        "id":RESPONSE_ID,
        "object":"chat.completion",
        "model":"synthetic-native-model",
        "choices":[{
            "index":0,
            "message":{
                "role":"assistant",
                "content":text,
                "tool_calls":[
                    {"id":CALL_ID,"type":"function","function":{
                        "name":"synthetic_tool_a","arguments":first_arguments
                    }},
                    {"id":"synthetic-c5-evidence-call-b","type":"function","function":{
                        "name":"synthetic_tool_b","arguments":second_arguments
                    }}
                ]
            },
            "finish_reason":"tool_calls"
        }],
        "usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7},
        "synthetic_extension":{"keep":true}
    }))
}

fn snapshots(body: &Value) -> Value {
    let message = &body["choices"][0]["message"];
    json!([
        message,
        &message["tool_calls"][0],
        &message["tool_calls"][1]
    ])
}

struct Fixture {
    store: DlpTokenStore,
    lease: DlpRequestLease,
    binding: ActualAttemptBinding,
    budget: PayloadBudget,
    attempt: AttemptProvenance,
    references: Vec<String>,
}

impl Fixture {
    fn mapped() -> TestResult<Self> {
        Self::with_store(
            DlpTokenStore::new(),
            &[TokenFragment {
                raw: SYNTHETIC_TEXT,
                whole_span_categories: &[TokenCategory::Custom],
            }],
        )
    }

    fn zero_mapping() -> TestResult<Self> {
        Self::with_store(DlpTokenStore::new(), &[])
    }

    fn with_store(store: DlpTokenStore, fragments: &[TokenFragment<'_>]) -> TestResult<Self> {
        let mut lease = admit(
            &store,
            native_scope(chat(), IngressNamespace::Direct),
            SESSION,
            1,
        )?;
        // Even the zero-map fixture goes through the real request preparation transaction.
        let references = lease.prepare(fragments, &[], |tokens| Ok(tokens.to_vec()))?;
        let binding = bind(&lease, PROVIDER, Some(PRINCIPAL))?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let attempt = AttemptProvenance::new(&budget)?;
        Ok(Self {
            store,
            lease,
            binding,
            budget,
            attempt,
            references,
        })
    }

    fn reference(&self) -> TestResult<&str> {
        self.references
            .first()
            .map(String::as_str)
            .ok_or_else(|| "c5_native_evidence_mapping_fixture_missing".into())
    }

    fn source(&self) -> TestResult<Value> {
        chat_json(self.reference()?)
    }

    fn next(&self) -> TestResult<(DlpRequestLease, ActualAttemptBinding)> {
        let lease = admit(
            &self.store,
            native_scope(chat(), IngressNamespace::Direct),
            SESSION,
            1,
        )?;
        let binding = bind(&lease, PROVIDER, Some(PRINCIPAL))?;
        Ok((lease, binding))
    }

    fn observe<'a>(&self, source: &'a Value) -> TestResult<NativeJsonObservation<'a>> {
        let message = &source["choices"][0]["message"];
        let mut sites = TextSites::new(self.budget.clone())?;
        sites.observe(
            &message["content"],
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        for index in 0..2 {
            sites.observe(
                &message["tool_calls"][index]["function"]["arguments"],
                SourceClass::ToolArguments,
                EditConstraint::Editable,
            )?;
        }
        NativeJsonObservation::new(
            source,
            chat(),
            sites,
            &[
                message,
                &message["tool_calls"][0],
                &message["tool_calls"][1],
            ],
            self.budget.clone(),
        )
        .map_err(|_| "c5_native_evidence_observation_fixture_rejected".into())
    }

    fn finalize(&self, source: &Value) -> TestResult<(FinalizedNativeJson, Value)> {
        let observed = self.observe(source)?;
        let stage = stage_native_json(&observed, &self.lease, &self.binding)
            .map_err(|_| "c5_native_evidence_stage_fixture_rejected")?;
        let delivered = stage.delivered().clone();
        assert!(
            delivered == snapshots(stage.as_value())
                && delivered.as_array().is_some_and(|items| items.len() == 3),
            "c5_native_evidence_ordered_assistant_call_snapshots_lost"
        );
        assert!(
            stage.output_tokens().len() == self.references.len()
                && stage
                    .output_tokens()
                    .iter()
                    .zip(&self.references)
                    .all(|(actual, expected)| actual.as_str() == expected.as_str()),
            "c5_native_evidence_exact_dependency_ledger_lost"
        );
        let original = Bytes::from(serde_json::to_vec(source)?);
        let proof = futures::executor::block_on(test_transport::native_json_proof(
            &self.attempt,
            &self.binding,
            &self.budget,
            original.clone(),
            source,
        ))
        .map_err(|_| "c5_native_evidence_real_verifier_fixture_rejected")?;
        let finalized = finalize_native_json(
            stage,
            &self.lease,
            proof,
            &self.attempt,
            original,
            MAX_JSON_BYTES,
        )
        .map_err(|_| "c5_native_evidence_sealed_finalization_rejected")?;
        Ok((finalized, delivered))
    }
}

fn inspect(
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    delivered: &Value,
) -> TestResult<CompletedContextHandle> {
    lease
        .verify_native_delivered(binding, chat(), delivered)
        .map_err(|_| "c5_native_evidence_exact_replay_rejected".into())
}

fn assert_rejected_boundary(
    store: &DlpTokenStore,
    lease: &mut DlpRequestLease,
    binding: &ActualAttemptBinding,
    client: ClientProtocol,
    handle: &CompletedContextHandle,
    delivered: &Value,
) -> TestResult {
    let before = store.stats()?;
    assert!(
        unavailable(lease.verify_native_delivered(binding, client, delivered)),
        "c5_native_evidence_wrong_boundary_replayed"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "c5_native_evidence_rejected_replay_granted_authority"
    );
    assert!(
        unavailable(lease.import_native_completed_context(binding, client, handle)),
        "c5_native_evidence_wrong_boundary_imported"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "c5_native_evidence_rejected_import_changed_authority_or_store"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_staging_is_not_delivery_or_import() -> TestResult {
    let fixture = Fixture::mapped()?;
    let source = fixture.source()?;
    let observed = fixture.observe(&source)?;
    let (next, binding) = fixture.next()?;
    let before = fixture.store.stats()?;
    let stage = stage_native_json(&observed, &fixture.lease, &fixture.binding)
        .map_err(|_| "c5_native_evidence_inspection_stage_rejected")?;
    assert!(
        stage.changed()
            && stage.restored_occurrences() == 3
            && stage.delivered() == &snapshots(&chat_json(SYNTHETIC_TEXT)?),
        "c5_native_evidence_staging_did_not_restore_selected_snapshots"
    );
    assert!(
        unavailable(fixture.lease.verify_native_delivered(
            &fixture.binding,
            chat(),
            stage.delivered()
        )) && unavailable(next.verify_native_delivered(&binding, chat(), stage.delivered())),
        "c5_native_evidence_unsealed_stage_became_replayable"
    );
    assert!(
        next.is_empty()
            && next.resolve_if_authorized(fixture.reference()?)?.is_none()
            && fixture.store.stats()? == before,
        "c5_native_evidence_staging_published_or_imported"
    );
    Ok(())
}

#[tokio::test]
async fn c5_native_evidence_json_replay_is_visible_before_first_body_poll() -> TestResult {
    let fixture = Fixture::mapped()?;
    let source = fixture.source()?;
    let (finalized, delivered) = fixture.finalize(&source)?;
    let (next, binding) = fixture.next()?;
    let before = fixture.store.stats()?;
    // JSON activates before returning its opaque owner, unlike SSE delivery-resume gates.
    let _handle = inspect(&next, &binding, &delivered)?;
    assert!(
        next.is_empty()
            && next.resolve_if_authorized(fixture.reference()?)?.is_none()
            && fixture.store.stats()? == before,
        "c5_native_evidence_replay_implicitly_imported"
    );
    let mut stream = finalized.into_delivery_stream();
    let _handle_before_poll = inspect(&next, &binding, &delivered)?;
    let chunk = stream
        .next()
        .await
        .ok_or("c5_native_evidence_json_body_missing")?
        .map_err(|_| "c5_native_evidence_json_body_failed")?;
    let decoded: Value =
        serde_json::from_slice(&chunk).map_err(|_| "c5_native_evidence_json_body_invalid")?;
    assert!(
        decoded == chat_json(SYNTHETIC_TEXT)? && snapshots(&decoded) == delivered,
        "c5_native_evidence_body_and_replay_snapshots_diverged"
    );
    assert!(
        stream.next().await.is_none(),
        "c5_native_evidence_json_body_not_single_chunk"
    );
    let _handle_after_poll = inspect(&next, &binding, &delivered)?;
    assert!(
        next.is_empty() && next.resolve_if_authorized(fixture.reference()?)?.is_none(),
        "c5_native_evidence_body_poll_implicitly_imported"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_json_unpolled_drop_does_not_revoke_replay() -> TestResult {
    let fixture = Fixture::mapped()?;
    let source = fixture.source()?;
    let (finalized, delivered) = fixture.finalize(&source)?;
    let (next, binding) = fixture.next()?;
    let _handle = inspect(&next, &binding, &delivered)?;
    let stream = finalized.into_delivery_stream();
    // No body poll, last-yield acknowledgement, or resume token is required for JSON.
    drop(stream);
    let _handle_after_drop = inspect(&next, &binding, &delivered)?;
    assert!(
        next.is_empty() && next.resolve_if_authorized(fixture.reference()?)?.is_none(),
        "c5_native_evidence_unpolled_drop_imported_dependencies"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_exact_replay_requires_explicit_same_scope_binding_import() -> TestResult {
    let fixture = Fixture::mapped()?;
    let source = fixture.source()?;
    let (_finalized, delivered) = fixture.finalize(&source)?;
    let (mut next, binding) = fixture.next()?;
    let before = fixture.store.stats()?;
    let handle = inspect(&next, &binding, &delivered)?;
    assert!(
        next.is_empty()
            && unavailable(next.resolve(fixture.reference()?))
            && next.resolve_if_authorized(fixture.reference()?)?.is_none()
            && fixture.store.stats()? == before,
        "c5_native_evidence_exact_replay_granted_restore_authority"
    );
    next.import_native_completed_context(&binding, chat(), &handle)
        .map_err(|_| "c5_native_evidence_explicit_same_boundary_import_rejected")?;
    assert!(
        !next.is_empty()
            && next.resolve_if_authorized(fixture.reference()?)? == Some(SYNTHETIC_TEXT)
            && next.authorized_tokens()?.len() == 1,
        "c5_native_evidence_explicit_import_lost_exact_dependency"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_zero_map_literal_message_replay_and_import_remain_empty() -> TestResult {
    for literal in [
        "Synthetic ordinary message",
        "rredact-custom-",
        "rredact-custom-invalid",
        "rredact-custom-aaaaaaaaaaaaaaaaaaaa-ai",
        "vary_cu_bbbbbbbbbbbbbbbbbbbb_ai",
    ] {
        let fixture = Fixture::zero_mapping()?;
        let source = chat_json(&format!("Synthetic literal message: {literal}"))?;
        let observed = fixture.observe(&source)?;
        let stage = stage_native_json(&observed, &fixture.lease, &fixture.binding)
            .map_err(|_| "c5_native_evidence_literal_stage_rejected")?;
        assert!(
            !stage.changed()
                && stage.output_tokens().is_empty()
                && stage.as_value() == &source
                && stage.delivered() == &snapshots(&source),
            "c5_native_evidence_literal_message_changed_or_created_dependency"
        );
        drop(stage);
        drop(observed);
        let (_finalized, delivered) = fixture.finalize(&source)?;
        let (mut next, binding) = fixture.next()?;
        let before = fixture.store.stats()?;
        let handle = inspect(&next, &binding, &delivered)?;
        assert!(
            next.is_empty() && fixture.store.stats()? == before,
            "c5_native_evidence_zero_map_replay_changed_authority_or_store"
        );
        next.import_native_completed_context(&binding, chat(), &handle)
            .map_err(|_| "c5_native_evidence_zero_map_explicit_import_rejected")?;
        assert!(
            next.is_empty()
                && next.authorized_tokens()?.is_empty()
                && next.resolve_if_authorized(literal)?.is_none()
                && fixture.store.stats()?.entries == 0,
            "c5_native_evidence_zero_map_import_created_mapping"
        );
    }
    Ok(())
}

#[test]
fn c5_native_evidence_provider_and_principal_changes_reject_replay_and_import() -> TestResult {
    let fixture = Fixture::mapped()?;
    let source = fixture.source()?;
    let (_finalized, delivered) = fixture.finalize(&source)?;
    let handle = inspect(&fixture.lease, &fixture.binding, &delivered)?;
    for (provider, principal) in [
        ("synthetic-other-provider", Some(PRINCIPAL)),
        (PROVIDER, Some("synthetic-other-principal")),
        (PROVIDER, None),
    ] {
        let (mut next, _) = fixture.next()?;
        let binding = bind(&next, provider, principal)?;
        assert_rejected_boundary(
            &fixture.store,
            &mut next,
            &binding,
            chat(),
            &handle,
            &delivered,
        )?;
    }
    Ok(())
}

#[test]
fn c5_native_evidence_cross_client_replay_and_import_are_rejected() -> TestResult {
    let fixture = Fixture::mapped()?;
    let source = fixture.source()?;
    let (_finalized, delivered) = fixture.finalize(&source)?;
    let handle = inspect(&fixture.lease, &fixture.binding, &delivered)?;
    for protocol in [Protocol::Responses, Protocol::Anthropic, Protocol::Gemini] {
        let client = ClientProtocol::new(protocol);
        let (mut same_scope, same_binding) = fixture.next()?;
        assert_rejected_boundary(
            &fixture.store,
            &mut same_scope,
            &same_binding,
            client,
            &handle,
            &delivered,
        )?;
        let mut other_scope = admit(
            &fixture.store,
            native_scope(client, IngressNamespace::Direct),
            SESSION,
            1,
        )?;
        let other_binding = bind(&other_scope, PROVIDER, Some(PRINCIPAL))?;
        assert_rejected_boundary(
            &fixture.store,
            &mut other_scope,
            &other_binding,
            client,
            &handle,
            &delivered,
        )?;
    }
    Ok(())
}

#[test]
fn c5_native_evidence_session_ingress_policy_and_legacy_scope_cannot_import() -> TestResult {
    let fixture = Fixture::mapped()?;
    let source = fixture.source()?;
    let (_finalized, delivered) = fixture.finalize(&source)?;
    let handle = inspect(&fixture.lease, &fixture.binding, &delivered)?;
    for (scope, session, revision) in [
        (
            native_scope(chat(), IngressNamespace::Direct),
            "synthetic-other-session",
            1,
        ),
        (native_scope(chat(), IngressNamespace::Gateway), SESSION, 1),
        (native_scope(chat(), IngressNamespace::Direct), SESSION, 2),
        (ContinuityScope::LegacyResponses, SESSION, 1),
    ] {
        let mut next = admit(&fixture.store, scope, session, revision)?;
        let binding = bind(&next, PROVIDER, Some(PRINCIPAL))?;
        assert_rejected_boundary(
            &fixture.store,
            &mut next,
            &binding,
            chat(),
            &handle,
            &delivered,
        )?;
    }
    let other_store = DlpTokenStore::new();
    let mut next = admit(
        &other_store,
        native_scope(chat(), IngressNamespace::Direct),
        SESSION,
        1,
    )?;
    let binding = bind(&next, PROVIDER, Some(PRINCIPAL))?;
    assert_rejected_boundary(
        &other_store,
        &mut next,
        &binding,
        chat(),
        &handle,
        &delivered,
    )?;
    Ok(())
}

#[test]
fn c5_native_evidence_expired_generation_rejects_stale_replay_and_import() -> TestResult {
    let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
        idle_ttl: Duration::from_secs(1),
        ..Default::default()
    });
    let fixture = Fixture::with_store(
        store.clone(),
        &[TokenFragment {
            raw: SYNTHETIC_TEXT,
            whole_span_categories: &[TokenCategory::Custom],
        }],
    )?;
    let source = fixture.source()?;
    let reference = fixture.reference()?.to_owned();
    let old_binding = fixture.binding.clone();
    let (finalized, delivered) = fixture.finalize(&source)?;
    let handle = inspect(&fixture.lease, &old_binding, &delivered)?;
    drop(finalized.into_delivery_stream());
    drop(fixture);
    store.expire_idle_at(Instant::now() + Duration::from_secs(60))?;
    assert!(
        store.stats()?.namespaces == 0,
        "c5_native_evidence_generation_fixture_not_expired"
    );
    let mut next = admit(
        &store,
        native_scope(chat(), IngressNamespace::Direct),
        SESSION,
        1,
    )?;
    let binding = bind(&next, PROVIDER, Some(PRINCIPAL))?;
    assert!(
        binding.generation != old_binding.generation,
        "c5_native_evidence_generation_fixture_not_rotated"
    );
    assert_rejected_boundary(&store, &mut next, &binding, chat(), &handle, &delivered)?;
    assert_rejected_boundary(&store, &mut next, &old_binding, chat(), &handle, &delivered)?;
    assert!(
        next.resolve_if_authorized(&reference)?.is_none(),
        "c5_native_evidence_rotated_generation_inherited_dependency"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_exact_replay_rejects_ordered_snapshot_mutations() -> TestResult {
    let fixture = Fixture::mapped()?;
    let source = fixture.source()?;
    let (_finalized, delivered) = fixture.finalize(&source)?;
    let (next, binding) = fixture.next()?;
    let _handle = inspect(&next, &binding, &delivered)?;
    let before = fixture.store.stats()?;
    let mut mutations = vec![json!([]), Value::Null, chat_json(SYNTHETIC_TEXT)?];
    let mut reordered = delivered.clone();
    reordered
        .as_array_mut()
        .ok_or("c5_native_evidence_snapshot_array_missing")?
        .swap(0, 1);
    mutations.push(reordered);
    let mut reordered_calls = delivered.clone();
    reordered_calls
        .as_array_mut()
        .ok_or("c5_native_evidence_snapshot_array_missing")?
        .swap(1, 2);
    mutations.push(reordered_calls);
    let mut truncated = delivered.clone();
    let _ = truncated
        .as_array_mut()
        .ok_or("c5_native_evidence_snapshot_array_missing")?
        .pop();
    mutations.push(truncated);
    let mut duplicated = delivered.clone();
    duplicated
        .as_array_mut()
        .ok_or("c5_native_evidence_snapshot_array_missing")?
        .push(delivered[1].clone());
    mutations.push(duplicated);
    let mut message = delivered.clone();
    message[0]["content"] = json!("Synthetic mutated message");
    mutations.push(message);
    let mut role = delivered.clone();
    role[0]["role"] = json!("user");
    mutations.push(role);
    let mut call_id = delivered.clone();
    call_id[1]["id"] = json!("synthetic-other-call");
    mutations.push(call_id);
    let mut name = delivered.clone();
    name[1]["function"]["name"] = json!("synthetic_other_tool");
    mutations.push(name);
    let mut arguments = delivered.clone();
    let original_arguments = delivered[1]["function"]["arguments"]
        .as_str()
        .ok_or("c5_native_evidence_arguments_fixture_missing")?;
    arguments[1]["function"]["arguments"] = json!(format!(" {original_arguments}"));
    mutations.push(arguments);
    let mut nested_call = delivered.clone();
    nested_call[0]["tool_calls"][0]["function"]["name"] = json!("synthetic_other_tool");
    mutations.push(nested_call);
    let mut result_body = delivered.clone();
    result_body
        .as_array_mut()
        .ok_or("c5_native_evidence_snapshot_array_missing")?
        .push(json!({
            "role":"tool","tool_call_id":CALL_ID,"content":"Synthetic unsealed result body"
        }));
    mutations.push(result_body);
    for mutation in mutations {
        assert!(
            unavailable(next.verify_native_delivered(&binding, chat(), &mutation)),
            "c5_native_evidence_mutated_snapshot_replayed"
        );
        assert!(
            next.is_empty()
                && next.resolve_if_authorized(fixture.reference()?)?.is_none()
                && fixture.store.stats()? == before,
            "c5_native_evidence_mutated_snapshot_granted_authority_or_changed_store"
        );
    }
    let _unchanged_handle = inspect(&next, &binding, &delivered)?;
    Ok(())
}

// Authority-guard tests below intentionally do not depend on native finalization.
#[test]
fn c5_native_evidence_responses_cursor_api_rejects_native_scope_even_empty() -> TestResult {
    let fixture = Fixture::mapped()?;
    let (mut next, binding) = fixture.next()?;
    let before = fixture.store.stats()?;
    assert!(
        unavailable(next.resume_completed_context(&binding, None, &[], &[])),
        "c5_native_evidence_empty_responses_cursor_api_accepted_native_scope"
    );
    assert!(
        next.is_empty() && fixture.store.stats()? == before,
        "c5_native_evidence_cursor_guard_mutated"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_responses_diagnosed_cursor_rejects_native_scope_even_empty() -> TestResult {
    let fixture = Fixture::mapped()?;
    let (mut next, binding) = fixture.next()?;
    let before = fixture.store.stats()?;
    let mut reason = None;
    assert!(
        unavailable(next.resume_completed_context_diagnosed(&binding, None, &[], &[], &mut reason)),
        "c5_native_evidence_empty_diagnosed_cursor_api_accepted_native_scope"
    );
    assert!(
        reason.is_some() && next.is_empty() && fixture.store.stats()? == before,
        "c5_native_evidence_diagnosed_guard_mutated_or_lost_fixed_reason"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_responses_output_call_api_rejects_native_scope_even_empty() -> TestResult {
    let fixture = Fixture::mapped()?;
    let (next, binding) = fixture.next()?;
    let before = fixture.store.stats()?;
    assert!(
        unavailable(next.verify_output_call_ids(&binding, None, &[])),
        "c5_native_evidence_empty_responses_call_api_accepted_native_scope"
    );
    assert!(
        next.is_empty() && fixture.store.stats()? == before,
        "c5_native_evidence_call_guard_mutated"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_responses_token_api_rejects_native_scope_even_empty() -> TestResult {
    let fixture = Fixture::mapped()?;
    let (mut next, binding) = fixture.next()?;
    let before = fixture.store.stats()?;
    assert!(
        unavailable(next.resume_completed_tokens(&binding, &[])),
        "c5_native_evidence_empty_responses_token_api_accepted_native_scope"
    );
    assert!(
        next.is_empty() && fixture.store.stats()? == before,
        "c5_native_evidence_token_guard_mutated"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_responses_matching_token_api_rejects_native_scope_even_empty() -> TestResult {
    let fixture = Fixture::mapped()?;
    let (mut next, binding) = fixture.next()?;
    let before = fixture.store.stats()?;
    assert!(
        unavailable(next.resume_matching_completed_tokens(&binding, &[])),
        "c5_native_evidence_empty_matching_token_api_accepted_native_scope"
    );
    assert!(
        next.is_empty() && fixture.store.stats()? == before,
        "c5_native_evidence_matching_guard_mutated"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_responses_token_api_rejects_current_native_mapping() -> TestResult {
    let mut fixture = Fixture::mapped()?;
    let reference = fixture.reference()?.to_owned();
    let before = fixture.store.stats()?;
    assert!(
        unavailable(
            fixture
                .lease
                .resume_completed_tokens(&fixture.binding, &[&reference])
        ),
        "c5_native_evidence_current_mapping_bypassed_responses_token_guard"
    );
    assert!(
        fixture.lease.resolve_if_authorized(&reference)? == Some(SYNTHETIC_TEXT)
            && fixture.store.stats()? == before,
        "c5_native_evidence_responses_guard_revoked_native_mapping"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_responses_matching_api_rejects_current_native_mapping() -> TestResult {
    let mut fixture = Fixture::mapped()?;
    let reference = fixture.reference()?.to_owned();
    let before = fixture.store.stats()?;
    assert!(
        unavailable(
            fixture
                .lease
                .resume_matching_completed_tokens(&fixture.binding, &[&reference])
        ),
        "c5_native_evidence_current_mapping_bypassed_matching_token_guard"
    );
    assert!(
        fixture.lease.resolve_if_authorized(&reference)? == Some(SYNTHETIC_TEXT)
            && fixture.store.stats()? == before,
        "c5_native_evidence_matching_guard_revoked_native_mapping"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_controlled_import_never_grants_responses_authority() -> TestResult {
    let fixture = Fixture::mapped()?;
    let source = fixture.source()?;
    let (_finalized, delivered) = fixture.finalize(&source)?;
    let (mut next, binding) = fixture.next()?;
    let handle = inspect(&next, &binding, &delivered)?;
    next.import_native_completed_context(&binding, chat(), &handle)
        .map_err(|_| "c5_native_evidence_isolation_control_import_rejected")?;
    let before = fixture.store.stats()?;
    let call = CompletedCallSpec {
        call_id: CALL_ID,
        item_type: "function_call",
        name: "synthetic_tool_a",
        payload: delivered[1]["function"]["arguments"]
            .as_str()
            .ok_or("c5_native_evidence_isolation_arguments_missing")?,
        reasoning: None,
        encrypted_reasoning: None,
    };
    assert!(
        unavailable(next.resume_completed_context(&binding, None, &[], &[])),
        "c5_native_evidence_import_granted_empty_responses_cursor_authority"
    );
    assert!(
        unavailable(next.resume_completed_context(&binding, Some(RESPONSE_ID), &[call], &[])),
        "c5_native_evidence_import_granted_responses_cursor_or_call_authority"
    );
    assert!(
        unavailable(next.verify_output_call_ids(&binding, Some(RESPONSE_ID), &[CALL_ID])),
        "c5_native_evidence_import_attested_result_pairing"
    );
    assert!(
        unavailable(next.resume_completed_tokens(&binding, &[fixture.reference()?])),
        "c5_native_evidence_import_granted_responses_token_authority"
    );
    assert!(
        unavailable(next.resume_matching_completed_tokens(&binding, &[fixture.reference()?])),
        "c5_native_evidence_import_granted_responses_matching_authority"
    );
    assert!(
        next.resolve_if_authorized(fixture.reference()?)? == Some(SYNTHETIC_TEXT)
            && next.authorized_tokens()?.len() == 1
            && fixture.store.stats()? == before,
        "c5_native_evidence_isolation_guards_changed_controlled_native_authority"
    );
    Ok(())
}

#[test]
fn c5_native_evidence_fixture_does_not_expand_integrated_registration() {
    for (client, operation) in [
        (Protocol::Chat, Operation::ChatCompletions),
        (Protocol::Anthropic, Operation::Messages),
        (Protocol::Gemini, Operation::GenerateContent),
    ] {
        for upstream in [
            Protocol::Chat,
            Protocol::Responses,
            Protocol::Anthropic,
            Protocol::Gemini,
        ] {
            for framing in [Framing::Json, Framing::Sse] {
                let expected = if client == Protocol::Chat && upstream == Protocol::Chat {
                    Registration::NativeChat
                } else if client == Protocol::Anthropic && upstream == Protocol::Anthropic {
                    Registration::NativeAnthropic
                } else if client == Protocol::Gemini && upstream == Protocol::Gemini {
                    Registration::NativeGemini
                } else {
                    Registration::Disabled
                };
                assert!(
                    registration(
                        ProtocolPair::new(
                            ClientProtocol::new(client),
                            UpstreamProtocol::new(upstream)
                        ),
                        operation,
                        framing,
                    ) == expected,
                    "c5_native_evidence_fixture_expanded_native_registration"
                );
            }
        }
    }
}
