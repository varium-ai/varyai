//! Observation-only native SSE contracts; no protocol adapter, EOF, staging or publication.
//! Fixture parsing supplies original JSON borrows, never transport/completion evidence.
use std::{cell::Cell, rc::Rc};

use bytes::Bytes;
use serde_json::{json, Value};

use super::{
    completed_context::ActualAttemptBinding,
    payload_budget::PayloadBudget,
    payload_json::{test_support as json_events, PayloadJson},
    payload_metadata::test_support as metadata_events,
    response_restoration::RestoreError,
    restoration_contract::{
        ClientProtocol, ContinuityScope, EditConstraint, IngressNamespace, SourceClass, TextSites,
    },
    restoration_core::{NativeSseFrameObservation, NativeSseObservation},
    tokenization::{DlpRequestLease, DlpTokenStore},
};
use crate::proxy::protocol_bridge::Protocol;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const BUDGET_BYTES: usize = 64 * 1024 * 1024;
const ENTITY_BYTES: usize = 1024 * 1024;
const DONE: &str = "data: [DONE]\n\n";
const FIRST: &str = "data: {\"text\":\"synthetic-first\"}\n\n";
const SECOND: &str = "event: synthetic\r\ndata:{\"text\":\"synthetic-second\"}\r\n\r\n";

fn require<T>(result: Result<T, RestoreError>) -> TestResult<T> {
    result.map_err(|_| "synthetic_native_sse_observation_rejected".into())
}

fn parsed(encoded: &str) -> TestResult<PayloadJson> {
    // This validates fixture syntax only; it does not assert any stream lifecycle fact.
    PayloadJson::try_from_str(
        encoded,
        PayloadBudget::with_limit(BUDGET_BYTES),
        BUDGET_BYTES,
    )
    .map_err(|_| "synthetic_fixture_json_rejected".into())
}

fn sites<'a>(nodes: &[&'a Value], budget: &PayloadBudget) -> TestResult<TextSites<'a>> {
    let mut sites = TextSites::new(budget.clone())?;
    for node in nodes {
        sites.observe(node, SourceClass::HistoricalText, EditConstraint::Editable)?;
    }
    Ok(sites)
}

fn frame<'a>(
    raw: &'a str,
    source: Option<&'a Value>,
    nodes: &[&'a Value],
    budget: &PayloadBudget,
) -> TestResult<NativeSseFrameObservation<'a>> {
    require(NativeSseFrameObservation::new(
        raw,
        source,
        sites(nodes, budget)?,
        budget.clone(),
    ))
}

fn fixture(
    protocol: Protocol,
) -> TestResult<(DlpTokenStore, DlpRequestLease, ActualAttemptBinding)> {
    let store = DlpTokenStore::new();
    let lease = store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(protocol),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-c5-sse-app",
        None,
        1,
    )?;
    let binding = lease.bind_actual_attempt(
        "synthetic-provider",
        "synthetic-native",
        "https://synthetic.invalid/native",
        None,
    )?;
    Ok((store, lease, binding))
}

fn observe<'a>(
    entity: &'a [u8],
    frames: &[&'a NativeSseFrameObservation<'a>],
    lease: &DlpRequestLease,
    binding: &'a ActualAttemptBinding,
    budget: &PayloadBudget,
) -> Result<NativeSseObservation<'a>, RestoreError> {
    NativeSseObservation::new(
        entity,
        frames,
        ClientProtocol::new(Protocol::Chat),
        lease,
        binding,
        budget.clone(),
    )
}

