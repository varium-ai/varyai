//! Synthetic-only tests for immutable, budget-bound JSON copies.
use super::{
    payload_budget::PayloadBudget,
    payload_json::{
        test_support::{self, Action, Phase},
        PayloadJson,
    },
    tokenization::TokenError,
};
use serde_json::{json, Map, Value};
use std::{cell::Cell, rc::Rc};

const LIMIT: usize = 8 * 1024 * 1024;
// Two 128-byte nodes, one conservatively charged 512-byte object entry,
// five key bytes and nine string bytes. Numbers additionally charge encoded bytes.
const SIMPLE_CHARGE: usize = 2 * 128 + 512 + 5 + 9;

#[test]
fn json_copy_precharges_full_owner_before_build_and_releases_after_drop() -> Result<(), TokenError>
{
    let budget = PayloadBudget::with_limit(SIMPLE_CHARGE);
    let builds = Rc::new(Cell::new(0));
    let observed = Rc::clone(&builds);
    let _hook = test_support::install(move |event| {
        assert_eq!(event.used, SIMPLE_CHARGE);
        assert_eq!(event.charged, SIMPLE_CHARGE);
        if event.phase == Phase::BeforeBuild {
            observed.set(observed.get() + 1);
        }
        Action::Continue
    });
    let body = json!({"input": "synthetic"});
    let owner = PayloadJson::try_from_value(&body, budget.clone(), LIMIT)?;
    assert_eq!(builds.get(), 1);
    assert!(owner.as_value() == &body);
    assert_eq!(budget.used(), SIMPLE_CHARGE);
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn json_copy_exhaustion_rejects_before_any_build() {
    for limit in [0, SIMPLE_CHARGE - 1] {
        let budget = PayloadBudget::with_limit(limit);
        let builds = Rc::new(Cell::new(0));
        let observed = Rc::clone(&builds);
        let _hook = test_support::install(move |_| {
            observed.set(observed.get() + 1);
            Action::Continue
        });
        let result =
            PayloadJson::try_from_value(&json!({"input":"synthetic"}), budget.clone(), LIMIT);
        assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
        assert_eq!(builds.get(), 0);
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn json_copy_local_limit_is_checked_before_shared_reservation() {
    let budget = PayloadBudget::with_limit(LIMIT);
    let result = PayloadJson::try_from_value(
        &json!({"input":"synthetic"}),
        budget.clone(),
        SIMPLE_CHARGE - 1,
    );
    assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
    assert_eq!(budget.used(), 0);
}

#[test]
fn json_copy_counts_uninspected_keys_nested_containers_and_number_bytes() -> Result<(), TokenError>
{
    let body = json!({"opaque": ["🦀", -0.0, null, true], "other": {"key": "x"}});
    // Eight nodes; three object entries; UTF-8 keys/strings; four bytes for -0.0.
    let expected = 8 * 128 + 3 * 512 + 6 + 5 + 3 + 4 + 1 + 4;
    let budget = PayloadBudget::with_limit(expected);
    let owner = PayloadJson::try_from_value(&body, budget.clone(), LIMIT)?;
    assert_eq!(budget.used(), expected);
    assert!(owner.as_value() == &body);
    assert_eq!(
        serde_json::to_string(owner.as_value()).ok(),
        serde_json::to_string(&body).ok()
    );
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn json_copy_distinct_copies_share_ceiling_but_arc_sharing_does_not_recharge(
) -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(SIMPLE_CHARGE * 2 - 1);
    let body = json!({"input":"synthetic"});
    let first = std::sync::Arc::new(PayloadJson::try_from_value(&body, budget.clone(), LIMIT)?);
    let shared = std::sync::Arc::clone(&first);
    assert_eq!(budget.used(), SIMPLE_CHARGE);
    assert_eq!(
        PayloadJson::try_from_value(&body, budget.clone(), LIMIT).err(),
        Some(TokenError::CapacityExceeded)
    );
    drop(first);
    assert_eq!(budget.used(), SIMPLE_CHARGE);
    drop(shared);
    assert_eq!(budget.used(), 0);
    let next = PayloadJson::try_from_value(&body, budget.clone(), LIMIT)?;
    drop(next);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn json_copy_rebuilds_source_spare_capacity_instead_of_cloning_it() -> Result<(), TokenError> {
    let mut fields = Map::with_capacity(4096);
    let mut text = String::with_capacity(4096);
    text.push_str("synthetic");
    let mut values = Vec::with_capacity(4096);
    values.push(Value::String(text));
    fields.insert("input".into(), Value::Array(values));
    let body = Value::Object(fields);
    let expected = SIMPLE_CHARGE + 128;
    let budget = PayloadBudget::with_limit(expected);
    let owner = PayloadJson::try_from_value(&body, budget.clone(), LIMIT)?;
    assert!(owner.as_value() == &body);
    let values = owner.as_value()["input"]
        .as_array()
        .ok_or(TokenError::Unavailable)?;
    assert_eq!(values.capacity(), 1);
    let text = values[0].as_str().ok_or(TokenError::Unavailable)?;
    assert_eq!(text.len(), 9);
    assert_eq!(budget.used(), expected);
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn json_copy_depth_and_node_limits_fail_without_building() {
    let budget = PayloadBudget::with_limit(16 * LIMIT);
    let mut deep = Value::Null;
    for _ in 0..65 {
        deep = Value::Array(vec![deep]);
    }
    let wide = Value::Array(vec![Value::Null; 65_536]);
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let _hook = test_support::install(move |_| {
        observed.set(observed.get() + 1);
        Action::Continue
    });
    for body in [&deep, &wide] {
        assert_eq!(
            PayloadJson::try_from_value(body, budget.clone(), 16 * LIMIT).err(),
            Some(TokenError::CapacityExceeded)
        );
        assert_eq!(budget.used(), 0);
    }
    assert_eq!(calls.get(), 0);
}

#[test]
fn json_copy_injected_errors_release_precharged_and_partially_built_owners() {
    for phase in [
        Phase::BeforeBuild,
        Phase::BeforeAllocation,
        Phase::AfterBuild,
    ] {
        let budget = PayloadBudget::with_limit(LIMIT);
        let hits = Rc::new(Cell::new(0));
        let observed = Rc::clone(&hits);
        let _hook = test_support::install(move |event| {
            assert!(event.used >= event.charged && event.charged > 0);
            if event.phase == phase {
                observed.set(observed.get() + 1);
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let result = PayloadJson::try_from_value(
            &json!({"input": ["synthetic", "second"]}),
            budget.clone(),
            LIMIT,
        );
        assert_eq!(hits.get(), 1);
        assert_eq!(result.err(), Some(TokenError::Unavailable));
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn json_copy_unwind_at_build_and_after_complete_value_releases_charge() {
    for phase in [Phase::BeforeAllocation, Phase::AfterBuild] {
        let budget = PayloadBudget::with_limit(LIMIT);
        let reached = Rc::new(Cell::new(false));
        let observed = Rc::clone(&reached);
        let _hook = test_support::install(move |event| {
            if event.phase == phase {
                observed.set(true);
                panic!("synthetic JSON owner unwind");
            }
            Action::Continue
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PayloadJson::try_from_value(
                &json!({"input": ["synthetic", "second"]}),
                budget.clone(),
                LIMIT,
            )
        }));
        assert!(reached.get());
        assert!(result.is_err());
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn json_copy_cancellation_releases_retained_owner() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(LIMIT);
    let owner = PayloadJson::try_from_value(&json!({"input":"synthetic"}), budget.clone(), LIMIT)?;
    assert_eq!(budget.used(), SIMPLE_CHARGE);
    let pending = async move {
        std::future::pending::<()>().await;
        drop(owner);
    };
    drop(pending);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn json_copy_empty_containers_and_scalar_types_preserve_semantics() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(LIMIT);
    for body in [
        Value::Null,
        json!({}),
        json!([]),
        json!(true),
        json!(u64::MAX),
        json!(i64::MIN),
        json!(-0.0),
    ] {
        let owner = PayloadJson::try_from_value(&body, budget.clone(), LIMIT)?;
        assert!(owner.as_value() == &body);
        drop(owner);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn json_copy_partial_build_error_and_unwind_release_all_precharged_bytes() {
    for unwind in [false, true] {
        let budget = PayloadBudget::with_limit(LIMIT);
        let allocations = Rc::new(Cell::new(0));
        let observed = Rc::clone(&allocations);
        let reached = Rc::new(Cell::new(false));
        let reached_hook = Rc::clone(&reached);
        let _hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeAllocation {
                observed.set(observed.get() + 1);
                // Root map, first key, first array, its two string values have
                // already been built when the second key allocation is reached.
                if observed.get() == 6 {
                    assert!(event.used >= event.charged && event.charged > 0);
                    reached_hook.set(true);
                    if unwind {
                        panic!("synthetic partial JSON build unwind");
                    }
                    return Action::Fail;
                }
            }
            Action::Continue
        });
        let body = json!({"first":["synthetic-one","synthetic-two"], "second":"three"});
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PayloadJson::try_from_value(&body, budget.clone(), LIMIT)
        }));
        assert!(reached.get());
        assert_eq!(allocations.get(), 6);
        if unwind {
            assert!(result.is_err());
        } else {
            assert_eq!(
                result.ok().and_then(Result::err),
                Some(TokenError::Unavailable)
            );
        }
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn json_copy_preserves_explicit_nonlexical_key_order_and_exact_scalar_forms(
) -> Result<(), TokenError> {
    let body: Value = serde_json::from_str(
        r#"{"z":[18446744073709551615,-9223372036854775808,-0.0,1.25],"a":{"y":"🦀","b":false}}"#,
    )
    .map_err(|_| TokenError::Unavailable)?;
    let budget = PayloadBudget::with_limit(LIMIT);
    let owner = PayloadJson::try_from_value(&body, budget.clone(), LIMIT)?;
    assert_eq!(
        owner
            .as_value()
            .as_object()
            .map(|map| map.keys().map(String::as_str).collect::<Vec<_>>()),
        Some(vec!["z", "a"])
    );
    assert_eq!(
        serde_json::to_vec(owner.as_value()).ok(),
        serde_json::to_vec(&body).ok()
    );
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

const DECODED_FIXTURE: &str = r#"{"x":"synthetic"}"#;
fn decoded_fixture_charges() -> (usize, usize) {
    // Object, key and value are three lexical nodes; one object entry.
    (
        DECODED_FIXTURE.len() + 3 * 512 + 1024,
        1024 + 4 * DECODED_FIXTURE.len() + 512,
    )
}

#[test]
fn decoded_json_precharges_retained_and_scratch_before_parser_then_releases_scratch(
) -> Result<(), TokenError> {
    let (retained, scratch) = decoded_fixture_charges();
    let budget = PayloadBudget::with_limit(retained + scratch);
    let hits = Rc::new(Cell::new(0));
    let observed = Rc::clone(&hits);
    let _hook = test_support::install(move |event| {
        if matches!(event.phase, Phase::BeforeParse | Phase::AfterParse) {
            assert_eq!(event.used, retained + scratch);
            assert_eq!(event.charged, retained);
            observed.set(observed.get() + 1);
        }
        Action::Continue
    });
    let owner = PayloadJson::try_from_str(DECODED_FIXTURE, budget.clone(), LIMIT)?;
    assert_eq!(hits.get(), 2);
    assert_eq!(budget.used(), retained);
    assert!(owner.as_value() == &json!({"x":"synthetic"}));
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn decoded_json_exhaustion_and_local_peak_limit_fail_before_parser() {
    let (retained, scratch) = decoded_fixture_charges();
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let _hook = test_support::install(move |_| {
        observed.set(observed.get() + 1);
        Action::Continue
    });
    for limit in [0, retained - 1, retained + scratch - 1] {
        let budget = PayloadBudget::with_limit(limit);
        assert_eq!(
            PayloadJson::try_from_str(DECODED_FIXTURE, budget.clone(), LIMIT).err(),
            Some(TokenError::CapacityExceeded)
        );
        assert_eq!(budget.used(), 0);
    }
    let budget = PayloadBudget::with_limit(LIMIT);
    assert_eq!(
        PayloadJson::try_from_str(DECODED_FIXTURE, budget.clone(), retained + scratch - 1).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(budget.used(), 0);
    assert_eq!(calls.get(), 0);
}

#[test]
fn decoded_json_rejects_duplicate_escaped_and_nested_keys_without_retaining_charge() {
    for encoded in [
        r#"{"x":"synthetic","x":"second"}"#,
        r#"{"x":1,"\u0078":2}"#,
        r#"{"outer":[{"x":1,"x":2}]}"#,
    ] {
        let budget = PayloadBudget::with_limit(LIMIT);
        assert_eq!(
            PayloadJson::try_from_str(encoded, budget.clone(), LIMIT).err(),
            Some(TokenError::RedactionFailed)
        );
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn decoded_json_malformed_input_returns_only_fixed_error_and_releases_scratch() {
    for encoded in [
        "",
        "{synthetic-private",
        "[1,]",
        r#"{"synthetic":"value"} trailing"#,
        r#""\ud800""#,
        "}",
    ] {
        let budget = PayloadBudget::with_limit(LIMIT);
        assert_eq!(
            PayloadJson::try_from_str(encoded, budget.clone(), LIMIT).err(),
            Some(TokenError::RedactionFailed)
        );
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn decoded_json_preserves_order_utf8_escapes_and_scalar_numeric_semantics() -> Result<(), TokenError>
{
    for encoded in [
        r#"{"z":["🦀","\u4e2d",18446744073709551615,-9223372036854775808,-0.0,1.25],"a":false}"#,
        "[]",
        "{}",
        "null",
        "true",
        r#""synthetic""#,
    ] {
        let budget = PayloadBudget::with_limit(LIMIT);
        let owner = PayloadJson::try_from_str(encoded, budget.clone(), LIMIT)?;
        let expected: Value = serde_json::from_str(encoded).map_err(|_| TokenError::Unavailable)?;
        assert_eq!(
            serde_json::to_vec(owner.as_value()).ok(),
            serde_json::to_vec(&expected).ok()
        );
        assert!(budget.used() > 0);
        drop(owner);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn decoded_json_encoded_depth_and_node_bounds_reject_before_parser() {
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let _hook = test_support::install(move |_| {
        observed.set(observed.get() + 1);
        Action::Continue
    });
    let wide = format!("[{}null]", "null,".repeat(65_536));
    for encoded in [
        "x".repeat(1024 * 1024 + 1),
        format!("{}null{}", "[".repeat(65), "]".repeat(65)),
        wide,
    ] {
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        assert_eq!(
            PayloadJson::try_from_str(&encoded, budget.clone(), 64 * 1024 * 1024).err(),
            Some(TokenError::CapacityExceeded)
        );
        assert_eq!(budget.used(), 0);
    }
    assert_eq!(calls.get(), 0);
}

#[test]
fn decoded_json_injected_parser_errors_and_unwind_release_complete_peak() {
    for phase in [Phase::BeforeParse, Phase::AfterParse] {
        for unwind in [false, true] {
            let budget = PayloadBudget::with_limit(LIMIT);
            let reached = Rc::new(Cell::new(false));
            let observed = Rc::clone(&reached);
            let _hook = test_support::install(move |event| {
                if event.phase == phase {
                    assert!(event.used > event.charged);
                    observed.set(true);
                    if unwind {
                        panic!("synthetic decoded JSON unwind");
                    }
                    return Action::Fail;
                }
                Action::Continue
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                PayloadJson::try_from_str(DECODED_FIXTURE, budget.clone(), LIMIT)
            }));
            assert!(reached.get());
            if unwind {
                assert!(result.is_err());
            } else {
                assert_eq!(
                    result.ok().and_then(Result::err),
                    Some(TokenError::Unavailable)
                );
            }
            assert_eq!(budget.used(), 0);
        }
    }
}

#[test]
fn decoded_json_distinct_owners_and_unpolled_cancellation_keep_retained_charge(
) -> Result<(), TokenError> {
    let (retained, scratch) = decoded_fixture_charges();
    let budget = PayloadBudget::with_limit(retained * 2 + scratch - 1);
    let first = PayloadJson::try_from_str(DECODED_FIXTURE, budget.clone(), LIMIT)?;
    assert_eq!(budget.used(), retained);
    assert_eq!(
        PayloadJson::try_from_str(DECODED_FIXTURE, budget.clone(), LIMIT).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(budget.used(), retained);
    let future = async move {
        std::future::pending::<()>().await;
        drop(first);
    };
    drop(future);
    assert_eq!(budget.used(), 0);
    let next = PayloadJson::try_from_str(DECODED_FIXTURE, budget.clone(), LIMIT)?;
    drop(next);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn decoded_json_retained_charge_covers_deep_singleton_array_minimum_capacity(
) -> Result<(), TokenError> {
    fn array_storage(value: &Value) -> usize {
        match value {
            Value::Array(values) => {
                values.capacity() * std::mem::size_of::<Value>()
                    + values.iter().map(array_storage).sum::<usize>()
            }
            _ => 0,
        }
    }
    let encoded = format!("{}null{}", "[".repeat(64), "]".repeat(64));
    let retained = encoded.len() + 65 * 512;
    let budget = PayloadBudget::with_limit(LIMIT);
    let owner = PayloadJson::try_from_str(&encoded, budget.clone(), LIMIT)?;
    assert_eq!(budget.used(), retained);
    assert!(budget.used() >= array_storage(owner.as_value()));
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn decoded_json_treats_raw_value_sentinel_and_escaped_equivalent_as_ordinary_keys(
) -> Result<(), TokenError> {
    for encoded in [
        r#"{"$serde_json::private::RawValue":"[0,1]","after":"synthetic"}"#,
        r#"{"\u0024serde_json::private::RawValue":"[0,1]"}"#,
    ] {
        let budget = PayloadBudget::with_limit(LIMIT);
        let owner = PayloadJson::try_from_str(encoded, budget.clone(), LIMIT)?;
        assert!(owner.as_value().is_object());
        assert_eq!(
            owner
                .as_value()
                .get("$serde_json::private::RawValue")
                .and_then(Value::as_str),
            Some("[0,1]")
        );
        drop(owner);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn decoded_json_raw_value_sentinel_cannot_hide_array_or_depth_allocations() -> Result<(), TokenError>
{
    for inner in [
        format!("[{}0]", "0,".repeat(4096)),
        format!("{}0{}", "[".repeat(65), "]".repeat(65)),
    ] {
        let encoded = format!(r#"{{"$serde_json::private::RawValue":"{inner}"}}"#);
        let retained = encoded.len() + 3 * 512 + 1024;
        let scratch = 1024 + 4 * encoded.len() + 512;
        let budget = PayloadBudget::with_limit(retained + scratch);
        let owner = PayloadJson::try_from_str(&encoded, budget.clone(), LIMIT)?;
        assert_eq!(
            owner
                .as_value()
                .get("$serde_json::private::RawValue")
                .and_then(Value::as_str),
            Some(inner.as_str())
        );
        assert_eq!(budget.used(), retained);
        drop(owner);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn decoded_json_sentinel_names_preserve_nonstring_values_and_normal_number_keys(
) -> Result<(), TokenError> {
    for key in [
        "$serde_json::private::RawValue",
        "$serde_json::private::Number",
    ] {
        let encoded = format!(r#"{{"{key}":{{"nested":[true,null]}}}}"#);
        let budget = PayloadBudget::with_limit(LIMIT);
        let owner = PayloadJson::try_from_str(&encoded, budget.clone(), LIMIT)?;
        assert!(owner.as_value().get(key).is_some_and(Value::is_object));
        drop(owner);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

// Additional already-green regressions; no separate behavioral RED is claimed.
#[test]
fn decoded_json_pending_key_without_colon_and_table_growth_errors_release_scratch() {
    for entries in [0, 1, 3, 4, 7, 8, 14, 15, 28, 29, 56, 57, 112, 113, 1024] {
        let mut encoded = String::from("{");
        for index in 0..entries {
            if index != 0 {
                encoded.push(',');
            }
            encoded.push_str(&format!(r#""synthetic-{index}":null"#));
        }
        if entries != 0 {
            encoded.push(',');
        }
        encoded.push_str(r#""\u4e2d\ud83e\udd80-pending""#);
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        assert_eq!(
            PayloadJson::try_from_str(&encoded, budget.clone(), 64 * 1024 * 1024).err(),
            Some(TokenError::RedactionFailed)
        );
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn decoded_json_growing_escaped_unicode_strings_and_invalid_numeric_edges_are_bounded(
) -> Result<(), TokenError> {
    for count in [1, 4, 8, 17, 257, 4097] {
        let encoded = format!(
            r#"{{"{}":"{}"}}"#,
            "\\u4e2d".repeat(count),
            "\\ud83e\\udd80".repeat(count)
        );
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let owner = PayloadJson::try_from_str(&encoded, budget.clone(), 64 * 1024 * 1024)?;
        let object = owner
            .as_value()
            .as_object()
            .ok_or(TokenError::Unavailable)?;
        assert_eq!(
            object.keys().next().map(String::as_str),
            Some("中".repeat(count).as_str())
        );
        assert_eq!(
            object.values().next().and_then(Value::as_str),
            Some("🦀".repeat(count).as_str())
        );
        drop(owner);
        assert_eq!(budget.used(), 0);
    }
    for encoded in [
        "1e400",
        "NaN",
        "01",
        "-",
        "1.",
        "true false",
        r#"{"\ud800":0}"#,
    ] {
        let budget = PayloadBudget::with_limit(LIMIT);
        assert_eq!(
            PayloadJson::try_from_str(encoded, budget.clone(), LIMIT).err(),
            Some(TokenError::RedactionFailed)
        );
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

// Restricted substitutions borrow targets from the exact source tree, not equal
// values from another tree. The caller owns/accounted the frozen edit metadata.
#[test]
fn json_replacement_changes_only_selected_string_values_and_preserves_order(
) -> Result<(), TokenError> {
    use super::payload_json::JsonStringReplacement;
    let body = json!({"z": ["same", "same", "", 1.25, true, null], "same": "keep", "a": "tail"});
    let edits = [
        JsonStringReplacement {
            target: &body["a"],
            text: "中\n\"\\",
        },
        JsonStringReplacement {
            target: &body["z"][1],
            text: "changed",
        },
        JsonStringReplacement {
            target: &body["z"][2],
            text: "empty-was-selected",
        },
    ];
    let budget = PayloadBudget::with_limit(LIMIT);
    let owner = PayloadJson::try_with_replacements(&body, &edits, budget.clone(), LIMIT)?;
    let expected = json!({"z": ["same", "changed", "empty-was-selected", 1.25, true, null], "same": "keep", "a": "中\n\"\\"});
    assert!(owner.as_value() == &expected);
    assert!(body["z"][1] == "same" && body["z"][2] == "");
    assert_eq!(
        serde_json::to_vec(owner.as_value()).ok(),
        serde_json::to_vec(&expected).ok()
    );
    drop(owner);
    assert_eq!(budget.used(), 0);
    let unchanged = PayloadJson::try_with_replacements(&body, &[], budget.clone(), LIMIT)?;
    assert!(unchanged.as_value() == &body);
    drop(unchanged);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn json_replacement_precharges_final_tree_while_source_stays_owned() -> Result<(), TokenError> {
    use super::payload_json::JsonStringReplacement;
    const FINAL: usize = 2 * 128 + 512 + 5 + 5;
    let budget = PayloadBudget::with_limit(SIMPLE_CHARGE + FINAL);
    let source = PayloadJson::try_from_value(&json!({"input":"synthetic"}), budget.clone(), LIMIT)?;
    let edits = [JsonStringReplacement {
        target: &source.as_value()["input"],
        text: "中\n\"",
    }];
    let hits = Rc::new(Cell::new(0));
    let observed = Rc::clone(&hits);
    let hook = test_support::install(move |event| {
        assert_eq!(event.charged, FINAL);
        assert_eq!(event.used, SIMPLE_CHARGE + FINAL);
        observed.set(observed.get() + 1);
        Action::Continue
    });
    let candidate =
        PayloadJson::try_with_replacements(source.as_value(), &edits, budget.clone(), LIMIT)?;
    assert!(hits.get() >= 3);
    drop(hook);
    assert_eq!(budget.used(), SIMPLE_CHARGE + FINAL);
    assert!(candidate.as_value()["input"] == "中\n\"");
    drop(source);
    assert_eq!(budget.used(), FINAL);
    drop(candidate);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn json_replacement_shared_and_local_exhaustion_precede_all_builds() {
    use super::payload_json::JsonStringReplacement;
    let body = json!({"input":"synthetic"});
    let edits = [JsonStringReplacement {
        target: &body["input"],
        text: "longer-synthetic",
    }];
    const FINAL: usize = 2 * 128 + 512 + 5 + 16;
    for (shared, local) in [(FINAL - 1, LIMIT), (LIMIT, FINAL - 1)] {
        let budget = PayloadBudget::with_limit(shared);
        let hits = Rc::new(Cell::new(0));
        let observed = Rc::clone(&hits);
        let _hook = test_support::install(move |_| {
            observed.set(observed.get() + 1);
            Action::Continue
        });
        let result = PayloadJson::try_with_replacements(&body, &edits, budget.clone(), local);
        assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
        assert_eq!(hits.get(), 0);
        assert_eq!(budget.used(), 0);
        assert!(body["input"] == "synthetic");
    }
}

#[test]
fn json_replacement_rejects_duplicate_foreign_and_nonstring_targets_before_build() {
    use super::payload_json::JsonStringReplacement;
    let body = json!({"input":"same", "number":7, "list":[]});
    let foreign = json!({"input":"same"});
    let groups = [
        vec![
            JsonStringReplacement {
                target: &body["input"],
                text: "first",
            },
            JsonStringReplacement {
                target: &body["input"],
                text: "second",
            },
        ],
        vec![JsonStringReplacement {
            target: &foreign["input"],
            text: "foreign",
        }],
        vec![JsonStringReplacement {
            target: &body["number"],
            text: "not-number",
        }],
        vec![JsonStringReplacement {
            target: &body["list"],
            text: "not-array",
        }],
        vec![JsonStringReplacement {
            target: &body,
            text: "not-object",
        }],
    ];
    let budget = PayloadBudget::with_limit(LIMIT);
    let hits = Rc::new(Cell::new(0));
    let observed = Rc::clone(&hits);
    let _hook = test_support::install(move |_| {
        observed.set(observed.get() + 1);
        Action::Continue
    });
    for edits in &groups {
        assert_eq!(
            PayloadJson::try_with_replacements(&body, edits, budget.clone(), LIMIT).err(),
            Some(TokenError::RedactionFailed)
        );
        assert_eq!(budget.used(), 0);
    }
    assert_eq!(hits.get(), 0);
}

#[test]
fn json_replacement_rebuilds_sparse_source_and_replacement_by_logical_length(
) -> Result<(), TokenError> {
    use super::payload_json::JsonStringReplacement;
    let mut fields = Map::with_capacity(4096);
    let mut values = Vec::with_capacity(4096);
    let mut original = String::with_capacity(4096);
    original.push_str("synthetic");
    values.push(Value::String(original));
    fields.insert("input".into(), Value::Array(values));
    let body = Value::Object(fields);
    let mut replacement = String::with_capacity(4096);
    replacement.push_str("中");
    let edits = [JsonStringReplacement {
        target: &body["input"][0],
        text: &replacement,
    }];
    const FINAL: usize = 3 * 128 + 512 + 5 + 3;
    let budget = PayloadBudget::with_limit(FINAL);
    let candidate = PayloadJson::try_with_replacements(&body, &edits, budget.clone(), FINAL)?;
    let values = candidate.as_value()["input"]
        .as_array()
        .ok_or(TokenError::Unavailable)?;
    assert_eq!(values.capacity(), 1);
    if let Value::String(text) = &values[0] {
        assert_eq!(text.capacity(), 3);
    } else {
        return Err(TokenError::RedactionFailed);
    }
    assert_eq!(budget.used(), FINAL);
    drop(candidate);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn json_replacement_each_injected_build_failure_releases_partial_candidate(
) -> Result<(), TokenError> {
    use super::payload_json::JsonStringReplacement;
    let body = json!({"z":["one", {"inner":"two"}], "a":"three"});
    let edits = [JsonStringReplacement {
        target: &body["z"][1]["inner"],
        text: "changed",
    }];
    let budget = PayloadBudget::with_limit(LIMIT);
    let hits = Rc::new(Cell::new(0));
    let observed = Rc::clone(&hits);
    let hook = test_support::install(move |_| {
        observed.set(observed.get() + 1);
        Action::Continue
    });
    let candidate = PayloadJson::try_with_replacements(&body, &edits, budget.clone(), LIMIT)?;
    drop(candidate);
    drop(hook);
    assert!(hits.get() > 5);
    for fail_at in 1..=hits.get() {
        let seen = Rc::new(Cell::new(0));
        let observed = Rc::clone(&seen);
        let _hook = test_support::install(move |_| {
            observed.set(observed.get() + 1);
            if observed.get() == fail_at {
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let result = PayloadJson::try_with_replacements(&body, &edits, budget.clone(), LIMIT);
        assert_eq!(result.err(), Some(TokenError::Unavailable));
        assert_eq!(seen.get(), fail_at);
        assert_eq!(budget.used(), 0);
        assert!(body["z"][1]["inner"] == "two");
    }
    Ok(())
}

#[test]
fn json_replacement_unwind_releases_charge_without_changing_source() {
    use super::payload_json::JsonStringReplacement;
    for phase in [
        Phase::BeforeBuild,
        Phase::BeforeAllocation,
        Phase::AfterBuild,
    ] {
        let body = json!({"input":["first","second"]});
        let edits = [JsonStringReplacement {
            target: &body["input"][1],
            text: "changed",
        }];
        let budget = PayloadBudget::with_limit(LIMIT);
        let reached = Rc::new(Cell::new(false));
        let observed = Rc::clone(&reached);
        let _hook = test_support::install(move |event| {
            if event.phase == phase {
                observed.set(true);
                panic!("synthetic replacement owner unwind");
            }
            Action::Continue
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PayloadJson::try_with_replacements(&body, &edits, budget.clone(), LIMIT)
        }));
        assert!(reached.get());
        assert!(result.is_err());
        assert_eq!(budget.used(), 0);
        assert!(body["input"][1] == "second");
    }
}

#[test]
fn json_replacement_bounds_edit_count_and_lookup_work_before_build() {
    use super::payload_json::JsonStringReplacement;
    let body = Value::Array(
        (0..2049)
            .map(|_| Value::String("synthetic".into()))
            .collect(),
    );
    let values = body.as_array().unwrap();
    let excessive = (0..4097)
        .map(|_| JsonStringReplacement {
            target: &values[0],
            text: "x",
        })
        .collect::<Vec<_>>();
    let expensive = values
        .iter()
        .map(|target| JsonStringReplacement { target, text: "x" })
        .collect::<Vec<_>>();
    let budget = PayloadBudget::with_limit(LIMIT);
    let hits = Rc::new(Cell::new(0));
    let observed = Rc::clone(&hits);
    let _hook = test_support::install(move |_| {
        observed.set(observed.get() + 1);
        Action::Continue
    });
    for edits in [&excessive, &expensive] {
        assert_eq!(
            PayloadJson::try_with_replacements(&body, edits, budget.clone(), LIMIT).err(),
            Some(TokenError::CapacityExceeded)
        );
        assert_eq!(budget.used(), 0);
    }
    assert_eq!(hits.get(), 0);
}

// Additional already-green boundary coverage; no behavioral RED is claimed.
#[test]
fn json_replacement_exact_comparison_bound_succeeds_and_releases_candidate(
) -> Result<(), TokenError> {
    use super::payload_json::JsonStringReplacement;
    const COUNT: usize = 2048;
    const FINAL: usize = (COUNT + 1) * 128 + COUNT;
    let body = Value::Array(
        (0..COUNT)
            .map(|_| Value::String("synthetic".into()))
            .collect(),
    );
    let values = body.as_array().ok_or(TokenError::Unavailable)?;
    let edits = values
        .iter()
        .map(|target| JsonStringReplacement { target, text: "x" })
        .collect::<Vec<_>>();
    let budget = PayloadBudget::with_limit(FINAL);
    let owner = PayloadJson::try_with_replacements(&body, &edits, budget.clone(), FINAL)?;
    let candidate = owner.as_value().as_array().ok_or(TokenError::Unavailable)?;
    assert_eq!(candidate.len(), COUNT);
    assert!(candidate.iter().all(|value| value.as_str() == Some("x")));
    assert!(values
        .iter()
        .all(|value| value.as_str() == Some("synthetic")));
    assert_eq!(budget.used(), FINAL);
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}
