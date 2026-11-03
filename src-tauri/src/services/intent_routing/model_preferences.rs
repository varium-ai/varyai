//! Provider editor overrides stay separate from the vendor's model facts.
use serde_json::Value;
use std::borrow::Cow;

pub(crate) fn valid(entry: &Value) -> bool {
    let Some(prefs) = entry.get("modelPreferences") else {
        return true;
    };
    let Some(prefs) = prefs.as_object() else {
        return false;
    };
    prefs.iter().all(|(key, value)| match key.as_str() {
        "name" => value.as_str().is_some_and(|name| {
            super::valid_text(name, 80)
                && name == name.trim()
                && !name.chars().any(char::is_control)
        }),
        "images" => value.is_boolean(),
        "sameAs" => value
            .as_str()
            .is_some_and(|model| super::valid_target("equivalent", model)),
        "reasoningLevels" => value.as_array().is_some_and(|levels| {
            let Some(levels) = levels
                .iter()
                .map(|level| level.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
            else {
                return false;
            };
            super::effort::valid_levels(&levels)
        }),
        _ => false,
    })
}

pub(crate) fn same_as(entry: &Value) -> Option<&str> {
    valid(entry)
        .then(|| {
            entry
                .pointer("/modelPreferences/sameAs")
                .and_then(Value::as_str)
        })
        .flatten()
}

pub(crate) fn name(entry: &Value) -> Option<&str> {
    let prefs = valid(entry)
        .then(|| entry.get("modelPreferences"))
        .flatten();
    prefs
        .and_then(|prefs| prefs.get("name"))
        .or_else(|| entry.get("displayName"))
        .or_else(|| entry.get("display_name"))
        .and_then(Value::as_str)
        .filter(|name| super::valid_text(name, 80) && !name.chars().any(char::is_control))
}

pub(crate) fn effective(entry: &Value) -> Cow<'_, Value> {
    let Some(prefs) = entry.get("modelPreferences").filter(|_| valid(entry)) else {
        return Cow::Borrowed(entry);
    };
    let mut effective = entry.clone();
    if let Some(levels) = prefs.get("reasoningLevels").and_then(Value::as_array) {
        effective["reasoningLevels"] = Value::Array(levels.clone());
        effective["reasoning"] = Value::Bool(!levels.is_empty());
        effective["supports_reasoning"] = Value::Bool(!levels.is_empty());
    }
    if let Some(images) = prefs.get("images").and_then(Value::as_bool) {
        effective["inputModalities"] = if images {
            serde_json::json!(["text", "image"])
        } else {
            serde_json::json!(["text"])
        };
    }
    Cow::Owned(effective)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overrides_preserve_vendor_facts_and_apply_to_wire_capabilities() {
        let entry = json!({"model":"exact-model","displayName":"Vendor name","reasoningLevels":["low","high"],"inputModalities":["text","image"],"modelPreferences":{"name":"My model","reasoningLevels":[],"images":false}});
        let capability = super::super::effort::Capability::parse(&entry);
        assert!(capability.quiet);
        assert_eq!(capability.images, Some(false));
        assert_eq!(name(&entry), Some("My model"));
        assert_eq!(entry["reasoningLevels"], json!(["low", "high"]));
        assert_eq!(entry["displayName"], "Vendor name");
        let mut restored = entry.clone();
        restored
            .as_object_mut()
            .expect("fixture")
            .remove("modelPreferences");
        let capability = super::super::effort::Capability::parse(&restored);
        assert!(!capability.quiet);
        assert_eq!(capability.levels, ["low", "high"]);
        assert_eq!(capability.images, Some(true));
    }

    #[test]
    fn preferences_reject_unknown_fields_bad_types_and_unsupported_levels() {
        for prefs in [
            json!({"name":" "}),
            json!({"name":"bad\nname"}),
            json!({"images":"true"}),
            json!({"reasoningLevels":["ultra"]}),
            json!({"reasoningLevels":["high","low"]}),
            json!({"endpoint":"synthetic"}),
        ] {
            assert!(!valid(&json!({"modelPreferences":prefs})));
        }
        assert!(valid(
            &json!({"modelPreferences":{"reasoningLevels":[],"images":false}})
        ));
    }
}
