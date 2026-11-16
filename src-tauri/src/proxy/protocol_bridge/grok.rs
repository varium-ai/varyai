//! Grok tool-schema compatibility: local references, regex patterns and required lists.
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
                    let mut expanded = schema.clone();
                    if expand_local_refs(&mut expanded, schema, 0, &mut 16_384) {
                        *schema = expanded;
                    }
                    normalize_schema(schema);
                }
            }
        }
    }
}

fn expand_local_refs(schema: &mut Value, root: &Value, depth: usize, budget: &mut usize) -> bool {
    // Keep the original schema if expansion is recursive, scoped or too large.
    if depth >= 64 || *budget == 0 {
        return false;
    }
    *budget -= 1;
    if [
        "$id",
        "$anchor",
        "$dynamicAnchor",
        "$dynamicRef",
        "$recursiveRef",
    ]
    .iter()
    .any(|key| schema.get(key).is_some())
    {
        return false;
    }
    if !visit_schema_children(schema, &mut |child| {
        expand_local_refs(child, root, depth + 1, budget)
    }) {
        return false;
    }
    let Some(reference) = schema.get("$ref") else {
        return true;
    };
    let Some(pointer) = reference.as_str().and_then(|value| value.strip_prefix('#')) else {
        return false;
    };
    let Some(target) = pointer
        .starts_with('/')
        .then(|| root.pointer(pointer))
        .flatten()
    else {
        return false;
    };
    if !target.is_object() && !target.is_boolean() {
        return false;
    }
    let mut target = target.clone();
    if !expand_local_refs(&mut target, root, depth + 1, budget) {
        return false;
    }
    let Some(object) = schema.as_object_mut() else {
        return false;
    };
    object.remove("$ref");
    if object.is_empty() {
        *schema = target;
    } else {
        // A reference and its siblings are intersected, never overwritten.
        let Some(all_of) = object
            .entry("allOf")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
        else {
            return false;
        };
        all_of.push(target);
    }
    true
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
    visit_schema_children(schema, &mut |child| {
        normalize_schema(child);
        true
    });
}

