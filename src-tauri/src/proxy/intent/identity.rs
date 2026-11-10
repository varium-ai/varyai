//! Magpie conversationID, affinityID, stickKey and opening normalization.
//! Reference: 4869d0c9665e1a815fee3a1a4d67a575e0baab93, gateway.go/prompt.go
//! (MIT; see third_party/routing-reference/NOTICE.md).
use http::HeaderMap;
use serde_json::{value::RawValue, Value};
use sha2::{Digest, Sha256};

const SESSION_HEADERS: &[&str] = &[
    "x-opencode-session",
    "x-session-affinity",
    "x-session-id",
    "session_id",
    "session-id",
    "x-claude-code-session-id",
    "x-conversation-id",
    "x-deepseek-harness-session-id",
];

pub(super) fn named(headers: &HeaderMap) -> Option<&str> {
    SESSION_HEADERS
        .iter()
        .copied()
        .chain(["x-magpie-session"])
        .find_map(|name| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        })
}

pub(super) fn affinity(headers: &HeaderMap, body: &[u8]) -> String {
    let mut key = named(headers)
        .map(str::to_owned)
        .unwrap_or_else(|| conversation(body));
    if let Some(sub) = headers
        .get("x-claude-code-agent-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        let mut end = sub.len().min(128);
        while !sub.is_char_boundary(end) {
            end -= 1;
        }
        key.push_str("|agent:");
        key.push_str(&sub[..end]);
    }
    key
}

fn elements(raw: &RawValue) -> Vec<&RawValue> {
    serde_json::from_str(raw.get()).unwrap_or_default()
}

fn object(raw: &RawValue) -> Option<std::collections::HashMap<String, &RawValue>> {
    serde_json::from_str(raw.get()).ok()
}

fn role(raw: &RawValue) -> Option<String> {
    serde_json::from_str(object(raw)?.get("role")?.get()).ok()
}

fn conversation(body: &[u8]) -> String {
    let parsed = serde_json::from_slice::<&RawValue>(body).ok();
    let fields = parsed.and_then(object).unwrap_or_default();
    let messages = fields
        .get("messages")
        .map(|r| elements(r))
        .unwrap_or_default();
    let contents = fields
        .get("contents")
        .map(|r| elements(r))
        .unwrap_or_default();
    let gemini = messages.is_empty() && !contents.is_empty();
    let mut items = if gemini { contents } else { messages };
    if items.is_empty() {
        items = fields.get("input").map(|r| elements(r)).unwrap_or_default();
    }
    let first = items
        .first()
        .copied()
        .or_else(|| fields.get("input").copied());
    let mut opening = first.into_iter().collect::<Vec<_>>();
    for (index, item) in items.iter().enumerate() {
        let r = role(item);
        if r.as_deref() == Some("user") || (gemini && r.as_deref().unwrap_or("").is_empty()) {
            opening = conversation_opening(&items[index..], gemini);
            break;
        }
    }
    let mut digest = Sha256::new();
    if opening.len() > 1 {
        for (index, item) in opening.iter().enumerate() {
            if index > 0 {
                digest.update(b"\n");
            }
            write_opening(&mut digest, item.get());
        }
    } else {
        digest.update(opening.first().map(|r| r.get().as_bytes()).unwrap_or(body));
    }
    let hash = digest.finalize();
    let mut key = String::from("magpie-");
    use std::fmt::Write;
    for byte in &hash[..12] {
        let _ = write!(key, "{byte:02x}");
    }
    key
}

fn conversation_opening<'a>(items: &[&'a RawValue], gemini: bool) -> Vec<&'a RawValue> {
    let mut opening = vec![items[0]];
    for item in &items[1..] {
        let r = role(item);
        if matches!(r.as_deref(), Some("system" | "developer")) {
            continue;
        }
        if r.as_deref() != Some("user") && !(gemini && r.as_deref().unwrap_or("").is_empty()) {
            break;
        }
        if opening.len() == 1 {
            let (theirs, result) = user_opens(opening[0]);
            if theirs || result {
                break;
            }
        }
        let (theirs, result) = user_opens(item);
        if result {
            break;
        }
        opening.push(item);
        if theirs {
            return opening;
        }
    }
    opening.truncate(1);
    opening
}

fn user_opens(raw: &RawValue) -> (bool, bool) {
    let Some(fields) = object(raw) else {
        return (true, false);
    };
    if let Some(text) = fields
        .get("content")
        .and_then(|r| serde_json::from_str::<String>(r.get()).ok())
    {
        return (!user_words(&text).is_empty(), false);
    }
    let (mut theirs, mut result) = (false, false);
    for field in ["content", "parts"] {
        for part in fields.get(field).map(|r| elements(r)).unwrap_or_default() {
            let Ok(part) = serde_json::from_str::<Value>(part.get()) else {
                continue;
            };
            let kind = part["type"].as_str().unwrap_or("");
            if kind == "tool_result" || part.get("functionResponse").is_some() {
                result = true;
            } else if part.get("inlineData").is_some()
                || part.get("fileData").is_some()
                || matches!(
                    kind,
                    "image"
                        | "image_url"
                        | "input_image"
                        | "document"
                        | "file"
                        | "input_file"
                        | "input_audio"
                )
            {
                theirs = true;
            } else if matches!(kind, "text" | "input_text")
                || (kind.is_empty() && part["thought"] != true)
            {
                theirs |= !user_words(part["text"].as_str().unwrap_or("")).is_empty();
            }
        }
    }
    (theirs, result)
}

