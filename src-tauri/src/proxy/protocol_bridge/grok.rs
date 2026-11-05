//! Grok tool-schema compatibility: Rust regex patterns and non-null required lists.
use serde_json::Value;

pub(in crate::proxy) fn normalize_tool_schemas(body: &mut Value) {
    let grok = body["model"].as_str().is_some_and(|model| {
        model
            .rsplit('/')
            .next()
            .unwrap_or(model)
            .to_ascii_lowercase()
            .starts_with("grok")
    });
    if !grok {
        return;
    }
    if let Some(tools) = body["tools"].as_array_mut() {
        for tool in tools {
            for path in ["/parameters", "/function/parameters", "/input_schema"] {
                if let Some(schema) = tool.pointer_mut(path) {
                    normalize_schema(schema);
                }
            }
        }
    }
}

fn normalize_schema(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    if object.get("required").is_some_and(Value::is_null) {
        object.remove("required");
    }
    if let Some(Value::String(pattern)) = object.get_mut("pattern") {
        *pattern = rust_pattern(pattern);
    }
    // Visit only schema-bearing keywords. Examples, defaults, const and enum
    // contain user data, including properties literally named "pattern".
    for key in [
        "properties",
        "patternProperties",
        "$defs",
        "definitions",
        "dependentSchemas",
    ] {
        if let Some(Value::Object(children)) = object.get_mut(key) {
            for child in children.values_mut() {
                normalize_schema(child);
            }
        }
    }
    for key in [
        "items",
        "additionalItems",
        "additionalProperties",
        "unevaluatedProperties",
        "unevaluatedItems",
        "contains",
        "propertyNames",
        "not",
        "if",
        "then",
        "else",
        "allOf",
        "anyOf",
        "oneOf",
        "prefixItems",
    ] {
        if let Some(child) = object.get_mut(key) {
            if let Value::Array(children) = child {
                for child in children {
                    normalize_schema(child);
                }
            } else {
                normalize_schema(child);
            }
        }
    }
}

fn rust_pattern(pattern: &str) -> String {
    let mut output = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                output.push(c);
                if let Some(escaped) = chars.next() {
                    output.push(escaped);
                }
                continue;
            }
            '[' if in_class => output.push('\\'),
            '[' => in_class = true,
            ']' => in_class = false,
            '&' | '-' | '~' if in_class && chars.peek() == Some(&c) => {
                output.push(c);
                output.push('\\');
                let _ = chars.next();
            }
            _ => {}
        }
        output.push(c);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn grok_patterns_keep_ecmascript_class_semantics() {
        for (input, expected) in [
            (r"^[^\s[\]]+$", r"^[^\s\[\]]+$"),
            (r"^[^\s\[\]]+$", r"^[^\s\[\]]+$"),
            ("[a&&b]", r"[a&\&b]"),
            ("[+--]", r"[+-\-]"),
            ("[~~]", r"[~\~]"),
            (r"a&&b--c~~d\[x", r"a&&b--c~~d\[x"),
            ("[中文[字]", r"[中文\[字]"),
        ] {
            assert_eq!(rust_pattern(input), expected);
        }
    }

    #[test]
    fn grok_null_required_is_omitted_only_in_tool_schema_nodes() {
        let mut body = json!({"model":"grok-4.5","messages":[{"role":"user","content":"Synthetic prompt"}],"tools":[{"function":{"parameters":{
            "type":"object","required":null,
            "properties":{"nested":{"type":"object","required":null},"required":{"type":"string"}},
            "anyOf":[{"required":null},{"required":["nested"]}],
            "default":{"required":null},"examples":[{"required":null}],"const":{"required":null}
        }}}]});
        let mut expected = body.clone();
        let schema = &mut expected["tools"][0]["function"]["parameters"];
        schema.as_object_mut().unwrap().remove("required");
        schema["properties"]["nested"]
            .as_object_mut()
            .unwrap()
            .remove("required");
        schema["anyOf"][0]
            .as_object_mut()
            .unwrap()
            .remove("required");
        normalize_tool_schemas(&mut body);
        assert_eq!(body, expected);
        let mut other =
            json!({"model":"synthetic-model","tools":[{"parameters":{"required":null}}]});
        let original = other.clone();
        normalize_tool_schemas(&mut other);
        assert_eq!(other, original);
    }

    #[test]
    fn grok_local_schema_refs_expand_without_losing_constraints_or_data() {
        let record = json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false});
        let schema = json!({
            "type":"object",
            "properties":{
                "record":{"anyOf":[{"$ref":"#/$defs/record~1part~0name"},{"type":"null"}]},
                "restricted":{"$ref":"#/$defs/record~1part~0name","minProperties":1,"allOf":[{"maxProperties":1}]},
                "alias":{"$ref":"#/$defs/alias"},
                "never":{"$ref":"#/definitions/never","description":"Synthetic constraint"}
            },
            "$defs":{"record/part~name":record,"alias":{"$ref":"#/$defs/record~1part~0name"}},
            "definitions":{"never":false},
            "required":["record"],"additionalProperties":false,
            "default":{"$ref":"#/$defs/alias"},
            "examples":[{"$ref":"#/$defs/alias"}]
        });
        for model in ["grok-4.5", "x-ai/grok-4.5", "synthetic-model"] {
            let mut body = json!({"model":model,"tools":[{"function":{"parameters":schema}}]});
            let original = body.clone();
            normalize_tool_schemas(&mut body);
            if model == "synthetic-model" {
                assert_eq!(body, original);
                continue;
            }
            let normalized = &body["tools"][0]["function"]["parameters"];
            assert_eq!(normalized["properties"]["record"]["anyOf"][0], record);
            assert_eq!(normalized["properties"]["alias"], record);
            assert_eq!(normalized["properties"]["restricted"]["minProperties"], 1);
            assert_eq!(
                normalized["properties"]["restricted"]["allOf"],
                json!([{"maxProperties":1},record])
            );
            assert_eq!(normalized["properties"]["never"]["allOf"], json!([false]));
            assert_eq!(normalized["required"], schema["required"]);
            assert_eq!(normalized["additionalProperties"], false);
            assert_eq!(normalized["default"], schema["default"]);
            assert_eq!(normalized["examples"], schema["examples"]);
        }
    }

    #[test]
    fn grok_recursive_unresolved_external_or_scoped_refs_are_not_discarded() {
        for schema in [
            json!({"type":"object","properties":{"item":{"$ref":"#/$defs/loop"}},"$defs":{"loop":{"$ref":"#/$defs/loop"}}}),
            json!({"type":"object","properties":{"item":{"$ref":"#/$defs/missing"}}}),
            json!({"type":"object","properties":{"item":{"$ref":"https://example.invalid/synthetic-schema"}}}),
            json!({"type":"object","properties":{"item":{"$ref":"#/$defs/item"}},"$defs":{"item":{"$id":"synthetic-scope","type":"string"}}}),
        ] {
            let mut body = json!({"model":"grok-4.5","tools":[{"parameters":schema}]});
            let original = body.clone();
            normalize_tool_schemas(&mut body);
            assert_eq!(body, original);
        }
    }

    #[test]
    fn grok_patterns_only_change_tool_schemas_for_grok_models() {
        for model in [
            "grok-4.5",
            "x-ai/grok-4.5",
            "xai/Grok-4.7",
            "synthetic-model",
        ] {
            for field in ["parameters", "input_schema"] {
                let mut body = json!({"model":model,"input":"[a&&b]","tools":[{field:{"pattern":"[a&&b]","default":{"pattern":"[a&&b]"},"properties":{"pattern":{"type":"string"}}}}]});
                let original = body.clone();
                normalize_tool_schemas(&mut body);
                let mut expected = original;
                if model != "synthetic-model" {
                    expected["tools"][0][field]["pattern"] = json!(r"[a&\&b]");
                }
                assert_eq!(body, expected);
                let once = body.clone();
                normalize_tool_schemas(&mut body);
                assert_eq!(body, once);
            }
        }
    }
}

