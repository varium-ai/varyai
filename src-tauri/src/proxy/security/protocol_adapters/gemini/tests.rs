//! GE1 test-first preparation; native routes and response/history behavior stay disabled.
//
// Protocol-state table (GE1 observes requests only; GE2/GE3 are not implemented here):
// | State / native index | Supported carriers or transition | Completion / mutation rule |
// | Request | systemInstruction.parts[*].text; original ProtoJSON system_instruction spelling | Inspect text, never rename the native field. |
// | Request contents[*].parts[*] | text; recursive string VALUES in functionCall.args / functionResponse.response objects and arrays | Keep JSON objects, keys, numbers, booleans and nulls; call names/IDs are not text sites. |
// | Candidate[index] / Part ordinal / native call ID if present | Independent text and structured function arguments | Never merge candidates/Parts or invent call IDs; presence of a call is not completion or execution proof. |
// | Active candidate | Missing/unspecified finishReason, content deltas or usage-only frames | Not terminal; metadata-only frames cannot invent successful content. |
// | Natural terminal candidate | STOP (natural stop / stop sequence) | Candidate completion is only a fact; strict snapshots, all relevant candidates and entity EOF are still required. No GE1 proof issuer. |
// | Incomplete / invalid terminal | MAX_TOKENS, MALFORMED_FUNCTION_CALL, UNEXPECTED_TOOL_CALL, TOO_MANY_TOOL_CALLS | No successful record or partial privileged tool release. |
// | Blocked candidate / prompt | SAFETY, RECITATION, LANGUAGE, BLOCKLIST, PROHIBITED_CONTENT, SPII, IMAGE_SAFETY and other image-block reasons; promptFeedback.blockReason | Preserve upstream blocked semantics; not successful privileged tool completion. |
// | Error / unsupported terminal | Native error envelope, OTHER, unknown reasons or contradictory terminal state | Never infer success; GE2 must resolve any additional terminal contract with evidence, not a permissive default. |
// | Transport finalization | Complete JSON entity or complete SSE frames followed by clean entity EOF | EOF alone, truncation, invalid UTF-8/JSON, cancellation or transport failure never prove completion. No Gemini [DONE] sentinel is assumed. |
// | Supported route/framing | POST on existing /v1beta/models/, /gemini/v1beta/models/, /gemini/v1/models/ aliases; :generateContent with absent alt or alt=json; :streamGenerateContent with exactly one alt=sse | Use URI/query, never body.stream; map to existing GenerateContent + Json/Sse enums, not route authority. |
// | Unsupported route/framing | GET/list/count/other methods, unknown aliases, non-generation RPCs, stream without alt=sse, JSON arrays/sequences, ambiguous alt, WebSocket/unlisted framing | No reversible request observation/tokenization; ordinary nonreversible fallback remains an integrator decision. |
// | Signed Part | Any Part carrying thoughtSignature, including readable text and native argument/result string leaves | Entire Part immutable before planning; signature stays opaque and on its original Part. Mandatory patch/authorized-reference conflict fails atomically. |
// | Opaque / unknown | inlineData, fileData, media bytes, signatures, unknown members, model/configuration/tool declarations | No decoding, no new restoration authority, no mutation. |
//
// Evidence (read 2026-11-07; external protocol data, not implementation authority):
// - docs/design/security-dlp-unified-core-protocol-adapters.md sections 4-5;
//   parallel implementation plan GE1; implementation evidence sections 44/46/50.
// - https://github.com/googleapis/googleapis/blob/master/google/ai/generativelanguage/v1beta/generative_service.proto
//   GenerateContent/StreamGenerateContent POST bindings, system_instruction,
//   Candidate.finish_reason (empty means still generating), FinishReason/PromptFeedback.
// - https://github.com/googleapis/googleapis/blob/master/google/ai/generativelanguage/v1beta/content.proto
//   Part.text/thought_signature; FunctionCall.args and FunctionResponse.response are Structs.
// - https://protobuf.dev/programming-guides/json/ specifies lowerCamelCase JSON names
//   and acceptance of original proto field names; test each system spelling separately.
// - https://github.com/googleapis/python-genai/blob/main/google/genai/models.py
//   uses :streamGenerateContent?alt=sse. Existing handler_context.rs::gemini_endpoint
//   fixes the local alias and duplicate/bare-alt boundary. No live provider claim.
use crate::proxy::{protocol_bridge::Protocol, security::restoration_contract::Operation};

const CONTRACT_PROTOCOL: Protocol = Protocol::Gemini;
const CONTRACT_OPERATION: Operation = Operation::GenerateContent;

#[path = "../contract_probe.rs"]
// Each native lane compiles this body against its own protocol constants, so the
// duplicate module is the point rather than an accident.
#[allow(clippy::duplicate_mod)]
mod contract_probe;

use std::cell::Cell;

use http::{Method, Uri};
use serde_json::{json, Value};

use super::request::{classify_generation, observe_request};
use crate::proxy::security::{
    payload_budget::PayloadBudget,
    request_plan::PlanError,
    request_preparation::prepare_observed_request,
    restoration_contract::{
        ClientProtocol, ContinuityScope, EditConstraint, Framing, IngressNamespace,
        RequestObservation, SourceClass,
    },
    runtime::{
        admit_security_runtime, get_security_runtime, set_security_runtime_for_test,
        CompiledCustomRule, SecurityRuntimeConfig,
    },
    tokenization::{DlpRequestLease, DlpTokenStore, TokenError},
    types::{AuditAction, RiskLevel},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const CONTACT: &str = "13900000000";
const JSON_ROUTE: &str = "/v1beta/models/synthetic-model:generateContent";
const SSE_ROUTE: &str = "/v1beta/models/synthetic-model:streamGenerateContent?alt=sse";
const RESTORE_RULES: &[&str] = &["chinese_phone_number"];
const BUDGET_BYTES: usize = 4 * 1024 * 1024;

struct RuntimeGuard(SecurityRuntimeConfig);

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        set_security_runtime_for_test(self.0.clone());
    }
}

fn native_lease(store: &DlpTokenStore) -> TestResult<(RuntimeGuard, DlpRequestLease)> {
    let guard = RuntimeGuard((*get_security_runtime()).clone());
    set_security_runtime_for_test(SecurityRuntimeConfig::default());
    let revision = admit_security_runtime()?.restoration_policy_revision;
    Ok((
        guard,
        store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: ClientProtocol::new(Protocol::Gemini),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-ge1-app",
            Some("synthetic-ge1-session"),
            revision,
        )?,
    ))
}

fn observe<'native>(
    body: &'native Value,
    budget: &PayloadBudget,
) -> TestResult<RequestObservation<'native>> {
    let uri: Uri = JSON_ROUTE.parse()?;
    observe_request(&Method::POST, &uri, body, budget.clone())
        .map_err(|_| "ge1_request_observation_missing".into())
}

fn native_fixture() -> Value {
    json!({
        "systemInstruction":{"parts":[{"text":CONTACT}], "unknown":CONTACT},
        "contents":[
            {"role":"user", "parts":[{"text":CONTACT},
                {"inlineData":{"mimeType":"image/png", "data":CONTACT}},
                {"fileData":{"mimeType":"image/png", "fileUri":"https://synthetic.invalid/media"}},
                {"unknownPart":{"text":CONTACT}}]},
            {"role":"model", "parts":[{"text":CONTACT},
                {"functionCall":{"id":CONTACT, "name":CONTACT,
                    "args":{"top":CONTACT, "nested":[CONTACT,{"note":CONTACT},7,true,null],
                        "count":2,"flag":false,"empty":null}, "unknown":CONTACT}}]},
            {"role":"user", "parts":[{"functionResponse":{"id":CONTACT, "name":CONTACT,
                "response":{"output":{"text":CONTACT}, "rows":[CONTACT,3,null]},
                "unknown":CONTACT}}]}
        ],
        "tools":[{"functionDeclarations":[{"name":CONTACT,"description":CONTACT}]}],
        "generationConfig":{"temperature":0.25,"maxOutputTokens":17},
        "model":"synthetic-model", "stream":true,
        "unknown":{"sameValue":CONTACT,"types":[true,null,7]}
    })
}

