//! Native SSE staging targets; fixture selections never grant completion or delivery authority.
use std::{cell::Cell, rc::Rc};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{
    stage_native_sse, NativeSseFrameObservation, NativeSseObservation, NativeSseSnapshotSelection,
    StagedNativeSse,
};
use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        completed_context::ActualAttemptBinding,
        payload_budget::PayloadBudget,
        payload_buffer::test_support as buffer_events,
        payload_json::{test_support as json_events, PayloadJson},
        payload_metadata::test_support as metadata_events,
        response_restoration::RestoreError,
        restoration_contract::{
            ClientProtocol, ContinuityScope, EditConstraint, IngressNamespace, SourceClass,
            TextSites,
        },
        tokenization::{DlpRequestLease, DlpTokenStore, TokenCategory, TokenFragment},
    },
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const BUDGET_BYTES: usize = 16 * 1024 * 1024;
const CONTACT: &str = "13900000000";
const QUOTED: &str = "synthetic-\"quoted\nline🔒";
const UNKNOWN: &str = "rredact-cn_phone-00000000000000000000-ai";
const DONE: &str = ": synthetic done\r\nid: synthetic-done\r\ndata: [DONE]\r\n\r\n";

fn require<T>(result: Result<T, RestoreError>) -> TestResult<T> {
    result.map_err(|_| "synthetic_native_sse_stage_rejected".into())
}

struct Fixture {
    store: DlpTokenStore,
    lease: DlpRequestLease,
    binding: ActualAttemptBinding,
    references: Vec<String>,
    sources: [Value; 3],
    expected: [Value; 3],
    entity: String,
    ends: [usize; 3],
    mapped: bool,
}

