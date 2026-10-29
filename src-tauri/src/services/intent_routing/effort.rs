//! Reasoning capabilities shared by Codex menu publication and request forwarding.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const LEVELS: &[&str] = &[
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Capability {
    pub levels: Vec<String>,
    pub quiet: bool,
    #[serde(default)]
    pub images: Option<bool>,
    #[serde(default)]
    pub free: Option<bool>,
}
impl Capability {
    pub fn parse(entry: &Value) -> Self {
        let listed = entry
            .get("reasoningLevels")
            .or_else(|| entry.get("reasoning_levels"))
            .or_else(|| entry.get("supported_reasoning_levels"))
            .and_then(Value::as_array);
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
            || levels == ["none"];
        let images = entry
            .get("inputModalities")
            .or_else(|| entry.get("input_modalities"))
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
            images,
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

#[cfg(test)]
mod tests {
    use super::*;
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
