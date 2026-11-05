//! Display-only subscription metadata shared by native, managed and imported accounts.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

fn display_text(value: &str) -> Option<&str> {
    if value.chars().any(char::is_control) {
        return None;
    }
    let value = value.trim();
    if value.is_empty()
        || value.len() > 96
        || !value
            .chars()
            .all(|c| c.is_alphanumeric() || " _-+().".contains(c))
    {
        return None;
    }
    Some(value)
}

pub(crate) fn vendor_plan(vendor: &str, plan: &str) -> Option<String> {
    let plan = display_text(plan)?;
    let prefix = format!("{vendor} ");
    let plan = if plan
        .to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
    {
        &plan[prefix.len()..]
    } else {
        plan
    };
    if !plan.is_ascii() || !plan.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let mut plan = plan.to_owned();
    if matches!(
        plan.to_ascii_lowercase().as_str(),
        "unknown" | "none" | "null" | "undefined"
    ) {
        return None;
    }
    plan.get_mut(..1)?.make_ascii_uppercase();
    Some(format!("{vendor} {plan}"))
}

pub(crate) fn chatgpt_plan_from_claims(claims: &Value) -> Option<String> {
    vendor_plan(
        "ChatGPT",
        claims
            .pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type")?
            .as_str()?,
    )
}

pub(crate) fn chatgpt_plan_from_token(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    chatgpt_plan_from_claims(&claims)
}

pub(crate) fn claude_plan(credentials: &Value) -> Option<String> {
    vendor_plan(
        "Claude",
        credentials
            .pointer("/claudeAiOauth/subscriptionType")?
            .as_str()?,
    )
}

pub(crate) fn google_plan(value: &Value) -> Option<String> {
    ["paidTier", "currentTier"].iter().find_map(|key| {
        let tier = value.get(key)?;
        if let Some(name) = tier
            .get("name")
            .and_then(Value::as_str)
            .and_then(display_text)
        {
            return Some(name.to_owned());
        }
        let id = tier.get("id")?.as_str()?;
        Some(match id {
            "g1-pro-tier" => "Google AI Pro".into(),
            "g1-ultra-tier" => "Google AI Ultra".into(),
            "free-tier" => "Free".into(),
            "standard-tier" => "Standard".into(),
            _ => display_text(id)?.to_owned(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn display_labels_keep_upstream_tiers_without_granting_or_guessing_entitlements() {
        for (raw, expected) in [
            ("free", "ChatGPT Free"),
            ("go", "ChatGPT Go"),
            ("plus", "ChatGPT Plus"),
            ("pro", "ChatGPT Pro"),
            ("prolite", "ChatGPT Prolite"),
            ("promax", "ChatGPT Promax"),
            ("team", "ChatGPT Team"),
            ("business", "ChatGPT Business"),
            ("education", "ChatGPT Education"),
            ("future_plan", "ChatGPT Future_plan"),
        ] {
            assert_eq!(vendor_plan("ChatGPT", raw).as_deref(), Some(expected));
        }
        assert_eq!(
            vendor_plan("ChatGPT", "ChatGPT Prolite").as_deref(),
            Some("ChatGPT Prolite")
        );
        for invalid in [
            "",
            "unknown",
            "none",
            "pro\n",
            "https://fixture.invalid/private",
            "fixture@example.test",
        ] {
            assert!(vendor_plan("ChatGPT", invalid).is_none());
        }
        assert!(vendor_plan("ChatGPT", &"x".repeat(97)).is_none());
        assert_eq!(
            claude_plan(&json!({"claudeAiOauth":{"subscriptionType":"max_5x"}})).as_deref(),
            Some("Claude Max_5x")
        );
        assert_eq!(
            google_plan(
                &json!({"paidTier":{"name":"Google AI Ultra"},"currentTier":{"name":"Free"}})
            )
            .as_deref(),
            Some("Google AI Ultra")
        );
    }
}
