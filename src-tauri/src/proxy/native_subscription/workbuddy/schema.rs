//! WorkBuddy tool parameter conversions for the outbound schema only.
use indexmap::IndexMap;
use serde_json::{json, Map, Value};

pub(super) fn normalize_schema(schema: &mut Value) {
    if !schema.is_object() {
        *schema = json!({"type":"object","properties":{}});
        return;
    }
    let original = schema.clone();
    if let Some(fields) = schema.as_object_mut() {
        flatten_root(fields, &original);
    }
    normalize_node(schema, &original, 0);
    let Some(fields) = schema.as_object_mut() else {
        return;
    };
    if !fields.contains_key("type") {
        fields.insert("type".into(), json!("object"));
        fields.entry("properties").or_insert_with(|| json!({}));
    }
}

fn normalize_node(node: &mut Value, root: &Value, depth: usize) {
    if depth > 64 {
        return;
    }
    if let Some(allowed) = node.as_bool() {
        *node = if allowed {
            json!({})
        } else {
            json!({"not":{}})
        };
        return;
    }
    let Some(fields) = node.as_object_mut() else {
        return;
    };
    normalize_pattern(fields);
    if let Some(Value::Array(tuple)) = fields.get_mut("items") {
        let mut schemas = std::mem::take(tuple);
        let items = match schemas.len() {
            0 => json!({}),
            1 => schemas.remove(0),
            _ => json!({"anyOf":schemas}),
        };
        fields.insert("items".into(), items);
        fields.remove("additionalItems");
    }
    normalize_type(fields);
    match fields.get_mut("required") {
        Some(Value::Array(keys)) => keys.retain(Value::is_string),
        Some(_) => {
            fields.remove("required");
        }
        None => {}
    }
    fit_enum(fields);
    if !fields.contains_key("type")
        && fields
            .get("oneOf")
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                !branches.is_empty()
                    && branches
                        .iter()
                        .all(|b| resolve(root, b).is_some_and(|s| s["type"] == "object"))
            })
    {
        fields.insert("type".into(), json!("object"));
    }
    for (key, child) in fields {
        match key.as_str() {
            "properties" | "patternProperties" | "definitions" | "$defs" | "dependentSchemas" => {
                if let Some(map) = child.as_object_mut() {
                    for v in map.values_mut() {
                        normalize_node(v, root, depth + 1);
                    }
                }
            }
            "anyOf" | "oneOf" | "allOf" | "prefixItems" => {
                if let Some(list) = child.as_array_mut() {
                    for v in list {
                        normalize_node(v, root, depth + 1);
                    }
                }
            }
            "additionalProperties" | "unevaluatedProperties" if child.is_boolean() => {}
            "items"
            | "contains"
            | "not"
            | "if"
            | "then"
            | "else"
            | "additionalItems"
            | "unevaluatedItems"
            | "propertyNames"
            | "additionalProperties"
            | "unevaluatedProperties" => normalize_node(child, root, depth + 1),
            _ => {}
        }
    }
}

fn normalize_pattern(fields: &mut Map<String, Value>) {
    if let Some(pattern) = fields.get("pattern").and_then(Value::as_str) {
        if pattern == r"^[^\uD800-\uDFFF]*$" {
            // Rust strings already contain only Unicode scalar values.
            fields.remove("pattern");
        } else if pattern.contains(r"\0") || pattern.contains('\0') {
            let mut normalized = String::new();
            let mut chars = pattern.chars().peekable();
            while let Some(ch) = chars.next() {
                match ch {
                    '\0' => normalized.push_str(r"\u0000"),
                    '\\' => match chars.next() {
                        Some('0') if !chars.peek().is_some_and(char::is_ascii_digit) => {
                            normalized.push_str(r"\u0000")
                        }
                        next => {
                            normalized.push('\\');
                            normalized.extend(next);
                        }
                    },
                    _ => normalized.push(ch),
                }
            }
            fields.insert("pattern".into(), json!(normalized));
        }
    }
}

fn normalize_type(fields: &mut Map<String, Value>) {
    if let Some(kind) = fields.get_mut("type") {
        let mut types = Vec::new();
        for v in kind
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(std::slice::from_ref(kind))
        {
            if let Some(t) = v.as_str().map(str::to_ascii_lowercase) {
                if matches!(
                    t.as_str(),
                    "integer" | "number" | "string" | "boolean" | "array" | "object" | "null"
                ) && !types.contains(&t)
                {
                    types.push(t);
                }
            }
        }
        match types.len() {
            0 => {
                fields.remove("type");
            }
            1 => *kind = json!(types[0]),
            _ => *kind = json!(types),
        }
    }
}