fn visit_schema_children(schema: &mut Value, visit: &mut impl FnMut(&mut Value) -> bool) -> bool {
    let Some(object) = schema.as_object_mut() else {
        return true;
    };
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
                if !visit(child) {
                    return false;
                }
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
                    if !visit(child) {
                        return false;
                    }
                }
            } else if !visit(child) {
                return false;
            }
        }
    }
    true
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
    fn grok_object_refs_expand_in_nullable_properties_and_array_items() {
        let record = json!({"type":"object","properties":{
            "description":{"type":"string"},"title":{"type":"string"}
        },"required":["description","title"],"additionalProperties":false});
        let schema = json!({"type":"object","properties":{
            "record":{"anyOf":[{"$ref":"#/$defs/Record"},{"type":"null"}]},
            "records":{"type":"array","items":{"$ref":"#/$defs/Record"}}
        },"$defs":{"Record":record},"additionalProperties":false,
        "examples":[{"description":{"$ref":"#/$defs/Record"}}]});
        for path in ["parameters", "input_schema"] {
            let mut body = json!({"model":"grok-4.5","tools":[{path:schema}]});
            normalize_tool_schemas(&mut body);
            let actual = &body["tools"][0][path];
            assert_eq!(actual["properties"]["record"]["anyOf"][0], record);
            assert_eq!(actual["properties"]["records"]["items"], record);
            assert_eq!(actual["examples"], schema["examples"]);
            assert_eq!(actual["additionalProperties"], false);
        }
    }

    #[test]
    fn grok_recursive_unresolved_external_or_scoped_refs_are_not_discarded() {
        for schema in [
            json!({"type":"object","properties":{"item":{"$ref":"#/$defs/loop"}},"$defs":{"loop":{"$ref":"#/$defs/loop"}}}),
            json!({"type":"object","properties":{"item":{"$ref":"#/$defs/missing"}}}),
            json!({"type":"object","properties":{"item":{"$ref":"https://example.invalid/synthetic-schema"}}}),
            json!({"type":"object","properties":{"item":{"$ref":"#/$defs/item"}},"$defs":{"item":{"$id":"synthetic-scope","type":"string"}}}),
            json!({"$id":"https://example.invalid/synthetic-scope","type":"object","properties":{"item":{"$ref":"#/$defs/item"}},"$defs":{"item":{"type":"string"}}}),
        ] {
            let mut body = json!({"model":"grok-4.5","tools":[{"parameters":schema}]});
            let original = body.clone();
            normalize_tool_schemas(&mut body);
            assert_eq!(body, original);
        }
    }

    #[test]
    fn grok_ref_expansion_is_bounded_and_preserves_the_original_on_overflow() {
        let mut schema = json!({"$ref":"#/$defs/item0","$defs":{}});
        schema["$defs"]["item0"] = json!({"type":"string"});
        for index in 1..16 {
            let previous = format!("#/$defs/item{}", index - 1);
            schema["$defs"][format!("item{index}")] =
                json!({"anyOf":[{"$ref":previous},{"$ref":previous}]});
        }
        schema["$ref"] = json!("#/$defs/item15");
        let mut body = json!({"model":"grok-4.5","tools":[{"parameters":schema}]});
        let original = body.clone();
        normalize_tool_schemas(&mut body);
        assert_eq!(body, original);
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

/// Emit only fixed labels from recognized error metadata, never upstream text.
pub(in crate::proxy) fn rejection_hints(body: &[u8]) -> Vec<&'static str> {
    if body.len() > 64 * 1024 {
        return vec!["oversized_error"];
    }
    let parsed = serde_json::from_slice::<Value>(body);
    let mut hints = Vec::new();
    let metadata = match &parsed {
        Ok(value) => {
            let error = value.get("error").unwrap_or(value);
            let mut fields = Vec::new();
            if let Some(message) = error.as_str() {
                fields.push(message.to_ascii_lowercase());
            }
            for key in ["message", "param", "code", "type"] {
                if let Some(value) = error.get(key).and_then(Value::as_str) {
                    fields.push(value.to_ascii_lowercase());
                }
            }
            if value.get("error").is_some() {
                if let Some(message) = value.get("message").and_then(Value::as_str) {
                    fields.push(message.to_ascii_lowercase());
                }
            }
            fields
        }
        Err(_) => {
            hints.push("non_json");
            vec![String::from_utf8_lossy(body).to_ascii_lowercase()]
        }
    };
    for label in ["invalid", "unsupported"] {
        if parsed.is_ok() && metadata.iter().any(|field| field.contains(label)) {
            hints.push(label);
        }
    }
    if parsed.is_ok() {
        for label in [
            "messages",
            "tools",
            "strict",
            "parameters",
            "required",
            "pattern",
            "tool_call_id",
            "tool_calls",
            "reasoning_effort",
            "reasoning_summary",
            "service_tier",
            "max_tokens",
            "max_completion_tokens",
            "max_output_tokens",
            "prompt_cache_key",
            "parallel_tool_calls",
            "stream_options",
            "response_format",
            "web_search_options",
        ] {
            if metadata.iter().any(|field| {
                field
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .any(|token| token == label)
            }) {
                hints.push(label);
            }
        }
    }
    if metadata.iter().any(|field| {
        [
            "header too large",
            "headers too large",
            "header fields too large",
            "header size",
        ]
        .iter()
        .any(|phrase| field.contains(phrase))
    }) {
        hints.push("header_size");
    }
    hints
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
    fn rejection_hints_only_emit_fixed_labels() {
        let body = json!({"error":{"message":"Invalid max_completion_tokens; synthetic-private-secret https://synthetic.invalid/private", "param":"max_completion_tokens"}});
        assert_eq!(
            rejection_hints(body.to_string().as_bytes()),
            vec!["invalid", "max_completion_tokens"]
        );
        assert_eq!(
            rejection_hints(b"<html>400 Request Header Too Large; synthetic-private</html>"),
            vec!["non_json", "header_size"]
        );
        assert_eq!(
            rejection_hints(br#"{"error":{"message":"synthetic-private-only"}}"#),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn rejection_hints_inspect_only_error_metadata_and_bound_the_output() {
        for body in [
            json!({"error":{"message":"Unsupported field", "param":"tools[0].function.strict"}}),
            json!({"message":"Unsupported field tools[0].function.strict"}),
            json!({"error":"Unsupported field tools[0].function.strict"}),
        ] {
            assert_eq!(
                rejection_hints(body.to_string().as_bytes()),
                vec!["unsupported", "tools", "strict"]
            );
        }
        let body = json!({
            "error":{"message":"Synthetic refusal", "param":"synthetic-private-parameter"},
            "request":{"service_tier":"invalid", "tools":[{"input":"unsupported strict"}]},
            "api_key":"synthetic-private-secret"
        });
        assert!(rejection_hints(body.to_string().as_bytes()).is_empty());
        assert_eq!(
            rejection_hints(&vec![b'x'; 65_537]),
            vec!["oversized_error"]
        );
    }

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
