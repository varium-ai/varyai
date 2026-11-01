//! Synthetic-only tests for the private Codex request carrier allowlist.
use super::{request_carriers::CodexRequestCarriers, tokenization::TokenError};
use serde_json::{json, Value};

fn isolated_carriers(body: &Value) -> Result<CodexRequestCarriers, TokenError> {
    CodexRequestCarriers::from_body_with_budget(
        body,
        super::payload_budget::PayloadBudget::with_limit(64 * 1024 * 1024),
    )
}

// Legacy protocol fixtures generate owned replacement text outside the production
// API. This test-only adapter freezes it once and uses the guarded borrowed path.
trait OwnedRewriteFixture {
    fn rewrite(
        &self,
        body: &mut Value,
        rewriter: impl FnMut(
            super::request_carriers::CarrierLeaf<'_>,
        ) -> Result<Option<String>, TokenError>,
    ) -> Result<(), TokenError>;
}
impl OwnedRewriteFixture for CodexRequestCarriers {
    fn rewrite(
        &self,
        body: &mut Value,
        mut rewriter: impl FnMut(
            super::request_carriers::CarrierLeaf<'_>,
        ) -> Result<Option<String>, TokenError>,
    ) -> Result<(), TokenError> {
        let mut replacements = Vec::new();
        let source_check = self.rewrite_candidate(body, |leaf| {
            replacements.push(rewriter(leaf)?);
            Ok(None)
        })?;
        drop(source_check);
        let candidate =
            self.rewrite_candidate(body, |leaf| Ok(replacements[leaf.id].as_deref()))?;
        *body = candidate.as_value().clone();
        Ok(())
    }
}

fn leaves(body: &Value) -> Result<Vec<(usize, String, bool, bool)>, TokenError> {
    let carriers = isolated_carriers(body)?;
    let mut result = Vec::new();
    carriers.visit(|leaf| {
        result.push((
            leaf.id,
            leaf.text.to_owned(),
            leaf.writable,
            leaf.is_password_field,
        ));
        Ok(())
    })?;
    Ok(result)
}

fn rewrite_visible(body: &mut Value) -> Result<(), TokenError> {
    isolated_carriers(body)?.rewrite(body, |leaf| {
        Ok(leaf.writable.then(|| format!("changed:{}", leaf.text)))
    })
}

#[test]
fn codex_roots_and_known_message_text_are_writable() -> Result<(), TokenError> {
    let mut body = json!({
        "instructions": "instruction", "prompt": ["prompt-one", "prompt-two"],
        "input": [
            {"role": "user", "content": "message"},
            {"type": "message", "role": "assistant", "content": [
                {"type": "input_text", "text": "input-text"},
                {"type": "output_text", "text": "output-text"},
                {"type": "text", "text": "plain-text"},
                {"type": "refusal", "refusal": "visible-refusal"}
            ]}
        ]
    });
    let found = leaves(&body)?;
    for text in [
        "instruction",
        "prompt-one",
        "prompt-two",
        "message",
        "input-text",
        "output-text",
        "plain-text",
        "visible-refusal",
    ] {
        assert!(found
            .iter()
            .any(|(_, value, writable, _)| value == text && *writable));
    }
    rewrite_visible(&mut body)?;
    assert!(body["input"][0]["content"] == "changed:message");
    assert!(body["input"][1]["content"][3]["refusal"] == "changed:visible-refusal");
    assert!(body["input"][1]["role"] == "assistant");
    Ok(())
}

#[test]
fn scalar_input_and_prompt_are_logical_strings_not_serialized_json() -> Result<(), TokenError> {
    for root in ["input", "prompt", "instructions"] {
        let mut body = json!({root: "line\n\"quoted\" \\ 中文"});
        let found = leaves(&body)?;
        assert!(found.len() == 1 && found[0].1 == "line\n\"quoted\" \\ 中文" && found[0].2);
        rewrite_visible(&mut body)?;
        assert!(body[root] == "changed:line\n\"quoted\" \\ 中文");
    }
    Ok(())
}

#[test]
fn decoded_function_arguments_preserve_keys_numbers_and_json_types() -> Result<(), TokenError> {
    let mut body = json!({"input": [{
        "type": "function_call", "id": "item-id", "call_id": "call-id", "name": "tool-name",
        "arguments": r#"{ "private-key-name": "line\n\u4e2d\"\\", "nested": ["value", 12, true, null], "password": "synthetic-password", "float": 1.25 }"#
    }]});
    let original: Value = serde_json::from_str(
        body["input"][0]["arguments"]
            .as_str()
            .ok_or(TokenError::Unavailable)?,
    )
    .map_err(|_| TokenError::Unavailable)?;
    let found = leaves(&body)?;
    assert!(found
        .iter()
        .any(|(_, text, writable, _)| text == "line\n中\"\\" && *writable));
    assert!(found
        .iter()
        .any(|(_, text, writable, password)| text == "synthetic-password"
            && *writable
            && *password));
    assert!(!found
        .iter()
        .any(|(_, text, _, _)| text == "private-key-name"));
    assert!(!found
        .iter()
        .any(|(_, text, _, _)| text.contains("\"nested\"")));
    rewrite_visible(&mut body)?;
    let rewritten: Value = serde_json::from_str(
        body["input"][0]["arguments"]
            .as_str()
            .ok_or(TokenError::Unavailable)?,
    )
    .map_err(|_| TokenError::Unavailable)?;
    assert!(rewritten["private-key-name"] == "changed:line\n中\"\\");
    for index in 1..4 {
        assert!(rewritten["nested"][index] == original["nested"][index]);
    }
    assert!(rewritten["float"] == original["float"]);
    assert!(body["input"][0]["name"] == "tool-name");
    Ok(())
}

#[test]
fn structured_arguments_are_writable_but_json_inside_a_leaf_is_not_recursively_parsed(
) -> Result<(), TokenError> {
    let mut body = json!({"input": [{"type": "function_call", "arguments": {
        "password": {"nested": "not-direct-password"}, "signature": "argument-not-protocol-opaque", "text": "{\"inner\":\"value\"}"
    }}]});
    let found = leaves(&body)?;
    assert!(found.iter().any(
        |(_, text, writable, password)| text == "not-direct-password" && *writable && !password
    ));
    assert!(found
        .iter()
        .any(|(_, text, writable, _)| text == "argument-not-protocol-opaque" && *writable));
    assert!(found
        .iter()
        .any(|(_, text, writable, _)| text == "{\"inner\":\"value\"}" && *writable));
    rewrite_visible(&mut body)?;
    assert!(body["input"][0]["arguments"]["text"] == "changed:{\"inner\":\"value\"}");
    Ok(())
}