fn fit_enum(fields: &mut Map<String, Value>) {
    let Some(variants) = fields.get("enum") else {
        return;
    };
    let Some(values) = variants.as_array().filter(|v| !v.is_empty()) else {
        fields.remove("enum");
        return;
    };
    let kind = fields.get("type");
    if kind.is_some_and(Value::is_string)
        || kind
            .and_then(Value::as_array)
            .is_some_and(|v| v.iter().filter(|t| *t != "null").count() == 1)
    {
        return;
    }
    if fields.contains_key("anyOf") && fields.get("allOf").is_some_and(|v| !v.is_array()) {
        return;
    }
    let floating = values
        .iter()
        .any(|v| v.as_f64().is_some_and(|n| n.fract() != 0.0));
    let mut groups: IndexMap<&str, Vec<&Value>> = IndexMap::new();
    for v in values {
        let ty = match v {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::String(_) => "string",
            Value::Number(_) if floating => "number",
            Value::Number(_) => "integer",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        if kind.and_then(Value::as_array).is_some_and(|types| {
            !types
                .iter()
                .any(|t| t == ty || (ty == "integer" && t == "number"))
        }) {
            return;
        }
        groups.entry(ty).or_default().push(v);
    }
    let nullable = groups.shift_remove("null").is_some();
    if groups.len() <= 1 {
        let ty = match groups.keys().next() {
            Some(t) if nullable => json!([t, "null"]),
            Some(t) => json!(t),
            None => json!("null"),
        };
        fields.insert("type".into(), ty);
    } else {
        let mut branches: Vec<_> = groups
            .into_iter()
            .map(|(t, vs)| json!({"type":t,"enum":vs}))
            .collect();
        if nullable {
            branches.push(json!({"type":"null"}));
        }
        fields.remove("enum");
        fields.remove("type");
        if fields.contains_key("anyOf") {
            if let Some(all) = fields
                .entry("allOf")
                .or_insert_with(|| json!([]))
                .as_array_mut()
            {
                all.push(json!({"anyOf":branches}));
            }
        } else {
            fields.insert("anyOf".into(), json!(branches));
        }
    }
}

fn resolve<'a>(root: &'a Value, mut node: &'a Value) -> Option<&'a Value> {
    for _ in 0..=16 {
        let Some(link) = node.get("$ref") else {
            return Some(node);
        };
        let path = link.as_str()?.strip_prefix('#')?;
        if !path.starts_with('/') || node.as_object()?.len() != 1 {
            return None;
        }
        node = root.pointer(path)?;
    }
    None
}

fn flatten_root(fields: &mut Map<String, Value>, root: &Value) {
    if fields.get("type").is_some_and(|kind| kind != "object") {
        return;
    }
    for key in ["allOf", "anyOf", "oneOf"] {
        let Some(branches) = root.get(key).and_then(Value::as_array) else {
            continue;
        };
        let branches: Option<Vec<_>> = branches.iter().map(|b| resolve(root, b)).collect();
        let Some(branches) = branches.filter(|v| {
            !v.is_empty()
                && v.iter().all(|b| {
                    (b["type"] == "object" || b.get("properties").is_some_and(Value::is_object))
                        && b.as_object().is_some_and(|m| {
                            m.keys().all(|k| {
                                matches!(
                                    k.as_str(),
                                    "type" | "properties" | "required" | "title" | "description"
                                )
                            })
                        })
                })
        }) else {
            continue;
        };
        let mut alternatives: IndexMap<&str, Vec<&Value>> = IndexMap::new();
        let mut required = Vec::new();
        for branch in &branches {
            if let Some(props) = branch["properties"].as_object() {
                for (name, shape) in props {
                    let choices = alternatives.entry(name).or_default();
                    if !choices.contains(&shape) {
                        choices.push(shape);
                    }
                }
            }
            if let Some(keys) = branch["required"].as_array() {
                for name in keys {
                    if !required.contains(&name)
                        && (key == "allOf"
                            || branches.iter().all(|b| {
                                b["required"].as_array().is_some_and(|r| r.contains(name))
                            }))
                    {
                        required.push(name);
                    }
                }
            }
        }
        let Some(dest) = fields
            .entry("properties")
            .or_insert_with(|| json!({}))
            .as_object_mut()
        else {
            continue;
        };
        for (name, choices) in alternatives {
            dest.entry(name)
                .or_insert_with(|| match choices.as_slice() {
                    [shape] => (*shape).clone(),
                    _ if key == "allOf" => json!({"allOf":choices}),
                    _ => json!({"anyOf":choices}),
                });
        }
        if !required.is_empty() {
            if let Some(dest) = fields
                .entry("required")
                .or_insert_with(|| json!([]))
                .as_array_mut()
            {
                for name in required {
                    if !dest.contains(name) {
                        dest.push(name.clone());
                    }
                }
            }
        }
        fields.remove(key);
        fields.insert("type".into(), json!("object"));
    }
}
