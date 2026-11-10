//! Merge Antigravity's shared windows and independent model allowances for display.

use crate::services::subscription::QuotaTier;
use serde_json::Value;

struct Pool {
    name: String,
    about: String,
    tiers: Vec<QuotaTier>,
}

fn remaining(value: &Value) -> Option<f64> {
    match value.get("remainingFraction") {
        None | Some(Value::Null) => Some(0.0),
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| value.clamp(0.0, 1.0)),
    }
}

fn tier(name: String, value: &Value) -> Option<QuotaTier> {
    if !value.is_object() || value["disabled"] == true {
        return None;
    }
    let remaining = remaining(value)?;
    Some(QuotaTier {
        name,
        utilization: (1.0 - remaining) * 100.0,
        resets_at: value["resetTime"]
            .as_str()
            .filter(|text| chrono::DateTime::parse_from_rfc3339(text).is_ok())
            .map(str::to_owned),
        used_value_usd: None,
        max_value_usd: None,
    })
}

fn pool_name(name: &str) -> String {
    let name = name.trim();
    let name = if name.to_ascii_lowercase().ends_with(" models") {
        name[..name.len() - 7].trim()
    } else {
        name
    };
    name.replace(" and ", " & ")
}

fn window(bucket: &Value) -> Option<String> {
    for raw in [
        bucket["window"].as_str(),
        bucket["displayName"].as_str(),
        bucket["bucketId"].as_str(),
    ]
    .into_iter()
    .flatten()
    {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        let lower = value.to_ascii_lowercase();
        return Some(
            if matches!(lower.as_str(), "5h" | "five_hour") || lower.contains("five hour limit") {
                "five_hour".into()
            } else if matches!(lower.as_str(), "7d" | "weekly" | "seven_day")
                || lower.contains("weekly limit")
            {
                "seven_day".into()
            } else {
                value.into()
            },
        );
    }
    None
}

fn pools(summary: &Value) -> Vec<Pool> {
    summary["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|group| {
            if group["disabled"] == true {
                return None;
            }
            let name = pool_name(group["displayName"].as_str()?);
            if name.is_empty() {
                return None;
            }
            let mut tiers: Vec<_> = group["buckets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|bucket| {
                    let window = window(bucket)?;
                    tier(format!("{name} · {window}"), bucket)
                })
                .collect();
            tiers.sort_by_key(|tier| {
                if tier.name.ends_with(" · five_hour") {
                    0
                } else if tier.name.ends_with(" · seven_day") {
                    1
                } else {
                    2
                }
            });
            if tiers.is_empty() {
                return None;
            }
            Some(Pool {
                name,
                about: format!(
                    "{} {}",
                    group["displayName"].as_str().unwrap_or_default(),
                    group["description"].as_str().unwrap_or_default()
                )
                .to_ascii_lowercase(),
                tiers,
            })
        })
        .collect()
}

fn same_reading(a: &QuotaTier, b: &QuotaTier) -> bool {
    if (a.utilization - b.utilization).abs() > 0.5 {
        return false;
    }
    match (a.resets_at.as_deref(), b.resets_at.as_deref()) {
        (Some(a), Some(b)) => match (
            chrono::DateTime::parse_from_rfc3339(a),
            chrono::DateTime::parse_from_rfc3339(b),
        ) {
            (Ok(a), Ok(b)) => (a.timestamp() - b.timestamp()).abs() < 60,
            _ => false,
        },
        _ => true,
    }
}

fn append_unique(out: &mut Vec<QuotaTier>, tier: QuotaTier) {
    if !out
        .iter()
        .any(|old| old.name == tier.name && same_reading(old, &tier))
    {
        out.push(tier);
    }
}

fn versioned(name: &str) -> bool {
    name.chars().any(|ch| ch.is_ascii_digit())
}

fn names_model(pool: &str, id: &str, label: &str) -> bool {
    if !versioned(pool) {
        return false;
    }
    let pool = pool.to_ascii_lowercase();
    [
        label.to_ascii_lowercase(),
        id.replace('-', " ").to_ascii_lowercase(),
    ]
    .iter()
    .any(|model| {
        model == &pool
            || model
                .strip_prefix(pool.as_str())
                .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with('('))
    })
}

fn family(id: &str, label: &str) -> String {
    let id = id.to_ascii_lowercase();
    if id.starts_with("gemini") {
        "Gemini".into()
    } else if id.starts_with("claude") {
        "Claude".into()
    } else if id.starts_with("gpt-oss") {
        "GPT-OSS".into()
    } else {
        label.split_whitespace().next().unwrap_or(label).into()
    }
}

