//! Reasoning capabilities shared by agent catalogs and request forwarding.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const LEVELS: &[&str] = &[
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

pub(crate) fn valid_member_level(level: &str) -> bool {
    LEVELS.contains(&level) && level != "ultra"
}

/// Fixed seats affect the request, never the destination's declared capability.
pub(crate) fn adapt_member_wire(
    raw: &[u8],
    format: &str,
    capability: &Capability,
    fixed: Option<&str>,
) -> Result<Vec<u8>, &'static str> {
    let Some(fixed) = fixed.filter(|_| !capability.quiet && !capability.levels.is_empty()) else {
        return adapt_wire(raw, format, capability);
    };
    if !valid_member_level(fixed) {
        return Err("routing_invalid_reasoning");
    }
    let (path, uppercase) = effort_path(format)?;
    let fitted = capability.fit(fixed).ok_or("routing_invalid_reasoning")?;
    replace_effort(
        raw,
        path,
        &if uppercase {
            fitted.to_ascii_uppercase()
        } else {
            fitted
        },
    )
}

fn effort_path(format: &str) -> Result<(&'static [&'static str], bool), &'static str> {
    match format {
        "openai_responses" => Ok((&["reasoning", "effort"], false)),
        "openai_chat" => Ok((&["reasoning_effort"], false)),
        "anthropic" => Ok((&["output_config", "effort"], false)),
        "gemini_native" => Ok((
            &["generationConfig", "thinkingConfig", "thinkingLevel"],
            true,
        )),
        _ => Err("routing_invalid_reasoning"),
    }
}

fn replace_effort(raw: &[u8], path: &[&str], value: &str) -> Result<Vec<u8>, &'static str> {
    use serde_json::value::RawValue;
    use std::collections::BTreeMap;
    const INVALID: &str = "routing_invalid_reasoning";
    let mut fields: BTreeMap<String, Box<RawValue>> =
        serde_json::from_slice(raw).map_err(|_| INVALID)?;
    let Some((key, rest)) = path.split_first() else {
        return Err(INVALID);
    };
    let replacement = if rest.is_empty() {
        serde_json::value::to_raw_value(value).map_err(|_| INVALID)?
    } else {
        let child = fields
            .get(*key)
            .map(|v| v.get())
            .filter(|v| *v != "null")
            .unwrap_or("{}");
        let bytes = replace_effort(child.as_bytes(), rest, value)?;
        RawValue::from_string(String::from_utf8(bytes).map_err(|_| INVALID)?)
            .map_err(|_| INVALID)?
    };
    fields.insert((*key).into(), replacement);
    serde_json::to_vec(&fields).map_err(|_| INVALID)
}

pub(super) fn valid_levels(levels: &[String]) -> bool {
    let mut previous = None;
    levels.iter().all(|level| {
        let Some(rank) = LEVELS
            .iter()
            .position(|known| *known == level && *known != "ultra")
        else {
            return false;
        };
        if previous.is_some_and(|last| last >= rank) {
            return false;
        }
        previous = Some(rank);
        true
    })
}