#[test]
fn tool_outputs_custom_input_and_visible_reasoning_use_explicit_shapes() -> Result<(), TokenError> {
    let mut body = json!({"input": [
        {"type": "function_call_output", "output": "function-output"},
        {"type": "custom_tool_call_output", "output": [{"type": "input_text", "text": "custom-output"}]},
        {"type": "custom_tool_call", "input": "custom-input", "namespace": "tool-namespace"},
        {"type": "reasoning", "summary": [{"type": "summary_text", "text": "summary"}], "content": [{"type": "reasoning_text", "text": "reasoning"}]}
    ]});
    let found = leaves(&body)?;
    for expected in [
        "function-output",
        "custom-output",
        "custom-input",
        "summary",
        "reasoning",
    ] {
        assert!(found
            .iter()
            .any(|(_, text, writable, _)| text == expected && *writable));
    }
    rewrite_visible(&mut body)?;
    assert!(body["input"][1]["output"][0]["text"] == "changed:custom-output");
    assert!(body["input"][3]["content"][0]["text"] == "changed:reasoning");
    Ok(())
}

#[test]
fn inspected_ids_tool_names_and_unknown_values_are_explicitly_read_only() -> Result<(), TokenError>
{
    let body = json!({"input": [
        {"type": "function_call", "id": "item-id", "call_id": "call-id", "name": "tool-name", "namespace": "tool-namespace", "arguments": "{}", "extra": {"password": "unknown-password"}},
        {"type": "future", "content": [{"type": "input_text", "text": "unknown-nested-text"}]},
        {"role": "user", "content": [{"type": "future", "text": "unknown-block-text"}]}
    ]});
    let found = leaves(&body)?;
    for expected in [
        "item-id",
        "call-id",
        "tool-name",
        "tool-namespace",
        "unknown-password",
        "unknown-nested-text",
        "unknown-block-text",
    ] {
        assert!(found
            .iter()
            .any(|(_, text, writable, _)| text == expected && !writable));
    }
    assert!(found
        .iter()
        .any(|(_, text, _, password)| text == "unknown-password" && *password));
    for (index, (id, _, _, _)) in found.iter().enumerate() {
        assert_eq!(*id, index);
    }
    Ok(())
}

#[test]
fn opaque_media_and_non_codex_roots_never_reach_the_callback() -> Result<(), TokenError> {
    let body = json!({
        "instructions": "visible", "previous_response_id": "cursor", "session_id": "session", "model": "model-value", "temperature": 0.5,
        "tools": [{"name": "schema-name", "parameters": {"description": "schema-description"}}],
        "system": "other-system", "messages": [{"content": "other-message"}], "query": "other-query", "contents": [{"text": "other-protocol"}],
        "input": [
            {"type": "reasoning", "encrypted_content": "opaque-cipher", "signature": "opaque-signature", "summary": []},
            {"type": "compaction", "encrypted_content": "compact-cipher", "compact_blob": "compact-blob"},
            {"role": "user", "content": [
                {"type": "input_image", "image_url": "media-url", "text": "media-caption"},
                {"type": "input_file", "file_data": "file-bytes", "filename": "media-name"},
                {"type": "input_audio", "input_audio": {"data": "audio-bytes"}}
            ]}
        ]
    });
    let found = leaves(&body)?;
    for hidden in [
        "cursor",
        "session",
        "model-value",
        "schema-name",
        "schema-description",
        "other-system",
        "other-message",
        "other-query",
        "other-protocol",
        "opaque-cipher",
        "opaque-signature",
        "compact-cipher",
        "compact-blob",
        "media-url",
        "media-caption",
        "file-bytes",
        "media-name",
        "audio-bytes",
    ] {
        assert!(!found.iter().any(|(_, text, _, _)| text == hidden));
    }
    Ok(())
}

#[test]
fn object_keys_are_never_visited_or_rewritten() -> Result<(), TokenError> {
    let mut body = json!({"input": [{"type": "function_call", "arguments": "{\"synthetic-secret-key\":\"safe\"}"}], "synthetic-root-key": "not-inspected"});
    assert!(!leaves(&body)?
        .iter()
        .any(|(_, text, _, _)| text == "synthetic-secret-key" || text == "synthetic-root-key"));
    rewrite_visible(&mut body)?;
    assert!(body["input"][0]["arguments"]
        .as_str()
        .is_some_and(|text| text.contains("synthetic-secret-key")));
    Ok(())
}

#[test]
fn callback_failure_rolls_back_direct_and_decoded_argument_changes() -> Result<(), TokenError> {
    let mut body = json!({"instructions": "direct", "input": [
        {"type": "function_call", "arguments": "{ \"one\": \"decoded\" }"},
        {"role": "user", "content": "fail-here"}
    ]});
    let original = body.clone();
    let carriers = isolated_carriers(&body)?;
    let result = carriers.rewrite(&mut body, |leaf| {
        if leaf.text == "fail-here" {
            return Err(TokenError::RedactionFailed);
        }
        Ok(leaf.writable.then(|| "changed".to_owned()))
    });
    assert_eq!(result, Err(TokenError::RedactionFailed));
    assert!(body == original);
    Ok(())
}

#[test]
fn read_only_replacement_is_rejected_even_when_other_leaves_are_writable() -> Result<(), TokenError>
{
    let mut body = json!({"instructions": "allowed", "input": [{"type": "function_call", "call_id": "not-writable", "arguments": "{}"}]});
    let original = body.clone();
    let result = isolated_carriers(&body)?.rewrite(&mut body, |_| Ok(Some("changed".to_owned())));
    assert_eq!(result, Err(TokenError::RedactionFailed));
    assert!(body == original);
    Ok(())
}

#[test]
fn no_op_rewrite_keeps_original_argument_serialization_exactly() -> Result<(), TokenError> {
    let mut body = json!({"input": [{"type": "function_call", "arguments": "{  \"v\" : \"\\u4e2d\", \"n\": 1e1 }"}]});
    let original = body.clone();
    isolated_carriers(&body)?.rewrite(&mut body, |_| Ok(None))?;
    assert!(body == original);
    Ok(())
}

