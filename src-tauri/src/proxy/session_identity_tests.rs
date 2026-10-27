//! Synthetic identity tests; no cursor or token recognition grants continuity.
use super::session::{extract_client_session_id, extract_codex_restoration_session_id};
use axum::http::{HeaderMap, HeaderValue};
use serde_json::{json, Value};

const HEADERS: [&str; 4] = ["thread-id", "session-id", "session_id", "x-session-id"];

fn assert_identity(headers: &HeaderMap, body: &Value, expected: &str) {
    let actual = extract_codex_restoration_session_id(headers, body);
    assert!(actual == Some(expected), "synthetic identity mismatch");
}

#[test]
fn each_header_accepts_short_raw_identity_without_prefixing() {
    for name in HEADERS {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_static("x"));
        assert_identity(&headers, &json!({}), "x");
    }
}

#[test]
fn header_precedence_is_first_present_not_longest_identity() {
    let mut headers = HeaderMap::new();
    for name in HEADERS {
        headers.insert(name, HeaderValue::from_static(name));
    }
    let body = json!({"metadata":{"session_id":"metadata-session"}});
    for name in HEADERS {
        assert_identity(&headers, &body, name);
        headers.remove(name);
    }
    assert_identity(&headers, &body, "metadata-session");
}

#[test]
fn selected_empty_or_whitespace_header_never_falls_back() {
    let body = json!({"metadata":{"session_id":"valid-fallback"}});
    for invalid in ["", " ", " leading", "trailing ", "two words", "tab\tvalue"] {
        for name in HEADERS {
            let mut headers = HeaderMap::new();
            headers.insert(
                name,
                HeaderValue::from_str(invalid).expect("synthetic header"),
            );
            assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
        }
    }
    let mut headers = HeaderMap::new();
    headers.insert("thread-id", HeaderValue::from_static(""));
    headers.insert("session-id", HeaderValue::from_static("valid-lower-header"));
    assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
}

#[test]
fn non_text_header_cannot_authorize_metadata_fallback() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "thread-id",
        HeaderValue::from_bytes(&[0xff]).expect("synthetic header"),
    );
    let body = json!({"metadata":{"session_id":"valid-fallback"}});
    assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
}

#[test]
fn duplicate_selected_header_is_anonymous_even_when_values_agree() {
    for name in HEADERS {
        for second in ["first", "second"] {
            let mut headers = HeaderMap::new();
            headers.append(name, HeaderValue::from_static("first"));
            headers.append(name, HeaderValue::from_static(second));
            let body = json!({"metadata":{"session_id":"valid-fallback"}});
            assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
        }
    }
}

#[test]
fn invalid_lower_sources_do_not_displace_valid_higher_identity() {
    let mut headers = HeaderMap::new();
    headers.insert("thread-id", HeaderValue::from_static("chosen"));
    headers.append("session-id", HeaderValue::from_static(""));
    headers.append("session-id", HeaderValue::from_static("duplicate"));
    assert_identity(
        &headers,
        &json!({"metadata":{"session_id":false}}),
        "chosen",
    );
}

#[test]
fn exact_256_byte_header_is_borrowed_and_257_is_anonymous() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "thread-id",
        HeaderValue::from_str(&"h".repeat(256)).expect("synthetic header"),
    );
    let body = json!({});
    let value = headers
        .get("thread-id")
        .expect("synthetic header")
        .to_str()
        .expect("text");
    assert_identity(&headers, &body, value);
    let selected = extract_codex_restoration_session_id(&headers, &body).expect("selected");
    assert_eq!(selected.as_ptr(), value.as_ptr());
    headers.insert(
        "thread-id",
        HeaderValue::from_str(&"h".repeat(257)).expect("synthetic header"),
    );
    assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
}

#[test]
fn metadata_identity_is_exact_and_borrowed_without_normalization() {
    let headers = HeaderMap::new();
    for raw in ["m", "é", "e\u{301}", "Case-Sensitive_1"] {
        let body = json!({"metadata":{"session_id":raw}});
        assert_identity(&headers, &body, raw);
        let stored = body
            .pointer("/metadata/session_id")
            .and_then(Value::as_str)
            .expect("fixture");
        let selected = extract_codex_restoration_session_id(&headers, &body).expect("selected");
        assert_eq!(selected.as_ptr(), stored.as_ptr());
    }
}

#[test]
fn metadata_limit_counts_utf8_bytes_not_characters() {
    let headers = HeaderMap::new();
    let exact = "é".repeat(128);
    let body = json!({"metadata":{"session_id":exact}});
    assert_identity(&headers, &body, &exact);
    for invalid in ["é".repeat(129), "a".repeat(257)] {
        let body = json!({"metadata":{"session_id":invalid}});
        assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
    }
}