#[test]
fn c5_native_sse_observation_accepts_exact_borrowed_frames_runtime_red() -> TestResult {
    let (store, lease, binding) = fixture(Protocol::Chat)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let entity = Bytes::from(format!("{FIRST}{SECOND}{DONE}"));
    let raw = std::str::from_utf8(entity.as_ref())?;
    let first_source = parsed(r#"{"text":"synthetic-first"}"#)?;
    let second_source = parsed(r#"{"text":"synthetic-second"}"#)?;
    let first = frame(
        &raw[..FIRST.len()],
        Some(first_source.as_value()),
        &[&first_source.as_value()["text"]],
        &budget,
    )?;
    let second = frame(
        &raw[FIRST.len()..FIRST.len() + SECOND.len()],
        Some(second_source.as_value()),
        &[&second_source.as_value()["text"]],
        &budget,
    )?;
    let done = frame(&raw[FIRST.len() + SECOND.len()..], None, &[], &budget)?;
    let before = store.stats()?;
    let baseline = budget.used();
    let observed = require(observe(
        entity.as_ref(),
        &[&first, &second, &done],
        &lease,
        &binding,
        &budget,
    ))?;
    assert!(
        std::ptr::eq(observed.entity(), entity.as_ref()),
        "entity_owner_replaced"
    );
    assert!(
        std::ptr::eq(observed.binding(), &binding),
        "attempt_borrow_replaced"
    );
    assert!(
        observed.client() == ClientProtocol::new(Protocol::Chat),
        "client_changed"
    );
    assert!(
        observed.frame_count() == 3 && observed.frame(3).is_none(),
        "frame_order_lost"
    );
    let selected = observed.frame(1).ok_or("synthetic_frame_missing")?;
    assert!(std::ptr::eq(selected, &second), "frame_reference_copied");
    assert!(
        std::ptr::eq(
            selected.raw().as_bytes(),
            &entity[FIRST.len()..FIRST.len() + SECOND.len()]
        ),
        "raw_frame_owner_replaced"
    );
    assert!(
        selected
            .source()
            .is_some_and(|value| std::ptr::eq(value, second_source.as_value())),
        "source_borrow_replaced"
    );
    let site = selected.sites().site(0).ok_or("synthetic_site_missing")?;
    assert!(
        std::ptr::eq(site.native(), &second_source.as_value()["text"]),
        "site_pointer_replaced"
    );
    assert!(
        done.source().is_none() && done.sites().len() == 0,
        "done_gained_text_sites"
    );
    assert!(
        budget.used() > baseline,
        "frame_reference_metadata_not_charged"
    );
    assert!(
        store.stats()? == before,
        "observation_published_store_state"
    );
    drop(observed);
    assert!(
        budget.used() == baseline,
        "observation_metadata_not_refunded"
    );
    drop((first, second, done));
    assert!(budget.used() == 0, "frame_metadata_not_refunded");
    Ok(())
}

#[test]
fn c5_native_sse_frame_accepts_multiline_json_and_utf8_without_rewriting() -> TestResult {
    let raw = ": synthetic metadata\r\nevent: opaque\r\nid: synthetic\r\ndata: {\r\ndata: \"text\":\"synthetic-🔒\",\r\ndata: \"unknown\":[true,null,1.0]}\r\n\r\n";
    let source = parsed(r#"{"text":"synthetic-🔒","unknown":[true,null,1.0]}"#)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = frame(
        raw,
        Some(source.as_value()),
        &[&source.as_value()["text"]],
        &budget,
    )?;
    assert!(std::ptr::eq(observed.raw(), raw), "frame_wire_reencoded");
    assert!(
        observed
            .source()
            .is_some_and(|value| std::ptr::eq(value, source.as_value())),
        "multiline_source_copied"
    );
    assert!(observed.sites().len() == 1, "multiline_site_lost");
    Ok(())
}

#[test]
fn c5_native_sse_observation_is_not_a_protocol_or_completion_validator() -> TestResult {
    // Neither missing terminal events nor data after DONE imply completion authority.
    for raw in ["event: response.completed\ndata: {}\n\n", "data: {}\n\n"] {
        for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
            let (_store, lease, binding) = fixture(protocol)?;
            let budget = PayloadBudget::with_limit(BUDGET_BYTES);
            let source = parsed("{}")?;
            let observed_frame = frame(raw, Some(source.as_value()), &[], &budget)?;
            require(NativeSseObservation::new(
                raw.as_bytes(),
                &[&observed_frame],
                ClientProtocol::new(protocol),
                &lease,
                &binding,
                budget.clone(),
            ))?;
        }
    }
    let (_store, lease, binding) = fixture(Protocol::Chat)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let entity = format!("{DONE}data: {{}}\n\n");
    let source = parsed("{}")?;
    let done = frame(&entity[..DONE.len()], None, &[], &budget)?;
    let later = frame(&entity[DONE.len()..], Some(source.as_value()), &[], &budget)?;
    require(observe(
        entity.as_bytes(),
        &[&done, &later],
        &lease,
        &binding,
        &budget,
    ))?;
    Ok(())
}

#[test]
fn c5_native_sse_frame_rejects_foreign_equal_and_cross_frame_sites() -> TestResult {
    let source = parsed(r#"{"text":"synthetic-first"}"#)?;
    let foreign = parsed(r#"{"text":"synthetic-first"}"#)?;
    let detached = source.as_value()["text"].clone();
    for node in [&foreign.as_value()["text"], &detached] {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let result = NativeSseFrameObservation::new(
            FIRST,
            Some(source.as_value()),
            sites(&[node], &budget)?,
            budget.clone(),
        );
        assert!(result.is_err(), "equal_foreign_site_accepted");
        drop(result);
        assert!(budget.used() == 0, "foreign_site_metadata_retained");
    }
    Ok(())
}

#[test]
fn c5_native_sse_frame_rejects_raw_source_mismatch_without_mutating_source() -> TestResult {
    let source = parsed(r#"{"text":"synthetic-second"}"#)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let result = NativeSseFrameObservation::new(
        FIRST,
        Some(source.as_value()),
        sites(&[&source.as_value()["text"]], &budget)?,
        budget.clone(),
    );
    assert!(result.is_err(), "unmatched_wire_value_accepted");
    drop(result);
    assert!(
        source.as_value() == &json!({"text":"synthetic-second"}),
        "source_mutated"
    );
    assert!(budget.used() == 0, "mismatch_metadata_retained");
    Ok(())
}

#[test]
fn c5_native_sse_frame_reparses_strict_json_even_with_caller_value() -> TestResult {
    let source = parsed(r#"{"text":"synthetic-first"}"#)?;
    for payload in [
        r#"{"text":"discarded","text":"synthetic-first"}"#,
        r#"{"text":"discarded","te\u0078t":"synthetic-first"}"#,
        r#"{"text":"synthetic-first"} {}"#,
        r#"{"text":"synthetic-first",}"#,
        r#"{"text":"\uD800"}"#,
        r#"{"text":NaN}"#,
    ] {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let raw = format!("data: {payload}\n\n");
        let result = NativeSseFrameObservation::new(
            &raw,
            Some(source.as_value()),
            sites(&[], &budget)?,
            budget.clone(),
        );
        assert!(result.is_err(), "non_strict_wire_json_accepted");
        drop(result);
        assert!(budget.used() == 0, "strict_parse_failure_retained_charge");
    }
    Ok(())
}

#[test]
fn c5_native_sse_frame_rejects_missing_or_multiple_exact_boundaries() -> TestResult {
    let source = parsed("{}")?;
    for raw in [
        "data: {}",
        "data: {}\n",
        "data: {}\r\n\r",
        "data: {}\r\r",
        "data: {}\n\ndata: {}\n\n",
        "data: {}\n\ntrailing",
        "\n\ndata: {}\n\n",
    ] {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        assert!(
            NativeSseFrameObservation::new(
                raw,
                Some(source.as_value()),
                sites(&[], &budget)?,
                budget.clone(),
            )
            .is_err(),
            "inexact_frame_boundary_accepted"
        );
        assert!(budget.used() == 0, "boundary_failure_retained_charge");
    }
    Ok(())
}

#[test]
fn c5_native_sse_frame_only_done_can_be_opaque_and_never_has_sites() -> TestResult {
    let source = parsed(r#""[DONE]""#)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    for raw in [
        "data: {}\n\n",
        "data: synthetic\n\n",
        ": synthetic\n\n",
        "\n\n",
    ] {
        assert!(
            NativeSseFrameObservation::new(raw, None, sites(&[], &budget)?, budget.clone())
                .is_err(),
            "arbitrary_opaque_frame_accepted"
        );
    }
    assert!(
        NativeSseFrameObservation::new(
            DONE,
            None,
            sites(&[source.as_value()], &budget)?,
            budget.clone(),
        )
        .is_err(),
        "opaque_done_sites_accepted"
    );
    assert!(
        NativeSseFrameObservation::new(
            DONE,
            Some(source.as_value()),
            sites(&[], &budget)?,
            budget.clone(),
        )
        .is_err(),
        "done_claimed_json_source"
    );
    assert!(budget.used() == 0, "opaque_failure_retained_charge");
    Ok(())
}

#[test]
fn c5_native_sse_observation_rejects_reordered_missing_duplicate_and_foreign_raw_frames(
) -> TestResult {
    let (_store, lease, binding) = fixture(Protocol::Chat)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let entity = Bytes::from(format!("{FIRST}{SECOND}"));
    let raw = std::str::from_utf8(entity.as_ref())?;
    let source_a = parsed(r#"{"text":"synthetic-first"}"#)?;
    let source_b = parsed(r#"{"text":"synthetic-second"}"#)?;
    let first = frame(&raw[..FIRST.len()], Some(source_a.as_value()), &[], &budget)?;
    let second = frame(&raw[FIRST.len()..], Some(source_b.as_value()), &[], &budget)?;
    let copy = raw[..FIRST.len()].to_owned();
    assert!(
        copy.as_bytes() == first.raw().as_bytes()
            && !std::ptr::eq(copy.as_bytes(), first.raw().as_bytes()),
        "fixture_not_foreign"
    );
    let foreign = frame(&copy, Some(source_a.as_value()), &[], &budget)?;
    let baseline = budget.used();
    for frames in [
        vec![&second, &first],
        vec![&first],
        vec![&second],
        vec![&first, &first],
        vec![&foreign, &second],
        vec![&first, &second, &second],
        vec![],
    ] {
        let result = observe(entity.as_ref(), &frames, &lease, &binding, &budget);
        assert!(result.is_err(), "unmatched_or_reordered_wire_accepted");
        drop(result);
        assert!(budget.used() == baseline, "owner_failure_retained_metadata");
    }
    Ok(())
}

#[test]
fn c5_native_sse_observation_rejects_frame_outside_the_exact_entity_slice() -> TestResult {
    let (_store, lease, binding) = fixture(Protocol::Chat)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let owner = Bytes::from(format!("{DONE}{DONE}{DONE}"));
    let raw = std::str::from_utf8(owner.as_ref())?;
    let first = frame(&raw[..DONE.len()], None, &[], &budget)?;
    let middle = frame(&raw[DONE.len()..2 * DONE.len()], None, &[], &budget)?;
    let baseline = budget.used();
    let selected_entity = &owner[DONE.len()..2 * DONE.len()];
    assert!(
        observe(selected_entity, &[&first], &lease, &binding, &budget).is_err(),
        "same_allocation_wrong_offset_accepted"
    );
    assert!(
        observe(owner.as_ref(), &[&middle], &lease, &binding, &budget).is_err(),
        "entity_prefix_suffix_omitted"
    );
    require(observe(
        selected_entity,
        &[&middle],
        &lease,
        &binding,
        &budget,
    ))?;
    assert!(budget.used() == baseline, "subentity_metadata_retained");
    Ok(())
}

#[test]
fn c5_native_sse_observation_rejects_empty_partial_and_invalid_utf8_entities() -> TestResult {
    let (_store, lease, binding) = fixture(Protocol::Chat)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    assert!(
        observe(&[], &[], &lease, &binding, &budget).is_err(),
        "empty_entity_accepted"
    );
    for suffix in [&b"data: {}"[..], &b"\xff"[..], &b"\xf0\x9f"[..]] {
        let entity = [DONE.as_bytes(), suffix].concat();
        let prefix = std::str::from_utf8(&entity[..DONE.len()])?;
        let done = frame(prefix, None, &[], &budget)?;
        let baseline = budget.used();
        let result = observe(&entity, &[&done], &lease, &binding, &budget);
        assert!(result.is_err(), "invalid_or_uncovered_entity_tail_accepted");
        drop(result);
        assert!(budget.used() == baseline, "entity_failure_retained_charge");
    }
    Ok(())
}

#[test]
fn c5_native_sse_observation_rejects_responses_wrong_client_and_foreign_attempts() -> TestResult {
    let (store, lease, binding) = fixture(Protocol::Chat)?;
    let (_other_store, _other_lease, foreign) = fixture(Protocol::Chat)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let done = frame(DONE, None, &[], &budget)?;
    let baseline = budget.used();
    let before = store.stats()?;
    for altered in [
        foreign,
        ActualAttemptBinding {
            store_id: binding.store_id.wrapping_add(1),
            ..binding.clone()
        },
        ActualAttemptBinding {
            namespace: binding.namespace.wrapping_add(1),
            ..binding.clone()
        },
        ActualAttemptBinding {
            generation: [0x7f; 16],
            ..binding.clone()
        },
        ActualAttemptBinding {
            policy_revision: binding.policy_revision.wrapping_add(1),
            ..binding.clone()
        },
    ] {
        assert!(
            observe(DONE.as_bytes(), &[&done], &lease, &altered, &budget).is_err(),
            "foreign_attempt_accepted"
        );
    }
    for protocol in [Protocol::Responses, Protocol::Anthropic, Protocol::Gemini] {
        assert!(
            NativeSseObservation::new(
                DONE.as_bytes(),
                &[&done],
                ClientProtocol::new(protocol),
                &lease,
                &binding,
                budget.clone(),
            )
            .is_err(),
            "responses_or_wrong_client_scope_accepted"
        );
    }
    let responses_store = DlpTokenStore::new();
    let responses_lease = responses_store.admit_published_policy("synthetic-responses", None, 1)?;
    let responses_binding = responses_lease.bind_actual_attempt(
        "synthetic-provider",
        "responses",
        "https://synthetic.invalid/responses",
        None,
    )?;
    assert!(
        NativeSseObservation::new(
            DONE.as_bytes(),
            &[&done],
            ClientProtocol::new(Protocol::Responses),
            &responses_lease,
            &responses_binding,
            budget.clone(),
        )
        .is_err(),
        "responses_scope_accepted"
    );
    assert!(
        budget.used() == baseline && store.stats()? == before,
        "rejection_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_native_sse_observation_enforces_1024_frame_boundary_without_terminal_semantics() -> TestResult
{
    let (_store, lease, binding) = fixture(Protocol::Chat)?;
    for count in [1024, 1025] {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let entity = DONE.repeat(count);
        let mut frames = Vec::new();
        for index in 0..count {
            frames.push(frame(
                &entity[index * DONE.len()..(index + 1) * DONE.len()],
                None,
                &[],
                &budget,
            )?);
        }
        let refs: Vec<_> = frames.iter().collect();
        let baseline = budget.used();
        let result = observe(entity.as_bytes(), &refs, &lease, &binding, &budget);
        if count == 1024 {
            let observed = require(result)?;
            assert!(observed.frame_count() == count, "frame_boundary_changed");
            drop(observed);
        } else {
            assert!(result.is_err(), "frame_count_limit_ignored");
            drop(result);
        }
        assert!(budget.used() == baseline, "frame_limit_metadata_retained");
    }
    Ok(())
}

#[test]
fn c5_native_sse_frame_and_entity_enforce_one_mib_including_metadata() -> TestResult {
    let (_store, lease, binding) = fixture(Protocol::Chat)?;
    let source = parsed("{}")?;
    for length in [ENTITY_BYTES, ENTITY_BYTES + 1] {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let raw = format!(
            ":{}\ndata: {{}}\n\n",
            "x".repeat(length - ":\ndata: {}\n\n".len())
        );
        assert!(raw.len() == length, "synthetic_frame_size_wrong");
        let result = NativeSseFrameObservation::new(
            &raw,
            Some(source.as_value()),
            sites(&[], &budget)?,
            budget.clone(),
        );
        if length == ENTITY_BYTES {
            let valid = require(result)?;
            require(observe(
                raw.as_bytes(),
                &[&valid],
                &lease,
                &binding,
                &budget,
            ))?;
            drop(valid);
        } else {
            assert!(result.is_err(), "frame_metadata_bytes_not_bounded");
            drop(result);
        }
        assert!(budget.used() == 0, "frame_size_metadata_retained");
    }
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let first_raw = format!(
        ":{}\ndata: {{}}\n\n",
        "x".repeat(ENTITY_BYTES - ":\ndata: {}\n\n".len())
    );
    let entity = format!("{first_raw}{DONE}");
    let first = frame(
        &entity[..first_raw.len()],
        Some(source.as_value()),
        &[],
        &budget,
    )?;
    let done = frame(&entity[first_raw.len()..], None, &[], &budget)?;
    assert!(
        observe(
            entity.as_bytes(),
            &[&first, &done],
            &lease,
            &binding,
            &budget
        )
        .is_err(),
        "aggregate_entity_bytes_not_bounded"
    );
    Ok(())
}

#[test]
fn c5_native_sse_frame_enforces_128_sites_and_source_depth_even_without_sites() -> TestResult {
    for count in [128, 129] {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let source = parsed(&json!(vec!["synthetic"; count]).to_string())?;
        let nodes: Vec<_> = source
            .as_value()
            .as_array()
            .ok_or("synthetic_array_missing")?
            .iter()
            .collect();
        let raw = format!("data: {}\n\n", source.as_value());
        let result = NativeSseFrameObservation::new(
            &raw,
            Some(source.as_value()),
            sites(&nodes, &budget)?,
            budget.clone(),
        );
        if count == 128 {
            let valid = require(result)?;
            assert!(
                valid.sites().len() == count,
                "equal_text_nodes_deduplicated_by_value"
            );
            drop(valid);
        } else {
            assert!(result.is_err(), "frame_site_limit_ignored");
            drop(result);
        }
        assert!(budget.used() == 0, "site_limit_metadata_retained");
    }
    // Deliberately untrusted source: strict reparsing must reject it even without searches.
    let mut deep = Value::String("synthetic".to_owned());
    for _ in 0..66 {
        deep = Value::Array(vec![deep]);
    }
    let raw = format!("data: {deep}\n\n");
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    assert!(
        NativeSseFrameObservation::new(&raw, Some(&deep), sites(&[], &budget)?, budget.clone(),)
            .is_err(),
        "source_depth_limit_ignored"
    );
    assert!(budget.used() == 0, "depth_failure_retained_charge");
    Ok(())
}

#[test]
fn c5_native_sse_observation_enforces_128_sites_across_all_frames() -> TestResult {
    let (_store, lease, binding) = fixture(Protocol::Chat)?;
    for second_count in [64, 65] {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let source_a = parsed(&json!(vec!["synthetic"; 64]).to_string())?;
        let source_b = parsed(&json!(vec!["synthetic"; second_count]).to_string())?;
        let nodes_a: Vec<_> = source_a
            .as_value()
            .as_array()
            .ok_or("synthetic_array_missing")?
            .iter()
            .collect();
        let nodes_b: Vec<_> = source_b
            .as_value()
            .as_array()
            .ok_or("synthetic_array_missing")?
            .iter()
            .collect();
        let first_raw = format!("data: {}\n\n", source_a.as_value());
        let entity = format!("{first_raw}data: {}\n\n", source_b.as_value());
        let first = frame(
            &entity[..first_raw.len()],
            Some(source_a.as_value()),
            &nodes_a,
            &budget,
        )?;
        let second = frame(
            &entity[first_raw.len()..],
            Some(source_b.as_value()),
            &nodes_b,
            &budget,
        )?;
        let baseline = budget.used();
        let result = observe(
            entity.as_bytes(),
            &[&first, &second],
            &lease,
            &binding,
            &budget,
        );
        if second_count == 64 {
            require(result)?;
        } else {
            assert!(result.is_err(), "entity_site_limit_ignored");
            drop(result);
        }
        assert!(
            budget.used() == baseline,
            "aggregate_site_failure_retained_charge"
        );
    }
    Ok(())
}

#[test]
fn c5_native_sse_frame_bounds_repeated_exact_node_search_work() -> TestResult {
    // 128 late sites in a wide source require over 4M visits under bounded identity searches.
    // The raw frame is below 1MiB and the source is shallow, so neither byte nor depth limits fire.
    let mut values = vec![Value::Null; 40_000];
    values.extend((0..128).map(|_| Value::String("synthetic".to_owned())));
    let source = Value::Array(values);
    let raw = format!("data: {source}\n\n");
    assert!(raw.len() < ENTITY_BYTES, "synthetic_work_fixture_too_large");
    let nodes: Vec<_> = source.as_array().ok_or("synthetic_array_missing")?[40_000..]
        .iter()
        .collect();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let result = NativeSseFrameObservation::new(
        &raw,
        Some(&source),
        sites(&nodes, &budget)?,
        budget.clone(),
    );
    assert!(result.is_err(), "identity_search_work_limit_ignored");
    drop(result);
    assert!(budget.used() == 0, "work_failure_retained_charge");
    Ok(())
}

#[test]
fn c5_native_sse_frame_independent_parse_is_precharged_and_faults_refund() -> TestResult {
    let source = parsed(r#"{"text":"synthetic-first"}"#)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let reparses = Rc::new(Cell::new(0));
    let seen = Rc::clone(&reparses);
    let hook = json_events::install(move |event| {
        if event.phase == json_events::Phase::BeforeParse {
            seen.set(seen.get() + 1);
            assert!(
                event.charged > 0 && event.used >= event.charged,
                "wire_parse_not_precharged"
            );
            return json_events::Action::Fail;
        }
        json_events::Action::Continue
    });
    let result = NativeSseFrameObservation::new(
        FIRST,
        Some(source.as_value()),
        sites(&[], &budget)?,
        budget.clone(),
    );
    drop(hook);
    assert!(
        reparses.get() > 0,
        "independent_strict_wire_parse_not_reached"
    );
    assert!(result.is_err(), "wire_parse_failure_ignored");
    drop(result);
    assert!(budget.used() == 0, "wire_parse_failure_retained_charge");
    let valid = frame(FIRST, Some(source.as_value()), &[], &budget)?;
    drop(valid);
    assert!(budget.used() == 0, "wire_parse_retry_retained_charge");
    Ok(())
}

#[test]
fn c5_native_sse_observation_metadata_failure_is_precharged_and_atomic() -> TestResult {
    let (store, lease, binding) = fixture(Protocol::Chat)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let done = frame(DONE, None, &[], &budget)?;
    let before = store.stats()?;
    let baseline = budget.used();
    let denied = Rc::new(Cell::new(false));
    let seen = Rc::clone(&denied);
    let hook = metadata_events::install(move |event| {
        if event.kind == metadata_events::Kind::Vector
            && event.phase == metadata_events::Phase::BeforeAllocation
        {
            seen.set(true);
            assert!(
                event.bytes > 0 && event.used >= baseline + event.bytes,
                "frame_reference_metadata_not_precharged"
            );
            metadata_events::Action::Fail
        } else {
            metadata_events::Action::Continue
        }
    });
    let result = observe(DONE.as_bytes(), &[&done], &lease, &binding, &budget);
    drop(hook);
    assert!(denied.get(), "frame_reference_allocation_not_reached");
    assert!(result.is_err(), "metadata_failure_ignored");
    drop(result);
    assert!(
        budget.used() == baseline && store.stats()? == before,
        "metadata_failure_not_atomic"
    );
    require(observe(
        DONE.as_bytes(),
        &[&done],
        &lease,
        &binding,
        &budget,
    ))?;
    assert!(
        budget.used() == baseline && store.stats()? == before,
        "retry_leaked_or_published"
    );
    Ok(())
}

#[test]
fn c5_native_sse_observation_full_budget_refusal_refunds_without_publication() -> TestResult {
    let (store, lease, binding) = fixture(Protocol::Chat)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let done = frame(DONE, None, &[], &budget)?;
    let held = budget.reserve(budget.limit() - budget.used())?;
    let baseline = budget.used();
    let before = store.stats()?;
    let result = observe(DONE.as_bytes(), &[&done], &lease, &binding, &budget);
    assert!(result.is_err(), "full_budget_accepted_metadata");
    drop(result);
    assert!(
        budget.used() == baseline && store.stats()? == before,
        "capacity_failure_not_atomic"
    );
    drop(held);
    require(observe(
        DONE.as_bytes(),
        &[&done],
        &lease,
        &binding,
        &budget,
    ))?;
    assert!(store.stats()? == before, "capacity_retry_published");
    Ok(())
}

#[test]
fn c5_native_sse_observation_shares_four_mib_work_across_ordered_frames() -> TestResult {
    let (_store, lease, binding) = fixture(Protocol::Chat)?;
    let mut values = vec![Value::Null; 40_000];
    values.extend((0..32).map(|_| Value::String("synthetic".to_owned())));
    let source = Value::Array(values);
    let one = format!("data: {source}\n\n");
    let entity = one.repeat(4);
    assert!(
        entity.len() < ENTITY_BYTES,
        "synthetic_aggregate_work_fixture_too_large"
    );
    let nodes: Vec<_> = source.as_array().ok_or("synthetic_array_missing")?[40_000..]
        .iter()
        .collect();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut frames = Vec::new();
    for index in 0..4 {
        frames.push(frame(
            &entity[index * one.len()..(index + 1) * one.len()],
            Some(&source),
            &nodes,
            &budget,
        )?);
    }
    let refs: Vec<_> = frames.iter().collect();
    let baseline = budget.used();
    let result = observe(entity.as_bytes(), &refs, &lease, &binding, &budget);
    assert!(result.is_err(), "work_allowance_reset_for_each_frame");
    drop(result);
    assert!(
        budget.used() == baseline,
        "aggregate_work_failure_retained_charge"
    );
    Ok(())
}