pub(crate) fn offered<'a>(
    group: &super::RoutingGroup,
    members: impl Iterator<Item = &'a Capability>,
) -> Vec<String> {
    if group.levels.is_empty() {
        shared(members)
    } else {
        group.levels.clone()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Capability {
    pub levels: Vec<String>,
    pub quiet: bool,
    #[serde(default)]
    pub declared: bool,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub default_level: Option<String>,
    #[serde(default)]
    pub reasonless: bool,
    #[serde(default)]
    pub images: Option<bool>,
    #[serde(default)]
    pub free: Option<bool>,
    #[serde(default)]
    pub context: super::context::Metadata,
    #[serde(default)]
    pub typed_window: bool,
}
impl Capability {
    pub(crate) fn has_reasoning_declaration(entry: &Value) -> bool {
        [
            "reasoningLevels",
            "reasoning_levels",
            "supported_reasoning_levels",
            "reasoning_options",
        ]
        .iter()
        .any(|key| entry.get(*key).is_some_and(Value::is_array))
            || ["reasoning", "supports_reasoning", "supportsReasoning"]
                .iter()
                .any(|key| entry.get(*key).is_some_and(Value::is_boolean))
            || (super::model_preferences::valid(entry)
                && entry
                    .pointer("/modelPreferences/reasoningLevels")
                    .is_some_and(Value::is_array))
    }

    pub(crate) fn for_provider(provider: &crate::provider::Provider, model: &str) -> Self {
        let mut resolved = provider
            .settings_config
            .pointer("/modelCatalog/models")
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["model"].as_str().is_some_and(|id| id.trim() == model))
            })
            .cloned()
            .unwrap_or_else(|| json!({"model":model}));
        for field in ["routingModelCapabilities", "routingPublicCapabilities"] {
            if let Some(fallback) = provider
                .settings_config
                .get(field)
                .and_then(|map| map.get(model))
            {
                let declared = Self::has_reasoning_declaration(&resolved);
                let images_declared = Self::parse(&resolved).images.is_some();
                if let (Some(resolved), Some(fallback)) =
                    (resolved.as_object_mut(), fallback.as_object())
                {
                    for (key, value) in fallback {
                        if declared
                            && !matches!(key.as_str(), "inputModalities" | "defaultReasoningLevel")
                        {
                            continue;
                        }
                        if key == "inputModalities" && images_declared {
                            continue;
                        }
                        resolved.entry(key).or_insert_with(|| value.clone());
                    }
                }
            }
        }
        let mut capability = Self::parse(&resolved);
        capability.context =
            super::context::Metadata::for_provider(&provider.settings_config, model);
        capability.typed_window =
            super::context::Metadata::typed_window(&provider.settings_config, model);
        capability
    }

    pub(crate) fn declaration(&self) -> Value {
        let mut entry = json!({});
        if self.declared || !self.levels.is_empty() || self.quiet {
            entry["reasoningLevels"] = json!(self.levels);
        }
        if let Some(reasoning) = self.reasoning.or_else(|| self.quiet.then_some(false)) {
            entry["reasoning"] = json!(reasoning);
        }
        if let Some(level) = &self.default_level {
            entry["defaultReasoningLevel"] = json!(level);
        }
        if let Some(images) = self.images {
            entry["inputModalities"] = if images {
                json!(["text", "image"])
            } else {
                json!(["text"])
            };
        }
        entry
    }

    pub(crate) fn thinks(&self) -> bool {
        !self.quiet
            && (self.reasoning == Some(true) || self.levels.iter().any(|level| level != "none"))
    }

    pub fn parse(entry: &Value) -> Self {
        let reasonless = super::model_preferences::valid(entry)
            && entry
                .pointer("/modelPreferences/reasoningLevels")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty);
        let effective = super::model_preferences::effective(entry);
        let entry = effective.as_ref();
        let options = entry
            .get("reasoning_options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|option| option["type"].as_str() == Some("effort"))
            .filter_map(|option| option["values"].as_array())
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        let listed = entry
            .get("reasoningLevels")
            .or_else(|| entry.get("reasoning_levels"))
            .or_else(|| entry.get("supported_reasoning_levels"))
            .and_then(Value::as_array)
            .or_else(|| (!options.is_empty()).then_some(&options));
        let levels = LEVELS
            .iter()
            .filter(|level| {
                listed.is_some_and(|items| {
                    items.iter().any(|item| {
                        item.as_str()
                            .or_else(|| item.get("effort").and_then(Value::as_str))
                            .is_some_and(|s| s.trim().eq_ignore_ascii_case(level))
                    })
                })
            })
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>();
        // An absent/empty list is unknown, not evidence that reasoning is disabled.
        let quiet = entry.get("reasoning") == Some(&Value::Bool(false))
            || entry.get("supports_reasoning") == Some(&Value::Bool(false))
            || entry.get("supportsReasoning") == Some(&Value::Bool(false))
            || levels == ["none"];
        let images = entry
            .get("inputModalities")
            .or_else(|| entry.get("input_modalities"))
            .or_else(|| entry.pointer("/modalities/input"))
            .and_then(Value::as_array)
            .filter(|values| !values.is_empty())
            .map(|values| values.iter().any(|v| v.as_str() == Some("image")));
        let listed_free = entry
            .get("free")
            .or_else(|| entry.get("is_free"))
            .or_else(|| entry.get("isFree"))
            .and_then(Value::as_bool);
        // A complete, explicit price table can prove zero cost; absent prices cannot.
        let priced_free = entry
            .get("pricing")
            .or_else(|| entry.get("price"))
            .and_then(|price| {
                [
                    ("input", "input"),
                    ("output", "output"),
                    ("cacheRead", "cache_read"),
                    ("cacheWrite", "cache_write"),
                ]
                .iter()
                .map(|(a, b)| {
                    let value = price.get(a).or_else(|| price.get(b))?;
                    value
                        .as_f64()
                        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
                        .filter(|n| n.is_finite() && *n >= 0.0)
                })
                .collect::<Option<Vec<_>>>()
                .map(|values| values.iter().all(|n| *n == 0.0))
            });
        Self {
            levels,
            quiet,
            declared: Self::has_reasoning_declaration(entry),
            reasoning: entry
                .get("reasoning")
                .or_else(|| entry.get("supports_reasoning"))
                .or_else(|| entry.get("supportsReasoning"))
                .and_then(Value::as_bool),
            default_level: entry
                .get("defaultReasoningLevel")
                .or_else(|| entry.get("default_reasoning_level"))
                .and_then(Value::as_str)
                .filter(|level| valid_member_level(level))
                .map(str::to_owned),
            reasonless,
            images,
            context: super::context::Metadata::parse(entry),
            typed_window: false,
            free: if listed_free == Some(true) || priced_free == Some(true) {
                Some(true)
            } else {
                listed_free.or(priced_free)
            },
        }
    }
    pub fn fit(&self, want: &str) -> Option<String> {
        if self.quiet {
            return None;
        }
        // Ultra is a Codex picker mode, not an upstream API effort.
        let want = if want == "ultra" { "max" } else { want };
        let levels = self
            .levels
            .iter()
            .filter(|s| s.as_str() != "ultra")
            .cloned()
            .collect::<Vec<_>>();
        let fitted = nearest(want, &levels);
        Some(if levels.is_empty() && fitted == "xhigh" {
            "high".into()
        } else {
            fitted
        })
    }
}
pub(crate) fn shared<'a>(members: impl Iterator<Item = &'a Capability>) -> Vec<String> {
    let mut common: Option<Vec<String>> = None;
    for member in members.filter(|c| !c.quiet && !c.levels.is_empty()) {
        match &mut common {
            None => common = Some(member.levels.clone()),
            Some(levels) => levels.retain(|v| member.levels.contains(v)),
        }
    }
    common.unwrap_or_default()
}
fn nearest(want: &str, levels: &[String]) -> String {
    let Some(rank) = LEVELS.iter().position(|s| *s == want) else {
        return want.into();
    };
    if levels.iter().any(|s| s == want) {
        return want.into();
    }
    levels
        .iter()
        .filter(|s| s.as_str() != "none")
        .filter_map(|s| LEVELS.iter().position(|l| *l == s).map(|i| (s, i)))
        .min_by_key(|(_, i)| (i.abs_diff(rank), std::cmp::Reverse(*i)))
        .map_or_else(|| want.into(), |(s, _)| s.clone())
}
pub(crate) fn advertise(model: &mut Value, levels: &[String], chosen: &str) {
    model["supported_reasoning_levels"] = json!(levels
        .iter()
        .map(|level| json!({"effort":level,"description":level}))
        .collect::<Vec<_>>());
    let fitted = nearest(chosen, levels);
    let default = levels
        .iter()
        .find(|s| **s == fitted)
        .or_else(|| levels.iter().find(|s| s.as_str() == "medium"))
        .or_else(|| levels.iter().find(|s| s.as_str() == "high"))
        .or_else(|| levels.first());
    model["default_reasoning_level"] = json!(default.map(String::as_str).unwrap_or("none"));
    model["supports_reasoning_summaries"] = json!(levels.iter().any(|s| s != "none"));
}