impl Fixture {
    fn new(mapped: bool) -> TestResult<Self> {
        let store = DlpTokenStore::new();
        let mut lease = store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: ClientProtocol::new(Protocol::Chat),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-c5-sse-staging-app",
            Some("synthetic-c5-sse-staging-session"),
            1,
        )?;
        let references = if mapped {
            lease.prepare(
                &[
                    TokenFragment {
                        raw: CONTACT,
                        whole_span_categories: &[TokenCategory::Phone],
                    },
                    TokenFragment {
                        raw: QUOTED,
                        whole_span_categories: &[TokenCategory::Phone],
                    },
                ],
                &[],
                |tokens| Ok(tokens.to_vec()),
            )?
        } else {
            vec![UNKNOWN.to_owned(), "synthetic-unmapped-🔒".to_owned()]
        };
        let binding = lease.bind_actual_attempt(
            "synthetic-provider",
            "chat",
            "https://synthetic.invalid/v1/chat/completions",
            None,
        )?;
        let first = json!({
            "id":"synthetic-first", "object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{
                "content":format!("pre🔒{} / {}post", references[0], references[0]),
                "tool_calls":[{"index":0,"id":"synthetic-call","type":"function",
                    "function":{"name":"synthetic-tool",
                        "arguments":format!("{{\"contact\":\"{}", references[0])}}],
                "unknown":references[0]}}],
            "unknown":{"literal":references[0],"types":[true,null,7,1.0]}
        });
        let second = json!({
            "id":"synthetic-second", "object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{"content":references[1]}}],
            "unknown":references[1]
        });
        let third = json!({"unknown":references[0],"number":1.0});
        let sources = [first, second, third];
        let mut expected = sources.clone();
        if mapped {
            expected[0]["choices"][0]["delta"]["content"] =
                json!(format!("pre🔒{CONTACT} / {CONTACT}post"));
            expected[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"] =
                json!(format!("{{\"contact\":\"{CONTACT}"));
            expected[1]["choices"][0]["delta"]["content"] = json!(QUOTED);
        }
        let first = format!(
            ": synthetic metadata\r\nid: synthetic-first\r\nevent: synthetic-delta\r\ndata: {}\r\n\r\n",
            sources[0]
        );
        let second = format!("data:{}\n\n", sources[1]);
        let third = format!("event: opaque\ndata: {}\n\n", sources[2]);
        let ends = [
            first.len(),
            first.len() + second.len(),
            first.len() + second.len() + third.len(),
        ];
        let entity = format!("{first}{second}{third}{DONE}");
        Ok(Self {
            store,
            lease,
            binding,
            references,
            sources,
            expected,
            entity,
            ends,
            mapped,
        })
    }

    fn frames(
        &self,
        budget: &PayloadBudget,
        second_constraint: EditConstraint,
    ) -> TestResult<[NativeSseFrameObservation<'_>; 4]> {
        let mut first_sites = TextSites::new(budget.clone())?;
        first_sites.observe(
            &self.sources[0]["choices"][0]["delta"]["content"],
            SourceClass::HistoricalText,
            EditConstraint::Editable,
        )?;
        // A real streamed argument fragment is intentionally not complete JSON here.
        first_sites.observe(
            &self.sources[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
            SourceClass::ToolArguments,
            EditConstraint::Editable,
        )?;
        let mut second_sites = TextSites::new(budget.clone())?;
        second_sites.observe(
            &self.sources[1]["choices"][0]["delta"]["content"],
            SourceClass::HistoricalText,
            second_constraint,
        )?;
        Ok([
            require(NativeSseFrameObservation::new(
                &self.entity[..self.ends[0]],
                Some(&self.sources[0]),
                first_sites,
                budget.clone(),
            ))?,
            require(NativeSseFrameObservation::new(
                &self.entity[self.ends[0]..self.ends[1]],
                Some(&self.sources[1]),
                second_sites,
                budget.clone(),
            ))?,
            require(NativeSseFrameObservation::new(
                &self.entity[self.ends[1]..self.ends[2]],
                Some(&self.sources[2]),
                TextSites::new(budget.clone())?,
                budget.clone(),
            ))?,
            require(NativeSseFrameObservation::new(
                &self.entity[self.ends[2]..],
                None,
                TextSites::new(budget.clone())?,
                budget.clone(),
            ))?,
        ])
    }

    fn observe<'a>(
        &'a self,
        frames: &'a [NativeSseFrameObservation<'a>; 4],
        budget: &PayloadBudget,
    ) -> TestResult<NativeSseObservation<'a>> {
        require(NativeSseObservation::new(
            self.entity.as_bytes(),
            &[&frames[0], &frames[1], &frames[2], &frames[3]],
            ClientProtocol::new(Protocol::Chat),
            &self.lease,
            &self.binding,
            budget.clone(),
        ))
    }

    fn snapshots(&self) -> [NativeSseSnapshotSelection<'_>; 4] {
        [
            NativeSseSnapshotSelection::new(1, &self.sources[1]["choices"][0]["delta"]),
            NativeSseSnapshotSelection::new(0, &self.sources[0]["choices"][0]["delta"]),
            NativeSseSnapshotSelection::new(
                0,
                &self.sources[0]["choices"][0]["delta"]["tool_calls"][0],
            ),
            NativeSseSnapshotSelection::new(0, &self.sources[0]["choices"][0]["delta"]),
        ]
    }

    fn stage<'a>(
        &'a self,
        observed: &NativeSseObservation<'a>,
        budget: &PayloadBudget,
    ) -> Result<StagedNativeSse<'a>, RestoreError> {
        stage_native_sse(
            observed,
            &self.snapshots(),
            &self.lease,
            &self.binding,
            budget.clone(),
        )
    }
}