/// Classify a rejection using fixed protocol metadata only. Never return any
/// substring of an upstream error, which may echo credentials or tool input.
pub(in crate::proxy) fn rejection_code(body: &[u8]) -> &'static str {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return "routing_upstream_rejected";
    };
    let error = value.get("error").unwrap_or(&value);
    let message = error
        .as_str()
        .or_else(|| error.get("message").and_then(Value::as_str))
        .or_else(|| value.get("message").and_then(Value::as_str))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if message.contains("required") && message.contains("null") {
        "routing_upstream_null_required_rejected"
    } else if message.contains("pattern")
        && (message.contains("regex") || message.contains("schema"))
    {
        "routing_upstream_schema_pattern_rejected"
    } else if message.contains("tool_call_id")
        || message.contains("tool_calls") && message.contains("must be followed")
    {
        "routing_upstream_tool_history_rejected"
    } else if message.contains("schema")
        && (message.contains("invalid")
            || message.contains("validation")
            || message.contains("unsupported"))
    {
        "routing_upstream_tool_schema_rejected"
    } else {
        "routing_upstream_rejected"
    }
}

#[cfg(test)]
mod rejection_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejection_reasons_are_fixed_and_never_echo_upstream_text() {
        for (message, code) in [
            (
                "Schema validation failed: required: null is not an array; synthetic-private",
                "routing_upstream_null_required_rejected",
            ),
            (
                "Schema validation failed: pattern is not a regex; synthetic-private",
                "routing_upstream_schema_pattern_rejected",
            ),
            (
                "tool_call_id is not found; synthetic-private",
                "routing_upstream_tool_history_rejected",
            ),
            (
                "Invalid schema; synthetic-private",
                "routing_upstream_tool_schema_rejected",
            ),
            ("synthetic-private", "routing_upstream_rejected"),
        ] {
            for body in [
                json!({"error":{"message":message}}),
                json!({"error":message}),
                json!({"message":message}),
            ] {
                assert_eq!(rejection_code(body.to_string().as_bytes()), code);
            }
        }
        assert_eq!(
            rejection_code(b"synthetic-private"),
            "routing_upstream_rejected"
        );
    }
}