/// Patch only the effort leaf; retain opaque JSON numbers and all unrelated fields.
pub(crate) fn adapt(raw: &[u8], capability: &Capability) -> Result<Vec<u8>, &'static str> {
    if capability.reasonless {
        return remove_wire_fields(raw, &[("reasoning", &[])]);
    }
    use serde_json::value::RawValue;
    use std::collections::BTreeMap;
    const INVALID: &str = "routing_invalid_reasoning";
    let mut root: BTreeMap<String, Box<RawValue>> =
        serde_json::from_slice(raw).map_err(|_| INVALID)?;
    let Some(reasoning) = root.get("reasoning") else {
        return Ok(raw.to_vec());
    };
    if reasoning.get() == "null" {
        return Ok(raw.to_vec());
    }
    let mut fields: BTreeMap<String, Box<RawValue>> =
        serde_json::from_str(reasoning.get()).map_err(|_| INVALID)?;
    let Some(effort) = fields.get("effort") else {
        return Ok(raw.to_vec());
    };
    if effort.get() == "null" {
        return Ok(raw.to_vec());
    }
    let want: String = serde_json::from_str(effort.get()).map_err(|_| INVALID)?;
    match capability.fit(&want) {
        Some(fitted) if fitted == want => return Ok(raw.to_vec()),
        Some(fitted) => {
            fields.insert(
                "effort".into(),
                serde_json::value::to_raw_value(&fitted).map_err(|_| INVALID)?,
            );
        }
        None => {
            fields.remove("effort");
        }
    }
    root.insert(
        "reasoning".into(),
        serde_json::value::to_raw_value(&fields).map_err(|_| INVALID)?,
    );
    serde_json::to_vec(&root).map_err(|_| INVALID)
}

