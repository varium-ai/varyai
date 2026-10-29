//! Stable JSON helpers for cache-sensitive request bodies.

use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) fn canonicalize_value(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_value).collect()),
        Value::Object(map) => {
            let mut entries = map.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));

            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key, canonicalize_value(value));
            }
            Value::Object(sorted)
        }
        other => other,
    }
}

pub(crate) fn canonical_json_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(_) => value.to_string(),
        Value::Array(values) => {
            let parts = values.iter().map(canonical_json_string).collect::<Vec<_>>();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(left, _)| *left);
            let parts = entries
                .into_iter()
                .map(|(key, value)| {
                    let key = Value::String(key.clone()).to_string();
                    format!("{key}:{}", canonical_json_string(value))
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", parts.join(","))
        }
    }
}

pub(crate) fn canonicalize_json_string_if_parseable(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return value.to_string();
    }

    serde_json::from_str::<Value>(trimmed)
        .map(|parsed| canonical_json_string(&parsed))
        .unwrap_or_else(|_| value.to_string())
}

/// Normalize a tool-call `arguments` string into a valid JSON payload.
///
/// Identical to [`canonicalize_json_string_if_parseable`] except that an empty
/// (or whitespace-only) value is coerced to `"{}"` instead of being passed
/// through verbatim. A no-argument tool call must serialize as `"{}"`; strict
/// upstreams such as Minimax reject `arguments: ""` with a 400
/// `invalid function arguments json string` error, whereas lenient ones
/// (OpenAI, Kimi) silently treat it as an empty object.
pub(crate) fn canonicalize_tool_arguments_str(value: &str) -> String {
    if value.trim().is_empty() {
        return "{}".to_string();
    }
    canonicalize_json_string_if_parseable(value)
}

/// Normalize a tool-call `arguments` field from a Responses/Chat item.
///
/// Mirrors the inline `match` that several transform paths used to duplicate:
/// a string is canonicalized (with empty coerced to `"{}"`), a structured
/// value is serialized canonically, and a missing field defaults to `"{}"`.
pub(crate) fn canonicalize_tool_arguments(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => canonicalize_tool_arguments_str(s),
        Some(v) => canonical_json_string(v),
        None => "{}".to_string(),
    }
}

pub(crate) fn short_value_hash(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return "absent".to_string();
    };
    short_sha256_hex(canonical_json_string(value).as_bytes())
}

pub(crate) fn short_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn baseline_proxy_production_sources_do_not_unwrap_or_expect() {
        let sources = [
            ("json_canonical.rs", include_str!("json_canonical.rs")),
            ("provider_router.rs", include_str!("provider_router.rs")),
            ("providers/claude.rs", include_str!("providers/claude.rs")),
            ("providers/codex.rs", include_str!("providers/codex.rs")),
            (
                "providers/transform_codex_anthropic.rs",
                include_str!("providers/transform_codex_anthropic.rs"),
            ),
            ("usage/logger.rs", include_str!("usage/logger.rs")),
        ];
        // Each inspected file ends with one test module; item-level test cfgs must
        // not truncate the production source included in this guard.
        let boundary = "\n#[cfg(test)]\nmod tests {";
        let forbidden = regex::Regex::new(r"\.\s*(?:unwrap|expect)\s*\(").unwrap();
        let mut violations = Vec::new();
        for (path, source) in sources {
            assert_eq!(source.matches(boundary).count(), 1, "{path}: test boundary");
            let (production, _) = source.split_once(boundary).unwrap();
            for call in forbidden.find_iter(production) {
                let line = production[..call.start()]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count()
                    + 1;
                violations.push(format!("{path}:{line}"));
            }
        }
        assert!(
            violations.is_empty(),
            "production panic calls ({}): {}",
            violations.len(),
            violations.join(", ")
        );
    }

    #[test]
    fn canonical_json_string_preserves_serde_string_and_key_bytes() {
        let controls: String = (0..=0x1f)
            .map(|code| char::from_u32(code).unwrap())
            .collect();
        for text in [
            String::new(),
            "synthetic \"quote\" \\ /\n\r\t\u{8}\u{c}".to_string(),
            controls,
            "synthetic é 中文 🦀 \u{2028}\u{2029}\u{10ffff}".to_string(),
        ] {
            let encoded = serde_json::to_string(&text).unwrap();
            assert_eq!(canonical_json_string(&json!(&text)), encoded);
            assert_eq!(
                canonical_json_string(&json!({text: "synthetic"})),
                format!("{{{encoded}:\"synthetic\"}}")
            );
        }
    }

    #[test]
    fn canonical_json_string_preserves_mixed_value_bytes() {
        let value = json!({
            "z": [null, true, false, -1, u64::MAX, 1.25, "synthetic\n"],
            "a": {"b": "\\", "a": "\""}
        });
        assert_eq!(
            canonical_json_string(&value),
            r#"{"a":{"a":"\"","b":"\\"},"z":[null,true,false,-1,18446744073709551615,1.25,"synthetic\n"]}"#
        );
    }

    #[test]
    fn canonical_json_string_sorts_nested_object_keys() {
        let left = json!({
            "b": 2,
            "a": {
                "d": true,
                "c": [3, {"z": 1, "y": 2}]
            }
        });
        let right = json!({
            "a": {
                "c": [3, {"y": 2, "z": 1}],
                "d": true
            },
            "b": 2
        });

        assert_eq!(canonical_json_string(&left), canonical_json_string(&right));
        assert_eq!(
            short_value_hash(Some(&left)),
            short_value_hash(Some(&right))
        );
    }

    #[test]
    fn canonicalize_value_sorts_map_storage_order() {
        let value = canonicalize_value(json!({"b": 2, "a": 1}));

        assert_eq!(serde_json::to_string(&value).unwrap(), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn canonicalize_json_string_if_parseable_sorts_keys_and_removes_whitespace() {
        assert_eq!(
            canonicalize_json_string_if_parseable(r#"{ "b": 2, "a": 1 }"#),
            r#"{"a":1,"b":2}"#
        );
    }

    #[test]
    fn canonicalize_json_string_if_parseable_preserves_plain_text() {
        assert_eq!(
            canonicalize_json_string_if_parseable("plain text"),
            "plain text"
        );
    }

    #[test]
    fn canonicalize_tool_arguments_str_coerces_empty_to_object() {
        assert_eq!(canonicalize_tool_arguments_str(""), "{}");
        assert_eq!(canonicalize_tool_arguments_str("   "), "{}");
        assert_eq!(canonicalize_tool_arguments_str("\n\t"), "{}");
    }

    #[test]
    fn canonicalize_tool_arguments_str_canonicalizes_valid_json() {
        assert_eq!(
            canonicalize_tool_arguments_str(r#"{ "b": 2, "a": 1 }"#),
            r#"{"a":1,"b":2}"#
        );
    }

    #[test]
    fn canonicalize_tool_arguments_handles_field_variants() {
        // Missing field -> empty object.
        assert_eq!(canonicalize_tool_arguments(None), "{}");
        // Empty string field -> empty object.
        assert_eq!(canonicalize_tool_arguments(Some(&json!(""))), "{}");
        // String field with JSON -> canonicalized.
        assert_eq!(
            canonicalize_tool_arguments(Some(&json!(r#"{"b":2,"a":1}"#))),
            r#"{"a":1,"b":2}"#
        );
        // Structured (non-string) field -> canonical serialization.
        assert_eq!(
            canonicalize_tool_arguments(Some(&json!({"b": 2, "a": 1}))),
            r#"{"a":1,"b":2}"#
        );
    }
}