fn user_words(text: &str) -> String {
    // These expressions and prefixes are Magpie prompt.go's userWords.
    static PATTERNS: std::sync::OnceLock<Option<[regex::Regex; 3]>> = std::sync::OnceLock::new();
    let Some(patterns) = PATTERNS.get_or_init(|| {
        Some([
            regex::Regex::new(r"(?s)<system-reminder(?:\s[^>]*)?>.*?</system-reminder>").ok()?,
            regex::Regex::new(r"(?m)^## My request(?: for Codex)?:[ \t]*\n?").ok()?,
            regex::Regex::new(r"^# (\S+\.md) instructions for (\S.*?)[ \t]*\n").ok()?,
        ])
    }) else {
        return text.trim().to_owned();
    };
    let without_reminders = if text.contains("<system-reminder") {
        patterns[0].replace_all(text, "")
    } else {
        std::borrow::Cow::Borrowed(text)
    };
    let mut t = without_reminders.trim();
    if t.contains("## My request") {
        if let Some(last) = patterns[1].find_iter(t).last() {
            t = t[last.end()..].trim();
        }
    }
    if t.is_empty() || patterns[2].is_match(t) {
        return String::new();
    }
    const INJECTED: &[&str] = &[
        "<system-reminder",
        "<environment_context>",
        "<user_instructions>",
        "<turn_aborted>",
        "<user_shell_command>",
        "<subagent_notification>",
        "<in-app-browser-context",
        "<external_codex_apps_open_page>",
        "<session-context",
        "<task-notification>",
        "<subagent-notification>",
        "<plugin_reference>",
        "Available agent types for the Agent tool:",
        "Available agent types and the tools they have access to:",
        "New agent types are now available for the Agent tool:",
        "The TodoWrite tool hasn't been used recently.",
        "The task tools haven't been used recently.",
        "Conversation rewind applied.",
        "Workspace rewind applied.",
    ];
    if INJECTED.iter().any(|prefix| t.starts_with(prefix)) {
        String::new()
    } else {
        t.to_owned()
    }
}

// Borrow original member spellings so escaped keys/strings and numeric lexemes
// have exactly the same normalization as Magpie's writeOpening.
fn raw_prefix(raw: &str) -> Option<(&RawValue, &str)> {
    let mut stream = serde_json::Deserializer::from_str(raw).into_iter::<&RawValue>();
    let value = stream.next()?.ok()?;
    Some((value, &raw[stream.byte_offset()..]))
}

fn write_opening(digest: &mut Sha256, raw: &str) {
    let raw = raw.trim();
    if let Some(mut rest) = raw.strip_prefix('{') {
        let mut members = Vec::new();
        while !rest.trim_start().starts_with('}') {
            let Some((key, after_key)) = raw_prefix(rest) else {
                return;
            };
            let Some(after_colon) = after_key.trim_start().strip_prefix(':') else {
                return;
            };
            let Some((value, after_value)) = raw_prefix(after_colon) else {
                return;
            };
            let decoded = serde_json::from_str::<String>(key.get()).ok();
            if decoded.as_deref() != Some("cache_control") {
                members.push((key.get(), decoded, value));
            }
            rest = after_value.trim_start();
            if let Some(after_comma) = rest.strip_prefix(',') {
                rest = after_comma;
            } else {
                break;
            }
        }
        members.sort_by(|a, b| a.0.cmp(b.0));
        digest.update(b"{");
        for (index, (key, decoded, value)) in members.iter().enumerate() {
            if index > 0 {
                digest.update(b",");
            }
            digest.update(key.as_bytes());
            digest.update(b":");
            let text = (decoded.as_deref() == Some("content"))
                .then(|| one_text_part(value))
                .flatten();
            write_opening(digest, text.unwrap_or(value).get());
        }
        digest.update(b"}");
    } else if raw.starts_with('[') {
        let Ok(items) = serde_json::from_str::<Vec<&RawValue>>(raw) else {
            return;
        };
        digest.update(b"[");
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                digest.update(b",");
            }
            write_opening(digest, item.get());
        }
        digest.update(b"]");
    } else {
        digest.update(raw.as_bytes());
    }
}

fn one_text_part(raw: &RawValue) -> Option<&RawValue> {
    let parts = elements(raw);
    if parts.len() != 1 {
        return None;
    }
    let fields = object(parts[0])?;
    if fields
        .keys()
        .any(|key| !matches!(key.as_str(), "cache_control" | "type" | "text"))
    {
        return None;
    }
    if !matches!(fields.get("type")?.get(), "\"text\"" | "\"input_text\"") {
        return None;
    }
    fields
        .get("text")
        .copied()
        .filter(|r| r.get().starts_with('"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magpie_identity_matches_pinned_go_reference() -> Result<(), Box<dyn std::error::Error>> {
        // Expected values were emitted by the original Go functions, not the
        // Rust implementation. All request bodies and identifiers are synthetic.
        #[derive(serde::Deserialize)]
        struct Case {
            name: String,
            headers: std::collections::HashMap<String, String>,
            body: String,
            expected: String,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("testdata/magpie_identity.json"))?;
        for case in cases {
            let mut headers = HeaderMap::new();
            for (name, value) in case.headers {
                headers.insert(name.parse::<http::HeaderName>()?, value.parse()?);
            }
            assert_eq!(
                affinity(&headers, case.body.as_bytes()),
                case.expected,
                "{}",
                case.name
            );
        }
        Ok(())
    }
}