/// Fit explicit wire-level effort fields without re-encoding unrelated values.
pub(crate) fn adapt_wire(
    raw: &[u8],
    format: &str,
    capability: &Capability,
) -> Result<Vec<u8>, &'static str> {
    if capability.reasonless {
        return match format {
            "openai_responses" => remove_wire_fields(raw, &[("reasoning", &[])]),
            "openai_chat" => remove_wire_fields(raw, &[("reasoning_effort", &[])]),
            "anthropic" => {
                remove_wire_fields(raw, &[("thinking", &[]), ("output_config", &["effort"])])
            }
            "gemini_native" => {
                remove_wire_fields(raw, &[("generationConfig", &["thinkingConfig"])])
            }
            _ => Err("routing_invalid_reasoning"),
        };
    }
    if format == "openai_responses" {
        return adapt(raw, capability);
    }
    let (path, uppercase) = effort_path(format)?;
    patch_effort(raw, path, uppercase, capability)
}

fn remove_wire_fields(raw: &[u8], paths: &[(&str, &[&str])]) -> Result<Vec<u8>, &'static str> {
    use serde_json::value::RawValue;
    use std::collections::BTreeMap;
    const INVALID: &str = "routing_invalid_reasoning";
    fn remove(raw: &[u8], key: &str, rest: &[&str]) -> Result<Vec<u8>, &'static str> {
        if raw == b"null" {
            return Ok(raw.to_vec());
        }
        let mut fields: BTreeMap<String, Box<RawValue>> =
            serde_json::from_slice(raw).map_err(|_| INVALID)?;
        let Some(value) = fields.get(key) else {
            return Ok(raw.to_vec());
        };
        if let Some((next, tail)) = rest.split_first() {
            let child = remove(value.get().as_bytes(), next, tail)?;
            if child == value.get().as_bytes() {
                return Ok(raw.to_vec());
            }
            fields.insert(
                key.into(),
                RawValue::from_string(String::from_utf8(child).map_err(|_| INVALID)?)
                    .map_err(|_| INVALID)?,
            );
        } else {
            fields.remove(key);
        }
        serde_json::to_vec(&fields).map_err(|_| INVALID)
    }
    let mut bytes = raw.to_vec();
    for (key, rest) in paths {
        bytes = remove(&bytes, key, rest)?;
    }
    Ok(bytes)
}