fn assert_staged(stage: &StagedNativeSse<'_>, fixture: &Fixture) -> TestResult {
    assert!(
        std::ptr::eq(stage.entity(), fixture.entity.as_bytes()),
        "stage_entity_identity_lost"
    );
    assert!(
        std::ptr::eq(stage.binding(), &fixture.binding),
        "stage_binding_borrow_replaced"
    );
    assert!(
        stage.client() == ClientProtocol::new(Protocol::Chat),
        "stage_client_changed"
    );
    assert!(stage.changed() == fixture.mapped, "stage_change_flag_wrong");
    assert!(
        stage.restored_occurrences() == if fixture.mapped { 4 } else { 0 },
        "stage_count_wrong"
    );
    let dependencies = stage.output_tokens();
    assert!(
        dependencies.len() == if fixture.mapped { 2 } else { 0 },
        "stage_dependencies_not_unique"
    );
    if fixture.mapped {
        assert!(
            dependencies[0].as_str() == fixture.references[0]
                && dependencies[1].as_str() == fixture.references[1],
            "dependency_order_or_value_changed"
        );
    }
    let expected = json!([
        fixture.expected[1]["choices"][0]["delta"],
        fixture.expected[0]["choices"][0]["delta"],
        fixture.expected[0]["choices"][0]["delta"]["tool_calls"][0],
        fixture.expected[0]["choices"][0]["delta"]
    ]);
    assert!(
        stage.delivered() == &expected,
        "snapshots_not_projected_from_restored_exact_nodes"
    );
    assert!(
        stage.frame_count() == 4 && stage.source_frame(4).is_none(),
        "staged_frame_count_changed"
    );
    let mut digest = Sha256::new();
    digest.update(b"native-sse-staging-wire-v1\0");
    for (index, frame) in stage.frames.as_slice().iter().enumerate() {
        let source = stage
            .source_frame(index)
            .ok_or("synthetic_source_frame_missing")?;
        assert!(
            std::ptr::eq(frame.source, source),
            "frame_observation_identity_lost"
        );
        let encoded = std::str::from_utf8(frame.encoded.as_slice())?;
        digest.update((encoded.len() as u64).to_le_bytes());
        digest.update(encoded.as_bytes());
        if index == 3 {
            assert!(
                frame.candidate.is_none() && encoded == DONE,
                "opaque_done_reencoded"
            );
            continue;
        }
        let candidate = frame
            .candidate
            .as_ref()
            .ok_or("synthetic_candidate_missing")?;
        assert!(
            candidate.as_value() == &fixture.expected[index],
            "candidate_changed_unselected_fields"
        );
        let data = encoded
            .lines()
            .filter_map(|line| {
                line.strip_prefix("data:")
                    .map(|value| value.strip_prefix(' ').unwrap_or(value))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let parsed =
            PayloadJson::try_from_str(&data, PayloadBudget::with_limit(BUDGET_BYTES), BUDGET_BYTES)
                .map_err(|_| "synthetic_final_frame_not_strict_json")?;
        assert!(
            parsed.as_value() == &fixture.expected[index],
            "wire_did_not_encode_actual_restoration"
        );
        if !fixture.mapped || index == 2 {
            assert!(encoded == source.raw(), "unchanged_wire_not_preserved");
        }
        if index == 0 {
            assert!(
                encoded.starts_with(
                    ": synthetic metadata\r\nid: synthetic-first\r\nevent: synthetic-delta\r\n"
                ) && encoded.ends_with("\r\n\r\n"),
                "sse_metadata_or_delimiter_changed"
            );
        }
    }
    let actual: [u8; 32] = digest.finalize().into();
    assert!(
        stage.release_digest() == actual,
        "release_digest_not_bound_to_actual_ordered_wire"
    );
    Ok(())
}

#[test]
fn c5_native_sse_staging_upstream_fixture_contract() -> TestResult {
    for mapped in [true, false] {
        let fixture = Fixture::new(mapped)?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let frames = fixture.frames(&budget, EditConstraint::Editable)?;
        let before = fixture.store.stats()?;
        let baseline = budget.used();
        let observed = fixture.observe(&frames, &budget)?;
        assert!(
            observed.frame_count() == 4,
            "synthetic_upstream_frame_count_wrong"
        );
        assert!(
            std::ptr::eq(observed.entity(), fixture.entity.as_bytes()),
            "synthetic_entity_not_exact"
        );
        assert!(
            std::ptr::eq(observed.binding(), &fixture.binding),
            "synthetic_binding_not_exact"
        );
        for (index, frame) in frames.iter().enumerate() {
            assert!(
                observed
                    .frame(index)
                    .is_some_and(|value| std::ptr::eq(value, frame)),
                "synthetic_frame_not_exact"
            );
        }
        assert!(
            fixture
                .lease
                .require_native_attempt_binding(observed.client(), observed.binding())
                .is_ok(),
            "synthetic_current_binding_rejected"
        );
        for reference in &fixture.references {
            assert!(
                fixture.lease.resolve_if_authorized(reference)?.is_some() == mapped,
                "synthetic_mapping_precondition_wrong"
            );
        }
        assert!(
            fixture.snapshots().len() == 4
                && frames[0].sites().len() == 2
                && frames[1].sites().len() == 1,
            "synthetic_adapter_selection_wrong"
        );
        let partial = fixture.sources[0]["choices"][0]["delta"]["tool_calls"][0]["function"]
            ["arguments"]
            .as_str()
            .ok_or("synthetic_argument_missing")?;
        assert!(
            serde_json::from_str::<Value>(partial).is_err(),
            "synthetic_argument_is_not_fragment"
        );
        assert!(
            fixture.store.stats()? == before,
            "upstream_fixture_published"
        );
        drop(observed);
        assert!(
            budget.used() == baseline,
            "upstream_observation_not_refunded"
        );
        drop(frames);
        assert!(budget.used() == 0, "upstream_sites_not_refunded");
    }
    Ok(())
}

#[test]
fn c5_native_sse_staging_restores_selected_frames_runtime_red() -> TestResult {
    let fixture = Fixture::new(true)?;
    let original = fixture.sources.clone();
    let original_entity = fixture.entity.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let frames = fixture.frames(&budget, EditConstraint::Editable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let baseline = budget.used();
    let before = fixture.store.stats()?;
    let stage = require(fixture.stage(&observed, &budget))?;
    assert_staged(&stage, &fixture)?;
    for (index, frame) in frames.iter().enumerate() {
        assert!(
            stage
                .source_frame(index)
                .is_some_and(|value| std::ptr::eq(value, frame)),
            "ordered_frame_borrow_replaced"
        );
    }
    let encoded_bytes: usize = stage
        .frames
        .as_slice()
        .iter()
        .map(|frame| frame.encoded.len())
        .sum();
    assert!(
        encoded_bytes > 0
            && budget.used() >= baseline + encoded_bytes
            && stage.budget().used() == budget.used(),
        "retained_wire_not_fully_charged"
    );
    assert!(
        fixture.sources == original
            && fixture.entity == original_entity
            && fixture.store.stats()? == before,
        "staging_mutated_input_or_published"
    );
    // The independent owner must not borrow the observation's metadata allocation.
    drop(observed);
    assert_staged(&stage, &fixture)?;
    drop(stage);
    drop(frames);
    assert!(
        budget.used() == 0 && fixture.store.stats()? == before,
        "stage_drop_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_native_sse_staging_zero_map_and_unknown_immutable_literals_still_encode() -> TestResult {
    let fixture = Fixture::new(false)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let frames = fixture.frames(&budget, EditConstraint::Immutable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let baseline = budget.used();
    let before = fixture.store.stats()?;
    let allocations = Rc::new(Cell::new(0));
    let seen = Rc::clone(&allocations);
    let hook = buffer_events::install(move |event| {
        if event.phase == buffer_events::Phase::BeforeAllocation {
            seen.set(seen.get() + 1);
            assert!(
                event.requested > 0 && event.used >= baseline + event.requested,
                "unchanged_encoding_not_precharged"
            );
        }
        buffer_events::Action::Continue
    });
    let result = fixture.stage(&observed, &budget);
    drop(hook);
    let stage = require(result)?;
    assert!(
        allocations.get() >= 4,
        "unchanged_or_done_frame_encoding_bypassed"
    );
    assert_staged(&stage, &fixture)?;
    let encoded_bytes: usize = stage
        .frames
        .as_slice()
        .iter()
        .map(|frame| frame.encoded.len())
        .sum();
    assert!(
        budget.used() >= baseline + encoded_bytes,
        "unchanged_wire_charge_missing"
    );
    drop(stage);
    assert!(
        budget.used() == baseline && fixture.store.stats()? == before,
        "zero_map_stage_leaked"
    );
    Ok(())
}

#[test]
fn c5_native_sse_staging_snapshot_identity_not_equal_value_or_frame_index() -> TestResult {
    let fixture = Fixture::new(true)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let frames = fixture.frames(&budget, EditConstraint::Editable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let baseline = budget.used();
    let before = fixture.store.stats()?;
    let foreign = fixture.sources[0]["choices"][0]["delta"].clone();
    for selection in [
        NativeSseSnapshotSelection::new(0, &foreign),
        NativeSseSnapshotSelection::new(1, &fixture.sources[0]["choices"][0]["delta"]),
        NativeSseSnapshotSelection::new(3, &fixture.sources[0]["choices"][0]["delta"]),
        NativeSseSnapshotSelection::new(4, &fixture.sources[0]["choices"][0]["delta"]),
    ] {
        let result = stage_native_sse(
            &observed,
            &[selection],
            &fixture.lease,
            &fixture.binding,
            budget.clone(),
        );
        assert!(result.is_err(), "foreign_or_wrong_frame_snapshot_accepted");
        drop(result);
        assert!(
            budget.used() == baseline && fixture.store.stats()? == before,
            "snapshot_identity_rejection_not_atomic"
        );
    }
    let stage = require(fixture.stage(&observed, &budget))?;
    assert_staged(&stage, &fixture)?;
    Ok(())
}

#[test]
fn c5_native_sse_staging_snapshot_count_is_nonempty_and_bounded() -> TestResult {
    let fixture = Fixture::new(false)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let frames = fixture.frames(&budget, EditConstraint::Editable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let baseline = budget.used();
    for count in [0, 129, 128] {
        let selections: Vec<_> = (0..count)
            .map(|_| NativeSseSnapshotSelection::new(0, &fixture.sources[0]["choices"][0]["delta"]))
            .collect();
        let result = stage_native_sse(
            &observed,
            &selections,
            &fixture.lease,
            &fixture.binding,
            budget.clone(),
        );
        if count == 128 {
            let stage = require(result)?;
            assert!(
                stage
                    .delivered()
                    .as_array()
                    .is_some_and(|values| values.len() == 128),
                "snapshot_upper_boundary_rejected_or_deduplicated"
            );
            drop(stage);
        } else {
            assert!(result.is_err(), "invalid_snapshot_count_accepted");
            drop(result);
        }
        assert!(
            budget.used() == baseline,
            "snapshot_count_failure_not_refunded"
        );
    }
    Ok(())
}

#[test]
fn c5_native_sse_staging_equal_siblings_project_only_exact_selected_source() -> TestResult {
    let fixture = Fixture::new(true)?;
    let source = json!({"selected":{"text":fixture.references[0]},
        "equal":{"text":fixture.references[0]}});
    let raw = format!("data: {source}\n\n");
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut sites = TextSites::new(budget.clone())?;
    sites.observe(
        &source["selected"]["text"],
        SourceClass::HistoricalText,
        EditConstraint::Editable,
    )?;
    let frame = require(NativeSseFrameObservation::new(
        &raw,
        Some(&source),
        sites,
        budget.clone(),
    ))?;
    let observed = require(NativeSseObservation::new(
        raw.as_bytes(),
        &[&frame],
        ClientProtocol::new(Protocol::Chat),
        &fixture.lease,
        &fixture.binding,
        budget.clone(),
    ))?;
    let stage = require(stage_native_sse(
        &observed,
        &[
            NativeSseSnapshotSelection::new(0, &source["equal"]),
            NativeSseSnapshotSelection::new(0, &source["selected"]),
        ],
        &fixture.lease,
        &fixture.binding,
        budget.clone(),
    ))?;
    assert!(
        stage.delivered()
            == &json!([
                {"text":fixture.references[0]}, {"text":CONTACT}
            ]),
        "equal_value_used_instead_of_exact_snapshot_identity"
    );
    assert!(
        stage.restored_occurrences() == 1 && stage.output_tokens().len() == 1,
        "equal_sibling_added_dependency_or_occurrence"
    );
    Ok(())
}

#[test]
fn c5_native_sse_staging_late_immutable_failure_refunds_dependency_ledger() -> TestResult {
    let fixture = Fixture::new(true)?;
    let original = fixture.sources.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let frames = fixture.frames(&budget, EditConstraint::Immutable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let baseline = budget.used();
    let before = fixture.store.stats()?;
    let result = fixture.stage(&observed, &budget);
    assert!(result.is_err(), "immutable_required_restore_escaped");
    drop(result);
    assert!(
        budget.used() == baseline
            && fixture.sources == original
            && fixture.store.stats()? == before,
        "immutable_failure_not_atomic"
    );
    drop(observed);
    drop(frames);
    let frames = fixture.frames(&budget, EditConstraint::Editable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let stage = require(fixture.stage(&observed, &budget))?;
    assert_staged(&stage, &fixture)?;
    Ok(())
}

#[test]
fn c5_native_sse_staging_foreign_binding_and_lease_reject_then_retry() -> TestResult {
    let fixture = Fixture::new(true)?;
    let foreign = Fixture::new(true)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let frames = fixture.frames(&budget, EditConstraint::Editable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let baseline = budget.used();
    let before = fixture.store.stats()?;
    for altered in [
        foreign.binding.clone(),
        ActualAttemptBinding {
            store_id: fixture.binding.store_id.wrapping_add(1),
            ..fixture.binding.clone()
        },
        ActualAttemptBinding {
            namespace: fixture.binding.namespace.wrapping_add(1),
            ..fixture.binding.clone()
        },
        ActualAttemptBinding {
            generation: [0x7f; 16],
            ..fixture.binding.clone()
        },
        ActualAttemptBinding {
            policy_revision: fixture.binding.policy_revision.wrapping_add(1),
            ..fixture.binding.clone()
        },
        ActualAttemptBinding {
            provider_fingerprint: [0x7f; 32],
            ..fixture.binding.clone()
        },
    ] {
        let result = stage_native_sse(
            &observed,
            &fixture.snapshots(),
            &fixture.lease,
            &altered,
            budget.clone(),
        );
        assert!(
            result.is_err(),
            "foreign_or_observation_mismatched_binding_accepted"
        );
        drop(result);
        assert!(
            budget.used() == baseline && fixture.store.stats()? == before,
            "binding_failure_not_atomic"
        );
    }
    let result = stage_native_sse(
        &observed,
        &fixture.snapshots(),
        &foreign.lease,
        &fixture.binding,
        budget.clone(),
    );
    assert!(result.is_err(), "foreign_lease_accepted");
    drop(result);
    assert!(budget.used() == baseline, "foreign_lease_charge_retained");
    let stage = require(fixture.stage(&observed, &budget))?;
    assert_staged(&stage, &fixture)?;
    Ok(())
}

#[test]
fn c5_native_sse_staging_full_budget_failure_refunds_then_retry() -> TestResult {
    let fixture = Fixture::new(true)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let frames = fixture.frames(&budget, EditConstraint::Editable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let held = budget.reserve(budget.limit() - budget.used())?;
    let baseline = budget.used();
    let before = fixture.store.stats()?;
    let result = fixture.stage(&observed, &budget);
    assert!(result.is_err(), "full_budget_stage_accepted");
    drop(result);
    assert!(
        budget.used() == baseline && fixture.store.stats()? == before,
        "capacity_failure_not_atomic"
    );
    drop(held);
    let stage = require(fixture.stage(&observed, &budget))?;
    assert_staged(&stage, &fixture)?;
    Ok(())
}

#[test]
fn c5_native_sse_staging_late_candidate_and_snapshot_build_faults_refund() -> TestResult {
    // Three real JSON candidates followed by the ordered snapshot projection.
    for fail_at in [2, 4] {
        let fixture = Fixture::new(true)?;
        let original = fixture.sources.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let frames = fixture.frames(&budget, EditConstraint::Editable)?;
        let observed = fixture.observe(&frames, &budget)?;
        let baseline = budget.used();
        let before = fixture.store.stats()?;
        let builds = Rc::new(Cell::new(0));
        let seen = Rc::clone(&builds);
        let hook = json_events::install(move |event| {
            if event.phase == json_events::Phase::BeforeBuild {
                seen.set(seen.get() + 1);
                assert!(
                    event.charged > 0 && event.used >= baseline + event.charged,
                    "late_json_build_not_precharged"
                );
                if seen.get() == fail_at {
                    return json_events::Action::Fail;
                }
            }
            json_events::Action::Continue
        });
        let result = fixture.stage(&observed, &budget);
        drop(hook);
        assert!(
            builds.get() >= fail_at,
            "late_candidate_or_projection_not_reached"
        );
        assert!(result.is_err(), "late_json_build_fault_ignored");
        drop(result);
        assert!(
            budget.used() == baseline
                && fixture.sources == original
                && fixture.store.stats()? == before,
            "late_build_failure_not_atomic"
        );
        let stage = require(fixture.stage(&observed, &budget))?;
        assert_staged(&stage, &fixture)?;
        drop(stage);
        assert!(
            budget.used() == baseline,
            "late_build_retry_charge_retained"
        );
    }
    Ok(())
}

#[test]
fn c5_native_sse_staging_changed_and_unchanged_encoding_faults_refund() -> TestResult {
    for mapped in [true, false] {
        for phase in [
            buffer_events::Phase::BeforeAllocation,
            buffer_events::Phase::BeforeCopy,
        ] {
            let fixture = Fixture::new(mapped)?;
            let budget = PayloadBudget::with_limit(BUDGET_BYTES);
            let frames = fixture.frames(&budget, EditConstraint::Editable)?;
            let observed = fixture.observe(&frames, &budget)?;
            let baseline = budget.used();
            let before = fixture.store.stats()?;
            let built = Rc::new(Cell::new(false));
            let seen = Rc::clone(&built);
            let json_hook = json_events::install(move |event| {
                if event.phase == json_events::Phase::AfterBuild {
                    seen.set(true);
                }
                json_events::Action::Continue
            });
            let denied = Rc::new(Cell::new(false));
            let seen = Rc::clone(&denied);
            let buffer_hook = buffer_events::install(move |event| {
                if built.get() && event.phase == phase {
                    seen.set(true);
                    assert!(
                        event.requested > 0 && event.used >= baseline + event.requested,
                        "late_encoding_not_precharged"
                    );
                    buffer_events::Action::FailAllocation
                } else {
                    buffer_events::Action::Continue
                }
            });
            let result = fixture.stage(&observed, &budget);
            drop(buffer_hook);
            drop(json_hook);
            assert!(denied.get(), "real_final_encoding_fault_not_reached");
            assert!(result.is_err(), "final_encoding_fault_ignored");
            drop(result);
            assert!(
                budget.used() == baseline && fixture.store.stats()? == before,
                "encoding_failure_not_atomic"
            );
            let stage = require(fixture.stage(&observed, &budget))?;
            assert_staged(&stage, &fixture)?;
            drop(stage);
            assert!(budget.used() == baseline, "encoding_retry_not_refunded");
        }
    }
    Ok(())
}

#[test]
fn c5_native_sse_staging_late_metadata_allocation_fault_refunds_dependencies() -> TestResult {
    let fixture = Fixture::new(true)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let frames = fixture.frames(&budget, EditConstraint::Editable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let baseline = budget.used();
    let before = fixture.store.stats()?;
    let built = Rc::new(Cell::new(false));
    let seen = Rc::clone(&built);
    let json_hook = json_events::install(move |event| {
        if event.phase == json_events::Phase::AfterBuild {
            seen.set(true);
        }
        json_events::Action::Continue
    });
    let denied = Rc::new(Cell::new(false));
    let seen = Rc::clone(&denied);
    let metadata_hook = metadata_events::install(move |event| {
        if built.get()
            && event.kind == metadata_events::Kind::Vector
            && event.phase == metadata_events::Phase::BeforeAllocation
        {
            seen.set(true);
            assert!(
                event.bytes > 0 && event.used >= baseline + event.bytes,
                "late_metadata_not_precharged"
            );
            metadata_events::Action::Fail
        } else {
            metadata_events::Action::Continue
        }
    });
    let result = fixture.stage(&observed, &budget);
    drop(metadata_hook);
    drop(json_hook);
    assert!(denied.get(), "late_metadata_allocation_not_reached");
    assert!(result.is_err(), "late_metadata_fault_ignored");
    drop(result);
    assert!(
        budget.used() == baseline && fixture.store.stats()? == before,
        "late_metadata_failure_not_atomic"
    );
    let stage = require(fixture.stage(&observed, &budget))?;
    assert_staged(&stage, &fixture)?;
    Ok(())
}

#[test]
fn c5_native_sse_staging_digest_binds_restored_wire_not_caller_snapshots() -> TestResult {
    let fixture = Fixture::new(true)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let frames = fixture.frames(&budget, EditConstraint::Editable)?;
    let observed = fixture.observe(&frames, &budget)?;
    let stage = require(fixture.stage(&observed, &budget))?;
    assert_staged(&stage, &fixture)?;
    let mut original = Sha256::new();
    original.update(b"native-sse-staging-wire-v1\0");
    for frame in &frames {
        original.update((frame.raw().len() as u64).to_le_bytes());
        original.update(frame.raw().as_bytes());
    }
    let original: [u8; 32] = original.finalize().into();
    assert!(
        stage.release_digest() != original,
        "digest_copied_from_unrestored_entity"
    );
    let projection = [NativeSseSnapshotSelection::new(2, &fixture.sources[2])];
    let different_snapshots = require(stage_native_sse(
        &observed,
        &projection,
        &fixture.lease,
        &fixture.binding,
        budget.clone(),
    ))?;
    assert!(
        stage.release_digest() == different_snapshots.release_digest(),
        "wire_digest_depended_on_caller_snapshot_claims"
    );
    assert!(
        different_snapshots.delivered() == &json!([fixture.expected[2]]),
        "snapshot_projection_not_source_exact"
    );
    assert!(
        different_snapshots.output_tokens().len() == 2
            && different_snapshots.restored_occurrences() == 4,
        "snapshot_choice_changed_real_restore_ledger"
    );
    Ok(())
}