#[test]
fn stale_body_is_rejected_before_callbacks_or_commit() -> Result<(), TokenError> {
    let mut body = json!({"input": "original"});
    let carriers = isolated_carriers(&body)?;
    body["input"] = json!("changed-outside");
    let original = body.clone();
    let mut called = false;
    let result = carriers.rewrite(&mut body, |_| {
        called = true;
        Ok(None)
    });
    assert_eq!(result, Err(TokenError::RedactionFailed));
    assert!(!called && body == original);
    Ok(())
}

#[test]
fn visit_propagates_fixed_errors_without_revealing_parser_or_values() -> Result<(), TokenError> {
    let carriers = isolated_carriers(&json!({"input": "synthetic-private"}))?;
    assert_eq!(
        carriers.visit(|_| Err(TokenError::ContextUnavailable)),
        Err(TokenError::ContextUnavailable)
    );
    for malformed in ["{synthetic-private", "{\"synthetic-private\":}", "[1,]"] {
        let result = isolated_carriers(
            &json!({"input": [{"type": "function_call", "arguments": malformed}]}),
        );
        assert!(matches!(result, Err(TokenError::RedactionFailed)));
        if let Err(error) = result {
            assert_eq!(error.to_string(), "dlp_redaction_failed");
        }
    }
    Ok(())
}

#[test]
fn request_and_decoded_json_depth_are_bounded() {
    let mut nested = json!("leaf");
    for _ in 0..65 {
        nested = json!([nested]);
    }
    assert!(matches!(
        isolated_carriers(&json!({"input": nested})),
        Err(TokenError::CapacityExceeded)
    ));
    let arguments = format!("{}\"leaf\"{}", "[".repeat(65), "]".repeat(65));
    assert!(matches!(
        isolated_carriers(&json!({"input": [{"type": "function_call", "arguments": arguments}]})),
        Err(TokenError::CapacityExceeded)
    ));
}

#[test]
fn string_leaf_count_and_total_text_limits_are_checked() {
    assert!(matches!(
        isolated_carriers(&json!({"input": "x".repeat(1024 * 1024 + 1)})),
        Err(TokenError::CapacityExceeded)
    ));
    assert!(matches!(
        isolated_carriers(&json!({"prompt": vec!["x"; 4097]})),
        Err(TokenError::CapacityExceeded)
    ));
    assert!(matches!(
        isolated_carriers(&json!({"prompt": vec!["x".repeat(900_000); 5]})),
        Err(TokenError::CapacityExceeded)
    ));
}

#[test]
fn decoded_json_bytes_structure_and_aggregate_parse_budget_are_bounded() {
    let too_large = format!("\"{}\"", "x".repeat(1024 * 1024));
    let too_many_nodes = format!("[{}]", vec!["0"; 70_000].join(","));
    for arguments in [too_large, too_many_nodes] {
        assert!(matches!(
            isolated_carriers(
                &json!({"input": [{"type": "function_call", "arguments": arguments}]})
            ),
            Err(TokenError::CapacityExceeded)
        ));
    }
    let arguments = format!("\"{}\"", " ".repeat(900_000));
    let calls: Vec<_> = (0..5)
        .map(|_| json!({"type": "function_call", "arguments": arguments}))
        .collect();
    assert!(matches!(
        isolated_carriers(&json!({"input": calls})),
        Err(TokenError::CapacityExceeded)
    ));
}

#[test]
fn oversized_candidate_leaf_and_aggregate_growth_roll_back() -> Result<(), TokenError> {
    let mut body = json!({"prompt": ["one", "two", "three", "four", "five"]});
    let original = body.clone();
    for bytes in [1024 * 1024 + 1, 900_000] {
        let result = isolated_carriers(&body)?.rewrite(&mut body, |_| Ok(Some("x".repeat(bytes))));
        assert_eq!(result, Err(TokenError::CapacityExceeded));
        assert!(body == original);
    }
    Ok(())
}

#[test]
fn full_request_clone_budget_includes_uninspected_values_and_node_overhead() {
    assert!(matches!(
        isolated_carriers(&json!({"model": "x".repeat(8 * 1024 * 1024)})),
        Err(TokenError::CapacityExceeded)
    ));
    assert!(matches!(
        isolated_carriers(&json!({"uninspected": vec![Value::Null; 65_537]})),
        Err(TokenError::CapacityExceeded)
    ));
}

#[test]
fn non_object_requests_fail_closed_without_text_inspection() {
    for body in [Value::Null, json!("input"), json!(["input"])] {
        assert!(matches!(
            isolated_carriers(&body),
            Err(TokenError::RedactionFailed)
        ));
    }
}

#[test]
fn unknown_nested_shapes_do_not_inherit_writability_from_familiar_keys() -> Result<(), TokenError> {
    let body = json!({"input": [
        {"type": "future", "role": "user", "content": "future-content", "arguments": "not-json"},
        {"type": "custom_tool_call", "input": {"text": "custom-structured"}},
        {"type": "function_call_output", "output": {"text": "output-structured"}},
        {"type": "message", "role": "future-role", "content": "unknown-role-content"},
        {"type": "reasoning", "summary": [{"type": "future", "text": "unknown-summary"}], "content": "unknown-reasoning"},
        {"role": "user", "content": [{"type": "input_text", "text": {"text": "nonstring-text"}}]}
    ]});
    let found = leaves(&body)?;
    for expected in [
        "future-content",
        "not-json",
        "custom-structured",
        "output-structured",
        "unknown-role-content",
        "unknown-summary",
        "unknown-reasoning",
        "nonstring-text",
    ] {
        assert!(found
            .iter()
            .any(|(_, text, writable, _)| text == expected && !writable));
    }
    Ok(())
}

#[test]
fn multiple_decoded_documents_keep_ordinals_and_targets_separate() -> Result<(), TokenError> {
    let mut body = json!({"input": [
        {"type": "function_call", "arguments": "\"first\""},
        {"type": "function_call", "arguments": "{\"key\":[\"second\",\"third\"]}"}
    ]});
    let carriers = isolated_carriers(&body)?;
    let original = leaves(&body)?;
    carriers.rewrite(&mut body, |leaf| {
        Ok(leaf.writable.then(|| format!("id-{}", leaf.id)))
    })?;
    let updated = leaves(&body)?;
    assert!(original.len() == updated.len());
    for ((id, _, writable, _), (new_id, text, new_writable, _)) in original.iter().zip(&updated) {
        assert!(id == new_id && writable == new_writable);
        if *writable {
            assert!(text == &format!("id-{id}"));
        }
    }
    Ok(())
}