fn patch_effort(
    raw: &[u8],
    path: &[&str],
    uppercase: bool,
    capability: &Capability,
) -> Result<Vec<u8>, &'static str> {
    use serde_json::value::RawValue;
    use std::collections::BTreeMap;
    const INVALID: &str = "routing_invalid_reasoning";
    if raw == b"null" {
        return Ok(raw.to_vec());
    }
    let mut fields: BTreeMap<String, Box<RawValue>> =
        serde_json::from_slice(raw).map_err(|_| INVALID)?;
    let Some((key, rest)) = path.split_first() else {
        return Err(INVALID);
    };
    let Some(value) = fields.get(*key) else {
        return Ok(raw.to_vec());
    };
    if value.get() == "null" {
        return Ok(raw.to_vec());
    }
    if rest.is_empty() {
        let want: String = serde_json::from_str(value.get()).map_err(|_| INVALID)?;
        let fitted = capability.fit(&if uppercase {
            want.to_ascii_lowercase()
        } else {
            want.clone()
        });
        match fitted.map(|value| {
            if uppercase {
                value.to_ascii_uppercase()
            } else {
                value
            }
        }) {
            Some(fitted) if fitted == want => return Ok(raw.to_vec()),
            Some(fitted) => {
                fields.insert(
                    (*key).to_owned(),
                    serde_json::value::to_raw_value(&fitted).map_err(|_| INVALID)?,
                );
            }
            None => {
                fields.remove(*key);
            }
        }
    } else {
        let patched = patch_effort(value.get().as_bytes(), rest, uppercase, capability)?;
        if patched == value.get().as_bytes() {
            return Ok(raw.to_vec());
        }
        fields.insert(
            (*key).to_owned(),
            RawValue::from_string(String::from_utf8(patched).map_err(|_| INVALID)?)
                .map_err(|_| INVALID)?,
        );
    }
    serde_json::to_vec(&fields).map_err(|_| INVALID)
}

