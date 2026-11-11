//! Chat CH1 request contracts against the frozen core; native routes stay Disabled.
//!
//! Protocol state table (CH1 implements no response or history authority):
//! | State/index | Supported carriers | Terminal/error/transport rule |
//! | Request/Parsed -> Observed | messages[i].content string or text parts;
//! | system/developer and latest user = CurrentInput; earlier user and assistant
//! | = HistoricalText; assistant tool_calls[j].function.arguments string
//! | = ToolArguments; every role-tool string/text part = untrusted ToolResult.
//! | Use native indices for the last-user boundary, never diagnostic paths.
//! | Classification never skips scanning or grants history/binding authority.
//! | Without any user, assistant and tool text still require mandatory scanning.
//! | Sites borrow exact native nodes and are editable.
//! | Request/Observed -> Prepared | Shared matcher, atomic native patch plan,
//! | residual inspection, encoding and validation all precede mapping commit.
//! | Request/Rejected | Capacity, Block, mandatory validation/encoding failure:
//! | no prepared body, no partial mapping, no mutation, no plaintext fallback.
//! | Response/choice[i]/tool[j] (CH2 deferred) | Isolated content/argument streams;
//! | existing reason classifier accepts stop/tool_calls/function_call, but legacy
//! | function_call carriers are unsupported and a reason alone is not proof.
//! | Response/Incomplete (CH2 deferred) | length/content_filter, unknown/missing
//! | reason, upstream error, malformed arguments or contradictory snapshots fail.
//! | Response/Transport (CH2 deferred) | SSE needs strict terminal facts, [DONE]
//! | and clean entity end; JSON needs strict validation and clean entity end.
//! | EOF alone, transport error and cancellation never authorize publication.
//! | Unsupported/Opaque | image_url, input_audio, file, unknown part types and
//! | unknown members remain untouched; no media decoding, Responses cursor,
//! | legacy function-call authority, HTTP admission, WebSocket or new framing.
//! AssistantItem and opaque ToolResult claims below are inspection metadata only.
//! Call/result pairing and delivered-history validation remain CH3 work.

use std::{cell::Cell, rc::Rc};

use serde_json::{json, Value};

use super::request::observe_request;
use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        native_test_support::{LIMIT, TEXT},
        payload_budget::PayloadBudget,
        payload_buffer::test_support as buffer_events,
        payload_json::test_support as json_events,
        request_plan::PlanError,
        request_preparation::prepare_observed_request,
        restoration_contract::{
            ClientProtocol, ContinuityScope, ContractError, EditConstraint, HistoryClaim,
            IngressNamespace, Operation, RequestObservation, SourceClass, TextPatch,
        },
        runtime::{admit_security_runtime, SecurityRuntimeConfig},
        tokenization::{publication_test_support, DlpRequestLease, DlpTokenStore, TokenError},
        types::AuditAction,
        unified_test_support::ResponsesFixture,
    },
};

// Preserve the original compile-only contract probe and Disabled registry control.
const CONTRACT_PROTOCOL: Protocol = Protocol::Chat;
const CONTRACT_OPERATION: Operation = Operation::ChatCompletions;

#[path = "../contract_probe.rs"]
mod contract_probe;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const RESTORE_RULES: &[&str] = &["chinese_phone_number"];

// Reuse the frozen synthetic runtime/store owner, but admit the native lease
// using the actually published policy revision instead of a hardcoded revision.
// This wrapper contains no observation, matcher, transaction or authority substitute.
struct RequestFixture {
    lease: DlpRequestLease,
    store: DlpTokenStore,
    budget: PayloadBudget,
    _runtime: ResponsesFixture,
}

impl RequestFixture {
    fn new() -> TestResult<Self> {
        let runtime = ResponsesFixture::new();
        let admission = admit_security_runtime()?;
        let store = runtime.store.clone();
        let lease = store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: ClientProtocol::new(Protocol::Chat),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-chat-ch1-app",
            Some("synthetic-chat-ch1-session"),
            admission.restoration_policy_revision,
        )?;
        let binding = lease.bind_actual_attempt(
            "synthetic-chat-ch1-provider",
            "openai_chat",
            "https://synthetic.invalid/v1/chat/completions",
            Some("synthetic-chat-ch1-principal"),
        )?;
        assert!(
            lease.history_binding_live(&binding),
            "chat_ch1_fixture_current_binding_invalid"
        );
        Ok(Self {
            lease,
            store,
            budget: PayloadBudget::with_limit(LIMIT),
            _runtime: runtime,
        })
    }

    fn counts(&self) -> TestResult<(usize, usize, usize)> {
        let (published, pending, pins, ..) = publication_test_support::counts(&self.store)?;
        Ok((published, pending, pins))
    }
}

// These are expected fixture locations, not an alternative observation traversal.
const TEXT_PATHS: [&str; 12] = [
    "/messages/0/content",
    "/messages/1/content/0/text",
    "/messages/2/content",
    "/messages/3/content/0/text",
    "/messages/4/content",
    "/messages/5/content/0/text",
    "/messages/6/content",
    "/messages/7/content/0/text",
    "/messages/8/tool_calls/0/function/arguments",
    "/messages/8/tool_calls/1/function/arguments",
    "/messages/9/content",
    "/messages/10/content/0/text",
];

fn native_request() -> Value {
    let mut messages = Vec::new();
    for role in ["system", "developer", "user", "assistant"] {
        messages.push(json!({"role":role,"content":TEXT,"extension":{"flag":true}}));
        messages.push(json!({"role":role,"content":[
            {"type":"text","text":TEXT,"extension":{"n":1.0}},
            {"type":"image_url","image_url":{"url":TEXT,"detail":"auto"}},
            {"type":"input_audio","input_audio":{"data":TEXT,"format":"wav"}},
            {"type":"file","file":{"file_data":TEXT,"filename":"synthetic.txt"}},
            {"type":"synthetic_future","text":TEXT,"types":[null,true,7,1.0]}
        ]}));
    }
    let arguments = json!({"contact":TEXT,"count":7}).to_string();
    messages.push(json!({"role":"assistant","content":null,"tool_calls":[
        {"id":"synthetic-call-a","type":"function","function":{
            "name":"synthetic_nonexecuting_tool","arguments":arguments,
            "extension":{"same_value":TEXT}}},
        {"id":"synthetic-call-b","type":"function","function":{
            "name":"synthetic_nonexecuting_tool","arguments":arguments}}
    ]}));
    messages.push(json!({"role":"tool","tool_call_id":"synthetic-call-a","content":TEXT}));
    messages.push(
        json!({"role":"tool","tool_call_id":"synthetic-call-b","content":[
            {"type":"text","text":TEXT},
            {"type":"image_url","image_url":{"url":TEXT}}
        ]}),
    );
    json!({"model":"synthetic-chat-model","messages":messages,"stream":false,
        "temperature":0.25,"seed":7,"tools":[{"type":"function","function":{
            "name":"synthetic_nonexecuting_tool","description":TEXT,
            "parameters":{"type":"object","properties":{"contact":{"type":"string"}}}}}],
        "extension":{"messages[0].content":TEXT,"same_value":TEXT,
            "types":[null,true,7,1.0,{"text":TEXT}]}})
}

fn observed<'native>(
    native: &'native Value,
    budget: &PayloadBudget,
) -> TestResult<RequestObservation<'native>> {
    observe_request(native, budget.clone()).map_err(|_| "chat_ch1_observation_missing".into())
}