// Test-only fixture locations; production writes must use exact borrowed sites, not paths.
fn leaf_specs() -> [(&'static str, SourceClass); 8] {
    [
        ("/systemInstruction/parts/0/text", SourceClass::CurrentInput),
        ("/contents/0/parts/0/text", SourceClass::CurrentInput),
        ("/contents/1/parts/0/text", SourceClass::HistoricalText),
        (
            "/contents/1/parts/1/functionCall/args/top",
            SourceClass::ToolArguments,
        ),
        (
            "/contents/1/parts/1/functionCall/args/nested/0",
            SourceClass::ToolArguments,
        ),
        (
            "/contents/1/parts/1/functionCall/args/nested/1/note",
            SourceClass::ToolArguments,
        ),
        (
            "/contents/2/parts/0/functionResponse/response/output/text",
            SourceClass::ToolResult,
        ),
        (
            "/contents/2/parts/0/functionResponse/response/rows/0",
            SourceClass::ToolResult,
        ),
    ]
}

fn assert_native_shape(candidate: &Value, original: &Value) {
    let mut normalized = candidate.clone();
    for (pointer, _) in leaf_specs() {
        if let Some(leaf) = normalized.pointer_mut(pointer) {
            *leaf = json!(CONTACT);
        }
    }
    assert!(
        normalized == *original,
        "ge1_unselected_native_data_changed"
    );
    assert!(
        candidate["contents"][1]["parts"][1]["functionCall"]["args"].is_object()
            && candidate["contents"][2]["parts"][0]["functionResponse"]["response"].is_object(),
        "ge1_native_objects_stringified"
    );
}

fn signed_parts() -> [Value; 3] {
    [
        json!({"text":CONTACT,"thoughtSignature":"c3ludGhldGlj"}),
        json!({"functionCall":{"name":"synthetic_tool","args":{"nested":[CONTACT,7,true,null]}},
            "thoughtSignature":"c3ludGhldGlj"}),
        json!({"functionResponse":{"name":"synthetic_tool","response":{"output":CONTACT}},
            "thoughtSignature":"c3ludGhldGlj"}),
    ]
}

#[test]
fn ge1_generation_uri_query_selects_json_or_sse() -> TestResult {
    for prefix in [
        "/v1beta/models/",
        "/gemini/v1beta/models/",
        "/gemini/v1/models/",
    ] {
        for (suffix, expected) in [
            (":generateContent", Framing::Json),
            (":generateContent?alt=json", Framing::Json),
            (":generateContent?synthetic=1", Framing::Json),
            (":streamGenerateContent?alt=sse", Framing::Sse),
            (":streamGenerateContent?synthetic=1&alt=sse", Framing::Sse),
            (":streamGenerateContent?alt=sse&synthetic=1", Framing::Sse),
        ] {
            let uri: Uri = format!("{prefix}synthetic-model{suffix}").parse()?;
            assert!(
                matches!(classify_generation(&Method::POST, &uri), Ok((operation, framing))
                    if operation == Operation::GenerateContent && framing == expected),
                "ge1_generation_classification_missing"
            );
        }
    }
    Ok(())
}

#[test]
fn ge1_body_stream_never_selects_generation_framing() -> TestResult {
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    for stream in [
        json!(true),
        json!(false),
        Value::Null,
        json!("synthetic-stream"),
    ] {
        let body = json!({"contents":[{"role":"user","parts":[{"text":"Synthetic text"}]}],
            "stream":stream});
        for (route, expected) in [(JSON_ROUTE, Framing::Json), (SSE_ROUTE, Framing::Sse)] {
            let uri: Uri = route.parse()?;
            assert!(
                matches!(classify_generation(&Method::POST, &uri), Ok((_, framing)) if framing == expected),
                "ge1_body_stream_selected_framing"
            );
            let observed = observe_request(&Method::POST, &uri, &body, budget.clone())
                .map_err(|_| "ge1_body_stream_overrode_uri")?;
            assert!(observed.sites().len() == 1, "ge1_stream_field_became_text");
        }
    }
    assert!(budget.used() == 0, "ge1_stream_observation_charge_retained");
    Ok(())
}

#[test]
fn ge1_non_generation_routes_and_methods_are_unsupported() -> TestResult {
    for (method, route) in [
        (Method::GET, JSON_ROUTE),
        (Method::PUT, JSON_ROUTE),
        (Method::DELETE, SSE_ROUTE),
        (Method::POST, "/v1beta/models"),
        (Method::GET, "/v1beta/models/synthetic-model"),
        (Method::POST, "/v1beta/models/synthetic-model:countTokens"),
        (Method::POST, "/v1beta/models/synthetic-model:embedContent"),
        (
            Method::POST,
            "/v1beta/models/synthetic-model:generateAnswer",
        ),
        (
            Method::POST,
            "/v1beta/models/synthetic-model:unknownGeneration",
        ),
        (Method::POST, "/v1beta/models/:generateContent"),
        (
            Method::POST,
            "/v1beta/models/nested/synthetic-model:generateContent",
        ),
        (
            Method::POST,
            "/v1beta/models/synthetic-model:generateContent/extra",
        ),
        (Method::POST, "/v1/models/synthetic-model:generateContent"),
        (
            Method::POST,
            "/other/v1beta/models/synthetic-model:generateContent",
        ),
    ] {
        let uri: Uri = route.parse()?;
        assert!(
            matches!(
                classify_generation(&method, &uri),
                Err(PlanError::Unsupported)
            ),
            "ge1_unsupported_route_granted_generation"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_unsupported_framing_grants_no_observation_or_mapping() -> TestResult {
    let body = json!({"contents":[{"role":"user","parts":[{"text":CONTACT}]}],"stream":true});
    let store = DlpTokenStore::new();
    let (_runtime, lease) = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    for suffix in [
        ":streamGenerateContent",
        ":streamGenerateContent?alt=json",
        ":streamGenerateContent?alt=",
        ":streamGenerateContent?alt",
        ":streamGenerateContent?alt=sse&alt=json",
        ":streamGenerateContent?alt=sse&alt=sse",
        ":streamGenerateContent?alt=sse&alt",
        ":streamGenerateContent?ALT=sse",
        ":streamGenerateContent?alt=SSE",
        ":streamGenerateContent?alt=json-seq",
        ":generateContent?alt=sse",
        ":generateContent?alt=json&alt=json",
        ":generateContent?alt",
        ":generateContent?alt=json&alt",
    ] {
        let uri: Uri = format!("/v1beta/models/synthetic-model{suffix}").parse()?;
        assert!(
            matches!(
                classify_generation(&Method::POST, &uri),
                Err(PlanError::Unsupported)
            ),
            "ge1_unsupported_framing_classified_supported"
        );
        assert!(
            matches!(
                observe_request(&Method::POST, &uri, &body, budget.clone()),
                Err(PlanError::Unsupported)
            ),
            "ge1_unsupported_framing_granted_observation"
        );
        assert!(
            lease.is_empty() && store.stats()? == before && budget.used() == 0,
            "ge1_unsupported_framing_created_authority_or_charge"
        );
    }
    Ok(())
}

#[test]
fn ge1_system_instruction_spellings_are_native_borrowed_sites() -> TestResult {
    for spelling in ["systemInstruction", "system_instruction"] {
        let mut body = json!({"contents":[], "system":{"text":CONTACT},"unknown":CONTACT});
        body[spelling] = json!({"parts":[{"text":CONTACT},{"inlineData":{"data":CONTACT}}]});
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = observe(&body, &budget)?;
        assert!(
            observed.sites().len() == 1,
            "ge1_system_text_not_selected_exactly"
        );
        let site = observed.sites().site(0).ok_or("ge1_system_site_missing")?;
        assert!(
            std::ptr::eq(site.native(), &body[spelling]["parts"][0]["text"])
                && site.source() == SourceClass::CurrentInput
                && site.constraint() == EditConstraint::Editable,
            "ge1_system_native_site_changed"
        );
        drop(observed);
        assert!(budget.used() == 0, "ge1_system_observation_charge_retained");
    }
    Ok(())
}

#[test]
fn ge1_contents_parts_and_recursive_function_leaves_are_source_classified() -> TestResult {
    let body = native_fixture();
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    assert!(
        std::ptr::eq(observed.native(), &body) && observed.client().protocol() == Protocol::Gemini,
        "ge1_native_request_owner_or_protocol_lost"
    );
    assert!(
        observed.sites().len() == leaf_specs().len(),
        "ge1_native_leaf_count_changed"
    );
    for (pointer, source) in leaf_specs() {
        let node = body.pointer(pointer).ok_or("ge1_fixture_leaf_missing")?;
        let found = (0..observed.sites().len()).any(|index| {
            observed.sites().site(index).is_some_and(|site| {
                std::ptr::eq(site.native(), node)
                    && site.source() == source
                    && site.constraint() == EditConstraint::Editable
            })
        });
        assert!(found, "ge1_native_leaf_source_or_identity_missing");
    }
    assert!(body == original, "ge1_observation_mutated_native_request");
    Ok(())
}

#[test]
fn ge1_observation_metadata_uses_supplied_budget_and_refunds() -> TestResult {
    let body = native_fixture();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    assert!(
        budget.used() > 0 && observed.budget().used() == budget.used(),
        "ge1_observation_not_charged_to_supplied_owner"
    );
    drop(observed);
    assert!(budget.used() == 0, "ge1_observation_charge_retained");
    let tiny = PayloadBudget::with_limit(1);
    assert!(
        matches!(
            observe_request(&Method::POST, &JSON_ROUTE.parse()?, &body, tiny.clone()),
            Err(PlanError::Token(TokenError::CapacityExceeded))
        ),
        "ge1_metadata_capacity_not_enforced"
    );
    assert!(tiny.used() == 0, "ge1_denied_metadata_charge_retained");
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_current_mandatory_redaction_changes_only_native_string_leaves() -> TestResult {
    let body = native_fixture();
    let original = body.clone();
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    let baseline = budget.used();
    let prepared = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        &[],
        &[],
        |candidate, encoded, proof| {
            assert_native_shape(candidate, &original);
            assert!(
                !encoded.is_empty() && proof.is_empty(),
                "ge1_redaction_encoding_or_proof_invalid"
            );
            for (pointer, _) in leaf_specs() {
                assert!(
                    candidate
                        .pointer(pointer)
                        .and_then(Value::as_str)
                        .is_some_and(|text| text != CONTACT),
                    "ge1_mandatory_native_redaction_missing"
                );
            }
            Ok(())
        },
    )
    .map_err(|_| "ge1_mandatory_native_redaction_failed")?;
    assert!(
        prepared.irreversible_occurrences == 8 && prepared.reversible_occurrences == 0,
        "ge1_mandatory_redaction_count_changed"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "ge1_redaction_granted_mapping"
    );
    assert!(body == original, "ge1_redaction_mutated_source");
    drop(prepared);
    assert!(
        budget.used() == baseline,
        "ge1_redaction_prepared_charge_retained"
    );
    drop(observed);
    assert!(
        budget.used() == 0,
        "ge1_redaction_observation_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_reversible_preparation_preserves_native_objects_types_and_opaque_fields() -> TestResult {
    let body = native_fixture();
    let original = body.clone();
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    let baseline = budget.used();
    let prepared = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, encoded, proof| {
            assert_native_shape(candidate, &original);
            assert!(
                proof.len() == 1 && !encoded.is_empty(),
                "ge1_native_candidate_not_encoded"
            );
            assert!(
                store.stats()?.entries == before.entries,
                "ge1_native_mapping_committed_before_validation"
            );
            for (pointer, _) in leaf_specs() {
                assert!(
                    candidate.pointer(pointer).and_then(Value::as_str) == Some(proof[0]),
                    "ge1_native_reference_not_applied_to_exact_leaf"
                );
            }
            Ok(())
        },
    )
    .map_err(|_| "ge1_native_reversible_preparation_failed")?;
    let reference = prepared.body.as_value()["contents"][0]["parts"][0]["text"]
        .as_str()
        .ok_or("ge1_native_reference_missing")?;
    assert!(
        lease.resolve(reference)? == CONTACT,
        "ge1_current_mapping_not_authorized"
    );
    assert!(
        prepared.reversible_occurrences == 8
            && prepared.irreversible_occurrences == 0
            && store.stats()?.entries == before.entries + 1,
        "ge1_native_occurrence_or_deduplication_changed"
    );
    assert!(
        body == original && budget.used() > baseline,
        "ge1_native_owner_not_preserved"
    );
    drop(prepared);
    assert!(
        budget.used() == baseline,
        "ge1_native_prepared_charge_retained"
    );
    drop(observed);
    assert!(budget.used() == 0, "ge1_native_observation_charge_retained");
    Ok(())
}

#[test]
fn ge1_signed_parts_keep_readable_sites_immutable_and_signature_opaque() -> TestResult {
    for part in signed_parts() {
        let body = json!({"contents":[{"role":"model","parts":[part]}]});
        let original = body.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = observe(&body, &budget)?;
        assert!(
            observed.sites().len() == 1,
            "ge1_signed_readable_leaf_missing_or_signature_scanned"
        );
        let site = observed.sites().site(0).ok_or("ge1_signed_site_missing")?;
        let part = &body["contents"][0]["parts"][0];
        let (node, source) = if part.get("text").is_some() {
            (&part["text"], SourceClass::HistoricalText)
        } else if part.get("functionCall").is_some() {
            (
                &part["functionCall"]["args"]["nested"][0],
                SourceClass::ToolArguments,
            )
        } else {
            (
                &part["functionResponse"]["response"]["output"],
                SourceClass::ToolResult,
            )
        };
        assert!(
            std::ptr::eq(site.native(), node)
                && site.source() == source
                && site.constraint() == EditConstraint::Immutable,
            "ge1_signed_native_leaf_not_immutable"
        );
        assert!(
            body == original,
            "ge1_signed_unit_mutated_during_observation"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_signed_mandatory_patch_conflict_rolls_back_all_native_edits() -> TestResult {
    for part in signed_parts() {
        for restore_rules in [&[][..], RESTORE_RULES] {
            let body = json!({"contents":[
                {"role":"user","parts":[{"text":CONTACT}]},
                {"role":"model","parts":[part.clone()]}]});
            let original = body.clone();
            let store = DlpTokenStore::new();
            let (_runtime, mut lease) = native_lease(&store)?;
            let before = store.stats()?;
            let budget = PayloadBudget::with_limit(BUDGET_BYTES);
            let observed = observe(&body, &budget)?;
            let baseline = budget.used();
            let called = Cell::new(false);
            let result = prepare_observed_request(
                &mut lease,
                &observed,
                &SecurityRuntimeConfig::default(),
                restore_rules,
                &[],
                |_, _, _| {
                    called.set(true);
                    Ok(())
                },
            );
            assert!(
                matches!(result, Err(PlanError::ImmutableContent)),
                "ge1_signed_mandatory_patch_not_rejected"
            );
            assert!(!called.get(), "ge1_signed_conflict_reached_validator");
            assert!(
                lease.is_empty() && store.stats()? == before && body == original,
                "ge1_signed_conflict_committed_partial_edit_or_mapping"
            );
            assert!(
                budget.used() == baseline,
                "ge1_signed_conflict_charge_retained"
            );
            drop(observed);
            assert!(
                budget.used() == 0,
                "ge1_signed_conflict_observation_retained"
            );
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_unchanged_signed_parts_and_opaque_signature_preserve_native_units() -> TestResult {
    // The signature deliberately resembles an inspectable match; it must stay opaque.
    let body = json!({"contents":[{"role":"model","parts":[
        {"text":"Synthetic signed text","thoughtSignature":CONTACT},
        {"functionCall":{"name":"synthetic_tool","args":{"nested":["Synthetic argument",7,true,null]}},
            "thoughtSignature":CONTACT},
        {"functionResponse":{"name":"synthetic_tool","response":{"output":"Synthetic result"}},
            "thoughtSignature":CONTACT},
        {"inlineData":{"mimeType":"image/png","data":CONTACT},"thoughtSignature":CONTACT}
    ]}]});
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    assert!(
        observed.sites().len() == 3,
        "ge1_signed_opaque_fields_scanned"
    );
    let baseline = budget.used();
    let prepared = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, encoded, proof| {
            let decoded: Value =
                serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
            assert!(
                candidate == &body && decoded == body && proof.is_empty(),
                "ge1_unchanged_signed_unit_rebuilt_incorrectly"
            );
            Ok(())
        },
    )
    .map_err(|_| "ge1_unchanged_signed_request_rejected")?;
    assert!(
        prepared.body.as_value() == &body,
        "ge1_signed_signature_association_changed"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "ge1_signed_opaque_data_granted_authority"
    );
    drop(prepared);
    assert!(
        budget.used() == baseline,
        "ge1_unchanged_signed_prepared_charge_retained"
    );
    drop(observed);
    assert!(
        budget.used() == 0,
        "ge1_unchanged_signed_observation_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_signed_authorized_reference_conflict_preserves_existing_lease_only() -> TestResult {
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let seed = json!({"contents":[{"role":"user","parts":[{"text":CONTACT}]}]});
    let seed_observed = observe(&seed, &budget)?;
    let prepared = prepare_observed_request(
        &mut lease,
        &seed_observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| Ok(()),
    )
    .map_err(|_| "ge1_current_mapping_seed_failed")?;
    let reference = prepared.body.as_value()["contents"][0]["parts"][0]["text"]
        .as_str()
        .ok_or("ge1_current_mapping_seed_missing")?
        .to_owned();
    assert!(
        lease.resolve(&reference)? == CONTACT,
        "ge1_current_mapping_seed_unauthorized"
    );
    drop(prepared);
    drop(seed_observed);
    assert!(
        budget.used() == 0,
        "ge1_current_mapping_seed_charge_retained"
    );
    let body = json!({"contents":[
        {"role":"user","parts":[{"text":"13800000000"}]},
        {"role":"model","parts":[{"functionCall":{"name":"synthetic_tool","args":{"value":reference}},
            "thoughtSignature":"c3ludGhldGlj"}]}]});
    let original = body.clone();
    let before = store.stats()?;
    let observed = observe(&body, &budget)?;
    let baseline = budget.used();
    let called = Cell::new(false);
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(result, Err(PlanError::ImmutableContent)),
        "ge1_signed_authorized_reference_not_rejected"
    );
    assert!(
        !called.get()
            && store.stats()? == before
            && body == original
            && lease.resolve(&reference)? == CONTACT
            && budget.used() == baseline,
        "ge1_signed_reference_conflict_not_atomic"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_final_validation_failure_rolls_back_provisional_native_mappings() -> TestResult {
    let body = native_fixture();
    let original = body.clone();
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    let baseline = budget.used();
    let called = Cell::new(false);
    let result = prepare_observed_request::<()>(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, encoded, proof| {
            called.set(true);
            assert_native_shape(candidate, &original);
            assert!(
                proof.len() == 1 && !encoded.is_empty() && store.stats()?.entries == before.entries,
                "ge1_native_validation_not_precommit"
            );
            Err(TokenError::RedactionFailed)
        },
    );
    assert!(called.get(), "ge1_native_final_validation_not_reached");
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::RedactionFailed))),
        "ge1_native_validation_failure_reclassified"
    );
    assert!(
        lease.is_empty()
            && store.stats()? == before
            && body == original
            && budget.used() == baseline,
        "ge1_failed_native_validation_committed_partial_state"
    );
    drop(observed);
    assert!(
        budget.used() == 0,
        "ge1_failed_native_validation_charge_retained"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_explicit_block_on_signed_readable_text_preserves_policy_precedence() -> TestResult {
    let body = json!({"contents":[{"role":"model","parts":[
        {"text":CONTACT,"thoughtSignature":"c3ludGhldGlj"}]}]});
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![CompiledCustomRule {
            id: "synthetic-ge1-block".into(),
            name: "Synthetic GE1 block".into(),
            rule_type: "regex".into(),
            pattern: CONTACT.into(),
            regex: Some(regex::Regex::new(CONTACT)?),
            risk_level: RiskLevel::High,
            action: AuditAction::Block,
            enabled: true,
        }],
        ..Default::default()
    };
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    let baseline = budget.used();
    let called = Cell::new(false);
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &runtime,
        RESTORE_RULES,
        &[],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(result, Err(PlanError::Blocked)),
        "ge1_signed_readable_block_lost_precedence"
    );
    assert!(
        !called.get() && lease.is_empty() && store.stats()? == before && budget.used() == baseline,
        "ge1_signed_block_advanced_transaction"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_resource_metadata_byte_boundary_uses_exact_borrowed_owner_and_refunds() -> TestResult {
    let body = json!({"contents":[{"role":"user","parts":[{"text":"Synthetic text"}]}]});
    let original = body.clone();
    let store = DlpTokenStore::new();
    let (_runtime, lease) = native_lease(&store)?;
    let before = store.stats()?;
    let uri: Uri = JSON_ROUTE.parse()?;
    let probe = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &probe)?;
    let metadata_bytes = probe.used();
    assert!(metadata_bytes > 0, "ge1_resource_metadata_not_charged");
    assert!(
        std::ptr::eq(observed.native(), &body)
            && observed.sites().len() == 1
            && observed.sites().site(0).is_some_and(|site| {
                std::ptr::eq(site.native(), &body["contents"][0]["parts"][0]["text"])
            }),
        "ge1_resource_metadata_native_owner_lost"
    );
    drop(observed);
    assert!(
        probe.used() == 0,
        "ge1_resource_metadata_probe_charge_retained"
    );

    let exact = PayloadBudget::with_limit(metadata_bytes);
    let observed = observe(&body, &exact)?;
    assert!(
        exact.used() == metadata_bytes && observed.budget().used() == exact.used(),
        "ge1_resource_metadata_exact_budget_not_used"
    );
    drop(observed);
    assert!(
        exact.used() == 0,
        "ge1_resource_metadata_exact_charge_retained"
    );

    let denied = PayloadBudget::with_limit(metadata_bytes - 1);
    assert!(
        matches!(
            observe_request(&Method::POST, &uri, &body, denied.clone()),
            Err(PlanError::Token(TokenError::CapacityExceeded))
        ),
        "ge1_resource_metadata_byte_limit_not_enforced"
    );
    assert!(
        denied.used() == 0 && lease.is_empty() && store.stats()? == before && body == original,
        "ge1_resource_metadata_failure_changed_owner_or_store"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_resource_site_count_accepts_4096_then_rejects_and_refunds() -> TestResult {
    let mut body = json!({"contents":[{"role":"user","parts":
        vec![json!({"text":"Synthetic text"}); 4096]}]});
    let original = body.clone();
    let store = DlpTokenStore::new();
    let (_runtime, lease) = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    assert!(
        std::ptr::eq(observed.native(), &body) && observed.sites().len() == 4096,
        "ge1_resource_site_limit_rejected_supported_count"
    );
    for index in 0..4096 {
        assert!(
            observed.sites().site(index).is_some_and(|site| {
                std::ptr::eq(site.native(), &body["contents"][0]["parts"][index]["text"])
                    && site.source() == SourceClass::CurrentInput
                    && site.constraint() == EditConstraint::Editable
            }),
            "ge1_resource_site_identity_or_classification_lost"
        );
    }
    assert!(
        budget.used() > 0 && body == original,
        "ge1_resource_site_owner_not_preserved"
    );
    drop(observed);
    assert!(
        budget.used() == 0,
        "ge1_resource_site_boundary_charge_retained"
    );

    body["contents"][0]["parts"]
        .as_array_mut()
        .ok_or("ge1_resource_parts_fixture_missing")?
        .push(json!({"text":"Synthetic overflow text"}));
    let original = body.clone();
    let uri: Uri = JSON_ROUTE.parse()?;
    assert!(
        matches!(
            observe_request(&Method::POST, &uri, &body, budget.clone()),
            Err(PlanError::Token(TokenError::CapacityExceeded))
        ),
        "ge1_resource_site_count_limit_not_enforced"
    );
    assert!(
        budget.used() == 0 && lease.is_empty() && store.stats()? == before && body == original,
        "ge1_resource_site_overflow_changed_owner_or_store"
    );
    Ok(())
}

// Synthetic native Struct values alternate object and array containers; only the leaf is text.
fn recursive_native_payload(depth: usize) -> Value {
    let mut value = json!("Synthetic recursive text");
    for level in 0..depth {
        value = if level % 2 == 0 {
            json!({"synthetic_child":value})
        } else {
            json!([value, 7, true, null])
        };
    }
    json!({"payload":value})
}

fn recursive_native_leaf(payload: &Value, depth: usize) -> TestResult<&Value> {
    let mut leaf = payload
        .get("payload")
        .ok_or("ge1_resource_payload_fixture_missing")?;
    for level in (0..depth).rev() {
        leaf = if level % 2 == 0 {
            leaf.get("synthetic_child")
        } else {
            leaf.get(0)
        }
        .ok_or("ge1_resource_recursive_fixture_missing")?;
    }
    Ok(leaf)
}

#[test]
#[serial_test::serial]
fn ge1_resource_within_limit_recursive_values_remain_exact_borrowed_sites() -> TestResult {
    // Native root -> contents -> content -> parts -> Part -> call/result -> Struct -> payload.
    // Forty-eight additional containers leave the string below the depth-64 boundary.
    for (carrier, field, source) in [
        ("functionCall", "args", SourceClass::ToolArguments),
        ("functionResponse", "response", SourceClass::ToolResult),
    ] {
        let mut part = json!({});
        part[carrier] = json!({"name":"synthetic_tool"});
        part[carrier][field] = recursive_native_payload(48);
        let body = json!({"contents":[{"role":"model","parts":[part]}]});
        let original = body.clone();
        let store = DlpTokenStore::new();
        let (_runtime, lease) = native_lease(&store)?;
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = observe(&body, &budget)?;
        let leaf = recursive_native_leaf(&body["contents"][0]["parts"][0][carrier][field], 48)?;
        assert!(
            std::ptr::eq(observed.native(), &body)
                && observed.sites().len() == 1
                && observed.sites().site(0).is_some_and(|site| {
                    std::ptr::eq(site.native(), leaf)
                        && site.source() == source
                        && site.constraint() == EditConstraint::Editable
                }),
            "ge1_resource_within_limit_recursive_site_lost"
        );
        assert!(
            budget.used() > 0 && body == original && lease.is_empty() && store.stats()? == before,
            "ge1_resource_within_limit_changed_owner_or_store"
        );
        drop(observed);
        assert!(budget.used() == 0, "ge1_resource_recursive_charge_retained");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_resource_excessive_native_depth_fails_atomically_and_refunds() -> TestResult {
    for (carrier, field) in [("functionCall", "args"), ("functionResponse", "response")] {
        let mut part = json!({});
        part[carrier] = json!({"name":"synthetic_tool"});
        part[carrier][field] = recursive_native_payload(64);
        let body = json!({"contents":[{"role":"user","parts":[
            {"text":"Synthetic first text"}, part]}]});
        let original = body.clone();
        let store = DlpTokenStore::new();
        let (_runtime, lease) = native_lease(&store)?;
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let uri: Uri = JSON_ROUTE.parse()?;
        assert!(
            matches!(
                observe_request(&Method::POST, &uri, &body, budget.clone()),
                Err(PlanError::Token(TokenError::CapacityExceeded))
            ),
            "ge1_resource_native_depth_limit_not_enforced"
        );
        assert!(
            budget.used() == 0 && lease.is_empty() && store.stats()? == before && body == original,
            "ge1_resource_depth_failure_changed_owner_or_store"
        );
    }
    Ok(())
}

// Reuse the signed fixtures and current native admission; never fabricate reference authority.
const GE1_SIGNATURE_ALIAS_LEAVES: [(&str, SourceClass); 3] = [
    ("/text", SourceClass::HistoricalText),
    ("/functionCall/args/nested/0", SourceClass::ToolArguments),
    (
        "/functionResponse/response/output/nested/0",
        SourceClass::ToolResult,
    ),
];

fn signature_alias_parts(signature_field: &str) -> TestResult<[Value; 3]> {
    let mut parts = signed_parts();
    for part in &mut parts {
        let fields = part
            .as_object_mut()
            .ok_or("ge1_signature_alias_part_fixture_missing")?;
        let signature = fields
            .remove("thoughtSignature")
            .ok_or("ge1_signature_alias_signature_fixture_missing")?;
        fields.insert(signature_field.to_owned(), signature);
    }
    // Exercise result strings through both an object and an array, like native arguments.
    let output = parts[2]["functionResponse"]["response"]["output"].take();
    parts[2]["functionResponse"]["response"]["output"] = json!({"nested":[output,7,true,null]});
    Ok(parts)
}

#[test]
fn ge1_signature_alias_original_spelling_observes_signed_leaves_immutable() -> TestResult {
    let body =
        json!({"contents":[{"role":"model","parts":signature_alias_parts("thought_signature")?}]});
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    assert!(
        observed.sites().len() == GE1_SIGNATURE_ALIAS_LEAVES.len(),
        "ge1_signature_alias_readable_leaf_missing_or_signature_scanned"
    );
    for (part_index, (pointer, source)) in GE1_SIGNATURE_ALIAS_LEAVES.into_iter().enumerate() {
        let node = body["contents"][0]["parts"][part_index]
            .pointer(pointer)
            .ok_or("ge1_signature_alias_leaf_fixture_missing")?;
        assert!(
            (0..observed.sites().len()).any(|index| {
                observed.sites().site(index).is_some_and(|site| {
                    std::ptr::eq(site.native(), node)
                        && site.source() == source
                        && site.constraint() == EditConstraint::Immutable
                })
            }),
            "ge1_signature_alias_signed_leaf_not_immutable_from_outset"
        );
    }
    assert!(
        body == original,
        "ge1_signature_alias_observation_mutated_source"
    );
    drop(observed);
    assert!(
        budget.used() == 0,
        "ge1_signature_alias_observation_charge_retained"
    );
    Ok(())
}

fn assert_signature_alias_mandatory_conflict(
    part_index: usize,
    restore_rules: &[&str],
    signature_field: &str,
) -> TestResult {
    let part = signature_alias_parts(signature_field)?[part_index].clone();
    let body = json!({"contents":[
        {"role":"user","parts":[{"text":"13800000000"}]},
        {"role":"model","parts":[part]}]});
    let original = body.clone();
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observe(&body, &budget)?;
    let baseline = budget.used();
    let called = Cell::new(false);
    // Reach the real shared preparation before asserting editability or failure.
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        restore_rules,
        &[],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(result, Err(PlanError::ImmutableContent)),
        "ge1_signature_alias_mandatory_conflict_not_typed_unsupported"
    );
    assert!(
        !called.get(),
        "ge1_signature_alias_mandatory_conflict_reached_validator"
    );
    assert!(
        lease.is_empty() && store.stats()? == before && body == original,
        "ge1_signature_alias_mandatory_conflict_committed_sibling_or_mapping"
    );
    assert!(
        budget.used() == baseline,
        "ge1_signature_alias_mandatory_conflict_charge_retained"
    );
    drop(observed);
    assert!(
        budget.used() == 0,
        "ge1_signature_alias_mandatory_observation_charge_retained"
    );
    Ok(())
}

#[test]
fn ge1_signature_alias_text_mandatory_redact_fails_before_commit() -> TestResult {
    assert_signature_alias_mandatory_conflict(0, &[], "thought_signature")
}

#[test]
fn ge1_signature_alias_text_mandatory_tokenize_fails_before_commit() -> TestResult {
    assert_signature_alias_mandatory_conflict(0, RESTORE_RULES, "thought_signature")
}

#[test]
fn ge1_signature_alias_function_call_args_mandatory_redact_fails_before_commit() -> TestResult {
    assert_signature_alias_mandatory_conflict(1, &[], "thought_signature")
}

#[test]
fn ge1_signature_alias_function_call_args_mandatory_tokenize_fails_before_commit() -> TestResult {
    assert_signature_alias_mandatory_conflict(1, RESTORE_RULES, "thought_signature")
}

#[test]
fn ge1_signature_alias_function_response_mandatory_redact_fails_before_commit() -> TestResult {
    assert_signature_alias_mandatory_conflict(2, &[], "thought_signature")
}

#[test]
fn ge1_signature_alias_function_response_mandatory_tokenize_fails_before_commit() -> TestResult {
    assert_signature_alias_mandatory_conflict(2, RESTORE_RULES, "thought_signature")
}

fn assert_signature_alias_authorized_reference_conflict(
    part_index: usize,
    signature_field: &str,
) -> TestResult {
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let seed = json!({"contents":[{"role":"user","parts":[{"text":CONTACT}]}]});
    let seed_observed = observe(&seed, &budget)?;
    let prepared = prepare_observed_request(
        &mut lease,
        &seed_observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| Ok(()),
    )
    .map_err(|_| "ge1_signature_alias_current_mapping_seed_failed")?;
    let reference = prepared.body.as_value()["contents"][0]["parts"][0]["text"]
        .as_str()
        .ok_or("ge1_signature_alias_current_mapping_seed_missing")?
        .to_owned();
    drop(prepared);
    drop(seed_observed);

    let mut part = signature_alias_parts(signature_field)?[part_index].clone();
    let (pointer, _) = GE1_SIGNATURE_ALIAS_LEAVES[part_index];
    *part
        .pointer_mut(pointer)
        .ok_or("ge1_signature_alias_reference_leaf_fixture_missing")? = json!(reference);
    let body = json!({"contents":[
        {"role":"user","parts":[{"text":"13800000000"}]},
        {"role":"model","parts":[part]}]});
    let original = body.clone();
    let before = store.stats()?;
    let observed = observe(&body, &budget)?;
    let baseline = budget.used();
    let called = Cell::new(false);
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(result, Err(PlanError::ImmutableContent)),
        "ge1_signature_alias_authorized_reference_not_typed_unsupported"
    );
    assert!(
        !called.get()
            && store.stats()? == before
            && body == original
            && lease.resolve(&reference)? == CONTACT
            && budget.used() == baseline,
        "ge1_signature_alias_reference_conflict_not_atomic"
    );
    let mut owned = 0;
    lease.visit_authorized_tokens(|known| {
        assert!(
            known == reference,
            "ge1_signature_alias_reference_conflict_expanded_lease"
        );
        owned += 1;
        Ok(())
    })?;
    assert!(
        owned == 1,
        "ge1_signature_alias_reference_conflict_changed_exact_ownership"
    );
    drop(observed);
    assert!(
        budget.used() == 0,
        "ge1_signature_alias_reference_observation_charge_retained"
    );
    Ok(())
}

#[test]
fn ge1_signature_alias_text_authorized_reference_fails_preserving_exact_lease() -> TestResult {
    assert_signature_alias_authorized_reference_conflict(0, "thought_signature")
}

#[test]
fn ge1_signature_alias_function_call_args_authorized_reference_fails_preserving_exact_lease(
) -> TestResult {
    assert_signature_alias_authorized_reference_conflict(1, "thought_signature")
}

#[test]
fn ge1_signature_alias_function_response_authorized_reference_fails_preserving_exact_lease(
) -> TestResult {
    assert_signature_alias_authorized_reference_conflict(2, "thought_signature")
}

#[test]
#[serial_test::serial]
fn ge1_signature_alias_unsigned_carriers_still_redact_and_tokenize() -> TestResult {
    for (part_index, mut part) in signature_alias_parts("thought_signature")?
        .into_iter()
        .enumerate()
    {
        part.as_object_mut()
            .ok_or("ge1_signature_alias_unsigned_part_fixture_missing")?
            .remove("thought_signature");
        for restore_rules in [&[][..], RESTORE_RULES] {
            let body = json!({"contents":[{"role":"model","parts":[part.clone()]}]});
            let original = body.clone();
            let store = DlpTokenStore::new();
            let (_runtime, mut lease) = native_lease(&store)?;
            let before = store.stats()?;
            let budget = PayloadBudget::with_limit(BUDGET_BYTES);
            let observed = observe(&body, &budget)?;
            let baseline = budget.used();
            let prepared = prepare_observed_request(
                &mut lease,
                &observed,
                &SecurityRuntimeConfig::default(),
                restore_rules,
                &[],
                |_, _, _| Ok(()),
            )
            .map_err(|_| "ge1_signature_alias_unsigned_preparation_failed")?;
            let (pointer, _) = GE1_SIGNATURE_ALIAS_LEAVES[part_index];
            let replacement = prepared.body.as_value()["contents"][0]["parts"][0]
                .pointer(pointer)
                .and_then(Value::as_str)
                .ok_or("ge1_signature_alias_unsigned_replacement_missing")?;
            assert!(
                replacement != CONTACT,
                "ge1_signature_alias_unsigned_mandatory_patch_missing"
            );
            if restore_rules.is_empty() {
                assert!(
                    prepared.irreversible_occurrences == 1
                        && prepared.reversible_occurrences == 0
                        && lease.is_empty()
                        && store.stats()? == before,
                    "ge1_signature_alias_unsigned_redaction_granted_authority"
                );
            } else {
                assert!(
                    prepared.reversible_occurrences == 1
                        && prepared.irreversible_occurrences == 0
                        && lease.resolve(replacement)? == CONTACT
                        && store.stats()?.entries == before.entries + 1,
                    "ge1_signature_alias_unsigned_tokenization_not_authorized"
                );
            }
            let mut normalized = prepared.body.as_value().clone();
            *normalized["contents"][0]["parts"][0]
                .pointer_mut(pointer)
                .ok_or("ge1_signature_alias_unsigned_native_leaf_missing")? = json!(CONTACT);
            assert!(
                normalized == original && body == original,
                "ge1_signature_alias_unsigned_unselected_native_data_changed"
            );
            drop(prepared);
            assert!(
                budget.used() == baseline,
                "ge1_signature_alias_unsigned_prepared_charge_retained"
            );
            drop(observed);
            assert!(
                budget.used() == 0,
                "ge1_signature_alias_unsigned_observation_charge_retained"
            );
        }
    }
    Ok(())
}

#[test]
fn ge1_signature_alias_camel_case_conflicts_remain_fail_closed() -> TestResult {
    for part_index in 0..GE1_SIGNATURE_ALIAS_LEAVES.len() {
        for restore_rules in [&[][..], RESTORE_RULES] {
            assert_signature_alias_mandatory_conflict(
                part_index,
                restore_rules,
                "thoughtSignature",
            )?;
        }
        assert_signature_alias_authorized_reference_conflict(part_index, "thoughtSignature")?;
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ge1_signature_alias_unchanged_units_keep_both_signature_spellings_opaque() -> TestResult {
    for signature_field in ["thought_signature", "thoughtSignature"] {
        let mut parts = signature_alias_parts(signature_field)?.to_vec();
        for (part, (pointer, _)) in parts.iter_mut().zip(GE1_SIGNATURE_ALIAS_LEAVES) {
            *part
                .pointer_mut(pointer)
                .ok_or("ge1_signature_alias_unchanged_leaf_fixture_missing")? =
                json!("Synthetic unchanged readable text");
            part[signature_field] = json!(CONTACT);
        }
        let mut opaque = json!({"inlineData":{"mimeType":"image/png","data":CONTACT}});
        opaque[signature_field] = json!(CONTACT);
        parts.push(opaque);
        let body = json!({"contents":[{"role":"model","parts":parts}]});
        let original = body.clone();
        let store = DlpTokenStore::new();
        let (_runtime, mut lease) = native_lease(&store)?;
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = observe(&body, &budget)?;
        let baseline = budget.used();
        let prepared = prepare_observed_request(
            &mut lease,
            &observed,
            &SecurityRuntimeConfig::default(),
            RESTORE_RULES,
            &[],
            |candidate, encoded, proof| {
                let decoded: Value =
                    serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
                assert!(
                    candidate == &original && decoded == original && proof.is_empty(),
                    "ge1_signature_alias_opaque_unit_changed_during_preparation"
                );
                Ok(())
            },
        )
        .map_err(|_| "ge1_signature_alias_unchanged_signed_preparation_failed")?;
        assert!(
            observed.sites().len() == GE1_SIGNATURE_ALIAS_LEAVES.len()
                && prepared.reversible_occurrences == 0
                && prepared.irreversible_occurrences == 0
                && prepared.body.as_value() == &original
                && body == original
                && lease.is_empty()
                && store.stats()? == before,
            "ge1_signature_alias_opaque_signature_scanned_or_reassociated"
        );
        drop(prepared);
        assert!(
            budget.used() == baseline,
            "ge1_signature_alias_unchanged_prepared_charge_retained"
        );
        drop(observed);
        assert!(
            budget.used() == 0,
            "ge1_signature_alias_unchanged_observation_charge_retained"
        );
    }
    Ok(())
}

// GE2/GE3 protocol-state table (conservative S8 subset):
// | Indexed state | Supported text carriers | Terminal, transport and history rule |
// | candidate.index / Content / Part ordinal | Part.text; recursive decoded string leaves in functionCall.args | Every candidate has one explicit unique index; candidates retain first-appearance order and whole Contents append in arrival order. |
// | Active candidate | Content with absent/null finishReason; usage-only frame only when candidates is absent | Not terminal; present null/object/string/number/empty candidates are malformed. |
// | Completed candidate | Consistent STOP after complete native Content/Part data | STOP alone is insufficient: real JSON/SSE entity EOF and sealed finalization remain mandatory; Gemini has no [DONE]. |
// | Rejected response | Post-STOP content/duplicate/contradictory reason, MAX_TOKENS, blocked/error/unknown/missing/malformed terminal or Content/Part | No staged edits, delivered history, privileged call or completed record. |
// | Signed Part | Existing thoughtSignature remains on its exact Part; readable text/argument leaves are immutable | No late signature back-attachment, cross-frame Part merge, manufactured Part ID or repeated-position identity. |
// | GE3 delivered history | Exact ordered delivered Content snapshots and native call/result structural claims | Native IDs when present; only unambiguous native ordering without IDs. Results stay untrusted; no invented IDs/cursors or cross-binding privilege. |
// | Held/unsupported | Fragmented logical Parts, late empty-text signature association, ambiguous same-name/parallel pairing, unknown-reference privilege | Preserve a reproducer and require a scoped decision rather than expanding semantics. |

struct Ge2Fixture {
    _runtime: RuntimeGuard,
    store: DlpTokenStore,
    lease: DlpRequestLease,
    budget: PayloadBudget,
    attempt: crate::proxy::security::restoration_core::AttemptProvenance,
    binding: crate::proxy::security::completed_context::ActualAttemptBinding,
    reference: String,
}

impl Ge2Fixture {
    fn new(mapped: bool) -> TestResult<Self> {
        let store = DlpTokenStore::new();
        let (runtime, mut lease) = native_lease(&store)?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let text = if mapped {
            CONTACT
        } else {
            "Synthetic public Gemini text"
        };
        let request = json!({"contents":[{"role":"user","parts":[{"text":text}]}],
            "opaque":{"sameValue":text,"types":[true,null,7]}});
        let observed = observe(&request, &budget)?;
        let prepared = prepare_observed_request(
            &mut lease,
            &observed,
            &SecurityRuntimeConfig::default(),
            if mapped { RESTORE_RULES } else { &[] },
            &[],
            |candidate, encoded, proof| {
                let decoded: Value =
                    serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
                assert!(decoded == *candidate && proof.len() == usize::from(mapped));
                Ok(())
            },
        )
        .map_err(|_| "ge2_fixture_request_rejected")?;
        let reference = prepared.body.as_value()["contents"][0]["parts"][0]["text"]
            .as_str()
            .ok_or("ge2_fixture_reference_missing")?
            .to_owned();
        assert!(prepared.reversible_occurrences == usize::from(mapped));
        drop(prepared);
        drop(observed);
        let binding = lease.bind_actual_attempt(
            "synthetic-ge2-provider",
            "gemini",
            "https://synthetic.invalid/v1beta/models/synthetic:generateContent",
            Some("synthetic-ge2-principal"),
        )?;
        let attempt = crate::proxy::security::restoration_core::AttemptProvenance::new(&budget)?;
        Ok(Self {
            _runtime: runtime,
            store,
            lease,
            budget,
            attempt,
            binding,
            reference,
        })
    }

    fn counts(&self) -> TestResult<(usize, usize, usize)> {
        let (published, pending, pins, ..) =
            crate::proxy::security::tokenization::publication_test_support::counts(&self.store)?;
        Ok((published, pending, pins))
    }
}

fn ge2_lane_json_source(reference: &str) -> Value {
    json!({"candidates":[{"index":7,"content":{"role":"model","parts":[
        {"text":reference}
    ],"opaque":{"types":[true,null,7]}},"finishReason":"STOP",
        "candidateOpaque":{"sameValue":reference}}],
        "usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":2,"totalTokenCount":3},
        "responseId":"synthetic-ge2-response","extension":{"preserve":true}})
}

#[test]
fn ge2_lane_json_text_exact_native_restoration() -> TestResult {
    let fixture = Ge2Fixture::new(true)?;
    let source = ge2_lane_json_source(&fixture.reference);
    let original = source.clone();
    let staged = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "ge2_json_supported_text_rejected")?;
    assert!(
        staged.restored_occurrences() == 1
            && staged.output_tokens().len() == 1
            && staged.as_value()["candidates"][0]["content"]["parts"][0]["text"].as_str()
                == Some(CONTACT)
            && staged.delivered()
                == &json!([staged.as_value()["candidates"][0]["content"].clone()])
            && source == original
            && fixture.counts()? == (0, 0, 0),
        "ge2_json_text_restoration_or_staging_boundary_incorrect"
    );
    Ok(())
}

#[test]
fn ge2_lane_json_function_argument_object_uses_decoded_string_leaves() -> TestResult {
    let fixture = Ge2Fixture::new(true)?;
    let mut source = ge2_lane_json_source(&fixture.reference);
    source["candidates"][0]["content"]["parts"]
        .as_array_mut()
        .ok_or("ge2_json_parts_missing")?
        .push(
            json!({"functionCall":{"id":"synthetic-ge2-call","name":"synthetic_tool",
            "args":{"direct":fixture.reference,"nested":[fixture.reference,7,true,null,
                {"jsonLooking":"{not an object}"}]},"callOpaque":fixture.reference}}),
        );
    let original = source.clone();
    let staged = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "ge2_json_native_argument_object_rejected")?;
    let args = &staged.as_value()["candidates"][0]["content"]["parts"][1]["functionCall"]["args"];
    assert!(
        staged.restored_occurrences() == 3
            && staged.output_tokens().len() == 1
            && args.is_object()
            && args["direct"].as_str() == Some(CONTACT)
            && args["nested"][0].as_str() == Some(CONTACT)
            && args["nested"][1] == json!(7)
            && args["nested"][2] == json!(true)
            && args["nested"][3] == Value::Null
            && args["nested"][4]["jsonLooking"].as_str() == Some("{not an object}")
            && staged.as_value()["candidates"][0]["candidateOpaque"]
                == original["candidates"][0]["candidateOpaque"]
            && staged.as_value()["extension"] == original["extension"]
            && source == original
            && fixture.counts()? == (0, 0, 0),
        "ge2_json_native_argument_leaves_or_opaque_data_changed"
    );
    Ok(())
}

#[test]
fn ge2_lane_json_failure_and_malformed_reason_projection_rejects_atomically() -> TestResult {
    for reason in [
        Value::Null,
        json!("MAX_TOKENS"),
        json!("SAFETY"),
        json!("OTHER"),
        json!("SYNTHETIC_UNKNOWN_REASON"),
        json!(7),
        json!({"unexpected":"object"}),
    ] {
        let fixture = Ge2Fixture::new(true)?;
        let mut source = ge2_lane_json_source(&fixture.reference);
        source["candidates"][0]["finishReason"] = reason;
        let original = source.clone();
        let baseline = fixture.budget.used();
        let result = super::response::stage_json(
            &source,
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            &fixture.attempt,
        );
        assert!(result.is_err(), "ge2_json_invalid_terminal_was_staged");
        drop(result);
        assert!(
            source == original
                && fixture.budget.used() == baseline
                && fixture.counts()? == (0, 0, 0),
            "ge2_json_invalid_terminal_not_atomic"
        );
    }

    for envelope in [
        json!({"error":{"code":400,"message":"synthetic failure","status":"INVALID_ARGUMENT"}}),
        json!({"promptFeedback":{"blockReason":"SAFETY"}}),
    ] {
        let fixture = Ge2Fixture::new(true)?;
        let mut source = ge2_lane_json_source(&fixture.reference);
        for (key, value) in envelope.as_object().ok_or("ge2_envelope_fixture_missing")? {
            source[key] = value.clone();
        }
        let original = source.clone();
        let baseline = fixture.budget.used();
        let result = super::response::stage_json(
            &source,
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            &fixture.attempt,
        );
        assert!(result.is_err(), "ge2_json_failure_envelope_was_staged");
        drop(result);
        assert!(
            source == original
                && fixture.budget.used() == baseline
                && fixture.counts()? == (0, 0, 0),
            "ge2_json_failure_envelope_not_atomic"
        );
    }

    let fixture = Ge2Fixture::new(true)?;
    let mut source = ge2_lane_json_source(&fixture.reference);
    source["candidates"][0]
        .as_object_mut()
        .ok_or("ge2_candidate_fixture_missing")?
        .remove("finishReason");
    assert!(
        super::response::stage_json(
            &source,
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            &fixture.attempt,
        )
        .is_err(),
        "ge2_json_missing_terminal_was_staged"
    );
    Ok(())
}

#[test]
fn ge2_lane_json_strict_shared_validation_and_negative_shapes() -> TestResult {
    use crate::proxy::security::completion_json::JsonCompletion;

    fn shared_permits(source: &Value, budget: &PayloadBudget) -> TestResult<bool> {
        let raw = serde_json::to_string(source)?;
        let mut completion = JsonCompletion::native_gemini();
        completion.observe_raw_json(&raw, budget);
        Ok(completion.permits_completion())
    }

    let fixture = Ge2Fixture::new(true)?;
    let valid = ge2_lane_json_source(&fixture.reference);
    let baseline = fixture.budget.used();
    assert!(
        shared_permits(&valid, &fixture.budget)?
            && fixture.budget.used() == baseline
            && fixture.counts()? == (0, 0, 0),
        "ge2_json_shared_valid_control_rejected"
    );

    let mut negatives = vec![(
        "usage_only_absent_candidates_is_not_terminal",
        json!({"usageMetadata":{"totalTokenCount":3}}),
        true,
    )];
    for (label, candidates) in [
        ("present_null_candidates", Value::Null),
        ("present_object_candidates", json!({})),
        ("present_string_candidates", json!("synthetic")),
        ("present_number_candidates", json!(1)),
        ("present_empty_candidates", json!([])),
    ] {
        let mut source = valid.clone();
        source["candidates"] = candidates;
        negatives.push((label, source, true));
    }
    let mut source = valid.clone();
    source["candidates"] = json!([
        valid["candidates"][0].clone(),
        valid["candidates"][0].clone()
    ]);
    negatives.push(("duplicate_candidate_index", source, true));
    let mut source = valid.clone();
    source["candidates"][0]
        .as_object_mut()
        .ok_or("ge2_json_strict_candidate_missing")?
        .remove("index");
    // The lane owns the explicit-index contract even where shared validation is broader.
    negatives.push(("missing_candidate_index", source, false));

    for (label, content) in [
        ("non_model_content", json!({"role":"user","parts":[]})),
        ("scalar_content", json!(7)),
    ] {
        let mut source = valid.clone();
        source["candidates"][0]["content"] = content;
        negatives.push((label, source, true));
    }
    for (label, part) in [
        ("scalar_part", json!(7)),
        ("non_string_text", json!({"text":7})),
        (
            "non_object_arguments",
            json!({"functionCall":{"name":"synthetic_tool","args":[]}}),
        ),
    ] {
        let mut source = valid.clone();
        source["candidates"][0]["content"]["parts"] = json!([part]);
        negatives.push((label, source, true));
    }

    for (label, source, check_shared) in negatives {
        if check_shared {
            assert!(
                !shared_permits(&source, &fixture.budget)?,
                "ge2_json_shared_negative_shape_permitted:{label}"
            );
        }
        let original = source.clone();
        let result = super::response::stage_json(
            &source,
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            &fixture.attempt,
        );
        assert!(
            result.is_err(),
            "ge2_json_lane_negative_shape_staged:{label}"
        );
        drop(result);
        assert!(
            source == original
                && fixture.budget.used() == baseline
                && fixture.counts()? == (0, 0, 0),
            "ge2_json_negative_shape_not_atomic:{label}"
        );
    }
    Ok(())
}

#[test]
fn ge2_lane_json_candidate_part_leaf_isolation_and_signed_units() -> TestResult {
    let fixture = Ge2Fixture::new(true)?;
    let reference = &fixture.reference;
    let source = json!({"candidates":[
        {"index":7,"content":{"role":"model","parts":[
            {"text":reference,"partOpaque":{"sameValue":reference,"types":[true,null,7]}},
            {"functionCall":{"name":"synthetic_tool","args":{
                "direct":reference,"nested":[reference,7,true,null,{"public":"{not json}"}]
            },"callOpaque":reference}},
            {"text":"Synthetic signed public text","thoughtSignature":"synthetic-signature",
                "signedOpaque":reference},
            {"functionCall":{"name":"signed_tool","args":{
                "immutable":"Synthetic signed argument","types":[7,null]
            }},"thought_signature":"synthetic-alias-signature"},
            {"inlineData":{"mimeType":"application/octet-stream","data":reference},
                "unknownType":{"sameValue":reference}}
        ],"contentOpaque":reference},"finishReason":"STOP","candidateOpaque":reference},
        {"index":2,"content":{"role":"model","parts":[
            {"text":reference},
            {"functionCall":{"name":"second_tool","args":{"leaf":reference}}}
        ]},"finishReason":"STOP"}
    ],"extension":{"preserve":reference}});
    let original = source.clone();
    let baseline = fixture.budget.used();
    let staged = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "ge2_json_isolation_valid_source_rejected")?;
    let projected = staged.as_value();
    let first = &projected["candidates"][0]["content"]["parts"];
    let second = &projected["candidates"][1]["content"]["parts"];
    let delivered = json!([
        projected["candidates"][0]["content"].clone(),
        projected["candidates"][1]["content"].clone()
    ]);
    assert!(
        projected["candidates"][0]["index"] == json!(7)
            && projected["candidates"][1]["index"] == json!(2)
            && staged.delivered() == &delivered
            && staged.restored_occurrences() == 5
            && staged.output_tokens().len() == 1
            && first[0]["text"].as_str() == Some(CONTACT)
            && first[1]["functionCall"]["args"]["direct"].as_str() == Some(CONTACT)
            && first[1]["functionCall"]["args"]["nested"][0].as_str() == Some(CONTACT)
            && first[1]["functionCall"]["args"]["nested"][1] == json!(7)
            && first[1]["functionCall"]["args"]["nested"][2] == json!(true)
            && first[1]["functionCall"]["args"]["nested"][3] == Value::Null
            && first[1]["functionCall"]["args"]["nested"][4]["public"].as_str()
                == Some("{not json}")
            && second[0]["text"].as_str() == Some(CONTACT)
            && second[1]["functionCall"]["args"]["leaf"].as_str() == Some(CONTACT)
            && first[0]["partOpaque"]
                == original["candidates"][0]["content"]["parts"][0]["partOpaque"]
            && first[1]["functionCall"]["callOpaque"].as_str() == Some(reference)
            && first[2]["text"].as_str() == Some("Synthetic signed public text")
            && first[2]["thoughtSignature"].as_str() == Some("synthetic-signature")
            && first[2]["signedOpaque"].as_str() == Some(reference)
            && first[3]["functionCall"]["args"]
                == original["candidates"][0]["content"]["parts"][3]["functionCall"]["args"]
            && first[3]["thought_signature"].as_str() == Some("synthetic-alias-signature")
            && first[4] == original["candidates"][0]["content"]["parts"][4]
            && projected["candidates"][0]["candidateOpaque"].as_str() == Some(reference)
            && projected["extension"] == original["extension"]
            && source == original
            && fixture.counts()? == (0, 0, 0)
            && fixture.budget.used() > baseline,
        "ge2_json_candidate_part_leaf_isolation_failed"
    );
    drop(staged);
    assert!(
        fixture.budget.used() == baseline && fixture.counts()? == (0, 0, 0),
        "ge2_json_staging_budget_was_not_refunded"
    );

    let other = Ge2Fixture::new(false)?;
    let other_baseline = other.budget.used();
    let foreign = super::response::stage_json(
        &source,
        &fixture.lease,
        &other.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    );
    assert!(
        foreign.is_err()
            && fixture.budget.used() == baseline
            && other.budget.used() == other_baseline
            && fixture.counts()? == (0, 0, 0)
            && other.counts()? == (0, 0, 0),
        "ge2_json_foreign_binding_was_not_atomic"
    );
    drop(foreign);
    drop(other);

    for signature in ["thoughtSignature", "thought_signature"] {
        for signed_part in [
            json!({"text":reference}),
            json!({"functionCall":{"name":"signed_tool","args":{"nested":[reference]}}}),
        ] {
            let mut source = ge2_lane_json_source(reference);
            let parts = source["candidates"][0]["content"]["parts"]
                .as_array_mut()
                .ok_or("ge2_json_signed_parts_missing")?;
            parts[0] = json!({"text":reference});
            let mut signed_part = signed_part;
            signed_part[signature] = json!("synthetic-signature");
            parts.push(signed_part);
            let original = source.clone();
            let result = super::response::stage_json(
                &source,
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
                &fixture.attempt,
            );
            assert!(
                result.is_err(),
                "ge2_json_signed_selected_leaf_was_editable:{signature}"
            );
            drop(result);
            assert!(
                source == original
                    && fixture.budget.used() == baseline
                    && fixture.counts()? == (0, 0, 0),
                "ge2_json_signed_conflict_was_not_atomic:{signature}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn ge2_lane_json_real_pair_proof_sealed_activation_precedes_body() -> TestResult {
    use crate::proxy::security::{
        http_restoration::test_transport,
        response_restoration::finalize_native_json,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    use bytes::Bytes;
    use futures::StreamExt;

    let fixture = Ge2Fixture::new(true)?;
    let other = Ge2Fixture::new(false)?;
    let mut source = ge2_lane_json_source(&fixture.reference);
    source["candidates"][0]["content"]["parts"] = json!([
        {"text":fixture.reference},
        {"functionCall":{"id":"synthetic-ge2-json-call","name":"synthetic_tool",
            "args":{"query":fixture.reference,"types":[true,null,7,1.0]}}},
        {"text":"Synthetic public Gemini reasoning","thoughtSignature":"synthetic-ge2-signature"}
    ]);
    let original = source.clone();
    let wire = Bytes::from(serde_json::to_vec_pretty(&source)?);
    let pair = ProtocolPair::new(
        ClientProtocol::new(Protocol::Gemini),
        UpstreamProtocol::new(Protocol::Gemini),
    );

    let wrong_proof = test_transport::native_json_proof_for_pair(
        pair,
        &fixture.attempt,
        &other.binding,
        &fixture.budget,
        wire.clone(),
        &source,
    )
    .await
    .map_err(|_| "ge2_json_real_pair_wrong_binding_proof_rejected")?;
    let wrong_staged = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "ge2_json_real_pair_wrong_binding_stage_rejected")?;
    assert!(
        fixture.counts()? == (0, 0, 0) && other.counts()? == (0, 0, 0),
        "ge2_json_real_pair_staging_published"
    );
    let wrong_final = finalize_native_json(
        wrong_staged,
        &fixture.lease,
        wrong_proof,
        &fixture.attempt,
        wire.clone(),
        1024 * 1024,
    );
    assert!(
        wrong_final.is_err()
            && fixture.counts()? == (0, 0, 0)
            && other.counts()? == (0, 0, 0)
            && source == original,
        "ge2_json_real_pair_wrong_binding_published_or_mutated"
    );
    drop(wrong_final);

    let proof = test_transport::native_json_proof_for_pair(
        pair,
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        wire.clone(),
        &source,
    )
    .await
    .map_err(|_| "ge2_json_real_pair_proof_rejected")?;
    let staged = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "ge2_json_real_pair_stage_rejected")?;
    assert!(
        fixture.counts()? == (0, 0, 0) && other.counts()? == (0, 0, 0),
        "ge2_json_real_pair_valid_staging_published"
    );
    let finalized = finalize_native_json(
        staged,
        &fixture.lease,
        proof,
        &fixture.attempt,
        wire,
        1024 * 1024,
    )
    .map_err(|_| "ge2_json_real_pair_finalization_rejected")?;
    assert!(
        fixture.counts()? == (1, 0, 0) && other.counts()? == (0, 0, 0),
        "ge2_json_sealed_activation_did_not_precede_body"
    );
    let mut delivery = finalized.into_delivery_stream();
    let body = delivery.next().await.ok_or("ge2_json_delivery_missing")??;
    assert!(
        delivery.next().await.is_none(),
        "ge2_json_delivery_not_single_body"
    );
    let actual: Value = serde_json::from_slice(&body)?;
    let parts = &actual["candidates"][0]["content"]["parts"];
    assert!(
        parts[0]["text"].as_str() == Some(CONTACT)
            && parts[1]["functionCall"]["args"]["query"].as_str() == Some(CONTACT)
            && parts[1]["functionCall"]["args"]["types"] == json!([true, null, 7, 1.0])
            && parts[2] == original["candidates"][0]["content"]["parts"][2]
            && actual["candidates"][0]["candidateOpaque"]
                == original["candidates"][0]["candidateOpaque"]
            && actual["extension"] == original["extension"]
            && source == original
            && fixture.counts()? == (1, 0, 0)
            && other.counts()? == (0, 0, 0),
        "ge2_json_real_pair_final_native_delivery_invalid"
    );
    Ok(())
}

// GE2 SSE accepts complete append-only Contents, never a cross-frame logical Part.
fn ge2_sse_wire(events: &[Value]) -> TestResult<String> {
    let mut wire = String::new();
    for event in events {
        wire.push_str(&format!("data: {}\n\n", serde_json::to_string(event)?));
    }
    Ok(wire)
}

fn ge2_sse_events(reference: &str) -> Vec<Value> {
    vec![
        json!({"candidates":[{"index":7,"content":{"role":"model","parts":[{"text":reference}]}}]}),
        json!({"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":2}}),
        json!({"candidates":[{"index":7,"finishReason":"STOP"}]}),
    ]
}

fn ge2_finalize_sse(
    fixture: &Ge2Fixture,
    wire: &str,
) -> TestResult<crate::proxy::security::response_restoration::FinalizedNativeSse> {
    use crate::proxy::security::{
        http_restoration::test_transport,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    let proof = futures::executor::block_on(test_transport::native_sse_proof_for_pair(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Gemini),
            UpstreamProtocol::new(Protocol::Gemini),
        ),
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        bytes::Bytes::copy_from_slice(wire.as_bytes()),
        bytes::Bytes::copy_from_slice(wire.as_bytes()),
    ));
    assert!(proof.is_ok(), "ge2_sse_fixture_real_pair_proof_rejected");
    super::response::finalize_sse(
        wire.as_bytes(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
        proof.map_err(|_| "ge2_sse_real_pair_proof_rejected")?,
        1024 * 1024,
    )
    .map_err(|_| "ge2_sse_native_consumer_behavior_missing".into())
}

#[test]
fn ge2_lane_sse_whole_content_text_requires_last_yield_continuation() -> TestResult {
    use futures::StreamExt;
    let fixture = Ge2Fixture::new(true)?;
    let baseline = fixture.budget.used();
    let events = ge2_sse_events(&fixture.reference);
    let wire = ge2_sse_wire(&events)?;
    let result = ge2_finalize_sse(&fixture, &wire);
    assert!(result.is_ok(), "ge2_sse_native_consumer_behavior_missing");
    let mut stream = result?.into_delivery_stream();
    for index in 0..events.len() {
        assert!(
            fixture.counts()? == (0, 1, 1),
            "ge2_sse_published_before_yield"
        );
        let chunk = futures::executor::block_on(stream.next()).ok_or("ge2_sse_frame_missing")??;
        if index == 0 {
            let data = std::str::from_utf8(&chunk)?
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .ok_or("ge2_sse_data_missing")?;
            let value: Value = serde_json::from_str(data)?;
            assert!(
                value["candidates"][0]["content"]["parts"][0]["text"].as_str() == Some(CONTACT),
                "ge2_sse_text_not_restored"
            );
        }
        assert!(
            fixture.counts()? == (0, 1, 1),
            "ge2_sse_published_after_yield"
        );
    }
    assert!(
        futures::executor::block_on(stream.next()).is_none(),
        "ge2_sse_extra_frame"
    );
    drop(stream);
    assert!(
        fixture.counts()? == (1, 0, 0) && fixture.budget.used() == baseline,
        "ge2_sse_continuation_or_refund_invalid"
    );
    Ok(())
}

#[test]
fn ge2_lane_sse_ordered_candidates_contents_native_args_and_signed_parts() -> TestResult {
    use crate::proxy::security::{
        native_test_support::record_delivery, restoration_core::compose_gemini_snapshots,
    };
    let fixture = Ge2Fixture::new(true)?;
    let baseline = fixture.budget.used();
    let events = vec![
        json!({"candidates":[{"index":7,"content":{"role":"model","parts":[
            {"text":fixture.reference},
            {"functionCall":{"id":"synthetic-ge2-call","name":"synthetic_tool","args":{"query":fixture.reference,"nested":[fixture.reference,7,1.0,true,null]}}},
            {"text":"Synthetic public thought","thought":true,"thoughtSignature":fixture.reference}
        ],"opaque":{"types":[true,null,7]}}},{"index":2,"content":{"role":"model","parts":[{"text":"Second candidate"}]}}],"extension":{"sameValue":fixture.reference}}),
        json!({"candidates":[{"index":2,"content":{"role":"model","parts":[{"text":"Second Content"}]}},{"index":7,"content":{"role":"model","parts":[{"text":"","thoughtSignature":"synthetic-late-signature"}]}}]}),
        json!({"usageMetadata":{"totalTokenCount":9}}),
        json!({"candidates":[{"index":2,"finishReason":"STOP"},{"index":7,"finishReason":"STOP"}]}),
    ];
    let wire = ge2_sse_wire(&events)?;
    let result = ge2_finalize_sse(&fixture, &wire);
    assert!(result.is_ok(), "ge2_sse_native_consumer_behavior_missing");
    let delivery = futures::executor::block_on(record_delivery(result?.into_delivery_stream()))?;
    assert!(
        fixture.counts()? == (1, 0, 0),
        "ge2_sse_record_not_published"
    );
    let mut frames = Vec::new();
    for bytes in &delivery {
        for line in std::str::from_utf8(bytes)?.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                frames.push(serde_json::from_str::<Value>(data)?);
            }
        }
    }
    let refs: Vec<&Value> = frames
        .iter()
        .filter_map(|frame| frame["candidates"].as_array())
        .flat_map(|candidates| candidates.iter())
        .collect();
    let delivered = compose_gemini_snapshots(&refs, &fixture.budget)
        .map_err(|_| "ge2_sse_composition_failed")?;
    let expected = json!([{"index":7,"contents":[
        {"role":"model","parts":[
            {"text":CONTACT},
            {"functionCall":{"id":"synthetic-ge2-call","name":"synthetic_tool","args":{"query":CONTACT,"nested":[CONTACT,7,1.0,true,null]}}},
            {"text":"Synthetic public thought","thought":true,"thoughtSignature":fixture.reference}
        ],"opaque":{"types":[true,null,7]}},
        {"role":"model","parts":[{"text":"","thoughtSignature":"synthetic-late-signature"}]}]},
        {"index":2,"contents":[{"role":"model","parts":[{"text":"Second candidate"}]},
        {"role":"model","parts":[{"text":"Second Content"}]}]}
    ]);
    assert!(
        delivered.as_value() == &expected && frames[0]["extension"] == events[0]["extension"],
        "ge2_sse_native_order_types_or_opaque_fields_changed"
    );
    drop(delivered);
    drop(delivery);
    assert!(
        fixture.budget.used() == baseline,
        "ge2_sse_delivery_charge_retained"
    );
    Ok(())
}

fn ge2_sse_proof(
    fixture: &Ge2Fixture,
    original: &str,
    client: &str,
) -> Result<
    crate::proxy::security::http_restoration::VerifiedNativeSse,
    crate::proxy::security::response_restoration::RestoreError,
> {
    use crate::proxy::security::{
        http_restoration::test_transport,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    futures::executor::block_on(test_transport::native_sse_proof_for_pair(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Gemini),
            UpstreamProtocol::new(Protocol::Gemini),
        ),
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        bytes::Bytes::copy_from_slice(original.as_bytes()),
        bytes::Bytes::copy_from_slice(client.as_bytes()),
    ))
}

#[test]
fn ge2_lane_sse_invalid_states_framing_and_strict_entity_mismatch_publish_nothing() -> TestResult {
    let fixture = Ge2Fixture::new(true)?;
    let baseline = fixture.budget.used();
    let valid = ge2_sse_events(&fixture.reference);
    let wire = ge2_sse_wire(&valid)?;
    drop(ge2_finalize_sse(&fixture, &wire)?);
    assert!(
        fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
        "ge2_sse_control_not_clean"
    );
    let mut cases = Vec::new();
    for reason in [
        "MAX_TOKENS",
        "SAFETY",
        "RECITATION",
        "MALFORMED_FUNCTION_CALL",
        "OTHER",
        "UNKNOWN",
        "",
    ] {
        let mut events = valid.clone();
        events[2]["candidates"][0]["finishReason"] = json!(reason);
        cases.push(events);
    }
    let mut events = valid.clone();
    events.pop();
    cases.push(events);
    let mut events = valid.clone();
    events.push(valid[2].clone());
    cases.push(events);
    let mut events = valid.clone();
    events.push(valid[0].clone());
    cases.push(events);
    let mut events = valid.clone();
    events[2]["candidates"][0]["finishReason"] = Value::Null;
    cases.push(events);
    let mut events = valid.clone();
    events[0]["candidates"][0]
        .as_object_mut()
        .ok_or("ge2_candidate_missing")?
        .remove("index");
    cases.push(events);
    let mut events = valid.clone();
    events[0]["candidates"][0]["index"] = json!(128);
    cases.push(events);
    let mut events = valid.clone();
    events[0]["candidates"]
        .as_array_mut()
        .ok_or("ge2_candidates_missing")?
        .push(valid[0]["candidates"][0].clone());
    cases.push(events);
    for malformed in [Value::Null, json!({}), json!("bad"), json!(7), json!([])] {
        let mut events = valid.clone();
        events[1]["candidates"] = malformed;
        cases.push(events);
    }
    for malformed in [
        Value::Null,
        json!({"role":"user","parts":[{"text":"public"}]}),
        json!({"role":"model","parts":"bad"}),
        json!({"role":"model","parts":[{"text":7}]}),
        json!({"role":"model","parts":[{"functionCall":{"name":"tool","args":[]}}]}),
        json!({"role":"model","parts":[{"text":"public","extension":true}]}),
    ] {
        let mut events = valid.clone();
        events[0]["candidates"][0]["content"] = malformed;
        cases.push(events);
    }
    for blocked in [
        json!({"error":{"code":400,"message":"Synthetic error"}}),
        json!({"promptFeedback":{"blockReason":"SAFETY"}}),
    ] {
        let mut events = valid.clone();
        events.insert(1, blocked);
        cases.push(events);
    }
    for (index, events) in cases.iter().enumerate() {
        let malformed = ge2_sse_wire(events)?;
        assert!(
            ge2_sse_proof(&fixture, &malformed, &malformed).is_err(),
            "ge2_sse_invalid_http_state_accepted: {index}"
        );
        let proof =
            ge2_sse_proof(&fixture, &wire, &wire).map_err(|_| "ge2_sse_valid_proof_failed")?;
        let result = super::response::finalize_sse(
            malformed.as_bytes(),
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            &fixture.attempt,
            proof,
            1024 * 1024,
        );
        assert!(
            result.is_err() && fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
            "ge2_sse_invalid_final_retained_authority_or_budget: {index}"
        );
    }
    for malformed in [
        wire.trim_end().to_owned(),
        format!("{wire}data: [DONE]\n\n"),
        format!("{wire}data: {{\n\n"),
    ] {
        assert!(
            ge2_sse_proof(&fixture, &malformed, &malformed).is_err(),
            "ge2_sse_invalid_framing_accepted"
        );
    }
    let mut changed = valid.clone();
    changed[0]["candidates"][0]["content"]["parts"][0]["text"] =
        json!("Synthetic changed public text");
    let changed = ge2_sse_wire(&changed)?;
    assert!(
        ge2_sse_proof(&fixture, &wire, &changed).is_err(),
        "ge2_sse_original_client_mismatch_accepted"
    );
    let proof = ge2_sse_proof(&fixture, &wire, &wire).map_err(|_| "ge2_sse_valid_proof_failed")?;
    assert!(
        super::response::finalize_sse(
            changed.as_bytes(),
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            &fixture.attempt,
            proof,
            1024 * 1024
        )
        .is_err(),
        "ge2_sse_valid_but_different_final_accepted"
    );
    assert!(
        fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
        "ge2_sse_mismatch_retained_authority_or_budget"
    );
    Ok(())
}

#[test]
fn ge2_lane_sse_signed_text_and_argument_units_are_immutable() -> TestResult {
    let fixture = Ge2Fixture::new(true)?;
    let baseline = fixture.budget.used();
    for part in [
        json!({"text":fixture.reference,"thoughtSignature":"synthetic-signature"}),
        json!({"functionCall":{"name":"synthetic_tool","args":{"nested":[fixture.reference,7,true,null]}},"thoughtSignature":"synthetic-signature"}),
    ] {
        let mut events = ge2_sse_events(&fixture.reference);
        events[0]["candidates"][0]["content"]["parts"] = json!([part]);
        let wire = ge2_sse_wire(&events)?;
        let proof = ge2_sse_proof(&fixture, &wire, &wire)
            .map_err(|_| "ge2_sse_signed_fixture_proof_failed")?;
        assert!(
            super::response::finalize_sse(
                wire.as_bytes(),
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
                &fixture.attempt,
                proof,
                1024 * 1024
            )
            .is_err(),
            "ge2_sse_signed_unit_edited"
        );
        assert!(
            fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
            "ge2_sse_signed_failure_retained_authority_or_budget"
        );
    }
    Ok(())
}

#[test]
fn ge2_lane_sse_drop_at_every_yield_including_last_never_publishes() -> TestResult {
    use futures::StreamExt;
    for yields in 0..=3 {
        let fixture = Ge2Fixture::new(true)?;
        let baseline = fixture.budget.used();
        let wire = ge2_sse_wire(&ge2_sse_events(&fixture.reference))?;
        let mut stream = ge2_finalize_sse(&fixture, &wire)?.into_delivery_stream();
        for _ in 0..yields {
            drop(
                futures::executor::block_on(stream.next())
                    .ok_or("ge2_sse_cancel_frame_missing")??,
            );
            assert!(
                fixture.counts()? == (0, 1, 1),
                "ge2_sse_cancel_published_at_yield"
            );
        }
        drop(stream);
        assert!(
            fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
            "ge2_sse_cancel_leaked_authority_or_budget"
        );
    }
    Ok(())
}

#[test]
fn ge2_lane_sse_binding_attempt_entity_and_budget_failures_refund_atomically() -> TestResult {
    use crate::proxy::security::restoration_core::AttemptProvenance;
    let fixture = Ge2Fixture::new(true)?;
    let baseline = fixture.budget.used();
    let wire = ge2_sse_wire(&ge2_sse_events(&fixture.reference))?;
    for index in 0..5 {
        let proof =
            ge2_sse_proof(&fixture, &wire, &wire).map_err(|_| "ge2_sse_isolation_proof_failed")?;
        let other_attempt = AttemptProvenance::new(&fixture.budget)?;
        let binding = fixture.lease.bind_actual_attempt(
            "synthetic-other-provider",
            "gemini",
            "https://synthetic.invalid/v1beta/models/synthetic:generateContent",
            Some("synthetic-ge2-principal"),
        )?;
        let tiny = PayloadBudget::with_limit(64);
        let result = super::response::finalize_sse(
            if index == 2 {
                b"data: {}\n\n"
            } else {
                wire.as_bytes()
            },
            &fixture.lease,
            if index == 0 {
                &binding
            } else {
                &fixture.binding
            },
            if index == 3 {
                tiny.clone()
            } else {
                fixture.budget.clone()
            },
            if index == 1 {
                &other_attempt
            } else {
                &fixture.attempt
            },
            proof,
            if index == 4 { 1 } else { 1024 * 1024 },
        );
        assert!(
            result.is_err(),
            "ge2_sse_isolation_or_budget_failure_accepted: {index}"
        );
        drop(other_attempt);
        assert!(
            fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline && tiny.used() == 0,
            "ge2_sse_isolation_or_budget_leak: {index}"
        );
    }
    Ok(())
}

#[test]
fn ge2_lane_sse_unchanged_wire_preserves_crlf_and_charged_bytes_owner() -> TestResult {
    use crate::proxy::security::native_test_support::record_delivery;
    let fixture = Ge2Fixture::new(false)?;
    let baseline = fixture.budget.used();
    let wire = ge2_sse_wire(&ge2_sse_events("Synthetic public Gemini text"))?
        .replace("\n", "\r\n")
        .replace("data:", ": synthetic comment\r\ndata:");
    let chunks = futures::executor::block_on(record_delivery(
        ge2_finalize_sse(&fixture, &wire)?.into_delivery_stream(),
    ))?;
    let actual: Vec<u8> = chunks
        .iter()
        .flat_map(|chunk| chunk.iter().copied())
        .collect();
    assert!(
        actual == wire.as_bytes()
            && fixture.counts()? == (1, 0, 0)
            && fixture.budget.used() > baseline,
        "ge2_sse_unchanged_wire_or_live_charge_changed"
    );
    drop(chunks);
    assert!(
        fixture.budget.used() == baseline,
        "ge2_sse_charged_bytes_not_refunded"
    );
    Ok(())
}

// GE3 state table: only exact ordered delivered Contents are lookup evidence.
// Native IDs take priority; an ID-free single call/result with the same name in
// adjacent model/user Contents is unambiguous. Parallel/duplicate uncertainty
// grants no pairing. Results stay current input; signed Parts remain whole.
fn ge3_next(
    fixture: &Ge2Fixture,
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
        "synthetic-ge1-app",
        session,
        fixture.binding.policy_revision,
    )?;
    let binding = lease.bind_actual_attempt(
        provider,
        "gemini",
        "https://synthetic.invalid/v1beta/models/synthetic:generateContent",
        Some("synthetic-ge2-principal"),
    )?;
    Ok((lease, binding))
}

fn ge3_source(reference: &str) -> Value {
    let mut source = ge2_lane_json_source(reference);
    source["candidates"][0]["content"]["parts"] = json!([
        {"text":reference},
        {"text":"Synthetic public Gemini reasoning","thoughtSignature":"synthetic-ge3-signature"},
        {"functionCall":{"id":"synthetic-ge3-call","name":"synthetic_tool","args":{"query":reference,"types":[true,null,7,1.0]}}}
    ]);
    if let Some(candidates) = source["candidates"].as_array_mut() {
        candidates.push(json!({"index":2,"content":{"role":"model","parts":[{"text":"Synthetic second candidate"}]},"finishReason":"STOP"}))
    }
    source
}

fn ge3_deliver_json(fixture: &Ge2Fixture, source: &Value) -> TestResult<Value> {
    use crate::proxy::security::{
        http_restoration::test_transport,
        native_test_support::record_delivery,
        response_restoration::finalize_native_json,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    let wire = bytes::Bytes::from(serde_json::to_vec(source)?);
    let proof = futures::executor::block_on(test_transport::native_json_proof_for_pair(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Gemini),
            UpstreamProtocol::new(Protocol::Gemini),
        ),
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        wire.clone(),
        source,
    ))
    .map_err(|_| "ge3_real_pair_proof_rejected")?;
    let stage = super::response::stage_json(
        source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "ge3_stage_rejected")?;
    let finalized = finalize_native_json(
        stage,
        &fixture.lease,
        proof,
        &fixture.attempt,
        wire,
        1024 * 1024,
    )
    .map_err(|_| "ge3_finalizer_rejected")?;
    assert!(
        fixture.counts()? == (1, 0, 0),
        "ge3_json_not_active_before_body"
    );
    let chunks = futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
    let response: Value = serde_json::from_slice(&chunks[0])?;
    Ok(Value::Array(
        response["candidates"]
            .as_array()
            .ok_or("ge3_delivered_candidates_missing")?
            .iter()
            .map(|candidate| candidate["content"].clone())
            .collect(),
    ))
}

fn ge3_observe<'a>(
    request: &'a Value,
    budget: &PayloadBudget,
) -> TestResult<RequestObservation<'a>> {
    super::history::observe_request(&Method::POST, &JSON_ROUTE.parse()?, request, budget.clone())
        .map_err(|_| "ge3_history_observation_rejected".into())
}

#[test]
fn ge3_lane_exact_ordered_contents_and_signed_two_turn_replay() -> TestResult {
    let fixture = Ge2Fixture::new(true)?;
    let source = ge3_source(&fixture.reference);
    let delivered = ge3_deliver_json(&fixture, &source)?;
    let (mut next, binding) = ge3_next(
        &fixture,
        Some("synthetic-ge1-session"),
        Protocol::Gemini,
        "synthetic-ge2-provider",
    )?;
    assert!(
        next.resolve(&fixture.reference).is_err(),
        "ge3_fresh_lease_authorized"
    );
    let result = super::history::import_delivered(&mut next, &binding, &delivered);
    assert!(result.is_ok(), "ge3_exact_delivered_import_missing");
    result?;
    assert!(
        next.resolve(&fixture.reference)? == CONTACT,
        "ge3_dependency_not_imported"
    );
    super::history::import_delivered(&mut next, &binding, &delivered)?;
    let request = json!({"contents":[delivered[0],{"role":"user","parts":[{"functionResponse":{"id":"synthetic-ge3-call","name":"synthetic_tool","response":{"text":"Synthetic public result"}}}]}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = ge3_observe(&request, &budget)?;
    let prepared = prepare_observed_request(
        &mut next,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| Ok(()),
    )
    .map_err(|_| "ge3_two_turn_preparation_rejected")?;
    let mut expected = request.clone();
    let actual = prepared.body.as_value();
    expected["contents"][0]["parts"][0]["text"] = actual["contents"][0]["parts"][0]["text"].clone();
    expected["contents"][0]["parts"][2]["functionCall"]["args"]["query"] =
        expected["contents"][0]["parts"][0]["text"].clone();
    let reference = expected["contents"][0]["parts"][0]["text"]
        .as_str()
        .ok_or("ge3_replay_reference_missing")?;
    assert!(
        actual == &expected
            && prepared.reversible_occurrences == 2
            && next.resolve(reference)? == CONTACT
            && actual["contents"][0]["parts"][1] == source["candidates"][0]["content"]["parts"][1],
        "ge3_two_turn_native_or_signed_part_changed"
    );
    drop(prepared);
    drop(observed);
    assert!(budget.used() == 0, "ge3_two_turn_budget_retained");
    Ok(())
}

#[test]
fn ge3_lane_native_ids_pair_reordered_results_without_text_authority() -> TestResult {
    use crate::proxy::security::restoration_contract::HistoryClaim;
    let request = json!({"contents":[{"role":"model","parts":[
        {"functionCall":{"id":"synthetic-a","name":"synthetic_tool","args":{"query":"public"}}},
        {"functionCall":{"id":"synthetic-b","name":"synthetic_tool","args":{"query":"public"}}}
    ]},{"role":"user","parts":[
        {"functionResponse":{"id":"synthetic-b","name":"synthetic_tool","response":{"text":CONTACT}}},
        {"functionResponse":{"id":"synthetic-a","name":"synthetic_tool","response":{"text":CONTACT}}}
    ]}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = ge3_observe(&request, &budget)?;
    let pairs: Vec<_> = observed
        .history()
        .iter()
        .filter_map(|claim| match claim {
            HistoryClaim::CallResultPair { call, result } => Some((*call, *result)),
            _ => None,
        })
        .collect();
    assert!(
        pairs.len() == 2
            && std::ptr::eq(pairs[0].0, &request["contents"][0]["parts"][1])
            && std::ptr::eq(pairs[0].1, &request["contents"][1]["parts"][0])
            && std::ptr::eq(pairs[1].0, &request["contents"][0]["parts"][0]),
        "ge3_native_id_pairing_missing"
    );
    assert!(
        observed.sites().len() == 4,
        "ge3_pairing_skipped_result_or_argument_dlp"
    );
    drop(observed);
    assert!(budget.used() == 0, "ge3_native_pair_charge_retained");
    Ok(())
}

#[test]
fn ge3_lane_id_free_single_native_call_result_uses_unambiguous_order_only() -> TestResult {
    use crate::proxy::security::restoration_contract::HistoryClaim;
    let request = json!({"contents":[{"role":"model","parts":[{"text":"Synthetic lead"},
        {"functionCall":{"name":"synthetic_tool","args":{"query":"public"}}}
    ]},{"role":"user","parts":[{"functionResponse":{"name":"synthetic_tool","response":{"text":CONTACT}}}]}]});
    let original = request.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = ge3_observe(&request, &budget)?;
    let pairs: Vec<_> = observed
        .history()
        .iter()
        .filter_map(|claim| match claim {
            HistoryClaim::CallResultPair { call, result } => Some((*call, *result)),
            _ => None,
        })
        .collect();
    assert!(
        pairs.len() == 1
            && std::ptr::eq(pairs[0].0, &request["contents"][0]["parts"][1])
            && std::ptr::eq(pairs[0].1, &request["contents"][1]["parts"][0])
            && request == original,
        "ge3_id_free_unambiguous_pairing_missing"
    );
    drop(observed);
    assert!(budget.used() == 0, "ge3_id_free_pair_charge_retained");
    Ok(())
}

#[test]
fn ge3_lane_inexact_delivered_contents_never_import_dependencies() -> TestResult {
    let fixture = Ge2Fixture::new(true)?;
    let delivered = ge3_deliver_json(&fixture, &ge3_source(&fixture.reference))?;
    let baseline = fixture.budget.used();
    let mut cases = Vec::new();
    for path in ["text", "signature", "args", "unknown"] {
        let mut changed = delivered.clone();
        match path {
            "text" => changed[0]["parts"][0]["text"] = json!("Synthetic edit"),
            "signature" => changed[0]["parts"][1]["thoughtSignature"] = json!("synthetic-edit"),
            "args" => {
                changed[0]["parts"][2]["functionCall"]["args"]["query"] = json!("Synthetic edit")
            }
            _ => changed[0]["unknown"] = json!(true),
        }
        cases.push(changed);
    }
    let mut changed = delivered.clone();
    changed[0]["parts"]
        .as_array_mut()
        .ok_or("ge3_parts_missing")?
        .swap(0, 2);
    cases.push(changed);
    cases.extend([
        json!([delivered[1], delivered[0]]),
        json!([delivered[0]]),
        json!([delivered[0], delivered[1], delivered[0]]),
        json!([{"role":"model","parts":[{"text":"Synthetic unknown history"}]}]),
    ]);
    for changed in cases {
        let (mut next, binding) = ge3_next(
            &fixture,
            Some("synthetic-ge1-session"),
            Protocol::Gemini,
            "synthetic-ge2-provider",
        )?;
        assert!(
            super::history::import_delivered(&mut next, &binding, &changed).is_err()
                && next.resolve(&fixture.reference).is_err(),
            "ge3_inexact_history_authorized"
        );
        assert!(
            fixture.budget.used() == baseline,
            "ge3_rejected_lookup_retained_budget"
        );
    }
    let (mut next, binding) = ge3_next(
        &fixture,
        Some("synthetic-ge1-session"),
        Protocol::Gemini,
        "synthetic-ge2-provider",
    )?;
    super::history::import_delivered(&mut next, &binding, &delivered)?;
    assert!(
        next.resolve(&fixture.reference)? == CONTACT,
        "ge3_exact_control_not_imported"
    );
    Ok(())
}

#[test]
fn ge3_lane_absent_cross_session_provider_protocol_store_retry_isolation() -> TestResult {
    let fixture = Ge2Fixture::new(true)?;
    let delivered = ge3_deliver_json(&fixture, &ge3_source(&fixture.reference))?;
    for (session, client, provider) in [
        (None, Protocol::Gemini, "synthetic-ge2-provider"),
        (
            Some("synthetic-other-session"),
            Protocol::Gemini,
            "synthetic-ge2-provider",
        ),
        (
            Some("synthetic-ge1-session"),
            Protocol::Gemini,
            "synthetic-other-provider",
        ),
        (
            Some("synthetic-ge1-session"),
            Protocol::Anthropic,
            "synthetic-ge2-provider",
        ),
    ] {
        let (mut next, binding) = ge3_next(&fixture, session, client, provider)?;
        assert!(
            super::history::import_delivered(&mut next, &binding, &delivered).is_err()
                && next.resolve(&fixture.reference).is_err(),
            "ge3_cross_binding_imported"
        );
    }
    for (format, endpoint, principal) in [
        (
            "anthropic",
            "https://synthetic.invalid/v1beta/models/synthetic:generateContent",
            Some("synthetic-ge2-principal"),
        ),
        (
            "gemini",
            "https://synthetic-other.invalid/v1beta/models/synthetic:generateContent",
            Some("synthetic-ge2-principal"),
        ),
        (
            "gemini",
            "https://synthetic.invalid/v1beta/models/synthetic:generateContent",
            Some("synthetic-other-principal"),
        ),
    ] {
        let (mut next, _) = ge3_next(
            &fixture,
            Some("synthetic-ge1-session"),
            Protocol::Gemini,
            "synthetic-ge2-provider",
        )?;
        let binding =
            next.bind_actual_attempt("synthetic-ge2-provider", format, endpoint, principal)?;
        assert!(
            super::history::import_delivered(&mut next, &binding, &delivered).is_err()
                && next.resolve(&fixture.reference).is_err(),
            "ge3_actual_attempt_mismatch_imported"
        );
    }
    let other = Ge2Fixture::new(false)?;
    let (mut next, binding) = ge3_next(
        &other,
        Some("synthetic-ge1-session"),
        Protocol::Gemini,
        "synthetic-ge2-provider",
    )?;
    assert!(
        super::history::import_delivered(&mut next, &binding, &delivered).is_err()
            && next.resolve(&fixture.reference).is_err(),
        "ge3_cross_store_imported"
    );
    let (next, _) = ge3_next(
        &fixture,
        Some("synthetic-ge1-session"),
        Protocol::Gemini,
        "synthetic-ge2-provider",
    )?;
    let mut retry = next.fork_attempt()?;
    let binding = retry.bind_actual_attempt(
        "synthetic-retry-provider",
        "gemini",
        "https://synthetic-retry.invalid/v1beta/models/synthetic:generateContent",
        Some("synthetic-ge2-principal"),
    )?;
    assert!(
        super::history::import_delivered(&mut retry, &binding, &delivered).is_err()
            && retry.resolve(&fixture.reference).is_err(),
        "ge3_retry_inherited_other_attempt_authority"
    );
    Ok(())
}

#[test]
fn ge3_lane_first_and_edited_function_response_remain_current_dlp() -> TestResult {
    use crate::proxy::security::restoration_contract::HistoryClaim;
    for text in [CONTACT, "Synthetic edited result 13900000000"] {
        let fixture = Ge2Fixture::new(true)?;
        let delivered = ge3_deliver_json(&fixture, &ge3_source(&fixture.reference))?;
        let (mut next, binding) = ge3_next(
            &fixture,
            Some("synthetic-ge1-session"),
            Protocol::Gemini,
            "synthetic-ge2-provider",
        )?;
        super::history::import_delivered(&mut next, &binding, &delivered)?;
        let request = json!({"contents":[delivered[0],{"role":"user","parts":[{"functionResponse":{"id":"synthetic-ge3-call","name":"synthetic_tool","response":{"text":text,"types":[7,true,null]}}}]}]});
        let original = request.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = ge3_observe(&request, &budget)?;
        assert!(
            observed
                .history()
                .iter()
                .any(|claim| matches!(claim, HistoryClaim::CallResultPair { .. })),
            "ge3_result_pair_missing"
        );
        let prepared = prepare_observed_request(
            &mut next,
            &observed,
            &SecurityRuntimeConfig::default(),
            RESTORE_RULES,
            &[],
            |_, _, _| Ok(()),
        )
        .map_err(|_| "ge3_result_current_dlp_rejected")?;
        assert!(
            prepared.reversible_occurrences == 3
                && !prepared.body.as_value()["contents"][1]["parts"][0]["functionResponse"]
                    ["response"]["text"]
                    .as_str()
                    .ok_or("ge3_result_text_missing")?
                    .contains(CONTACT)
                && prepared.body.as_value()["contents"][0]["parts"][1]
                    == original["contents"][0]["parts"][1]
                && prepared.body.as_value()["contents"][1]["parts"][0]["functionResponse"]
                    ["response"]["types"]
                    == json!([7, true, null])
                && request == original
                && fixture.counts()? == (1, 0, 0),
            "ge3_result_trusted_or_native_signed_unit_changed"
        );
        drop(prepared);
        drop(observed);
        assert!(budget.used() == 0, "ge3_result_dlp_budget_retained");
    }
    Ok(())
}

#[test]
fn ge3_lane_authorized_reference_in_signed_text_or_args_fails_atomically() -> TestResult {
    let fixture = Ge2Fixture::new(true)?;
    let delivered = ge3_deliver_json(&fixture, &ge3_source(&fixture.reference))?;
    for part in [
        json!({"text":fixture.reference,"thoughtSignature":"synthetic-ge3-signature"}),
        json!({"functionCall":{"id":"synthetic-ge3-call","name":"synthetic_tool","args":{"query":fixture.reference}},"thoughtSignature":"synthetic-ge3-signature"}),
    ] {
        let (mut next, binding) = ge3_next(
            &fixture,
            Some("synthetic-ge1-session"),
            Protocol::Gemini,
            "synthetic-ge2-provider",
        )?;
        super::history::import_delivered(&mut next, &binding, &delivered)?;
        let request = json!({"contents":[{"role":"model","parts":[part]}]});
        let original = request.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = ge3_observe(&request, &budget)?;
        let baseline = budget.used();
        let before = fixture.store.stats()?;
        let called = Cell::new(false);
        let result = prepare_observed_request(
            &mut next,
            &observed,
            &SecurityRuntimeConfig::default(),
            RESTORE_RULES,
            &[],
            |_, _, _| {
                called.set(true);
                Ok(())
            },
        );
        assert!(
            matches!(result, Err(PlanError::ImmutableContent))
                && !called.get()
                && request == original
                && fixture.store.stats()? == before
                && budget.used() == baseline,
            "ge3_signed_authorized_reference_mutated_or_committed"
        );
        drop(observed);
        assert!(budget.used() == 0, "ge3_signed_failure_budget_retained");
    }
    Ok(())
}

#[test]
fn ge3_lane_invalid_mixed_duplicate_and_id_free_parallel_pairs_stay_unpaired() -> TestResult {
    use crate::proxy::security::restoration_contract::HistoryClaim;
    let call = json!({"functionCall":{"id":"synthetic-ge3-call","name":"synthetic_tool","args":{"query":"public"}}});
    let result = json!({"functionResponse":{"id":"synthetic-ge3-call","name":"synthetic_tool","response":{"text":CONTACT}}});
    let mut cases = vec![
        (vec![call.clone(), call.clone()], vec![result.clone()]),
        (vec![call.clone()], vec![result.clone(), result.clone()]),
    ];
    for id in [json!(""), json!(7), Value::Null, json!("x".repeat(513))] {
        let mut c = call.clone();
        c["functionCall"]["id"] = id.clone();
        let mut r = result.clone();
        r["functionResponse"]["id"] = id;
        cases.push((vec![c], vec![r]));
    }
    let mut no_call_id = call.clone();
    no_call_id["functionCall"]
        .as_object_mut()
        .ok_or("ge3_call_missing")?
        .remove("id");
    let mut no_result_id = result.clone();
    no_result_id["functionResponse"]
        .as_object_mut()
        .ok_or("ge3_result_missing")?
        .remove("id");
    cases.extend([
        (vec![no_call_id.clone()], vec![result.clone()]),
        (vec![call.clone()], vec![no_result_id.clone()]),
        (
            vec![no_call_id.clone(), no_call_id.clone()],
            vec![no_result_id.clone(), no_result_id.clone()],
        ),
    ]);
    let mut mismatched = result.clone();
    mismatched["functionResponse"]["name"] = json!("other_tool");
    cases.push((vec![call.clone()], vec![mismatched]));
    let mut unknown = result.clone();
    unknown["functionResponse"]["id"] = json!("synthetic-unknown");
    cases.push((vec![call.clone()], vec![unknown]));
    for field in ["args", "name"] {
        let mut malformed = call.clone();
        malformed["functionCall"][field] = Value::Null;
        cases.push((vec![malformed], vec![result.clone()]));
    }
    let mut malformed = result.clone();
    malformed["functionResponse"]["response"] = json!([]);
    cases.push((vec![call.clone()], vec![malformed]));
    let mixed =
        json!({"functionCall":call["functionCall"],"functionResponse":result["functionResponse"]});
    cases.extend([
        (vec![mixed.clone()], vec![result.clone()]),
        (vec![call.clone()], vec![mixed]),
    ]);
    for (calls, results) in cases {
        let request =
            json!({"contents":[{"role":"model","parts":calls},{"role":"user","parts":results}]});
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = ge3_observe(&request, &budget)?;
        assert!(
            !observed
                .history()
                .iter()
                .any(|claim| matches!(claim, HistoryClaim::CallResultPair { .. })),
            "ge3_ambiguous_native_pair_authorized"
        );
        drop(observed);
        assert!(budget.used() == 0, "ge3_ambiguous_pair_budget_retained");
    }
    let request = json!({"contents":[{"role":"model","parts":[call]},{"role":"user","parts":[{"text":"Synthetic gap"}]},{"role":"user","parts":[result]}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = ge3_observe(&request, &budget)?;
    assert!(
        !observed
            .history()
            .iter()
            .any(|claim| matches!(claim, HistoryClaim::CallResultPair { .. })),
        "ge3_nonadjacent_pair_authorized"
    );
    drop(observed);
    assert!(budget.used() == 0, "ge3_nonadjacent_pair_budget_retained");
    Ok(())
}

#[test]
fn ge3_lane_pair_metadata_exact_capacity_and_work_ceiling_refund() -> TestResult {
    let request = json!({"contents":[{"role":"model","parts":[{"functionCall":{"name":"synthetic_tool","args":{"query":"public"}}}]},{"role":"user","parts":[{"functionResponse":{"name":"synthetic_tool","response":{"text":CONTACT}}}]}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = ge3_observe(&request, &budget)?;
    let measured = budget.used();
    drop(observed);
    assert!(
        measured > 0 && budget.used() == 0,
        "ge3_pair_metadata_not_charged_or_refunded"
    );
    for limit in [0, 1, measured - 1, measured] {
        let budget = PayloadBudget::with_limit(limit);
        let result = super::history::observe_request(
            &Method::POST,
            &JSON_ROUTE.parse()?,
            &request,
            budget.clone(),
        );
        assert!(
            result.is_ok() == (limit == measured),
            "ge3_pair_metadata_exact_capacity_wrong"
        );
        drop(result);
        assert!(
            budget.used() == 0,
            "ge3_pair_metadata_failure_refund_missing"
        );
    }
    // Keep every native ID valid while exceeding the bounded comparison work.
    let calls:Vec<Value> = (0..160).map(|index| json!({"functionCall":{"id":format!("{index:03}{}","x".repeat(253)),"name":"synthetic_tool","args":{}}})).collect();
    let results:Vec<Value> = calls.iter().map(|call| json!({"functionResponse":{"id":call["functionCall"]["id"],"name":"synthetic_tool","response":{}}})).collect();
    let request =
        json!({"contents":[{"role":"model","parts":calls},{"role":"user","parts":results}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let baseline = super::request::observe_request(
        &Method::POST,
        &JSON_ROUTE.parse()?,
        &request,
        budget.clone(),
    )
    .map_err(|_| "ge3_work_fixture_request_observer_rejected")?;
    drop(baseline);
    let result = super::history::observe_request(
        &Method::POST,
        &JSON_ROUTE.parse()?,
        &request,
        budget.clone(),
    );
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::CapacityExceeded))),
        "ge3_pair_work_ceiling_missing"
    );
    drop(result);
    assert!(budget.used() == 0, "ge3_pair_work_failure_refund_missing");
    Ok(())
}

#[test]
fn ge3_lane_sse_exact_replay_requires_post_last_yield_continuation() -> TestResult {
    use crate::proxy::security::restoration_core::compose_gemini_snapshots;
    use futures::StreamExt;
    let fixture = Ge2Fixture::new(true)?;
    let baseline = fixture.budget.used();
    let events = vec![
        json!({"candidates":[{"index":7,"content":{"role":"model","parts":[{"text":fixture.reference},{"functionCall":{"id":"synthetic-ge3-call","name":"synthetic_tool","args":{"query":fixture.reference}}},{"text":"Synthetic public thought","thoughtSignature":"synthetic-ge3-signature"}]}},{"index":2,"content":{"role":"model","parts":[{"text":"Synthetic second"}]}}]}),
        json!({"candidates":[{"index":7,"content":{"role":"model","parts":[{"text":"","thoughtSignature":"synthetic-late-signature"}]}}]}),
        json!({"usageMetadata":{"totalTokenCount":9}}),
        json!({"candidates":[{"index":2,"finishReason":"STOP"},{"index":7,"finishReason":"STOP"}]}),
    ];
    let wire = ge2_sse_wire(&events)?;
    let mut stream = ge2_finalize_sse(&fixture, &wire)?.into_delivery_stream();
    let (mut next, binding) = ge3_next(
        &fixture,
        Some("synthetic-ge1-session"),
        Protocol::Gemini,
        "synthetic-ge2-provider",
    )?;
    let mut chunks = Vec::new();
    let mut frames = Vec::new();
    for _ in 0..events.len() {
        let chunk = futures::executor::block_on(stream.next()).ok_or("ge3_sse_frame_missing")??;
        for data in std::str::from_utf8(&chunk)?
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
        {
            frames.push(serde_json::from_str::<Value>(data)?);
        }
        chunks.push(chunk);
        assert!(
            fixture.counts()? == (0, 1, 1),
            "ge3_sse_published_during_yields"
        );
    }
    let refs: Vec<&Value> = frames
        .iter()
        .filter_map(|frame| frame["candidates"].as_array())
        .flat_map(|candidates| candidates.iter())
        .collect();
    let delivered = compose_gemini_snapshots(&refs, &fixture.budget)
        .map_err(|_| "ge3_sse_composition_rejected")?;
    assert!(
        super::history::import_delivered(&mut next, &binding, delivered.as_value()).is_err()
            && next.resolve(&fixture.reference).is_err(),
        "ge3_pending_sse_history_imported"
    );
    assert!(
        futures::executor::block_on(stream.next()).is_none(),
        "ge3_sse_extra_yield"
    );
    super::history::import_delivered(&mut next, &binding, delivered.as_value())?;
    assert!(
        next.resolve(&fixture.reference)? == CONTACT && fixture.counts()? == (1, 0, 0),
        "ge3_sse_completed_history_not_imported"
    );
    let (mut other, binding) = ge3_next(
        &fixture,
        Some("synthetic-ge1-session"),
        Protocol::Gemini,
        "synthetic-ge2-provider",
    )?;
    assert!(
        super::history::import_delivered(&mut other, &binding, &json!([delivered.as_value()[0]]))
            .is_err()
            && other.resolve(&fixture.reference).is_err(),
        "ge3_sse_truncated_history_imported"
    );
    drop(delivered);
    drop(stream);
    assert!(
        fixture.budget.used() > baseline,
        "ge3_live_sse_bytes_lost_budget_owner"
    );
    drop(chunks);
    assert!(
        fixture.budget.used() == baseline,
        "ge3_sse_delivery_budget_not_refunded"
    );
    Ok(())
}

#[test]
fn ge3_lane_eviction_removes_history_privilege_not_current_dlp() -> TestResult {
    use std::time::{Duration, Instant};
    let fixture = Ge2Fixture::new(true)?;
    let delivered = ge3_deliver_json(&fixture, &ge3_source(&fixture.reference))?;
    let store = fixture.store.clone();
    let reference = fixture.reference.clone();
    let revision = fixture.binding.policy_revision;
    let budget = fixture.budget.clone();
    drop(fixture);
    assert!(budget.used() == 0, "ge3_eviction_fixture_budget_retained");
    store.expire_idle_at(Instant::now() + Duration::from_secs(7 * 24 * 60 * 60))?;
    assert!(store.stats()?.namespaces == 0, "ge3_namespace_not_evicted");
    let mut next = store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Gemini),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-ge1-app",
        Some("synthetic-ge1-session"),
        revision,
    )?;
    let binding = next.bind_actual_attempt(
        "synthetic-ge2-provider",
        "gemini",
        "https://synthetic.invalid/v1beta/models/synthetic:generateContent",
        Some("synthetic-ge2-principal"),
    )?;
    assert!(
        super::history::import_delivered(&mut next, &binding, &delivered).is_err()
            && next.resolve(&reference).is_err(),
        "ge3_evicted_history_authorized"
    );
    let request = json!({"contents":[{"role":"user","parts":[{"text":CONTACT}]}]});
    let observed = ge3_observe(&request, &budget)?;
    let prepared = prepare_observed_request(
        &mut next,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| Ok(()),
    )
    .map_err(|_| "ge3_eviction_broke_current_dlp")?;
    assert!(
        prepared.reversible_occurrences == 1
            && !prepared.body.as_value()["contents"][0]["parts"][0]["text"]
                .as_str()
                .ok_or("ge3_current_text_missing")?
                .contains(CONTACT),
        "ge3_current_dlp_not_applied_after_eviction"
    );
    drop(prepared);
    drop(observed);
    assert!(budget.used() == 0, "ge3_eviction_current_budget_retained");
    Ok(())
}

#[test]
fn ge3_lane_m1_native_id_matches_frozen_shared_label_contract() -> TestResult {
    use crate::proxy::security::restoration_contract::HistoryClaim;
    for (id, valid) in [
        ("synthetic-valid".to_owned(), true),
        ("x".repeat(256), true),
        ("é".repeat(128), true),
        (String::new(), false),
        ("x".repeat(257), false),
        ("é".repeat(129), false),
        ("synthetic id".to_owned(), false),
        ("synthetic\tid".to_owned(), false),
        ("synthetic\nid".to_owned(), false),
        ("synthetic\0id".to_owned(), false),
        ("synthetic\u{007f}id".to_owned(), false),
        ("synthetic\u{00a0}id".to_owned(), false),
        ("synthetic\u{2003}id".to_owned(), false),
    ] {
        let request = json!({"contents":[
            {"role":"model","parts":[{"functionCall":{"id":id,"name":"synthetic_tool","args":{"query":"public"}}}]},
            {"role":"user","parts":[{"functionResponse":{"id":id,"name":"synthetic_tool","response":{"text":CONTACT}}}]}
        ]});
        let original = request.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = ge3_observe(&request, &budget)?;
        let paired = observed
            .history()
            .iter()
            .filter(|claim| matches!(claim, HistoryClaim::CallResultPair { .. }))
            .count();
        assert!(
            paired == usize::from(valid),
            "ge3_m1_native_id_contract_mismatch"
        );
        assert!(
            observed.sites().len() == 2 && request == original,
            "ge3_m1_native_id_changed_text_observation"
        );
        drop(observed);
        assert!(budget.used() == 0, "ge3_m1_native_id_budget_retained");
    }
    Ok(())
}

#[test]
fn ge3_lane_m1_names_match_frozen_shared_label_contract_with_or_without_ids() -> TestResult {
    use crate::proxy::security::restoration_contract::HistoryClaim;
    for (name, valid) in [
        ("synthetic_tool".to_owned(), true),
        ("x".repeat(256), true),
        ("é".repeat(128), true),
        (String::new(), false),
        ("x".repeat(257), false),
        ("é".repeat(129), false),
        ("synthetic tool".to_owned(), false),
        ("synthetic\ttool".to_owned(), false),
        ("synthetic\ntool".to_owned(), false),
        ("synthetic\0tool".to_owned(), false),
        ("synthetic\u{007f}tool".to_owned(), false),
        ("synthetic\u{00a0}tool".to_owned(), false),
        ("synthetic\u{2003}tool".to_owned(), false),
    ] {
        for with_id in [false, true] {
            let mut call = json!({"functionCall":{"name":name,"args":{"query":"public"}}});
            let mut result = json!({"functionResponse":{"name":name,"response":{"text":CONTACT}}});
            if with_id {
                call["functionCall"]["id"] = json!("synthetic-ge3-call");
                result["functionResponse"]["id"] = call["functionCall"]["id"].clone();
            }
            let request = json!({"contents":[{"role":"model","parts":[call]},{"role":"user","parts":[result]}]});
            let original = request.clone();
            let budget = PayloadBudget::with_limit(BUDGET_BYTES);
            let observed = ge3_observe(&request, &budget)?;
            let paired = observed
                .history()
                .iter()
                .filter(|claim| matches!(claim, HistoryClaim::CallResultPair { .. }))
                .count();
            assert!(
                paired == usize::from(valid),
                "ge3_m1_native_name_contract_mismatch"
            );
            assert!(
                observed.sites().len() == 2 && request == original,
                "ge3_m1_native_name_changed_text_observation"
            );
            drop(observed);
            assert!(budget.used() == 0, "ge3_m1_native_name_budget_retained");
        }
    }
    Ok(())
}