#[test]
fn metadata_whitespace_and_controls_are_rejected_without_trim() {
    let headers = HeaderMap::new();
    for invalid in [
        "",
        " a",
        "a ",
        "a\tb",
        "a\nb",
        "a\rb",
        "a\0b",
        "a\u{7f}b",
        "a\u{85}b",
        "a\u{a0}b",
        "a\u{2003}b",
        "a\u{2028}b",
        "a\u{3000}b",
    ] {
        let body =
            json!({"metadata":{"session_id":invalid,"user_id":"user_fixture_session_fallback"}});
        assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
    }
}

#[test]
fn present_non_string_metadata_identity_never_uses_user_fallback() {
    let headers = HeaderMap::new();
    for invalid in [Value::Null, json!(false), json!(17), json!([]), json!({})] {
        let body =
            json!({"metadata":{"session_id":invalid,"user_id":"user_fixture_session_fallback"}});
        assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
    }
}

#[test]
fn metadata_session_identity_precedes_legacy_user_suffix() {
    assert_identity(
        &HeaderMap::new(),
        &json!({"metadata":{"session_id":"chosen","user_id":"user_fixture_session_other"}}),
        "chosen",
    );
}

#[test]
fn recognized_legacy_user_suffix_is_last_raw_borrowed_source() {
    let body = json!({"metadata":{"user_id":"user_fixture_session_raw-suffix"}});
    let headers = HeaderMap::new();
    assert_identity(&headers, &body, "raw-suffix");
    let raw = body
        .pointer("/metadata/user_id")
        .and_then(Value::as_str)
        .expect("fixture");
    let suffix = raw.split_once("_session_").expect("fixture suffix").1;
    let selected = extract_codex_restoration_session_id(&headers, &body).expect("selected");
    assert_eq!(selected.as_ptr(), suffix.as_ptr());
}

#[test]
fn legacy_user_suffix_is_validated_and_never_uses_entire_user_id() {
    let headers = HeaderMap::new();
    for invalid in [
        "no-session-marker".to_owned(),
        "user_x_session_".to_owned(),
        "user_x_session_two words".to_owned(),
        format!("user_x_session_{}", "x".repeat(257)),
    ] {
        let body = json!({"metadata":{"user_id":invalid}});
        assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
    }
    let body = json!({"metadata":{"user_id":42}});
    assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
}

#[test]
fn cursors_cache_keys_and_request_ids_are_not_identity_sources() {
    let mut headers = HeaderMap::new();
    for name in [
        "x-request-id",
        "request-id",
        "conversation-id",
        "x-grok-conv-id",
    ] {
        headers.insert(name, HeaderValue::from_static("not-stable-codex-identity"));
    }
    let body = json!({"previous_response_id":"resp_fixture","prompt_cache_key":"cache_fixture","session_id":"unrecognized-root","metadata":{"request_id":"request_fixture"}});
    assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
}

#[test]
fn legacy_extractor_keeps_trim_prefix_and_invalid_high_source_fallback() {
    let mut headers = HeaderMap::new();
    headers.insert("thread-id", HeaderValue::from_static(""));
    headers.insert(
        "session-id",
        HeaderValue::from_static("  synthetic-long-session-identity  "),
    );
    let body = json!({"metadata":{"session_id":"metadata-fallback"}});
    let legacy = extract_client_session_id(&headers, &body, "codex").expect("legacy identity");
    assert!(legacy.session_id == "codex_synthetic-long-session-identity");
    assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
}

#[test]
fn extraction_does_not_mutate_headers_or_body() {
    let mut headers = HeaderMap::new();
    headers.insert("thread-id", HeaderValue::from_static("raw"));
    let body = json!({"input":"synthetic text","metadata":{"session_id":"other"}});
    let original_headers = headers.clone();
    let original_body = body.clone();
    assert_identity(&headers, &body, "raw");
    assert!(headers == original_headers);
    assert!(body == original_body);
}

#[test]
fn header_identity_accepts_exact_utf8_bytes_without_normalization() {
    let body = json!({});
    for raw in ["é".to_owned(), "e\u{301}".to_owned(), "é".repeat(128)] {
        let mut headers = HeaderMap::new();
        headers.insert(
            "thread-id",
            HeaderValue::from_bytes(raw.as_bytes()).expect("synthetic header"),
        );
        assert_identity(&headers, &body, &raw);
        let selected = extract_codex_restoration_session_id(&headers, &body).expect("selected");
        let stored = headers.get("thread-id").expect("fixture");
        assert_eq!(selected.as_ptr(), stored.as_bytes().as_ptr());
    }
}

#[test]
fn utf8_header_whitespace_controls_and_byte_overflow_are_anonymous() {
    for raw in [
        "é".repeat(129),
        "a\u{a0}b".to_owned(),
        "a\u{85}b".to_owned(),
        "a\u{3000}b".to_owned(),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(
            "thread-id",
            HeaderValue::from_bytes(raw.as_bytes()).expect("synthetic header"),
        );
        headers.insert("session-id", HeaderValue::from_static("lower-fallback"));
        assert!(extract_codex_restoration_session_id(&headers, &json!({})).is_none());
    }
}