#[cfg(test)]
mod tests {
    #[test]
    fn vendor_declarations_take_precedence_over_public_fallbacks() {
        let provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            serde_json::json!({
                "modelCatalog":{"models":[{"model":"known"},{"model":"disabled","supportsReasoning":false},{"model":"limited","reasoningLevels":[]}]},
                "routingModelCapabilities":{"known":{"reasoningLevels":["low","high"]}},
                "routingPublicCapabilities":{"known":{"reasoningLevels":["medium","max"]},"disabled":{"reasoningLevels":["high"]},"limited":{"reasoningLevels":["high"]}}
            }),
            None,
        );
        assert_eq!(
            super::Capability::for_provider(&provider, "known").levels,
            ["low", "high"]
        );
        assert!(super::Capability::for_provider(&provider, "disabled").quiet);
        assert!(super::Capability::for_provider(&provider, "limited")
            .levels
            .is_empty());
    }
    #[test]
    fn explicit_no_reasoning_removes_protocol_thinking_without_touching_other_json() {
        let capability = super::Capability::parse(
            &serde_json::json!({"modelPreferences":{"reasoningLevels":[]}}),
        );
        for (format, raw, removed, kept) in [
            (
                "openai_responses",
                r#"{"reasoning":{"effort":"high","summary":"auto"},"input":[],"vendor":1.000000000000000001}"#,
                "reasoning",
                "vendor",
            ),
            (
                "openai_chat",
                r#"{"reasoning_effort":"high","messages":[],"vendor":1.000000000000000001}"#,
                "reasoning_effort",
                "vendor",
            ),
            (
                "anthropic",
                r#"{"thinking":{"type":"enabled","budget_tokens":1234},"output_config":{"effort":"high","vendor":1.000000000000000001},"messages":[]}"#,
                "thinking",
                "output_config",
            ),
            (
                "gemini_native",
                r#"{"generationConfig":{"thinkingConfig":{"thinkingLevel":"HIGH","includeThoughts":true},"vendor":1.000000000000000001},"contents":[]}"#,
                "generationConfig/thinkingConfig",
                "generationConfig",
            ),
        ] {
            let bytes = super::adapt_wire(raw.as_bytes(), format, &capability)
                .expect("synthetic adaptation");
            assert!(std::str::from_utf8(&bytes)
                .expect("utf8")
                .contains("1.000000000000000001"));
            let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
            assert!(value.pointer(&format!("/{removed}")).is_none());
            assert!(value.get(kept).is_some());
            if format == "anthropic" {
                assert!(value["output_config"].get("effort").is_none());
            }
        }
    }
    use super::*;
    #[test]
    fn fixed_member_effort_is_fitted_and_inserted_without_touching_opaque_fields() {
        let capability = Capability::parse(&json!({"reasoningLevels":["low", "high"]}));
        for (format, raw, path) in [
            (
                "openai_responses",
                r#"{"reasoning":{"effort":"low","summary":"auto"},"vendor":123456789012345678901234567890}"#,
                "/reasoning/effort",
            ),
            (
                "openai_chat",
                r#"{"messages":[],"vendor":123456789012345678901234567890}"#,
                "/reasoning_effort",
            ),
            (
                "anthropic",
                r#"{"output_config":{"format":{"type":"json_schema"}},"vendor":123456789012345678901234567890}"#,
                "/output_config/effort",
            ),
            (
                "gemini_native",
                r#"{"generationConfig":{"thinkingConfig":{"includeThoughts":true}},"vendor":123456789012345678901234567890}"#,
                "/generationConfig/thinkingConfig/thinkingLevel",
            ),
        ] {
            let bytes = adapt_member_wire(raw.as_bytes(), format, &capability, Some("max"))
                .expect("fixed effort");
            let text = String::from_utf8(bytes).expect("json");
            assert!(text.contains("123456789012345678901234567890"));
            let value: Value = serde_json::from_str(&text).expect("value");
            assert_eq!(
                value.pointer(path).and_then(Value::as_str),
                Some(if format == "gemini_native" {
                    "HIGH"
                } else {
                    "high"
                })
            );
            if format == "openai_responses" {
                assert_eq!(value["reasoning"]["summary"], "auto");
            }
            if format == "gemini_native" {
                assert_eq!(
                    value["generationConfig"]["thinkingConfig"]["includeThoughts"],
                    true
                );
            }
            if format == "anthropic" {
                assert_eq!(value["output_config"]["format"]["type"], "json_schema");
            }
        }
        let raw = br#"{"input":"synthetic"}"#;
        assert_eq!(
            adapt_member_wire(raw, "openai_responses", &Capability::default(), Some("max"))
                .expect("unknown capability"),
            raw
        );
    }
    #[test]
    fn named_group_levels_override_only_the_menu_not_the_models_capability() {
        let model = Capability::parse(&json!({"reasoningLevels":["low", "high"]}));
        let mut group = super::super::tests::fixture();
        assert_eq!(offered(&group, [&model].into_iter()), ["low", "high"]);
        group.levels = vec!["medium".into(), "max".into()];
        assert_eq!(offered(&group, [&model].into_iter()), ["medium", "max"]);
        assert_eq!(model.fit("medium").as_deref(), Some("high"));
        assert_eq!(model.fit("max").as_deref(), Some("high"));
    }

    #[test]
    fn wire_effort_fits_explicit_fields_without_editing_opaque_siblings() {
        let capability = Capability::parse(&json!({"reasoningLevels":["low", "high"]}));
        for (protocol, raw, path) in [
            (
                "openai_chat",
                r#"{"reasoning_effort":"medium","vendor":123456789012345678901234567890,"messages":[]}"#,
                "/reasoning_effort",
            ),
            (
                "anthropic",
                r#"{"output_config":{"effort":"medium","vendor":123456789012345678901234567890},"messages":[]}"#,
                "/output_config/effort",
            ),
            (
                "gemini_native",
                r#"{"generationConfig":{"thinkingConfig":{"thinkingLevel":"MEDIUM","vendor":123456789012345678901234567890}},"contents":[]}"#,
                "/generationConfig/thinkingConfig/thinkingLevel",
            ),
            (
                "openai_responses",
                r#"{"reasoning":{"effort":"medium","vendor":123456789012345678901234567890},"input":[]}"#,
                "/reasoning/effort",
            ),
        ] {
            let bytes = adapt_wire(raw.as_bytes(), protocol, &capability).expect("fit effort");
            assert!(std::str::from_utf8(&bytes)
                .expect("json")
                .contains("123456789012345678901234567890"));
            let value: Value = serde_json::from_slice(&bytes).expect("json");
            let expected = if protocol == "gemini_native" {
                "HIGH"
            } else {
                "high"
            };
            assert_eq!(value.pointer(path).and_then(Value::as_str), Some(expected));
            let quiet = Capability {
                quiet: true,
                ..Default::default()
            };
            let bytes = adapt_wire(raw.as_bytes(), protocol, &quiet).expect("quiet");
            let value: Value = serde_json::from_slice(&bytes).expect("json");
            assert!(value.pointer(path).is_none());
        }
    }
    #[test]
    fn cost_metadata_keeps_unknown_distinct_from_explicit_free_or_paid() {
        assert_eq!(Capability::parse(&json!({"model":"free-model"})).free, None);
        assert_eq!(Capability::parse(&json!({"free":true})).free, Some(true));
        assert_eq!(
            Capability::parse(&json!({"is_free":false})).free,
            Some(false)
        );
        assert_eq!(Capability::parse(&json!({"isFree":true})).free, Some(true));
        assert_eq!(Capability::parse(&json!({"free":"true"})).free, None);
        let mut entry = json!({"pricing":{"input":"0","output":0,"cacheRead":0,"cacheWrite":"0"}});
        assert_eq!(Capability::parse(&entry).free, Some(true));
        entry["pricing"]["output"] = json!(1);
        assert_eq!(Capability::parse(&entry).free, Some(false));
        entry["pricing"]["output"] = json!(-1);
        assert_eq!(Capability::parse(&entry).free, None);
        assert_eq!(
            Capability::parse(&json!({"pricing":{"input":0,"output":0}})).free,
            None
        );
    }
    #[test]
    fn image_capability_distinguishes_missing_from_explicit_text_only() {
        assert_eq!(Capability::parse(&json!({})).images, None);
        assert_eq!(
            Capability::parse(&json!({"input_modalities":["text","image"]})).images,
            Some(true)
        );
        assert_eq!(
            Capability::parse(&json!({"inputModalities":["text"]})).images,
            Some(false)
        );
        let old: Capability =
            serde_json::from_value(json!({"levels":["high"],"quiet":false})).unwrap();
        assert_eq!(old.images, None);
    }
    #[test]
    fn capabilities_intersect_without_guessing_unknown_members() {
        let a = Capability::parse(
            &json!({"supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"}]}),
        );
        let b = Capability::parse(&json!({"reasoningLevels":["high","LOW","max"]}));
        let unknown = Capability::default();
        let quiet = Capability::parse(&json!({"reasoning":false}));
        assert_eq!(
            shared([&a, &b, &unknown, &quiet].into_iter()),
            ["low", "high"]
        );
        assert!(shared([&unknown, &quiet].into_iter()).is_empty());
        assert_eq!(b.fit("medium").as_deref(), Some("high"));
        assert_eq!(b.fit("ultra").as_deref(), Some("max"));
        assert_eq!(unknown.fit("xhigh").as_deref(), Some("high"));
        assert_eq!(quiet.fit("high"), None);
        let high_only = Capability::parse(&json!({"reasoningLevels":["high"]}));
        let low_only = Capability::parse(&json!({"reasoningLevels":["low"]}));
        assert!(shared([&high_only, &low_only].into_iter()).is_empty());
        let switch = Capability::parse(&json!({"reasoningLevels":["none","high"]}));
        assert_eq!(switch.fit("minimal").as_deref(), Some("high"));
        assert_eq!(switch.fit("none").as_deref(), Some("none"));
        let mut model = json!({});
        advertise(&mut model, &a.levels, "xhigh");
        assert_eq!(model["default_reasoning_level"], "xhigh");
        advertise(&mut model, &a.levels, "");
        assert_eq!(model["default_reasoning_level"], "medium");
    }
    #[test]
    fn patch_preserves_opaque_values_and_other_reasoning_options() {
        let raw = br#"{"reasoning":{"effort":"medium","summary":"auto","budget":123456789012345678901234567890},"vendor":1.0000000000000000001}"#;
        let capability = Capability::parse(&json!({"reasoningLevels":["low","high"]}));
        let patched = String::from_utf8(adapt(raw, &capability).unwrap()).unwrap();
        assert!(patched.contains("123456789012345678901234567890"));
        assert!(patched.contains("1.0000000000000000001"));
        assert!(patched.contains(r#""effort":"high""#));
        assert!(patched.contains(r#""summary":"auto""#));
        let quiet = Capability::parse(&json!({"supports_reasoning":false}));
        let removed = String::from_utf8(adapt(raw, &quiet).unwrap()).unwrap();
        assert!(!removed.contains("effort"));
        assert!(removed.contains("summary"));
        assert_eq!(
            adapt(br#"{"input":"test"}"#, &capability).unwrap(),
            br#"{"input":"test"}"#
        );
        assert_eq!(
            adapt(br#"{"reasoning":{"effort":null}}"#, &capability).unwrap(),
            br#"{"reasoning":{"effort":null}}"#
        );
    }
}