#[test]
fn nested_json_encoded_growth_is_checked_before_serialization_commit() -> Result<(), TokenError> {
    let mut body =
        json!({"input": [{"type": "function_call", "arguments": "{\"key\":\"value\"}"}]});
    let original = body.clone();
    let result = isolated_carriers(&body)?.rewrite(&mut body, |leaf| {
        Ok(leaf.writable.then(|| "\n".repeat(600_000)))
    });
    assert_eq!(result, Err(TokenError::CapacityExceeded));
    assert!(body == original);
    Ok(())
}

#[test]
fn unchanged_string_replacements_preserve_encoded_json_bytes() -> Result<(), TokenError> {
    let mut body =
        json!({"input": [{"type": "function_call", "arguments": "{ \"key\": \"\\u4e2d\" }"}]});
    let original = body.clone();
    isolated_carriers(&body)?.rewrite(&mut body, |leaf| {
        Ok(leaf.writable.then(|| leaf.text.to_owned()))
    })?;
    assert!(body == original);
    Ok(())
}

#[test]
fn decoded_arguments_reject_duplicate_and_escaped_equivalent_keys() {
    for arguments in [
        r#"{"key":"first","key":"second"}"#,
        r#"{"key":"first","\u006bey":"second"}"#,
        r#"{"outer":[{"nested":1,"nested":2}]}"#,
    ] {
        assert!(matches!(
            isolated_carriers(
                &json!({"input": [{"type": "function_call", "arguments": arguments}]})
            ),
            Err(TokenError::RedactionFailed)
        ));
    }
}