#[test]
#[serial_test::serial]
fn ch1_observes_role_strings_and_content_arrays_at_exact_native_nodes() -> TestResult {
    for role in ["system", "developer", "user", "assistant"] {
        for array in [false, true] {
            let content = if array {
                json!([{"type":"text","text":TEXT},{"type":"image_url","image_url":{"url":TEXT}}])
            } else {
                json!(TEXT)
            };
            let body =
                json!({"messages":[{"role":role,"content":content}],"extension":{"text":TEXT}});
            let budget = PayloadBudget::with_limit(LIMIT);
            let observation = observed(&body, &budget)?;
            let node = if array {
                &body["messages"][0]["content"][0]["text"]
            } else {
                &body["messages"][0]["content"]
            };
            let source = if role == "assistant" {
                SourceClass::HistoricalText
            } else {
                SourceClass::CurrentInput
            };
            assert!(
                std::ptr::eq(observation.native(), &body)
                    && observation.client().protocol() == Protocol::Chat
                    && observation.sites().len() == 1,
                "chat_ch1_native_owner_or_carrier_set_changed"
            );
            let site = observation.sites().site(0).ok_or("chat_ch1_site_missing")?;
            assert!(
                std::ptr::eq(site.native(), node)
                    && site.text() == TEXT
                    && site.source() == source
                    && site.constraint() == EditConstraint::Editable,
                "chat_ch1_role_site_classification_invalid"
            );
            if role == "assistant" {
                assert!(
                    observation.history().iter().any(|claim| matches!(claim,
                        HistoryClaim::AssistantItem { native }
                            if std::ptr::eq(*native, &body["messages"][0]))),
                    "chat_ch1_assistant_claim_missing"
                );
            }
            assert!(
                budget.used() > 0 && observation.budget().used() == budget.used(),
                "chat_ch1_observation_metadata_not_charged"
            );
            drop(observation);
            assert!(budget.used() == 0, "chat_ch1_observation_charge_retained");
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_observes_tool_argument_strings_and_untrusted_result_text_without_authority() -> TestResult {
    let fixture = RequestFixture::new()?;
    let body = native_request();
    let before = fixture.store.stats()?;
    let baseline = fixture.budget.used();
    let observation = observed(&body, &fixture.budget)?;
    assert!(
        observation.sites().len() == TEXT_PATHS.len(),
        "chat_ch1_carrier_count_invalid"
    );
    for (index, path) in TEXT_PATHS.iter().enumerate() {
        let node = body.pointer(path).ok_or("chat_ch1_fixture_node_missing")?;
        let source = match index {
            4 | 6 | 7 => SourceClass::HistoricalText,
            8 | 9 => SourceClass::ToolArguments,
            10 | 11 => SourceClass::ToolResult,
            _ => SourceClass::CurrentInput,
        };
        assert!(
            (0..observation.sites().len()).any(|site_index| {
                observation.sites().site(site_index).is_some_and(|site| {
                    std::ptr::eq(site.native(), node)
                        && site.source() == source
                        && site.constraint() == EditConstraint::Editable
                })
            }),
            "chat_ch1_expected_native_site_missing"
        );
    }
    for index in [9, 10] {
        assert!(
            observation.history().iter().any(|claim| matches!(claim,
                HistoryClaim::OpaqueItem { native, source: SourceClass::ToolResult }
                    if std::ptr::eq(*native, &body["messages"][index]))),
            "chat_ch1_result_not_classified_as_untrusted_input"
        );
    }
    assert!(
        observation
            .history()
            .iter()
            .all(|claim| !matches!(claim, HistoryClaim::ResponsesCursor { .. }))
            && fixture.lease.is_empty()
            && fixture.store.stats()? == before
            && fixture.counts()? == (0, 0, 0),
        "chat_ch1_observation_granted_authority"
    );
    drop(observation);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch1_tool_observation_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_real_preparation_redacts_all_carriers_preserving_opaque_media_unknown_fields_and_types(
) -> TestResult {
    let mut fixture = RequestFixture::new()?;
    let body = native_request();
    let original = body.clone();
    let before = fixture.store.stats()?;
    let baseline = fixture.budget.used();
    let observation = observed(&body, &fixture.budget)?;
    let observation_charge = fixture.budget.used();
    let validated = Cell::new(false);
    let prepared = prepare_observed_request(
        &mut fixture.lease,
        &observation,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, encoded, proof| {
            validated.set(true);
            assert!(
                fixture.store.stats()? == before && proof.len() == 1,
                "chat_ch1_mapping_committed_before_validation"
            );
            let decoded: Value =
                serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
            assert!(&decoded == candidate, "chat_ch1_native_encoding_mismatch");
            // Revert only approved fixture leaves, then compare the entire native tree.
            // Equal text in extensions, tool schemas, media and diagnostic-like keys stays opaque.
            let mut restored_shape = candidate.clone();
            for (index, path) in TEXT_PATHS.iter().enumerate() {
                let node = candidate.pointer(path).ok_or(TokenError::RedactionFailed)?;
                if matches!(index, 8 | 9) {
                    let arguments: Value =
                        serde_json::from_str(node.as_str().ok_or(TokenError::RedactionFailed)?)
                            .map_err(|_| TokenError::RedactionFailed)?;
                    assert!(
                        arguments["contact"].as_str() == Some(proof[0]) && arguments["count"] == 7,
                        "chat_ch1_argument_json_string_not_patched"
                    );
                } else {
                    assert!(
                        node.as_str() == Some(proof[0]),
                        "chat_ch1_text_site_not_patched"
                    );
                }
                *restored_shape
                    .pointer_mut(path)
                    .ok_or(TokenError::RedactionFailed)? = original
                    .pointer(path)
                    .ok_or(TokenError::RedactionFailed)?
                    .clone();
            }
            assert!(
                restored_shape == original,
                "chat_ch1_opaque_or_unknown_native_data_changed"
            );
            assert!(
                candidate["extension"]["types"][3].is_f64()
                    && candidate["extension"]["types"][2].is_i64()
                    && candidate["extension"]["types"][0].is_null(),
                "chat_ch1_native_json_types_changed"
            );
            Ok(proof.len())
        },
    )
    .map_err(|_| "chat_ch1_real_preparation_rejected")?;
    assert!(
        validated.get()
            && prepared.validated == 1
            && prepared.reversible_occurrences == TEXT_PATHS.len()
            && prepared.irreversible_occurrences == 0,
        "chat_ch1_request_occurrences_invalid"
    );
    let reference = prepared.body.as_value()["messages"][0]["content"]
        .as_str()
        .ok_or("chat_ch1_reference_missing")?;
    assert!(
        fixture.lease.resolve(reference)? == TEXT
            && fixture.store.stats()?.entries == before.entries + 1,
        "chat_ch1_request_mapping_not_committed"
    );
    assert!(
        body == original && fixture.counts()? == (0, 0, 0),
        "chat_ch1_input_or_publication_changed"
    );
    assert!(
        fixture.budget.used() > observation_charge,
        "chat_ch1_prepared_owners_not_charged"
    );
    drop(prepared);
    assert!(
        fixture.budget.used() == observation_charge,
        "chat_ch1_prepared_charge_retained"
    );
    drop(observation);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch1_request_metadata_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_first_tool_results_need_no_delivered_result_snapshot_but_receive_current_dlp() -> TestResult
{
    for array in [false, true] {
        let mut fixture = RequestFixture::new()?;
        let baseline = fixture.budget.used();
        let content = if array {
            json!([{"type":"text","text":TEXT}])
        } else {
            json!(TEXT)
        };
        let body = json!({"messages":[{"role":"tool","tool_call_id":"synthetic-unproven-call","content":content}]});
        let original = body.clone();
        let observation = observed(&body, &fixture.budget)?;
        let prepared = prepare_observed_request(
            &mut fixture.lease,
            &observation,
            &SecurityRuntimeConfig::default(),
            RESTORE_RULES,
            &[],
            |_, _, proof| {
                assert!(proof.len() == 1, "chat_ch1_result_current_proof_missing");
                Ok(())
            },
        )
        .map_err(|_| "chat_ch1_first_tool_result_rejected")?;
        let path = if array {
            "/messages/0/content/0/text"
        } else {
            "/messages/0/content"
        };
        let reference = prepared
            .body
            .as_value()
            .pointer(path)
            .and_then(Value::as_str)
            .ok_or("chat_ch1_result_reference_missing")?;
        assert!(
            fixture.lease.resolve(reference)? == TEXT
                && prepared.reversible_occurrences == 1
                && fixture.counts()? == (0, 0, 0)
                && body == original,
            "chat_ch1_first_result_not_current_input"
        );
        drop(prepared);
        drop(observation);
        assert!(
            fixture.budget.used() == baseline,
            "chat_ch1_result_charge_retained"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_zero_edit_preparation_keeps_native_tree_without_scanning_opaque_carriers() -> TestResult {
    let mut fixture = RequestFixture::new()?;
    let baseline = fixture.budget.used();
    let before = fixture.store.stats()?;
    let body = json!({"messages":[{"role":"user","content":[
        {"type":"text","text":"Synthetic public text"},
        {"type":"image_url","image_url":{"url":TEXT}},
        {"type":"synthetic_future","text":TEXT}]}],
        "extension":{"text":TEXT,"types":[null,7,1.0]}});
    let observation = observed(&body, &fixture.budget)?;
    let prepared = prepare_observed_request(
        &mut fixture.lease,
        &observation,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, encoded, proof| {
            let decoded: Value =
                serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
            assert!(
                candidate == &body && &decoded == candidate && proof.is_empty(),
                "chat_ch1_zero_edit_tree_changed"
            );
            Ok(())
        },
    )
    .map_err(|_| "chat_ch1_zero_edit_preparation_rejected")?;
    assert!(
        prepared.reversible_occurrences == 0
            && prepared.irreversible_occurrences == 0
            && fixture.lease.is_empty()
            && fixture.store.stats()? == before,
        "chat_ch1_opaque_text_received_mapping"
    );
    drop(prepared);
    drop(observation);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch1_zero_edit_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_observation_metadata_capacity_fails_closed_without_partial_owner() -> TestResult {
    let body = native_request();
    let original = body.clone();
    let budget = PayloadBudget::with_limit(1);
    let result = observe_request(&body, budget.clone());
    assert!(
        matches!(
            result,
            Err(ContractError::Resource(TokenError::CapacityExceeded))
        ),
        "chat_ch1_metadata_budget_not_enforced"
    );
    drop(result);
    assert!(
        body == original && budget.used() == 0,
        "chat_ch1_failed_observation_leaked_or_mutated"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_observation_respects_frozen_site_count_limit_and_refunds_partial_metadata() -> TestResult {
    // The frozen TextSites cap is 4096, independent of the payload byte ceiling.
    let body = json!({"messages":[{"role":"user","content":vec![json!({"type":"text","text":"Synthetic public text"});4097]}]});
    let budget = PayloadBudget::with_limit(LIMIT);
    let result = observe_request(&body, budget.clone());
    assert!(
        matches!(
            result,
            Err(ContractError::Resource(TokenError::CapacityExceeded))
        ),
        "chat_ch1_site_count_limit_not_enforced"
    );
    drop(result);
    assert!(
        budget.used() == 0,
        "chat_ch1_site_limit_partial_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_atomic_patch_plan_rejects_invalid_utf8_range_without_partial_native_edits() -> TestResult {
    let body = json!({"messages":[{"role":"user","content":"Synthetic public text"},
        {"role":"assistant","content":"🔒"}]});
    let original = body.clone();
    let budget = PayloadBudget::with_limit(LIMIT);
    let observation = observed(&body, &budget)?;
    let baseline = budget.used();
    let first = observation
        .sites()
        .site(0)
        .ok_or("chat_ch1_first_site_missing")?;
    let second = observation
        .sites()
        .site(1)
        .ok_or("chat_ch1_second_site_missing")?;
    let result = observation.sites().plan(&[
        TextPatch::new(first, 0..first.text().len(), "Synthetic replacement"),
        TextPatch::new(second, 1..2, "Synthetic replacement"),
    ]);
    assert!(
        matches!(result, Err(ContractError::InvalidRange)),
        "chat_ch1_invalid_range_plan_escaped"
    );
    drop(result);
    assert!(
        body == original && budget.used() == baseline,
        "chat_ch1_invalid_plan_not_atomic"
    );
    drop(observation);
    assert!(budget.used() == 0, "chat_ch1_invalid_plan_charge_retained");
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_preparation_capacity_failure_keeps_input_mappings_and_budget_atomic() -> TestResult {
    let mut fixture = RequestFixture::new()?;
    let body = native_request();
    let original = body.clone();
    let baseline = fixture.budget.used();
    let before = fixture.store.stats()?;
    let observation = observed(&body, &fixture.budget)?;
    let observation_charge = fixture.budget.used();
    let reserved = fixture
        .budget
        .reserve(fixture.budget.limit() - fixture.budget.used())?;
    let validated = Cell::new(false);
    let result = prepare_observed_request(
        &mut fixture.lease,
        &observation,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| {
            validated.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::CapacityExceeded))) && !validated.get(),
        "chat_ch1_capacity_failure_not_preserved"
    );
    drop(result);
    drop(reserved);
    assert!(
        fixture.lease.is_empty()
            && fixture.store.stats()? == before
            && body == original
            && fixture.counts()? == (0, 0, 0)
            && fixture.budget.used() == observation_charge,
        "chat_ch1_capacity_failure_not_atomic"
    );
    drop(observation);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch1_capacity_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_mandatory_validation_failure_rolls_back_all_native_patches_and_provisional_mappings(
) -> TestResult {
    let mut fixture = RequestFixture::new()?;
    let body = native_request();
    let original = body.clone();
    let baseline = fixture.budget.used();
    let before = fixture.store.stats()?;
    let observation = observed(&body, &fixture.budget)?;
    let observation_charge = fixture.budget.used();
    let validated = Cell::new(false);
    let result = prepare_observed_request(
        &mut fixture.lease,
        &observation,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, _, proof| {
            validated.set(true);
            assert!(
                fixture.store.stats()? == before && proof.len() == 1,
                "chat_ch1_failed_validation_committed_early"
            );
            assert!(
                candidate["messages"][0]["content"].as_str() == Some(proof[0])
                    && candidate["messages"][10]["content"][0]["text"].as_str() == Some(proof[0]),
                "chat_ch1_validation_did_not_receive_atomic_candidate"
            );
            Err::<(), _>(TokenError::RedactionFailed)
        },
    );
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::RedactionFailed))) && validated.get(),
        "chat_ch1_validation_failure_not_preserved"
    );
    drop(result);
    assert!(
        fixture.lease.is_empty()
            && fixture.store.stats()? == before
            && body == original
            && fixture.counts()? == (0, 0, 0)
            && fixture.budget.used() == observation_charge,
        "chat_ch1_validation_failure_not_atomic"
    );
    drop(observation);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch1_validation_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_mandatory_encoding_allocation_failure_has_no_unredacted_fallback() -> TestResult {
    let mut fixture = RequestFixture::new()?;
    let body = native_request();
    let original = body.clone();
    let baseline = fixture.budget.used();
    let before = fixture.store.stats()?;
    let observation = observed(&body, &fixture.budget)?;
    let observation_charge = fixture.budget.used();
    let ready = Rc::new(Cell::new(false));
    let seen = Rc::clone(&ready);
    let json_hook = json_events::install(move |event| {
        if event.phase == json_events::Phase::AfterBuild {
            seen.set(true);
        }
        json_events::Action::Continue
    });
    let denied = Rc::new(Cell::new(false));
    let seen = Rc::clone(&denied);
    let buffer_hook = buffer_events::install(move |event| {
        if ready.get() && event.phase == buffer_events::Phase::BeforeAllocation {
            seen.set(true);
            buffer_events::Action::FailAllocation
        } else {
            buffer_events::Action::Continue
        }
    });
    let validated = Cell::new(false);
    let result = prepare_observed_request(
        &mut fixture.lease,
        &observation,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| {
            validated.set(true);
            Ok(())
        },
    );
    drop(buffer_hook);
    drop(json_hook);
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::Unavailable)))
            && denied.get()
            && !validated.get(),
        "chat_ch1_encoding_failure_not_reached_or_preserved"
    );
    drop(result);
    assert!(
        fixture.lease.is_empty()
            && fixture.store.stats()? == before
            && body == original
            && fixture.counts()? == (0, 0, 0)
            && fixture.budget.used() == observation_charge,
        "chat_ch1_encoding_failure_released_fallback_or_partial_mapping"
    );
    drop(observation);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch1_encoding_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_explicit_block_remains_policy_rejection_without_preparing_or_committing() -> TestResult {
    let mut fixture = RequestFixture::new()?;
    let body = native_request();
    let original = body.clone();
    let before = fixture.store.stats()?;
    let baseline = fixture.budget.used();
    let observation = observed(&body, &fixture.budget)?;
    let observation_charge = fixture.budget.used();
    let mut runtime = SecurityRuntimeConfig::default();
    runtime
        .builtin_action_overrides
        .insert("chinese_phone_number".into(), AuditAction::Block);
    let validated = Cell::new(false);
    let result = prepare_observed_request(
        &mut fixture.lease,
        &observation,
        &runtime,
        RESTORE_RULES,
        &[],
        |_, _, _| {
            validated.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(result, Err(PlanError::Blocked)) && !validated.get(),
        "chat_ch1_explicit_block_not_preserved"
    );
    drop(result);
    assert!(
        fixture.lease.is_empty()
            && fixture.store.stats()? == before
            && body == original
            && fixture.counts()? == (0, 0, 0)
            && fixture.budget.used() == observation_charge,
        "chat_ch1_block_released_candidate_or_mapping"
    );
    drop(observation);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch1_block_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_role_and_native_position_classification_never_skips_mandatory_scanning() -> TestResult {
    for with_user in [false, true] {
        let mut fixture = RequestFixture::new()?;
        let baseline = fixture.budget.used();
        let mut messages = vec![
            json!({"role":"system","content":TEXT}),
            json!({"role":"developer","content":TEXT}),
        ];
        if with_user {
            messages.push(json!({"role":"user","content":TEXT}));
        }
        messages.push(json!({"role":"assistant","content":TEXT}));
        messages
            .push(json!({"role":"tool","tool_call_id":"synthetic-unproven-call","content":TEXT}));
        if with_user {
            messages.push(json!({"role":"user","content":[{"type":"text","text":TEXT}]}));
            messages.push(json!({"role":"assistant","content":TEXT}));
            messages.push(
                json!({"role":"tool","tool_call_id":"synthetic-unproven-call","content":TEXT}),
            );
        }
        let body = json!({"messages":messages});
        // Explicit expected classes for two fixtures, not a test-local classifier.
        let classes: &[SourceClass] = if with_user {
            &[
                SourceClass::CurrentInput,
                SourceClass::CurrentInput,
                SourceClass::HistoricalText,
                SourceClass::HistoricalText,
                SourceClass::ToolResult,
                SourceClass::CurrentInput,
                SourceClass::HistoricalText,
                SourceClass::ToolResult,
            ]
        } else {
            &[
                SourceClass::CurrentInput,
                SourceClass::CurrentInput,
                SourceClass::HistoricalText,
                SourceClass::ToolResult,
            ]
        };
        let observation = observed(&body, &fixture.budget)?;
        assert!(
            observation.sites().len() == classes.len(),
            "chat_ch1_position_site_count_invalid"
        );
        for (index, source) in classes.iter().enumerate() {
            let content = &body["messages"][index]["content"];
            let node = if content.is_array() {
                &content[0]["text"]
            } else {
                content
            };
            assert!(
                (0..observation.sites().len()).any(|site_index| {
                    observation.sites().site(site_index).is_some_and(|site| {
                        std::ptr::eq(site.native(), node) && site.source() == *source
                    })
                }),
                "chat_ch1_role_or_position_source_invalid"
            );
        }
        let prepared = prepare_observed_request(
            &mut fixture.lease,
            &observation,
            &SecurityRuntimeConfig::default(),
            RESTORE_RULES,
            &[],
            |_, _, proof| {
                assert!(proof.len() == 1, "chat_ch1_position_current_proof_missing");
                Ok(())
            },
        )
        .map_err(|_| "chat_ch1_classified_text_not_prepared")?;
        assert!(
            prepared.reversible_occurrences == classes.len() && fixture.counts()? == (0, 0, 0),
            "chat_ch1_classification_skipped_scanning_or_granted_history"
        );
        drop(prepared);
        drop(observation);
        assert!(
            fixture.budget.used() == baseline,
            "chat_ch1_position_charge_retained"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_boundary_malformed_and_unknown_roles_are_opaque_not_schema_validation() -> TestResult {
    let fixture = RequestFixture::new()?;
    let before = fixture.store.stats()?;
    let baseline = fixture.budget.used();
    let messages = json!([
        null, true, 7, TEXT, [],
        {"content":TEXT},
        {"role":null,"content":TEXT},
        {"role":true,"content":TEXT},
        {"role":7,"content":TEXT},
        {"role":{},"content":TEXT},
        {"role":["user"],"content":TEXT},
        {"role":"USER","content":TEXT},
        {"role":"synthetic_future","content":[{"type":"text","text":TEXT}],
            "tool_calls":[{"type":"function","function":{"arguments":TEXT}}]}
    ]);
    for body in [
        json!({}),
        json!({"messages":null}),
        json!({"messages":TEXT}),
        json!({"messages":{"role":"user","content":TEXT}}),
        json!({"messages":messages}),
    ] {
        let original = body.clone();
        let observation = observed(&body, &fixture.budget)?;
        assert!(
            std::ptr::eq(observation.native(), &body)
                && observation.sites().len() == 0
                && observation.history().is_empty(),
            "chat_ch1_boundary_malformed_role_not_opaque"
        );
        drop(observation);
        assert!(
            body == original
                && fixture.lease.is_empty()
                && fixture.store.stats()? == before
                && fixture.counts()? == (0, 0, 0)
                && fixture.budget.used() == baseline,
            "chat_ch1_boundary_malformed_role_changed_state"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_boundary_wrong_shaped_text_and_unsupported_tools_are_opaque() -> TestResult {
    let fixture = RequestFixture::new()?;
    let before = fixture.store.stats()?;
    let baseline = fixture.budget.used();
    let body = json!({"messages":[
        {"role":"system","content":false},
        {"role":"developer","content":7},
        {"role":"user","content":{"type":"text","text":TEXT}},
        {"role":"assistant","content":null,
            "tool_calls":{"type":"function","function":{"arguments":TEXT}},
            "function_call":{"name":"synthetic_nonexecuting_tool","arguments":TEXT}},
        {"role":"tool","content":{"type":"text","text":TEXT}},
        {"role":"user","content":[
            null, true, 7, TEXT, [], {"text":TEXT}, {"type":null,"text":TEXT},
            {"type":"text"}, {"type":"text","text":null},
            {"type":"text","text":false}, {"type":"text","text":7},
            {"type":"text","text":{"text":TEXT}}, {"type":"text","text":[TEXT]},
            {"type":"synthetic_future","text":TEXT}, {"type":"text","text":TEXT}],
            "tool_calls":[{"type":"function","function":{"arguments":TEXT}}]},
        {"role":"assistant","tool_calls":[
            null, true, 7, TEXT, [], {}, {"function":{"arguments":TEXT}},
            {"type":7,"function":{"arguments":TEXT}},
            {"type":"synthetic_future","function":{"arguments":TEXT}},
            {"type":"function"}, {"type":"function","function":TEXT},
            {"type":"function","function":null},
            {"type":"function","function":{"arguments":null}},
            {"type":"function","function":{"arguments":false}},
            {"type":"function","function":{"arguments":7}},
            {"type":"function","function":{"arguments":{"text":TEXT}}},
            {"type":"function","function":{"arguments":[TEXT]}},
            {"type":"function","function":{"arguments":TEXT}}]}
    ]});
    let original = body.clone();
    let observation = observed(&body, &fixture.budget)?;
    // Only the final well-shaped carrier in each mixed array is observable.
    // Successful observation does not certify this request's native schema.
    assert!(
        observation.sites().len() == 2 && observation.history().len() == 3,
        "chat_ch1_boundary_wrong_shape_carrier_set_changed"
    );
    for (index, path, source) in [
        (0, "/messages/5/content/14/text", SourceClass::CurrentInput),
        (
            1,
            "/messages/6/tool_calls/17/function/arguments",
            SourceClass::ToolArguments,
        ),
    ] {
        let node = body
            .pointer(path)
            .ok_or("chat_ch1_boundary_control_missing")?;
        let site = observation
            .sites()
            .site(index)
            .ok_or("chat_ch1_boundary_site_missing")?;
        assert!(
            std::ptr::eq(site.native(), node)
                && site.text() == TEXT
                && site.source() == source
                && site.constraint() == EditConstraint::Editable,
            "chat_ch1_boundary_wrong_shape_observed_or_control_skipped"
        );
    }
    assert!(
        observation.history().iter().all(|claim| match claim {
            HistoryClaim::AssistantItem { native } =>
                std::ptr::eq(*native, &body["messages"][3])
                    || std::ptr::eq(*native, &body["messages"][6]),
            HistoryClaim::OpaqueItem {
                native,
                source: SourceClass::ToolResult,
            } => std::ptr::eq(*native, &body["messages"][4]),
            _ => false,
        }) && fixture.budget.used() > baseline,
        "chat_ch1_boundary_wrong_shape_claim_or_charge_invalid"
    );
    drop(observation);
    assert!(
        body == original
            && fixture.lease.is_empty()
            && fixture.store.stats()? == before
            && fixture.counts()? == (0, 0, 0)
            && fixture.budget.used() == baseline,
        "chat_ch1_boundary_wrong_shape_changed_state"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_boundary_exact_metadata_bytes_accept_and_one_short_refunds() -> TestResult {
    let fixture = RequestFixture::new()?;
    let before = fixture.store.stats()?;
    let baseline = fixture.budget.used();
    // Each metadata vector allocates once, so retained bytes also cover its peak.
    let body = json!({"messages":[{"role":"user","content":TEXT},
        {"role":"assistant","content":TEXT}]});
    let original = body.clone();
    let measured = observed(&body, &fixture.budget)?;
    let bytes = fixture.budget.used() - baseline;
    assert!(
        bytes > 0 && measured.sites().len() == 2 && measured.history().len() == 1,
        "chat_ch1_boundary_metadata_measurement_invalid"
    );
    drop(measured);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch1_boundary_measured_charge_retained"
    );

    let exact = PayloadBudget::with_limit(bytes);
    let observation = observe_request(&body, exact.clone())?;
    assert!(
        observation.sites().len() == 2 && observation.history().len() == 1 && exact.used() == bytes,
        "chat_ch1_boundary_exact_metadata_budget_rejected"
    );
    drop(observation);
    assert!(
        exact.used() == 0,
        "chat_ch1_boundary_exact_metadata_charge_retained"
    );

    let short = PayloadBudget::with_limit(bytes - 1);
    let result = observe_request(&body, short.clone());
    assert!(
        matches!(
            result,
            Err(ContractError::Resource(TokenError::CapacityExceeded))
        ),
        "chat_ch1_boundary_one_short_metadata_not_capacity"
    );
    drop(result);
    assert!(
        short.used() == 0
            && fixture.budget.used() == baseline
            && body == original
            && fixture.lease.is_empty()
            && fixture.store.stats()? == before
            && fixture.counts()? == (0, 0, 0),
        "chat_ch1_boundary_one_short_metadata_not_atomic"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_boundary_4096_sites_accept_then_4097_fails_and_refunds() -> TestResult {
    let fixture = RequestFixture::new()?;
    let before = fixture.store.stats()?;
    let baseline = fixture.budget.used();
    let part = json!({"type":"text","text":"Synthetic public text"});
    let mut body = json!({"messages":[{"role":"user","content":vec![part.clone();4096]}]});
    let original = body.clone();
    let observation = observed(&body, &fixture.budget)?;
    assert!(
        observation.sites().len() == 4096
            && observation.history().is_empty()
            && fixture.budget.used() > baseline,
        "chat_ch1_boundary_4096_sites_not_accepted"
    );
    for index in [0, 4095] {
        let site = observation
            .sites()
            .site(index)
            .ok_or("chat_ch1_boundary_limit_site_missing")?;
        assert!(
            std::ptr::eq(
                site.native(),
                &body["messages"][0]["content"][index]["text"]
            ) && site.source() == SourceClass::CurrentInput
                && site.constraint() == EditConstraint::Editable,
            "chat_ch1_boundary_limit_native_site_invalid"
        );
    }
    drop(observation);
    assert!(
        body == original && fixture.budget.used() == baseline,
        "chat_ch1_boundary_4096_sites_mutated_or_retained"
    );
    body["messages"][0]["content"]
        .as_array_mut()
        .ok_or("chat_ch1_boundary_content_array_missing")?
        .push(part);
    let original = body.clone();
    let result = observe_request(&body, fixture.budget.clone());
    assert!(
        matches!(
            result,
            Err(ContractError::Resource(TokenError::CapacityExceeded))
        ),
        "chat_ch1_boundary_4097_sites_not_capacity"
    );
    drop(result);
    assert!(
        body == original
            && fixture.budget.used() == baseline
            && fixture.lease.is_empty()
            && fixture.store.stats()? == before
            && fixture.counts()? == (0, 0, 0),
        "chat_ch1_boundary_4097_sites_not_atomic"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ch1_boundary_shared_prepare_depth_fails_closed_despite_successful_observation() -> TestResult {
    for depth in [8, 65] {
        let mut fixture = RequestFixture::new()?;
        let before = fixture.store.stats()?;
        let baseline = fixture.budget.used();
        let mut opaque = json!({"text":TEXT,"types":[null,7,1.0]});
        for _ in 0..depth {
            opaque = json!({"opaque":opaque});
        }
        // Insert the opaque sibling first: shared membership search encounters it
        // before the genuine native text site, even though observation ignores it.
        let body = json!({"a_opaque":opaque,"messages":[{"role":"user","content":TEXT}],
            "extension":{"same_value":TEXT,"flag":true}});
        let original = body.clone();
        let observation = observed(&body, &fixture.budget)?;
        let observation_charge = fixture.budget.used();
        let site = observation
            .sites()
            .site(0)
            .ok_or("chat_ch1_boundary_depth_site_missing")?;
        assert!(
            observation.sites().len() == 1
                && observation.history().is_empty()
                && std::ptr::eq(site.native(), &body["messages"][0]["content"])
                && site.text() == TEXT
                && site.source() == SourceClass::CurrentInput
                && site.constraint() == EditConstraint::Editable
                && observation_charge > baseline,
            "chat_ch1_boundary_depth_observation_not_native"
        );
        let validated = Cell::new(false);
        let result = prepare_observed_request(
            &mut fixture.lease,
            &observation,
            &SecurityRuntimeConfig::default(),
            RESTORE_RULES,
            &[],
            |candidate, encoded, proof| {
                validated.set(true);
                assert!(
                    fixture.store.stats()? == before && proof.len() == 1,
                    "chat_ch1_boundary_depth_committed_before_validation"
                );
                let decoded: Value =
                    serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
                assert!(
                    &decoded == candidate
                        && candidate["messages"][0]["content"].as_str() == Some(proof[0]),
                    "chat_ch1_boundary_depth_control_encoding_or_patch_invalid"
                );
                let mut restored_shape = candidate.clone();
                restored_shape["messages"][0]["content"] =
                    original["messages"][0]["content"].clone();
                assert!(
                    restored_shape == original,
                    "chat_ch1_boundary_depth_control_opaque_fields_changed"
                );
                Ok(())
            },
        );
        if depth > 64 {
            assert!(
                matches!(result, Err(PlanError::Token(TokenError::CapacityExceeded)))
                    && !validated.get(),
                "chat_ch1_boundary_shared_depth_not_capacity"
            );
            drop(result);
            assert!(
                fixture.lease.is_empty()
                    && fixture.store.stats()? == before
                    && fixture.budget.used() == observation_charge,
                "chat_ch1_boundary_shared_depth_partial_mapping_or_charge"
            );
        } else {
            let prepared = result.map_err(|_| "chat_ch1_boundary_depth_control_rejected")?;
            let reference = prepared.body.as_value()["messages"][0]["content"]
                .as_str()
                .ok_or("chat_ch1_boundary_depth_control_reference_missing")?;
            assert!(
                validated.get()
                    && prepared.reversible_occurrences == 1
                    && prepared.irreversible_occurrences == 0
                    && fixture.lease.resolve(reference)? == TEXT
                    && fixture.store.stats()?.entries == before.entries + 1
                    && fixture.budget.used() > observation_charge,
                "chat_ch1_boundary_depth_control_mapping_missing"
            );
            drop(prepared);
            assert!(
                fixture.budget.used() == observation_charge,
                "chat_ch1_boundary_depth_control_prepared_charge_retained"
            );
        }
        assert!(
            body == original && fixture.counts()? == (0, 0, 0),
            "chat_ch1_boundary_depth_mutated_input_or_published"
        );
        drop(observation);
        assert!(
            fixture.budget.used() == baseline,
            "chat_ch1_boundary_depth_observation_charge_retained"
        );
    }
    Ok(())
}

// CH2 JSON state table: Parsed -> exact native observation -> staged, never published.
// Native choices use unique indices below 128, independent of array position. Complete
// content strings/known text parts, refusal and readable reasoning are inspected with
// distinct policies; tool argument strings are strictly decoded by the shared owner.
// stop/tool_calls/function_call require matching complete messages; legacy carriers,
// incomplete/unknown reasons, malformed calls and errors never release or publish.
// Finalization additionally requires original HTTP proof and actual entity EOF.

fn ch2_lane_source(reference: &str, kind: usize) -> Value {
    let mut message = json!({"role":"assistant","content":reference,
        "opaque":{"same_value":reference,"types":[true,null,7,1.0]}});
    match kind {
        1 => message["refusal"] = json!(reference),
        2 => {
            message["reasoning"] = json!(reference);
            message["reasoning_content"] = json!(reference);
        }
        3 => {
            message["content"] = json!([
                {"type":"text","text":reference,"extension":true},
                {"type":"output_text","text":reference},
                {"type":"refusal","refusal":reference},
                {"type":"image_url","image_url":{"url":reference}}
            ])
        }
        4 => {
            message["tool_calls"] = json!([{"id":"synthetic-ch2-call","type":"function",
            "function":{"name":"synthetic-tool","arguments":format!("{{\"value\":\"{reference}\",\"n\":7}}")}}])
        }
        _ => {}
    }
    json!({"id":"synthetic-ch2-response","object":"chat.completion","choices":[
        {"index":7,"message":message,"finish_reason":if kind == 4 {"tool_calls"} else {"stop"}}
    ],"usage":{"opaque":reference},"extension":[true,null,7,1.0]})
}

fn ch2_lane_json_restoration_run(kind: usize) -> TestResult {
    use crate::proxy::security::{
        native_test_support::{NativeFixture, TEXT},
        restoration_core::stage_native_json_with_lifecycle,
    };
    // This unused function item asserts we still target the real shared API surface.
    let _ = stage_native_json_with_lifecycle;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let source = ch2_lane_source(&fixture.reference, kind);
    let original = source.clone();
    let result = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    );
    assert!(
        result.is_ok(),
        "chat_ch2_json_lane_supported_response_rejected"
    );
    let staged = result.map_err(|_| "chat_ch2_json_stage_rejected")?;
    let expected = match kind {
        1 => 2,
        3 => 3,
        4 => 2,
        _ => 1,
    };
    assert!(
        staged.restored_occurrences() == expected && staged.output_tokens().len() == 1,
        "chat_ch2_json_shared_ledger_or_occurrences_incorrect"
    );
    let message = &staged.as_value()["choices"][0]["message"];
    assert!(
        message["opaque"] == source["choices"][0]["message"]["opaque"]
            && staged.as_value()["usage"] == source["usage"]
            && staged.as_value()["extension"] == source["extension"],
        "chat_ch2_json_opaque_or_envelope_changed"
    );
    match kind {
        1 => assert!(
            message["refusal"].as_str() == Some(TEXT),
            "chat_ch2_json_refusal_not_restored"
        ),
        2 => assert!(
            message["reasoning"].as_str() == Some(fixture.reference.as_str())
                && message["reasoning_content"].as_str() == Some(fixture.reference.as_str()),
            "chat_ch2_json_reasoning_policy_erased"
        ),
        3 => assert!(
            message["content"][0]["text"].as_str() == Some(TEXT)
                && message["content"][1]["text"].as_str() == Some(TEXT)
                && message["content"][2]["refusal"].as_str() == Some(TEXT)
                && message["content"][3] == source["choices"][0]["message"]["content"][3],
            "chat_ch2_json_content_parts_not_exact"
        ),
        4 => {
            let decoded: Value = serde_json::from_str(
                message["tool_calls"][0]["function"]["arguments"]
                    .as_str()
                    .ok_or("chat_ch2_json_arguments_missing")?,
            )?;
            assert!(
                decoded["value"].as_str() == Some(TEXT) && decoded["n"] == json!(7),
                "chat_ch2_json_native_arguments_not_restored"
            );
        }
        _ => assert!(
            message["content"].as_str() == Some(TEXT),
            "chat_ch2_json_content_not_restored"
        ),
    }
    assert!(
        staged.delivered() == &json!([message]),
        "chat_ch2_json_delivered_order_incorrect"
    );
    assert!(
        fixture.counts()? == (0, 0, 0) && source == original,
        "chat_ch2_json_staging_published_or_mutated"
    );
    drop(staged);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch2_json_stage_charge_retained"
    );
    Ok(())
}

#[test]
fn ch2_lane_json_content_exact_native_restoration() -> TestResult {
    ch2_lane_json_restoration_run(0)
}
#[test]
fn ch2_lane_json_refusal_uses_distinct_policy() -> TestResult {
    ch2_lane_json_restoration_run(1)
}
#[test]
fn ch2_lane_json_reasoning_is_readable_not_restorable() -> TestResult {
    ch2_lane_json_restoration_run(2)
}
#[test]
fn ch2_lane_json_complete_content_array_preserves_opaque_parts() -> TestResult {
    ch2_lane_json_restoration_run(3)
}
#[test]
fn ch2_lane_json_tool_arguments_share_one_ledger() -> TestResult {
    ch2_lane_json_restoration_run(4)
}

#[test]
fn ch2_lane_json_sparse_multichoice_order_and_final_delivery() -> TestResult {
    use crate::proxy::security::{
        native_test_support::{chat, record_delivery, NativeFixture, MAX_ENTITY, TEXT},
        response_restoration::finalize_native_json,
    };
    use bytes::Bytes;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let mut source = ch2_lane_source(&fixture.reference, 0);
    let mut second = source["choices"][0].clone();
    second["index"] = json!(2);
    source["choices"]
        .as_array_mut()
        .ok_or("chat_ch2_json_choices_missing")?
        .push(second);
    let wire = Bytes::from(serde_json::to_vec_pretty(&source)?);
    let proof = futures::executor::block_on(fixture.json_proof(
        &source,
        futures::stream::iter(vec![Ok::<_, std::io::Error>(wire.clone())]),
    ))
    .map_err(|_| "chat_ch2_json_original_http_proof_rejected")?;
    let staged = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "chat_ch2_json_multichoice_stage_rejected")?;
    assert!(
        staged.restored_occurrences() == 2
            && staged.output_tokens().len() == 1
            && staged
                .delivered()
                .as_array()
                .is_some_and(|items| items.len() == 2),
        "chat_ch2_json_multichoice_ledger_or_order_missing"
    );
    let finalized = finalize_native_json(
        staged,
        &fixture.lease,
        proof,
        &fixture.attempt,
        wire,
        MAX_ENTITY,
    )
    .map_err(|_| "chat_ch2_json_finalization_rejected")?;
    // JSON's frozen owner activates only after strict final encoding, before returning.
    assert!(
        fixture.counts()? == (1, 0, 0),
        "chat_ch2_json_sealed_finalization_did_not_activate_exact_record"
    );
    let chunks = futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
    let actual: Value = serde_json::from_slice(&chunks[0])?;
    assert!(
        actual["choices"][0]["index"] == json!(7)
            && actual["choices"][1]["index"] == json!(2)
            && actual["choices"][0]["message"]["content"].as_str() == Some(TEXT)
            && actual["choices"][1]["message"]["content"].as_str() == Some(TEXT),
        "chat_ch2_json_final_native_order_or_restoration_incorrect"
    );
    assert!(
        fixture
            .lease
            .verify_native_delivered(
                &fixture.binding,
                chat(),
                &json!([
                    actual["choices"][0]["message"],
                    actual["choices"][1]["message"]
                ])
            )
            .is_ok(),
        "chat_ch2_json_exact_delivered_history_missing"
    );
    // The returned Bytes owns the charged encoding until the consumer releases it.
    drop(chunks);
    assert!(
        fixture.budget.used() == baseline,
        "chat_ch2_json_delivery_charge_retained"
    );
    Ok(())
}

#[test]
fn ch2_lane_json_no_change_preserves_original_wire() -> TestResult {
    use crate::proxy::security::{
        native_test_support::{record_delivery, NativeFixture, MAX_ENTITY},
        response_restoration::finalize_native_json,
    };
    use bytes::Bytes;
    let fixture = NativeFixture::new(false)?;
    let source = ch2_lane_source(&fixture.reference, 0);
    let wire = Bytes::from(serde_json::to_vec_pretty(&source)?);
    let proof = futures::executor::block_on(fixture.json_proof(
        &source,
        futures::stream::iter(vec![Ok::<_, std::io::Error>(wire.clone())]),
    ))
    .map_err(|_| "chat_ch2_json_unchanged_proof_rejected")?;
    let staged = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "chat_ch2_json_unchanged_stage_rejected")?;
    assert!(
        !staged.changed(),
        "chat_ch2_json_public_content_marked_changed"
    );
    let finalized = finalize_native_json(
        staged,
        &fixture.lease,
        proof,
        &fixture.attempt,
        wire.clone(),
        MAX_ENTITY,
    )
    .map_err(|_| "chat_ch2_json_unchanged_finalization_rejected")?;
    let chunks = futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
    assert!(
        chunks.len() == 1 && chunks[0] == wire,
        "chat_ch2_json_unmodified_wire_changed"
    );
    Ok(())
}

#[test]
fn ch2_lane_json_invalid_shape_reason_arguments_and_budget_reject_atomically() -> TestResult {
    use crate::proxy::security::native_test_support::NativeFixture;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    for mode in 0..11 {
        let mut source = ch2_lane_source(&fixture.reference, 4);
        match mode {
            0 => source["choices"][0]["finish_reason"] = json!("length"),
            1 => source["choices"][0]["finish_reason"] = Value::Null,
            2 => {
                source["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
                    json!("[]")
            }
            3 => source["choices"][0]["message"]["function_call"] = json!({"name":"legacy"}),
            4 => source["choices"][0]["message"]["role"] = json!("user"),
            5 => source["error"] = json!({"type":"synthetic_error"}),
            6 => source["choices"][0]["index"] = json!(128),
            7 => {
                let duplicate = source["choices"][0].clone();
                source["choices"]
                    .as_array_mut()
                    .ok_or("chat_ch2_json_choices_missing")?
                    .push(duplicate);
            }
            8 => source["choices"][0]["message"]["content"] = json!(17),
            9 => source["id"] = Value::Null,
            _ => {
                source["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
                    json!("not-json")
            }
        }
        let original = source.clone();
        assert!(
            super::response::stage_json(
                &source,
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
                &fixture.attempt
            )
            .is_err(),
            "chat_ch2_json_invalid_native_response_accepted"
        );
        assert!(
            fixture.budget.used() == baseline
                && fixture.counts()? == (0, 0, 0)
                && source == original,
            "chat_ch2_json_invalid_stage_not_atomic"
        );
    }
    let source = ch2_lane_source(&fixture.reference, 0);
    let tiny = PayloadBudget::with_limit(1);
    assert!(
        super::response::stage_json(
            &source,
            &fixture.lease,
            &fixture.binding,
            tiny.clone(),
            &fixture.attempt
        )
        .is_err(),
        "chat_ch2_json_budget_exhaustion_accepted"
    );
    assert!(
        tiny.used() == 0 && fixture.counts()? == (0, 0, 0),
        "chat_ch2_json_budget_not_refunded"
    );
    Ok(())
}

// CH2 SSE state table: exact complete frames -> budgeted native sites -> lifecycle
// staging -> strict original/final proof -> sealed tool/byte delivery. Source-less
// comments and DONE have no sites. Choice indices and choice-local call indices are
// sparse identities; fragments from different fields/choices/calls never join.
// Every choice needs a supported terminal reason, DONE and genuine transport EOF.
// Before and after each yield publication is pending; only the final continuation
// activates it. Error/cancellation/budget/unsupported frames cannot release a record.

fn ch2_lane_sse_wire(reference: &str, split: usize, tools: bool) -> TestResult<String> {
    let (left, right) = reference.split_at(split);
    let delta1 = if tools {
        json!({"role":"assistant","content":left,"tool_calls":[
            {"index":9,"id":"synthetic-sse-call","type":"function","function":{"name":"synthetic-tool","arguments":format!("{{\"value\":\"{left}")}}
        ]})
    } else {
        json!({"role":"assistant","content":left})
    };
    let delta2 = if tools {
        json!({"content":right,"tool_calls":[
            {"index":9,"function":{"arguments":format!("{right}\"}}")}}
        ]})
    } else {
        json!({"content":right})
    };
    let chunks = [
        json!({"id":"synthetic-sse-response","choices":[{"index":7,"delta":delta1,"finish_reason":null}],"extension":{"types":[true,null,7]}}),
        json!({"id":"synthetic-sse-response","choices":[{"index":7,"delta":delta2,"finish_reason":null}],"extension":{"types":[true,null,7]}}),
        json!({"id":"synthetic-sse-response","choices":[{"index":7,"delta":{},"finish_reason":if tools {"tool_calls"} else {"stop"}}]}),
        json!({"id":"synthetic-sse-response","choices":[],"usage":{"total_tokens":7}}),
    ];
    let mut wire = String::from(": synthetic-keepalive\n\n");
    for chunk in chunks {
        wire.push_str(&format!("data: {}\n\n", serde_json::to_string(&chunk)?));
    }
    wire.push_str("data: [DONE]\n\n");
    Ok(wire)
}

fn ch2_lane_finalize_sse(
    fixture: &crate::proxy::security::native_test_support::NativeFixture,
    wire: &str,
) -> TestResult<crate::proxy::security::response_restoration::FinalizedNativeSse> {
    use crate::proxy::security::{
        http_restoration::test_transport, native_test_support::MAX_ENTITY,
    };
    use bytes::Bytes;
    let proof = futures::executor::block_on(test_transport::native_sse_proof(
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        Bytes::copy_from_slice(wire.as_bytes()),
        Bytes::copy_from_slice(wire.as_bytes()),
    ))
    .map_err(|_| "chat_ch2_sse_http_proof_rejected")?;
    super::response::finalize_sse(
        wire.as_bytes(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
        proof,
        MAX_ENTITY,
    )
    .map_err(|_| "chat_ch2_sse_lane_finalization_rejected".into())
}

#[test]
fn ch2_lane_sse_all_reference_splits_restore_with_last_yield_continuation() -> TestResult {
    use crate::proxy::security::native_test_support::{NativeFixture, TEXT};
    use futures::StreamExt;
    for split in 0..=NativeFixture::new(true)?.reference.len() {
        let fixture = NativeFixture::new(true)?;
        let baseline = fixture.budget.used();
        let wire = ch2_lane_sse_wire(&fixture.reference, split, false)?;
        let finalized = ch2_lane_finalize_sse(&fixture, &wire)?;
        assert!(
            fixture.counts()? == (0, 1, 1),
            "chat_ch2_sse_prepared_or_early_publication_incorrect"
        );
        let mut stream = finalized.into_delivery_stream();
        let mut emitted = Vec::new();
        // One comment, four JSON frames and DONE: every yield retains the pending owner.
        for _ in 0..6 {
            let chunk = futures::executor::block_on(stream.next())
                .ok_or("chat_ch2_sse_frame_missing")??;
            emitted.extend_from_slice(&chunk);
            assert!(
                fixture.counts()? == (0, 1, 1),
                "chat_ch2_sse_published_before_last_continuation"
            );
        }
        assert!(
            futures::executor::block_on(stream.next()).is_none() && fixture.counts()? == (1, 0, 0),
            "chat_ch2_sse_final_continuation_did_not_publish"
        );
        drop(stream);
        let text = std::str::from_utf8(&emitted)?;
        let mut content = String::new();
        for data in text.lines().filter_map(|line| line.strip_prefix("data: ")) {
            if data == "[DONE]" {
                continue;
            }
            let source: Value = serde_json::from_str(data)?;
            if let Some(part) = source["choices"][0]["delta"]["content"].as_str() {
                content.push_str(part);
            }
        }
        assert!(
            content == TEXT
                && text.starts_with(": synthetic-keepalive\n\n")
                && text.ends_with("data: [DONE]\n\n"),
            "chat_ch2_sse_fragment_restore_or_opaque_wire_changed"
        );
        assert!(
            fixture.budget.used() == baseline,
            "chat_ch2_sse_delivered_metadata_charge_retained"
        );
    }
    Ok(())
}

#[test]
fn ch2_lane_sse_fragmented_sparse_call_is_held_and_strictly_decoded() -> TestResult {
    use crate::proxy::security::native_test_support::{record_delivery, NativeFixture, TEXT};
    let fixture = NativeFixture::new(true)?;
    let wire = ch2_lane_sse_wire(&fixture.reference, fixture.reference.len() / 2, true)?;
    let finalized = ch2_lane_finalize_sse(&fixture, &wire)?;
    assert!(
        fixture.counts()? == (0, 1, 1),
        "chat_ch2_sse_tool_published_before_delivery"
    );
    let chunks = futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
    let mut arguments = String::new();
    for chunk in &chunks {
        let raw = std::str::from_utf8(chunk)?;
        for data in raw.lines().filter_map(|line| line.strip_prefix("data: ")) {
            if data == "[DONE]" {
                continue;
            }
            let source: Value = serde_json::from_str(data)?;
            if let Some(part) =
                source["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str()
            {
                arguments.push_str(part);
            }
        }
    }
    let decoded: Value = serde_json::from_str(&arguments)?;
    assert!(
        decoded["value"].as_str() == Some(TEXT) && fixture.counts()? == (1, 0, 0),
        "chat_ch2_sse_sparse_tool_not_restored_or_published"
    );
    Ok(())
}

#[test]
fn ch2_lane_sse_drop_after_last_yield_does_not_publish() -> TestResult {
    use crate::proxy::security::native_test_support::NativeFixture;
    use futures::StreamExt;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let wire = ch2_lane_sse_wire(&fixture.reference, fixture.reference.len() / 2, false)?;
    let mut stream = ch2_lane_finalize_sse(&fixture, &wire)?.into_delivery_stream();
    for _ in 0..6 {
        drop(
            futures::executor::block_on(stream.next())
                .ok_or("chat_ch2_sse_cancel_frame_missing")??,
        );
    }
    assert!(
        fixture.counts()? == (0, 1, 1),
        "chat_ch2_sse_cancel_early_publication"
    );
    drop(stream);
    assert!(
        fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
        "chat_ch2_sse_cancellation_retained_authority_or_charge"
    );
    Ok(())
}

#[test]
fn ch2_lane_sse_unchanged_preserves_every_wire_byte() -> TestResult {
    use crate::proxy::security::native_test_support::{record_delivery, NativeFixture};
    let fixture = NativeFixture::new(false)?;
    let wire = ch2_lane_sse_wire(&fixture.reference, 0, false)?;
    let chunks = futures::executor::block_on(record_delivery(
        ch2_lane_finalize_sse(&fixture, &wire)?.into_delivery_stream(),
    ))?;
    let emitted: Vec<u8> = chunks
        .iter()
        .flat_map(|chunk| chunk.iter().copied())
        .collect();
    assert!(
        emitted == wire.as_bytes(),
        "chat_ch2_sse_unchanged_wire_reencoded"
    );
    Ok(())
}

fn ch2_lane_sse_chunks(chunks: &[Value]) -> TestResult<String> {
    let mut wire = String::from("id: synthetic-event\r\n: synthetic-comment\r\n\r\n");
    for chunk in chunks {
        let mut chunk = chunk.clone();
        chunk["id"] = json!("synthetic-sse-response");
        wire.push_str(&format!(
            "event: message\r\ndata: {}\r\n\r\n",
            serde_json::to_string(&chunk)?
        ));
    }
    wire.push_str("data: [DONE]\r\n\r\n");
    Ok(wire)
}

#[test]
fn ch2_lane_sse_interleaved_choices_calls_and_typed_fields_are_isolated() -> TestResult {
    use crate::proxy::security::native_test_support::{record_delivery, NativeFixture, TEXT};
    let fixture = NativeFixture::new(true)?;
    let (left, right) = fixture.reference.split_at(fixture.reference.len() / 2);
    let chunks = [
        json!({"choices":[
            {"index":7,"delta":{"role":"assistant","content":left,"refusal":left,
                "reasoning":left,"reasoning_content":left,"opaque":{"same":fixture.reference},
                "tool_calls":[{"index":9,"id":"synthetic-seven-nine","type":"function","function":{"name":"synthetic-a","arguments":format!("{{\"value\":\"{left}")}},
                    {"index":2,"id":"synthetic-seven-two","type":"function","function":{"name":"synthetic-b","arguments":"{\"value\":\"synthetic-public\"}"}}]},"finish_reason":null},
            {"index":2,"delta":{"role":"assistant","content":"synthetic-other","tool_calls":[
                {"index":9,"id":"synthetic-two-nine","type":"function","function":{"name":"synthetic-c","arguments":"{\"value\":\"synthetic-other\"}"}}]},"finish_reason":null}]}),
        json!({"choices":[
            {"index":2,"delta":{"content":"-tail"},"finish_reason":"tool_calls"},
            {"index":7,"delta":{"content":right,"refusal":right,"reasoning":right,"reasoning_content":right,
                "tool_calls":[{"index":9,"function":{"arguments":format!("{right}\"}}")}}]},"finish_reason":"tool_calls"}]}),
        json!({"choices":[],"usage":{"total_tokens":11}}),
    ];
    let wire = ch2_lane_sse_chunks(&chunks)?;
    let emitted = futures::executor::block_on(record_delivery(
        ch2_lane_finalize_sse(&fixture, &wire)?.into_delivery_stream(),
    ))?;
    let mut content = String::new();
    let mut refusal = String::new();
    let mut reasoning = String::new();
    let mut reasoning_content = String::new();
    let mut arguments = String::new();
    let mut other = String::new();
    for chunk in &emitted {
        for data in std::str::from_utf8(chunk)?
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
        {
            if data == "[DONE]" {
                continue;
            }
            let source: Value = serde_json::from_str(data)?;
            for choice in source["choices"]
                .as_array()
                .ok_or("chat_ch2_choices_missing")?
            {
                let delta = &choice["delta"];
                if choice["index"] == 7 {
                    for (field, target) in [
                        ("content", &mut content),
                        ("refusal", &mut refusal),
                        ("reasoning", &mut reasoning),
                        ("reasoning_content", &mut reasoning_content),
                    ] {
                        if let Some(text) = delta[field].as_str() {
                            target.push_str(text);
                        }
                    }
                    if let Some(opaque) = delta.get("opaque") {
                        assert!(
                            opaque["same"].as_str() == Some(fixture.reference.as_str()),
                            "chat_ch2_opaque_sibling_changed"
                        );
                    }
                    if let Some(calls) = delta["tool_calls"].as_array() {
                        for call in calls {
                            if call["index"] == 9 {
                                if let Some(text) = call["function"]["arguments"].as_str() {
                                    arguments.push_str(text);
                                }
                            } else {
                                assert!(
                                    call["function"]["arguments"]
                                        == "{\"value\":\"synthetic-public\"}",
                                    "chat_ch2_sparse_call_mixed"
                                );
                            }
                        }
                    }
                } else {
                    if let Some(text) = delta["content"].as_str() {
                        other.push_str(text);
                    }
                    if let Some(calls) = delta["tool_calls"].as_array() {
                        assert!(
                            calls[0]["function"]["arguments"] == "{\"value\":\"synthetic-other\"}",
                            "chat_ch2_choice_local_call_mixed"
                        );
                    }
                }
            }
        }
    }
    let decoded: Value = serde_json::from_str(&arguments)?;
    assert!(
        content == TEXT
            && refusal == TEXT
            && reasoning == fixture.reference
            && reasoning_content == fixture.reference
            && decoded["value"].as_str() == Some(TEXT)
            && other == "synthetic-other-tail"
            && fixture.counts()? == (1, 0, 0),
        "chat_ch2_typed_stream_isolation_failed"
    );
    Ok(())
}

#[test]
fn ch2_lane_sse_partial_references_never_join_across_choice_or_field() -> TestResult {
    use crate::proxy::security::native_test_support::{record_delivery, NativeFixture};
    let fixture = NativeFixture::new(true)?;
    let (left, right) = fixture.reference.split_at(fixture.reference.len() / 2);
    let wire = ch2_lane_sse_chunks(&[json!({"choices":[
        {"index":7,"delta":{"role":"assistant","content":left,"refusal":right},"finish_reason":"stop"},
        {"index":2,"delta":{"role":"assistant","content":right,"refusal":left},"finish_reason":"stop"}]
    })])?;
    let emitted = futures::executor::block_on(record_delivery(
        ch2_lane_finalize_sse(&fixture, &wire)?.into_delivery_stream(),
    ))?;
    let bytes: Vec<u8> = emitted
        .iter()
        .flat_map(|chunk| chunk.iter().copied())
        .collect();
    assert!(bytes == wire.as_bytes(), "chat_ch2_partial_carriers_joined");
    Ok(())
}

#[test]
fn ch2_lane_sse_strict_original_and_final_controls_reject_without_publication() -> TestResult {
    use crate::proxy::security::native_test_support::NativeFixture;
    let bad = [
        json!({"choices":[{"index":7,"delta":{"role":"assistant","content":"synthetic"},"finish_reason":"length"}]}),
        json!({"choices":[{"index":7,"delta":{"role":"assistant","content":"synthetic"},"finish_reason":"content_filter"}]}),
        json!({"choices":[{"index":7,"delta":{"role":"assistant","content":"synthetic"},"finish_reason":"unknown"}]}),
        json!({"choices":[{"index":7,"delta":{"role":"assistant","content":"synthetic"},"finish_reason":null}]}),
        json!({"choices":[{"index":7,"delta":{"role":"assistant","content":[{"type":"text","text":"synthetic"}]},"finish_reason":"stop"}]}),
        json!({"choices":[{"index":7,"delta":{"role":"assistant","function_call":{"name":"synthetic","arguments":"{}"}},"finish_reason":"function_call"}]}),
        json!({"choices":[{"index":7,"delta":{"role":"assistant","tool_calls":[{"index":9,"id":"synthetic","type":"function","function":{"name":"synthetic","arguments":"[]"}}]},"finish_reason":"tool_calls"}]}),
        json!({"choices":[{"index":7,"delta":{"role":"assistant","content":"synthetic"},"finish_reason":"stop"},{"index":7,"delta":{},"finish_reason":"stop"}]}),
        json!({"choices":[],"error":{"message":"Synthetic upstream failure."}}),
    ];
    for source in bad {
        let fixture = NativeFixture::new(true)?;
        let baseline = fixture.budget.used();
        let wire = ch2_lane_sse_chunks(&[source])?;
        assert!(
            ch2_lane_finalize_sse(&fixture, &wire).is_err()
                && fixture.counts()? == (0, 0, 0)
                && fixture.budget.used() == baseline,
            "chat_ch2_invalid_original_released_or_retained"
        );
    }
    let fixture = NativeFixture::new(true)?;
    let wire = ch2_lane_sse_wire(&fixture.reference, 0, false)?;
    for changed in [
        wire.replace("data: [DONE]\n\n", ""),
        format!("{wire}data: {{}}\n\n"),
        wire.replace("data: [DONE]\n\n", "data: [DONE]\n"),
    ] {
        assert!(
            ch2_lane_finalize_sse(&fixture, &changed).is_err() && fixture.counts()? == (0, 0, 0),
            "chat_ch2_invalid_terminal_or_truncated_frame_released"
        );
    }
    Ok(())
}

#[test]
fn ch2_lane_sse_finalizer_rejects_substituted_entity_attempt_and_binding() -> TestResult {
    use crate::proxy::security::{
        http_restoration::test_transport,
        native_test_support::{NativeFixture, MAX_ENTITY},
        restoration_core::AttemptProvenance,
    };
    use bytes::Bytes;
    let fixture = NativeFixture::new(true)?;
    let wire = ch2_lane_sse_wire(&fixture.reference, 0, false)?;
    for case in 0..3 {
        let proof = futures::executor::block_on(test_transport::native_sse_proof(
            &fixture.attempt,
            &fixture.binding,
            &fixture.budget,
            Bytes::copy_from_slice(wire.as_bytes()),
            Bytes::copy_from_slice(wire.as_bytes()),
        ))
        .map_err(|_| "chat_ch2_control_proof_rejected")?;
        let changed = wire.replace("synthetic-sse-response", "synthetic-other-response");
        let other_attempt = AttemptProvenance::new(&fixture.budget)?;
        let other_binding = fixture.lease.bind_actual_attempt(
            "synthetic-other-provider",
            "openai_chat",
            "https://synthetic-other.invalid/v1/chat/completions",
            Some("synthetic-principal"),
        )?;
        assert!(
            super::response::finalize_sse(
                if case == 0 {
                    changed.as_bytes()
                } else {
                    wire.as_bytes()
                },
                &fixture.lease,
                if case == 2 {
                    &other_binding
                } else {
                    &fixture.binding
                },
                fixture.budget.clone(),
                if case == 1 {
                    &other_attempt
                } else {
                    &fixture.attempt
                },
                proof,
                MAX_ENTITY
            )
            .is_err()
                && fixture.counts()? == (0, 0, 0),
            "chat_ch2_substituted_authority_accepted"
        );
    }
    Ok(())
}

#[test]
fn ch2_lane_sse_budget_failures_refund_all_staging_and_publication() -> TestResult {
    use crate::proxy::security::{
        http_restoration::test_transport,
        native_test_support::{NativeFixture, MAX_ENTITY},
    };
    use bytes::Bytes;
    for limit in [0, 1, 256, 2048, 8192, 32768] {
        let fixture = NativeFixture::new(true)?;
        let wire = ch2_lane_sse_wire(&fixture.reference, fixture.reference.len() / 2, true)?;
        let proof = futures::executor::block_on(test_transport::native_sse_proof(
            &fixture.attempt,
            &fixture.binding,
            &fixture.budget,
            Bytes::copy_from_slice(wire.as_bytes()),
            Bytes::copy_from_slice(wire.as_bytes()),
        ))
        .map_err(|_| "chat_ch2_budget_control_proof_rejected")?;
        let budget = PayloadBudget::with_limit(limit);
        let result = super::response::finalize_sse(
            wire.as_bytes(),
            &fixture.lease,
            &fixture.binding,
            budget.clone(),
            &fixture.attempt,
            proof,
            MAX_ENTITY,
        );
        assert!(
            result.is_err(),
            "chat_ch2_budget_control_unexpected_success"
        );
        drop(result);
        assert!(
            budget.used() == 0 && fixture.counts()? == (0, 0, 0),
            "chat_ch2_budget_failure_leaked"
        );
    }
    Ok(())
}

#[test]
fn ch2_lane_sse_real_http_eof_error_and_pending_cancel_controls() -> TestResult {
    use crate::proxy::security::{
        http_restoration::test_transport,
        native_test_support::{NativeFixture, ScriptedSource, Step},
    };
    use bytes::Bytes;
    use futures::FutureExt;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let wire = Bytes::from(ch2_lane_sse_wire(&fixture.reference, 0, false)?);
    let (original, original_stats) = ScriptedSource::new([
        Step::Bytes(wire.clone()),
        Step::Error(std::io::ErrorKind::UnexpectedEof),
    ]);
    let (client, _) = ScriptedSource::split(wire.clone(), &[1, 2, 3])?;
    assert!(
        futures::executor::block_on(test_transport::native_sse_from_streams(
            &fixture.attempt,
            &fixture.binding,
            &fixture.budget,
            original,
            client
        ))
        .is_err()
            && original_stats.eof.get() == 0
            && fixture.counts()? == (0, 0, 0),
        "chat_ch2_transport_error_minted_eof"
    );
    let (original, stats) = ScriptedSource::new([Step::Bytes(wire.clone()), Step::Park]);
    let (client, _) = ScriptedSource::split(wire, &[])?;
    assert!(
        test_transport::native_sse_from_streams(
            &fixture.attempt,
            &fixture.binding,
            &fixture.budget,
            original,
            client
        )
        .now_or_never()
        .is_none()
            && stats.eof.get() == 0
            && fixture.counts()? == (0, 0, 0)
            && fixture.budget.used() == baseline,
        "chat_ch2_pending_cancel_minted_authority_or_charge"
    );
    Ok(())
}

// CH3 state table: exact ordered delivered assistant/call array -> strict shared
// lookup -> atomic dependency import. A call/result pair is structural metadata
// only; first, replayed and edited result bodies always remain untrusted CH1 sites.
// Missing session/record, edited/order-truncated evidence, unknown references and
// mismatched store/provider/protocol/generation never grant historical authority.

fn ch3_lane_next(
    fixture: &crate::proxy::security::native_test_support::NativeFixture,
    session: Option<&str>,
    client: Protocol,
    provider: &str,
) -> TestResult<(
    DlpRequestLease,
    crate::proxy::security::completed_context::ActualAttemptBinding,
)> {
    let lease = fixture.store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(client),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-execution-app",
        session,
        fixture.binding.policy_revision,
    )?;
    let binding = lease.bind_actual_attempt(
        provider,
        "openai_chat",
        "https://synthetic.invalid/v1/chat/completions",
        Some("synthetic-principal"),
    )?;
    Ok((lease, binding))
}

fn ch3_lane_deliver_json(
    fixture: &crate::proxy::security::native_test_support::NativeFixture,
    source: &Value,
) -> TestResult<Value> {
    use crate::proxy::security::{
        native_test_support::{record_delivery, MAX_ENTITY},
        response_restoration::finalize_native_json,
    };
    use bytes::Bytes;
    let wire = Bytes::from(serde_json::to_vec(source)?);
    let proof = futures::executor::block_on(fixture.json_proof(
        source,
        futures::stream::iter([Ok::<_, std::io::Error>(wire.clone())]),
    ))
    .map_err(|_| "chat_ch3_json_original_rejected")?;
    let stage = super::response::stage_json(
        source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "chat_ch3_json_stage_rejected")?;
    let finalized = finalize_native_json(
        stage,
        &fixture.lease,
        proof,
        &fixture.attempt,
        wire,
        MAX_ENTITY,
    )
    .map_err(|_| "chat_ch3_json_finalizer_rejected")?;
    let chunks = futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
    let response: Value = serde_json::from_slice(&chunks[0])?;
    let delivered = response["choices"]
        .as_array()
        .ok_or("chat_ch3_choices_missing")?
        .iter()
        .map(|choice| choice["message"].clone())
        .collect::<Vec<_>>();
    Ok(Value::Array(delivered))
}

#[test]
fn ch3_lane_exact_ordered_json_delivery_imports_only_complete_dependency_set() -> TestResult {
    use crate::proxy::security::native_test_support::NativeFixture;
    let fixture = NativeFixture::new(true)?;
    let mut source = ch2_lane_source(&fixture.reference, 4);
    source["choices"].as_array_mut().ok_or("chat_ch3_fixture_choices_missing")?.push(
        json!({"index":2,"message":{"role":"assistant","content":"synthetic-other"},"finish_reason":"stop"}));
    let delivered = ch3_lane_deliver_json(&fixture, &source)?;
    let (mut next, binding) = ch3_lane_next(
        &fixture,
        Some("synthetic-execution-session"),
        Protocol::Chat,
        "synthetic-provider",
    )?;
    assert!(
        next.resolve(&fixture.reference).is_err(),
        "chat_ch3_next_lease_already_authorized"
    );
    super::history::import_delivered(&mut next, &binding, &delivered)?;
    assert!(
        next.resolve(&fixture.reference)? == TEXT,
        "chat_ch3_exact_delivery_not_imported"
    );
    super::history::import_delivered(&mut next, &binding, &delivered)?;
    assert!(
        next.resolve(&fixture.reference)? == TEXT && fixture.counts()? == (1, 0, 0),
        "chat_ch3_repeat_import_changed_authority"
    );
    Ok(())
}

#[test]
fn ch3_lane_edited_reordered_truncated_or_unknown_delivery_grants_nothing() -> TestResult {
    use crate::proxy::security::native_test_support::NativeFixture;
    let fixture = NativeFixture::new(true)?;
    let mut source = ch2_lane_source(&fixture.reference, 4);
    source["choices"].as_array_mut().ok_or("chat_ch3_fixture_choices_missing")?.push(
        json!({"index":2,"message":{"role":"assistant","content":"synthetic-other"},"finish_reason":"stop"}));
    let delivered = ch3_lane_deliver_json(&fixture, &source)?;
    let mut edited = delivered.clone();
    edited[0]["tool_calls"][0]["function"]["arguments"] = json!("{}");
    let mut reordered = delivered.clone();
    reordered
        .as_array_mut()
        .ok_or("chat_ch3_fixture_order_missing")?
        .reverse();
    let mut unknown = delivered.clone();
    unknown[0]["content"] = json!("__DLP_REF_synthetic_unknown__");
    for bad in [edited, reordered, json!([delivered[0]]), unknown, json!([])] {
        let (mut next, binding) = ch3_lane_next(
            &fixture,
            Some("synthetic-execution-session"),
            Protocol::Chat,
            "synthetic-provider",
        )?;
        assert!(
            super::history::import_delivered(&mut next, &binding, &bad).is_err()
                && next.resolve(&fixture.reference).is_err(),
            "chat_ch3_bad_replay_imported_dependencies"
        );
    }
    Ok(())
}

#[test]
fn ch3_lane_no_session_cross_session_provider_protocol_store_and_retry_fail_closed() -> TestResult {
    use crate::proxy::security::native_test_support::NativeFixture;
    let fixture = NativeFixture::new(true)?;
    let delivered = ch3_lane_deliver_json(&fixture, &ch2_lane_source(&fixture.reference, 4))?;
    for (session, client, provider) in [
        (None, Protocol::Chat, "synthetic-provider"),
        (
            Some("synthetic-other-session"),
            Protocol::Chat,
            "synthetic-provider",
        ),
        (
            Some("synthetic-execution-session"),
            Protocol::Chat,
            "synthetic-other-provider",
        ),
        (
            Some("synthetic-execution-session"),
            Protocol::Gemini,
            "synthetic-provider",
        ),
    ] {
        let (mut next, binding) = ch3_lane_next(&fixture, session, client, provider)?;
        assert!(
            super::history::import_delivered(&mut next, &binding, &delivered).is_err()
                && next.resolve(&fixture.reference).is_err(),
            "chat_ch3_cross_binding_replay_accepted"
        );
    }
    let other = NativeFixture::new(false)?;
    let (mut next, binding) = ch3_lane_next(
        &other,
        Some("synthetic-execution-session"),
        Protocol::Chat,
        "synthetic-provider",
    )?;
    assert!(
        super::history::import_delivered(&mut next, &binding, &delivered).is_err(),
        "chat_ch3_cross_store_replay_accepted"
    );
    let (next, _) = ch3_lane_next(
        &fixture,
        Some("synthetic-execution-session"),
        Protocol::Chat,
        "synthetic-provider",
    )?;
    let mut retry = next.fork_attempt()?;
    let binding = retry.bind_actual_attempt(
        "synthetic-retry-provider",
        "openai_chat",
        "https://synthetic-retry.invalid/v1/chat/completions",
        Some("synthetic-principal"),
    )?;
    assert!(
        super::history::import_delivered(&mut retry, &binding, &delivered).is_err()
            && retry.resolve(&fixture.reference).is_err(),
        "chat_ch3_retry_inherited_other_provider_success"
    );
    Ok(())
}

#[test]
fn ch3_lane_native_pairing_is_exact_local_metadata_without_result_authority() -> TestResult {
    let body = json!({"messages":[
        {"role":"assistant","tool_calls":[{"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":"{}"}}]},
        {"role":"tool","tool_call_id":"synthetic-call","content":"synthetic-first"},
        {"role":"tool","tool_call_id":"synthetic-call","content":"synthetic-duplicate"},
        {"role":"tool","tool_call_id":"synthetic-fabricated","content":"synthetic-fabricated"},
        {"role":"user","content":"synthetic-current"},
        {"role":"tool","tool_call_id":"synthetic-call","content":"synthetic-stale"}
    ]});
    let budget = PayloadBudget::with_limit(LIMIT);
    let observation = super::history::observe_request(&body, budget.clone())?;
    let pairs = observation
        .history()
        .iter()
        .filter_map(|claim| match claim {
            HistoryClaim::CallResultPair { call, result } => Some((*call, *result)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        pairs.len() == 1
            && std::ptr::eq(pairs[0].0, &body["messages"][0]["tool_calls"][0])
            && std::ptr::eq(pairs[0].1, &body["messages"][1]),
        "chat_ch3_pairing_missing_or_fabricated"
    );
    assert!(
        observation.sites().len() == 6,
        "chat_ch3_pairing_skipped_untrusted_or_current_sites"
    );
    drop(observation);
    assert!(
        budget.used() == 0,
        "chat_ch3_pairing_metadata_charge_retained"
    );
    Ok(())
}

#[test]
fn ch3_lane_first_and_edited_result_bodies_are_always_untrusted_current_dlp() -> TestResult {
    for text in [TEXT, "synthetic-edited 13900000000"] {
        let mut fixture = RequestFixture::new()?;
        let body = json!({"messages":[
            {"role":"assistant","tool_calls":[{"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"synthetic-call","content":text},
            {"role":"user","content":"synthetic-next"}
        ]});
        let observation = super::history::observe_request(&body, fixture.budget.clone())?;
        assert!(
            observation
                .history()
                .iter()
                .any(|claim| matches!(claim, HistoryClaim::CallResultPair { .. })),
            "chat_ch3_result_pair_missing"
        );
        let prepared = prepare_observed_request(
            &mut fixture.lease,
            &observation,
            &SecurityRuntimeConfig::default(),
            RESTORE_RULES,
            &[],
            |_, _, _| Ok(()),
        )
        .map_err(|_| "chat_ch3_current_result_preparation_rejected")?;
        assert!(
            prepared.reversible_occurrences == 1
                && !prepared.body.as_value()["messages"][1]["content"]
                    .as_str()
                    .ok_or("chat_ch3_result_text_missing")?
                    .contains(TEXT)
                && fixture.counts()? == (0, 0, 0),
            "chat_ch3_result_body_trusted_or_published"
        );
    }
    Ok(())
}

#[test]
fn ch3_lane_eviction_removes_history_privilege_without_authorizing_unknown_refs() -> TestResult {
    use crate::proxy::security::native_test_support::ExpiredNativeHistory;
    let expired = ExpiredNativeHistory::new()?;
    let (mut next, binding) = expired.current()?;
    assert!(
        super::history::import_delivered(&mut next, &binding, &expired.delivered).is_err()
            && next.resolve(&expired.reference).is_err(),
        "chat_ch3_evicted_history_restored"
    );
    let current = expired.prepare_current(next)?;
    assert!(
        current.lease.resolve(&current.reference)? == TEXT,
        "chat_ch3_eviction_broke_current_redaction"
    );
    Ok(())
}

#[test]
fn ch3_lane_sse_delivered_snapshot_and_calls_require_final_continuation() -> TestResult {
    use crate::proxy::security::native_test_support::NativeFixture;
    use futures::StreamExt;
    let fixture = NativeFixture::new(true)?;
    let wire = ch2_lane_sse_wire(&fixture.reference, fixture.reference.len() / 2, true)?;
    let mut stream = ch2_lane_finalize_sse(&fixture, &wire)?.into_delivery_stream();
    let mut content = String::new();
    let mut arguments = String::new();
    let (mut next, binding) = ch3_lane_next(
        &fixture,
        Some("synthetic-execution-session"),
        Protocol::Chat,
        "synthetic-provider",
    )?;
    for _ in 0..6 {
        let chunk =
            futures::executor::block_on(stream.next()).ok_or("chat_ch3_sse_frame_missing")??;
        for data in std::str::from_utf8(&chunk)?
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
        {
            if data == "[DONE]" {
                continue;
            }
            let source: Value = serde_json::from_str(data)?;
            if let Some(text) = source["choices"][0]["delta"]["content"].as_str() {
                content.push_str(text);
            }
            if let Some(text) =
                source["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str()
            {
                arguments.push_str(text);
            }
        }
    }
    let call = json!({"id":"synthetic-sse-call","type":"function","function":{"name":"synthetic-tool","arguments":arguments}});
    let delivered = json!([{"role":"assistant","content":content,"tool_calls":[call]},call]);
    assert!(
        super::history::import_delivered(&mut next, &binding, &delivered).is_err()
            && next.resolve(&fixture.reference).is_err()
            && fixture.counts()? == (0, 1, 1),
        "chat_ch3_sse_pending_history_accepted"
    );
    assert!(
        futures::executor::block_on(stream.next()).is_none(),
        "chat_ch3_sse_continuation_missing"
    );
    super::history::import_delivered(&mut next, &binding, &delivered)?;
    assert!(
        content == TEXT && next.resolve(&fixture.reference)? == TEXT,
        "chat_ch3_sse_complete_snapshot_not_imported"
    );
    let (mut another, binding) = ch3_lane_next(
        &fixture,
        Some("synthetic-execution-session"),
        Protocol::Chat,
        "synthetic-provider",
    )?;
    assert!(
        super::history::import_delivered(&mut another, &binding, &json!([delivered[0]])).is_err()
            && another.resolve(&fixture.reference).is_err(),
        "chat_ch3_sse_omitted_call_evidence_accepted"
    );
    Ok(())
}

#[test]
fn ch3_lane_pairing_ambiguous_ids_and_capacity_controls_do_not_grant_authority() -> TestResult {
    let call = json!({"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":"{}"}});
    let body = json!({"messages":[{"role":"assistant","tool_calls":[call,call]},
        {"role":"tool","tool_call_id":"synthetic-call","content":"synthetic-result"}]});
    let budget = PayloadBudget::with_limit(LIMIT);
    let observed = super::history::observe_request(&body, budget.clone())?;
    assert!(
        !observed
            .history()
            .iter()
            .any(|claim| matches!(claim, HistoryClaim::CallResultPair { .. }))
            && observed.sites().len() == 3,
        "chat_ch3_ambiguous_pair_trusted_or_skipped"
    );
    drop(observed);
    assert!(budget.used() == 0, "chat_ch3_ambiguous_metadata_leaked");
    for limit in [0, 1, 64] {
        let budget = PayloadBudget::with_limit(limit);
        let result = super::history::observe_request(&body, budget.clone());
        assert!(result.is_err(), "chat_ch3_pair_budget_unexpected_success");
        drop(result);
        assert!(budget.used() == 0, "chat_ch3_pair_budget_leaked");
    }
    Ok(())
}