fn parse_at(models: &Value, summary: Option<&Value>, now: i64) -> Vec<QuotaTier> {
    let pools = summary.map(pools).unwrap_or_default();
    let mut out: Vec<_> = pools.iter().flat_map(|pool| pool.tiers.clone()).collect();
    let mut unpooled: Vec<(String, String, QuotaTier)> = vec![];
    for (id, model) in models["models"]
        .as_object()
        .into_iter()
        .flat_map(|models| models.iter())
    {
        if model["disabled"] == true || id.starts_with("chat_") || id.starts_with("tab_") {
            continue;
        }
        let label = model["displayName"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(id);
        let own_name = label.split(" (").next().unwrap_or(label).trim();
        let Some(mut reading) = tier(format!("{own_name} · five_hour"), &model["quotaInfo"])
        else {
            continue;
        };
        if let Some(pool) = pools.iter().find(|pool| names_model(&pool.name, id, label)) {
            // A model-specific week does not replace the model's five-hour reading.
            if !pool
                .tiers
                .iter()
                .any(|tier| tier.name.ends_with(" · five_hour"))
            {
                reading.name = format!("{} · five_hour", pool.name);
                append_unique(&mut out, reading);
            }
            continue;
        }
        let family = family(id, label);
        let family_lower = family.to_ascii_lowercase();
        let shared = pools
            .iter()
            .filter(|pool| {
                !versioned(&pool.name)
                    && (pool.about.contains(&family_lower)
                        || (family == "GPT-OSS" && pool.about.contains("gpt")))
            })
            .find_map(|pool| {
                pool.tiers
                    .iter()
                    .find(|tier| tier.name.ends_with(" · five_hour"))
            });
        if let Some(shared) = shared {
            if same_reading(&reading, shared) {
                continue;
            }
            // A distinct model reading must remain visible beside its family's pool.
            let weekly = reading
                .resets_at
                .as_deref()
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                .is_some_and(|reset| reset.timestamp().saturating_sub(now) > 5 * 3600 + 60);
            reading.name = format!(
                "{own_name} · {}",
                if weekly { "seven_day" } else { "five_hour" }
            );
            append_unique(&mut out, reading);
        } else {
            unpooled.push((family, own_name.into(), reading));
        }
    }
    for (family, label, reading) in &unpooled {
        let mut reading = reading.clone();
        let common = unpooled
            .iter()
            .filter(|(other, _, _)| other == family)
            .all(|(_, _, other)| same_reading(&reading, other));
        reading.name = format!("{} · five_hour", if common { family } else { label });
        append_unique(&mut out, reading);
    }
    out
}

pub(super) fn parse(models: &Value, summary: Option<&Value>) -> Vec<QuotaTier> {
    parse_at(models, summary, chrono::Utc::now().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/antigravity-quota-pools.json"
        ))
        .expect("synthetic quota fixture")
    }

    #[test]
    fn summary_does_not_hide_independent_or_exhausted_model_pools() {
        let fixture = fixture();
        let now = chrono::DateTime::parse_from_rfc3339(fixture["now"].as_str().expect("clock"))
            .expect("clock");
        let tiers = parse_at(
            &fixture["models"],
            Some(&fixture["summary"]),
            now.timestamp(),
        );
        let expected = fixture["expectedTiers"].as_array().expect("tiers");
        assert_eq!(tiers.len(), expected.len());
        for expected in expected {
            let tier = tiers
                .iter()
                .find(|tier| tier.name == expected["name"].as_str().expect("name"))
                .expect("quota pool retained");
            assert!(
                (tier.utilization - expected["utilization"].as_f64().expect("used")).abs()
                    < 0.00001
            );
            assert_eq!(tier.resets_at.as_deref(), expected["resetsAt"].as_str());
        }
    }

    #[test]
    fn unavailable_summary_retains_explicit_zeroes_but_not_unknown_or_disabled_models() {
        let models = json!({"models": {
            "claude-one":{"displayName":"Claude One", "quotaInfo":{}},
            "claude-two":{"displayName":"Claude Two", "quotaInfo":{}},
            "unknown":{"displayName":"Unknown"},
            "invalid":{"quotaInfo":{"remainingFraction":"invalid"}},
            "disabled":{"disabled":true,"quotaInfo":{}},
            "tab_internal":{"quotaInfo":{}}
        }});
        let original = models.clone();
        let tiers = parse_at(&models, None, 0);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, "Claude · five_hour");
        assert_eq!(tiers[0].utilization, 100.0);
        assert_eq!(models, original);
    }

    #[test]
    fn explicit_model_pool_precedes_family_matching_and_keeps_an_unreported_span() {
        let mut fixture = fixture();
        let argon = json!({"displayName":"Gemini 4 Argon", "buckets":[{"window":"weekly","remainingFraction":0.6}]});
        fixture["summary"]["groups"]
            .as_array_mut()
            .expect("groups")
            .insert(0, argon);
        let tiers = parse_at(&fixture["models"], Some(&fixture["summary"]), 0);
        assert!(tiers
            .iter()
            .any(|tier| tier.name == "Gemini 4 Argon · seven_day"
                && (tier.utilization - 40.0).abs() < 0.00001));
        assert!(tiers
            .iter()
            .any(|tier| tier.name == "Gemini 4 Argon · five_hour" && tier.utilization == 100.0));
        assert_eq!(
            tiers
                .iter()
                .filter(|tier| tier.name == "Gemini · five_hour")
                .count(),
            1
        );
    }

    #[test]
    fn models_with_different_readings_are_not_hidden_behind_the_family_window() {
        let mut fixture = fixture();
        fixture["models"]["models"]["gemini-3.8-flash"]["quotaInfo"]["remainingFraction"] =
            json!(0.4);
        let tiers = parse_at(&fixture["models"], Some(&fixture["summary"]), 1794268800);
        assert!(tiers
            .iter()
            .any(|tier| tier.name.starts_with("Gemini 3.8 Flash ·") && tier.utilization == 60.0));
        assert!(tiers
            .iter()
            .any(|tier| tier.name == "Gemini · five_hour" && tier.utilization == 0.0));
    }

    #[test]
    fn malformed_or_disabled_summary_windows_do_not_replace_valid_model_readings() {
        let models = json!({"models":{"gemini-one":{"displayName":"Gemini One","quotaInfo":{"remainingFraction":0.5}}}});
        let summary = json!({"groups":[{"displayName":"Gemini Models","buckets":[
            {"window":"5h","disabled":true}, {"window":"weekly","remainingFraction":"invalid"}
        ]}]});
        let tiers = parse_at(&models, Some(&summary), 0);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].utilization, 50.0);
    }
}