#[test]
fn rewriting_argument_strings_preserves_exact_numeric_lexemes() -> Result<(), TokenError> {
    for number in [
        "18446744073709551615",
        "18446744073709551617",
        "-9223372036854775809",
        "0.123456789012345678901234567890",
        "1.230000e+12",
        "-0",
        "-0.0",
    ] {
        let arguments = format!(r#"{{ "n": {number}, "s": "synthetic" }}"#);
        let expected = format!(r#"{{ "n": {number}, "s": "changed:synthetic" }}"#);
        let mut body = json!({"input": [{"type":"function_call", "arguments": arguments}]});
        rewrite_visible(&mut body)?;
        assert!(
            body["input"][0]["arguments"] == expected,
            "numeric lexeme changed"
        );
    }
    Ok(())
}

#[test]
fn present_nonstring_item_types_never_receive_message_write_authority() -> Result<(), TokenError> {
    for invalid_type in [Value::Null, json!(123), json!(true), json!([]), json!({})] {
        let body = json!({"input": [{"type": invalid_type, "role":"user", "content":"synthetic"}]});
        let found = leaves(&body)?;
        assert!(found
            .iter()
            .any(|(_, text, writable, _)| text == "synthetic" && !writable));
    }
    Ok(())
}

#[test]
fn unknown_root_objects_and_nested_protocol_arrays_remain_read_only() -> Result<(), TokenError> {
    for body in [
        json!({"instructions": {"type":"message", "role":"user", "content":"synthetic"}}),
        json!({"prompt": {"type":"message", "role":"user", "content":"synthetic"}}),
        json!({"input": [[{"type":"message", "role":"user", "content":"synthetic"}]]}),
        json!({"input": [{"role":"user", "content":[[{"type":"input_text", "text":"synthetic"}]]}]}),
        json!({"input": [{"type":"reasoning", "summary":[[{"type":"summary_text", "text":"synthetic"}]]}]}),
    ] {
        let found = leaves(&body)?;
        assert!(found
            .iter()
            .any(|(_, text, writable, _)| text == "synthetic" && !writable));
    }
    Ok(())
}

#[test]
fn lexical_splices_follow_original_order_with_escaped_keys_and_nested_values(
) -> Result<(), TokenError> {
    let arguments = r#"{ "z": [ {"\u0062":"one", "a":18446744073709551617}, "two" ], "\u0061": {"z":"three", "a":[true, null, -0.0, "four"]}, "keep":"literal\\\"quote" }"#;
    let expected = r#"{ "z": [ {"\u0062":"changed:one", "a":18446744073709551617}, "changed:two" ], "\u0061": {"z":"changed:three", "a":[true, null, -0.0, "changed:four"]}, "keep":"literal\\\"quote" }"#;
    let mut body = json!({"input": [{"type":"function_call", "arguments": arguments}]});
    isolated_carriers(&body)?.rewrite(&mut body, |leaf| {
        Ok(matches!(leaf.text, "one" | "two" | "three" | "four")
            .then(|| format!("changed:{}", leaf.text)))
    })?;
    assert!(
        body["input"][0]["arguments"] == expected,
        "non-value bytes changed"
    );
    Ok(())
}

#[test]
fn review_regression_nested_identity_named_values_are_inspected_read_only() -> Result<(), TokenError>
{
    let body = json!({
        "previous_response_id": "root-cursor-excluded",
        "session_id": "root-session-excluded",
        "input": [{"type": "future", "extra": {
            "previous_response_id": "nested-cursor-sensitive",
            "session_id": "nested-session-sensitive"
        }}]
    });
    let found = leaves(&body)?;
    for text in ["nested-cursor-sensitive", "nested-session-sensitive"] {
        assert!(found
            .iter()
            .any(|(_, value, writable, _)| value == text && !*writable));
    }
    assert!(found
        .iter()
        .all(|(_, value, _, _)| !value.starts_with("root-")));
    Ok(())
}

#[test]
fn direct_typed_content_and_output_objects_remain_inspected_read_only() -> Result<(), TokenError> {
    for item in [
        json!({"role": "user", "content": {"type": "input_text", "text": "synthetic-shape"}}),
        json!({"type": "function_call_output", "output": {"type": "input_text", "text": "synthetic-shape"}}),
        json!({"type": "custom_tool_call_output", "output": {"type": "output_text", "text": "synthetic-shape"}}),
        json!({"role": "assistant", "content": {"type": "refusal", "refusal": "synthetic-shape"}}),
    ] {
        let mut body = json!({"input": [item]});
        let original = body.clone();
        let found = leaves(&body)?;
        assert!(found
            .iter()
            .any(|(_, text, writable, _)| text == "synthetic-shape" && !writable));
        let result = isolated_carriers(&body)?.rewrite(&mut body, |leaf| {
            Ok((leaf.text == "synthetic-shape").then(|| "changed".to_owned()))
        });
        assert_eq!(result.err(), Some(TokenError::RedactionFailed));
        assert!(body == original);
    }
    Ok(())
}

#[test]
fn carrier_shared_original_copy_precharges_and_retains_until_drop() -> Result<(), TokenError> {
    use super::{
        payload_budget::PayloadBudget,
        payload_json::test_support::{self, Action, Phase},
    };
    let charge = 2 * 128 + 512 + 5 + 9;
    let metadata = {
        let (step, leaf, _) = super::request_carriers::metadata_sizes();
        4 * step + 5 + 4 * leaf
    };
    let budget = PayloadBudget::with_limit(
        charge + metadata + 4 * super::request_carriers::metadata_sizes().0 + 5,
    );
    let hits = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&hits);
    let _hook = test_support::install(move |event| {
        assert_eq!(event.used, charge);
        if event.phase == Phase::BeforeBuild {
            observed.set(observed.get() + 1);
        }
        Action::Continue
    });
    let body = json!({"input":"synthetic"});
    let carriers = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    assert_eq!(hits.get(), 1);
    assert_eq!(budget.used(), charge + metadata);
    carriers.visit(|leaf| {
        assert_eq!(leaf.text.len(), 9);
        Ok(())
    })?;
    drop(carriers);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn carrier_shared_original_copy_exhaustion_rejects_without_callback() {
    use super::payload_budget::PayloadBudget;
    let budget = PayloadBudget::with_limit(0);
    let result =
        CodexRequestCarriers::from_body_with_budget(&json!({"input":"synthetic"}), budget.clone());
    assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
    assert_eq!(budget.used(), 0);
}

#[test]
fn carrier_shared_original_copy_counts_uninspected_values_and_distinct_owners(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    let body = json!({"input":"synthetic", "opaque":"x".repeat(4096)});
    let charge = 3 * 128 + 2 * 512 + 5 + 9 + 6 + 4096 + {
        let (step, leaf, _) = super::request_carriers::metadata_sizes();
        4 * step + 5 + 4 * leaf
    };
    let budget = PayloadBudget::with_limit(charge * 2 - 1);
    let first = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    assert_eq!(budget.used(), charge);
    assert_eq!(
        CodexRequestCarriers::from_body_with_budget(&body, budget.clone()).err(),
        Some(TokenError::CapacityExceeded)
    );
    drop(first);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn carrier_shared_original_copy_failure_and_unwind_release_charge() {
    use super::{
        payload_budget::PayloadBudget,
        payload_json::test_support::{self, Action, Phase},
    };
    for unwind in [false, true] {
        let budget = PayloadBudget::with_limit(8192);
        let hits = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = std::rc::Rc::clone(&hits);
        let _hook = test_support::install(move |event| {
            if event.phase == Phase::AfterBuild {
                observed.set(true);
                if unwind {
                    panic!("synthetic carrier owner unwind");
                }
                return Action::Fail;
            }
            Action::Continue
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            CodexRequestCarriers::from_body_with_budget(
                &json!({"input":"synthetic"}),
                budget.clone(),
            )
        }));
        assert!(hits.get());
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

// Independent primitive ownership measures only the already-budgeted original copy.
fn decoded_carrier_original_charge(body: &Value) -> Result<usize, TokenError> {
    use super::{payload_budget::PayloadBudget, payload_json::PayloadJson};
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let owner = PayloadJson::try_from_value(body, budget.clone(), 8 * 1024 * 1024)?;
    let bytes = budget.used();
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(bytes)
}

const DECODED_CARRIER_ARGUMENTS: &str = r#"{"value":"synthetic"}"#;

fn decoded_carrier_body(copies: usize) -> Value {
    json!({"input": (0..copies).map(|_| json!({
        "type":"function_call", "name":"synthetic", "arguments":DECODED_CARRIER_ARGUMENTS
    })).collect::<Vec<_>>()})
}

fn decoded_carrier_charges() -> (usize, usize) {
    (
        DECODED_CARRIER_ARGUMENTS.len() + 3 * 512 + 1024,
        1024 + 4 * DECODED_CARRIER_ARGUMENTS.len() + 512,
    )
}

#[test]
fn carrier_shared_decoded_precharges_parser_peak_and_retains_document_owner(
) -> Result<(), TokenError> {
    use super::{
        payload_budget::PayloadBudget,
        payload_json::test_support::{self, Action, Phase},
    };
    let body = decoded_carrier_body(1);
    let (retained, _) = decoded_carrier_charges();
    let profile = carrier_profile(&body)?;
    let parse_peak = profile.parses[0];
    let budget = PayloadBudget::with_limit(profile.peak);
    let hits = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&hits);
    let _hook = test_support::install(move |event| {
        if matches!(event.phase, Phase::BeforeParse | Phase::AfterParse) {
            assert_eq!(event.charged, retained);
            assert_eq!(event.used, parse_peak);
            observed.set(observed.get() + 1);
        }
        Action::Continue
    });
    let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    assert_eq!(hits.get(), 2);
    assert_eq!(budget.used(), profile.retained);
    let mut leaves = 0;
    plan.visit(|leaf| {
        if leaf.writable {
            assert_eq!(leaf.text, "synthetic");
            leaves += 1;
        }
        Ok(())
    })?;
    assert_eq!(leaves, 1);
    drop(plan);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn carrier_shared_decoded_exhaustion_rejects_before_parser_and_releases_original(
) -> Result<(), TokenError> {
    use super::{
        payload_budget::PayloadBudget,
        payload_json::test_support::{self, Action, Phase},
    };
    let body = decoded_carrier_body(1);
    let original = decoded_carrier_original_charge(&body)?;
    let (retained, scratch) = decoded_carrier_charges();
    for limit in [original + retained - 1, original + retained + scratch - 1] {
        let budget = PayloadBudget::with_limit(limit);
        let hits = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = std::rc::Rc::clone(&hits);
        let _hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeParse {
                observed.set(observed.get() + 1);
            }
            Action::Continue
        });
        assert_eq!(
            CodexRequestCarriers::from_body_with_budget(&body, budget.clone()).err(),
            Some(TokenError::CapacityExceeded)
        );
        assert_eq!(hits.get(), 0);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn carrier_shared_decoded_multiple_documents_and_distinct_plans_keep_separate_owners(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    let body = decoded_carrier_body(2);
    let profile = carrier_profile(&body)?;
    let budget = PayloadBudget::with_limit(profile.parses[1] - 1);
    assert_eq!(
        CodexRequestCarriers::from_body_with_budget(&body, budget.clone()).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(budget.used(), 0);
    let budget = PayloadBudget::with_limit(profile.retained + profile.peak);
    let first = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    let second = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    assert_eq!(budget.used(), 2 * profile.retained);
    drop(first);
    assert_eq!(budget.used(), profile.retained);
    drop(second);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn carrier_shared_decoded_parse_failure_and_unwind_release_all_owners() -> Result<(), TokenError> {
    use super::{
        payload_budget::PayloadBudget,
        payload_json::test_support::{self, Action, Phase},
    };
    for phase in [Phase::BeforeParse, Phase::AfterParse] {
        for unwind in [false, true] {
            let budget = PayloadBudget::with_limit(1024 * 1024);
            let hits = std::rc::Rc::new(std::cell::Cell::new(false));
            let observed = std::rc::Rc::clone(&hits);
            let _hook = test_support::install(move |event| {
                if event.phase == phase {
                    observed.set(true);
                    if unwind {
                        panic!("synthetic decoded carrier unwind");
                    }
                    return Action::Fail;
                }
                Action::Continue
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                CodexRequestCarriers::from_body_with_budget(
                    &decoded_carrier_body(1),
                    budget.clone(),
                )
            }));
            assert!(hits.get());
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
    Ok(())
}

#[test]
fn carrier_shared_decoded_sentinel_keys_are_ordinary_and_rewrite_only_value_lexemes(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    for key in [
        "$serde_json::private::RawValue",
        "\\u0024serde_json::private::RawValue",
        "$serde_json::private::Number",
    ] {
        let encoded = format!(r#"{{ "{key}" : "synthetic", "number":9007199254740993 }}"#);
        let mut body = json!({"input":[{"type":"function_call", "arguments":encoded}]});
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
        let mut leaves = 0;
        plan.rewrite(&mut body, |leaf| {
            if !leaf.writable {
                return Ok(None);
            }
            assert_eq!(leaf.text, "synthetic");
            leaves += 1;
            Ok(Some("changed".to_owned()))
        })?;
        assert_eq!(leaves, 1);
        let expected = encoded.replace("\"synthetic\"", "\"changed\"");
        assert_eq!(
            body["input"][0]["arguments"].as_str(),
            Some(expected.as_str())
        );
        drop(plan);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn carrier_metadata_simple_path_leaf_retention_and_exact_peak() -> Result<(), TokenError> {
    use super::{payload_budget::PayloadBudget, request_carriers::metadata_sizes};
    let body = json!({"input":"synthetic"});
    let original = decoded_carrier_original_charge(&body)?;
    let (step, leaf, _) = metadata_sizes();
    let retained = original + 4 * step + 5 + 4 * leaf;
    let peak = retained + 4 * step + 5;
    let exhausted = PayloadBudget::with_limit(peak - 1);
    assert_eq!(
        CodexRequestCarriers::from_body_with_budget(&body, exhausted.clone()).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(exhausted.used(), 0);
    let budget = PayloadBudget::with_limit(peak);
    let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    assert_eq!(budget.used(), retained);
    drop(plan);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn carrier_metadata_allocation_failures_and_unwind_release_original_and_partial_paths() {
    use super::{
        payload_budget::PayloadBudget,
        payload_metadata::test_support::{self, Action, Phase},
    };
    for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
        for fail_at in 1..=5 {
            for unwind in [false, true] {
                let budget = PayloadBudget::with_limit(1024 * 1024);
                let hits = std::rc::Rc::new(std::cell::Cell::new(0));
                let observed = std::rc::Rc::clone(&hits);
                let _hook = test_support::install(move |event| {
                    if event.phase == phase {
                        observed.set(observed.get() + 1);
                        if observed.get() == fail_at {
                            if unwind {
                                panic!("synthetic carrier metadata unwind");
                            }
                            return Action::Fail;
                        }
                    }
                    Action::Continue
                });
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    CodexRequestCarriers::from_body_with_budget(
                        &json!({"input":"synthetic"}),
                        budget.clone(),
                    )
                }));
                assert_eq!(hits.get(), fail_at);
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
}

#[test]
fn carrier_metadata_decoded_documents_and_nested_path_copies_are_independently_owned(
) -> Result<(), TokenError> {
    use super::{
        payload_budget::PayloadBudget,
        payload_metadata::test_support::{self, Action, Kind, Phase},
    };
    let encoded = r#"{"z":{"\u0070assword":"synthetic-a"},"a":["synthetic-b"]}"#;
    let body = json!({"input":[{"type":"function_call", "arguments":encoded}]});
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let hits = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&hits);
    let _hook = test_support::install(move |event| {
        if event.phase == Phase::AfterAllocation && event.kind == Kind::Key {
            observed.set(observed.get() + 1);
        }
        Action::Continue
    });
    let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    assert!(hits.get() >= 10);
    let retained = budget.used();
    let second = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    assert_eq!(budget.used(), 2 * retained);
    let mut values = Vec::new();
    plan.visit(|leaf| {
        if leaf.writable {
            values.push((leaf.text.to_owned(), leaf.is_password_field));
        }
        Ok(())
    })?;
    assert_eq!(
        values,
        vec![
            ("synthetic-a".to_owned(), true),
            ("synthetic-b".to_owned(), false)
        ]
    );
    drop(second);
    assert_eq!(budget.used(), retained);
    drop(plan);
    assert_eq!(budget.used(), 0);
    Ok(())
}

// Test fixture sizing only. Independent exact-formula tests above establish metadata
// precharging; these profiles keep older JSON/serialization boundaries composable.
pub(super) struct CarrierProfile {
    pub retained: usize,
    pub peak: usize,
    pub parses: Vec<usize>,
}
pub(super) fn carrier_profile(body: &Value) -> Result<CarrierProfile, TokenError> {
    use super::{
        payload_budget::PayloadBudget, payload_json::test_support as json_support,
        payload_metadata::test_support as metadata_support,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let peak = Rc::new(Cell::new(0));
    let parses = Rc::new(RefCell::new(Vec::new()));
    let p = Rc::clone(&peak);
    let events = Rc::clone(&parses);
    let _json = json_support::install(move |event| {
        p.set(p.get().max(event.used));
        if event.phase == json_support::Phase::BeforeParse {
            events.borrow_mut().push(event.used);
        }
        json_support::Action::Continue
    });
    let p = Rc::clone(&peak);
    let _metadata = metadata_support::install(move |event| {
        p.set(p.get().max(event.used));
        metadata_support::Action::Continue
    });
    let owner = CodexRequestCarriers::from_body_with_budget(body, budget.clone())?;
    let retained = budget.used();
    let peak = peak.get().max(retained);
    let parses = parses.borrow().clone();
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(CarrierProfile {
        retained,
        peak,
        parses,
    })
}

#[test]
fn carrier_metadata_decoded_document_retention_matches_all_independent_allocations(
) -> Result<(), TokenError> {
    use super::{payload_budget::PayloadBudget, request_carriers::metadata_sizes};
    let encoded = r#"{"x":"synthetic"}"#;
    let body = json!({"input":[{"type":"function_call", "arguments":encoded}]});
    let (step, leaf, document) = metadata_sizes();
    // Independent paths: readonly type, argument outer path, and decoded x.
    let metadata = 3 * 4 * step + (5 + 4) + (5 + 9) + 1 + 4 * leaf + 4 * document;
    let retained =
        decoded_carrier_original_charge(&body)? + encoded.len() + 3 * 512 + 1024 + metadata;
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    assert_eq!(budget.used(), retained);
    drop(plan);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn carrier_metadata_deep_paths_and_leaf_document_growth_all_failures_release_reservations(
) -> Result<(), TokenError> {
    use super::{
        payload_budget::PayloadBudget,
        payload_metadata::test_support::{self, Action, Phase},
    };
    let mut nested = json!([
        "synthetic-a",
        "synthetic-b",
        "synthetic-c",
        "synthetic-d",
        "synthetic-e"
    ]);
    for _ in 0..6 {
        nested = json!({"synthetic-key":nested});
    }
    let encoded = serde_json::to_string(&nested).map_err(|_| TokenError::Unavailable)?;
    let body = json!({"input":(0..5).map(|_| json!({"type":"function_call", "arguments":encoded})).collect::<Vec<_>>()});
    let budget = PayloadBudget::with_limit(8 * 1024 * 1024);
    let hits = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&hits);
    let hook = test_support::install(move |event| {
        if event.phase == Phase::AfterAllocation {
            observed.set(observed.get() + 1);
        }
        Action::Continue
    });
    let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    let total = hits.get();
    assert!(total > 100);
    drop(plan);
    assert_eq!(budget.used(), 0);
    drop(hook);
    for fail_at in 1..=total {
        let hits = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = std::rc::Rc::clone(&hits);
        let _hook = test_support::install(move |event| {
            if event.phase == Phase::AfterAllocation {
                observed.set(observed.get() + 1);
                if observed.get() == fail_at {
                    return Action::Fail;
                }
            }
            Action::Continue
        });
        assert_eq!(
            CodexRequestCarriers::from_body_with_budget(&body, budget.clone()).err(),
            Some(TokenError::Unavailable)
        );
        assert_eq!(hits.get(), fail_at);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn carrier_argument_order_escaped_keys_password_and_leaf_ids_keep_exact_lexemes(
) -> Result<(), TokenError> {
    let encoded = r#"{ "z" : "synthetic-z", "\u0070assword":"synthetic-password", "a":"synthetic-a", "number":9007199254740993 }"#;
    let mut body = json!({"input":[{"type":"function_call", "arguments":encoded}]});
    let plan = isolated_carriers(&body)?;
    let mut order = Vec::new();
    plan.rewrite(&mut body, |leaf| {
        if !leaf.writable {
            return Ok(None);
        }
        order.push((leaf.id, leaf.text.to_owned(), leaf.is_password_field));
        Ok(Some(format!("changed-{}", leaf.id)))
    })?;
    assert_eq!(
        order
            .iter()
            .map(|(_, text, _)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["synthetic-z", "synthetic-password", "synthetic-a"]
    );
    assert!(order[0].0 < order[1].0 && order[1].0 < order[2].0);
    assert!(!order[0].2 && order[1].2 && !order[2].2);
    let mut expected = encoded.to_owned();
    for (id, text, _) in order {
        expected = expected.replace(&format!("\"{text}\""), &format!("\"changed-{id}\""));
    }
    assert_eq!(
        body["input"][0]["arguments"].as_str(),
        Some(expected.as_str())
    );
    Ok(())
}

#[test]
fn carrier_rewrite_owner_borrows_once_and_retains_only_final_candidate() -> Result<(), TokenError> {
    use super::{payload_budget::PayloadBudget, payload_json::JsonStringReplacement};
    let body = json!({"input":"synthetic"});
    let retained = carrier_profile(&body)?.retained;
    const FINAL: usize = 2 * 128 + 512 + 5 + 7;
    let edit_capacity = 4 * std::mem::size_of::<(usize, &str)>();
    let replacement_capacity = 4 * std::mem::size_of::<JsonStringReplacement<'_>>();
    let exact = retained + edit_capacity + replacement_capacity + FINAL;
    for limit in [exact - 1, exact] {
        let budget = PayloadBudget::with_limit(limit);
        let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
        let calls = std::cell::Cell::new(0);
        let result = plan.rewrite_candidate(&body, |leaf| {
            calls.set(calls.get() + 1);
            assert_eq!(leaf.text, "synthetic");
            Ok(Some("changed"))
        });
        assert_eq!(calls.get(), 1);
        if limit < exact {
            assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
            assert_eq!(budget.used(), retained);
        } else {
            let candidate = result?;
            assert!(candidate.as_value()["input"] == "changed");
            assert_eq!(budget.used(), retained + FINAL);
            drop(plan);
            assert_eq!(budget.used(), FINAL);
            drop(candidate);
            assert_eq!(budget.used(), 0);
        }
        assert!(body["input"] == "synthetic");
    }
    Ok(())
}

#[test]
fn carrier_rewrite_owner_splices_decoded_values_into_precharged_buffer() -> Result<(), TokenError> {
    use super::{
        payload_budget::PayloadBudget,
        payload_buffer::test_support::{self, Action, Phase},
    };
    let encoded = r#"{ "z":"one", "\u0070assword":"two", "n":9007199254740993, "a":"three" }"#;
    let body =
        json!({"instructions":"direct", "input":[{"type":"function_call", "arguments":encoded}]});
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    let retained = budget.used();
    let allocations = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&allocations);
    let hook = test_support::install(move |event| {
        if event.phase == Phase::BeforeAllocation {
            observed.set(observed.get() + 1);
            assert!(event.used >= retained + event.requested);
        }
        Action::Continue
    });
    let mut order = Vec::new();
    let candidate = plan.rewrite_candidate(&body, |leaf| {
        if !leaf.writable {
            return Ok(None);
        }
        order.push((leaf.id, leaf.is_password_field));
        Ok(Some(match leaf.text {
            "direct" => "changed",
            "one" => "\n中\"\\",
            "two" => "",
            "three" => "tail",
            _ => return Err(TokenError::RedactionFailed),
        }))
    })?;
    drop(hook);
    assert_eq!(allocations.get(), 1);
    assert_eq!(order.len(), 4);
    assert!(order.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert!(order[2].1 && !order[1].1);
    let expected = encoded
        .replace(
            "\"one\"",
            &serde_json::to_string("\n中\"\\").map_err(|_| TokenError::Unavailable)?,
        )
        .replace("\"two\"", "\"\"")
        .replace("\"three\"", "\"tail\"");
    assert!(candidate.as_value()["instructions"] == "changed");
    assert_eq!(
        candidate.as_value()["input"][0]["arguments"].as_str(),
        Some(expected.as_str())
    );
    let final_charge = decoded_carrier_original_charge(candidate.as_value())?;
    assert_eq!(budget.used(), retained + final_charge);
    drop(plan);
    assert_eq!(budget.used(), final_charge);
    drop(candidate);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn carrier_rewrite_owner_stale_readonly_errors_and_noop_keep_input_atomic() -> Result<(), TokenError>
{
    use super::payload_budget::PayloadBudget;
    let body = json!({"instructions":"direct", "input":[{"type":"function_call", "call_id":"readonly", "arguments":"{ \"v\": \"\\u4e2d\", \"n\":1e1 }"}]});
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    let retained = budget.used();
    let mut stale = body.clone();
    stale["instructions"] = json!("stale");
    let called = std::cell::Cell::new(false);
    assert_eq!(
        plan.rewrite_candidate(&stale, |_| {
            called.set(true);
            Ok(None)
        })
        .err(),
        Some(TokenError::RedactionFailed)
    );
    assert!(!called.get());
    assert_eq!(budget.used(), retained);
    assert_eq!(
        plan.rewrite_candidate(&body, |_| Ok(Some("forbidden")))
            .err(),
        Some(TokenError::RedactionFailed)
    );
    assert_eq!(budget.used(), retained);
    assert_eq!(
        plan.rewrite_candidate(&body, |_| Err(TokenError::ContextUnavailable))
            .err(),
        Some(TokenError::ContextUnavailable)
    );
    assert_eq!(budget.used(), retained);
    let noop = plan.rewrite_candidate(&body, |_| Ok(None))?;
    assert!(noop.as_value() == &body);
    drop(noop);
    assert_eq!(budget.used(), retained);
    let same =
        plan.rewrite_candidate(
            &body,
            |leaf| Ok((leaf.text == "direct").then_some("direct")),
        )?;
    assert!(same.as_value() == &body);
    drop(same);
    assert_eq!(budget.used(), retained);
    drop(plan);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn carrier_rewrite_owner_metadata_buffer_and_tree_failures_release_temporaries(
) -> Result<(), TokenError> {
    use super::{payload_budget::PayloadBudget, payload_buffer, payload_json, payload_metadata};
    let body = json!({"instructions":"direct", "input":[{"type":"function_call", "arguments":"{ \"v\": \"decoded\" }"}]});
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let plan = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
    let retained = budget.used();
    for source in 0..3 {
        for unwind in [false, true] {
            let hits = std::rc::Rc::new(std::cell::Cell::new(0));
            let observed = std::rc::Rc::clone(&hits);
            let _metadata = (source == 0).then(|| {
                payload_metadata::test_support::install(move |_| {
                    observed.set(observed.get() + 1);
                    if unwind {
                        panic!("synthetic rewrite metadata unwind");
                    }
                    payload_metadata::test_support::Action::Fail
                })
            });
            let observed = std::rc::Rc::clone(&hits);
            let _buffer = (source == 1).then(|| {
                payload_buffer::test_support::install(move |_| {
                    observed.set(observed.get() + 1);
                    if unwind {
                        panic!("synthetic rewrite buffer unwind");
                    }
                    payload_buffer::test_support::Action::FailAllocation
                })
            });
            let observed = std::rc::Rc::clone(&hits);
            let _tree = (source == 2).then(|| {
                payload_json::test_support::install(move |_| {
                    observed.set(observed.get() + 1);
                    if unwind {
                        panic!("synthetic rewrite tree unwind");
                    }
                    payload_json::test_support::Action::Fail
                })
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plan.rewrite_candidate(&body, |leaf| Ok(leaf.writable.then_some("changed")))
            }));
            assert_eq!(hits.get(), 1);
            if unwind {
                assert!(result.is_err());
            } else {
                assert_eq!(
                    result.ok().and_then(Result::err),
                    Some(TokenError::Unavailable)
                );
            }
            assert_eq!(budget.used(), retained);
            assert!(body["instructions"] == "direct");
        }
    }
    drop(plan);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn carrier_rewrite_owner_rejects_reordered_and_float_sign_stale_bodies_before_callbacks(
) -> Result<(), TokenError> {
    let source = json!({"input":{"z":"one","a":"two"},"temperature":-0.0});
    let plan = isolated_carriers(&source)?;
    let variants = [
        json!({"temperature":-0.0,"input":{"z":"one","a":"two"}}),
        json!({"input":{"a":"two","z":"one"},"temperature":-0.0}),
        json!({"input":{"z":"one","a":"two"},"temperature":0.0}),
    ];
    for stale in &variants {
        let called = std::cell::Cell::new(false);
        let result = plan.rewrite_candidate(stale, |_| {
            called.set(true);
            Ok(None)
        });
        assert_eq!(result.err(), Some(TokenError::RedactionFailed));
        assert!(!called.get());
    }
    let candidate = plan.rewrite_candidate(&source, |_| Ok(None))?;
    assert_eq!(
        serde_json::to_vec(candidate.as_value()).ok(),
        serde_json::to_vec(&source).ok()
    );
    Ok(())
}
