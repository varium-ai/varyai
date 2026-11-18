//! Responses replay compatibility after a same-conversation model change.
// Behavior reference: Magpie f9492fd, codexInputAs and withoutUnreadable (MIT).
use super::hyper_client::ProxyResponse;
use futures::StreamExt;
use serde_json::{value::RawValue, Value};
use std::{collections::BTreeMap, sync::OnceLock};

type Fields = BTreeMap<String, Box<RawValue>>;

fn raw_string(fields: &Fields, key: &str) -> Option<String> {
    serde_json::from_str(fields.get(key)?.get()).ok()
}

/// Official Codex cannot resolve a translated reasoning ID with no native seal.
/// Keep stored history and malformed/unknown shapes for the upstream to validate.
fn nonportable(item: &Value) -> bool {
    if item["type"] != "reasoning" {
        return false;
    }
    let seal = item.get("encrypted_content");
    if seal.is_some_and(|value| !value.is_null() && !value.is_string()) {
        return false;
    }
    let content = item.get("content");
    if content.is_some_and(|value| !value.is_null() && !value.is_array()) {
        return false;
    }
    seal.and_then(Value::as_str).is_none_or(str::is_empty)
        || content
            .and_then(Value::as_array)
            .is_some_and(|parts| !parts.is_empty())
}

pub(crate) fn normalize_official(body: &mut Value) {
    if body["store"] == true {
        return;
    }
    if let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) {
        input.retain(|item| !nonportable(item));
    }
}

/// Preserve every untouched value's original numeric lexemes on raw transports.
pub(crate) fn normalize_official_wire(body: &[u8]) -> Option<Vec<u8>> {
    let root: Fields = serde_json::from_slice(body).ok()?;
    if root.get("store").is_some_and(|value| value.get() == "true") {
        return None;
    }
    rewrite_input(root, |item| {
        serde_json::from_str::<Value>(item.get())
            .ok()
            .is_some_and(|item| nonportable(&item))
    })
}

fn rewrite_input(mut root: Fields, remove: impl Fn(&RawValue) -> bool) -> Option<Vec<u8>> {
    let items: Vec<Box<RawValue>> = serde_json::from_str(root.get("input")?.get()).ok()?;
    let count = items.len();
    let kept: Vec<_> = items.into_iter().filter(|item| !remove(item)).collect();
    if kept.len() == count {
        return None;
    }
    root.insert("input".into(), serde_json::value::to_raw_value(&kept).ok()?);
    serde_json::to_vec(&root).ok()
}

/// Never drop messages, calls, outputs or compaction checkpoints to fix a reasoning ID.
pub(crate) fn remove_wire(body: &[u8], id: &str) -> Option<Vec<u8>> {
    let root: Fields = serde_json::from_slice(body).ok()?;
    let items: Vec<&RawValue> = serde_json::from_str(root.get("input")?.get()).ok()?;
    let count = items
        .iter()
        .filter(|item| {
            serde_json::from_str::<Fields>(item.get())
                .ok()
                .is_some_and(|fields| raw_string(&fields, "id").as_deref() == Some(id))
        })
        .count();
    if count != 1 {
        return None;
    }
    rewrite_input(root, |item| {
        let Ok(fields) = serde_json::from_str::<Fields>(item.get()) else {
            return false;
        };
        raw_string(&fields, "type").as_deref() == Some("reasoning")
            && raw_string(&fields, "id").as_deref() == Some(id)
    })
}

/// Reuse the admitted attempt's token authority and validate its retained history.
/// Roll back when removing this item would invalidate a completed-call witness.
pub(crate) fn remove_checked(
    body: &mut Value,
    id: &str,
    validate: impl FnOnce(&Value) -> bool,
) -> bool {
    let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return false;
    };
    let Some(index) = items
        .iter()
        .position(|item| item["type"] == "reasoning" && item["id"] == id)
    else {
        return false;
    };
    // Ambiguous repeated IDs are not safe to interpret as one historical item.
    if items.iter().filter(|item| item["id"] == id).count() != 1 {
        return false;
    }
    let removed = items.remove(index);
    if validate(body) {
        return true;
    }
    if let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) {
        items.insert(index, removed);
    }
    false
}

fn unreadable_id(error: &[u8]) -> Option<String> {
    let error: Value = serde_json::from_slice(error).ok()?;
    let message = error.pointer("/error/message")?.as_str()?;
    if !message.to_ascii_lowercase().contains("not found") {
        return None;
    }
    static PATTERN: OnceLock<Option<regex::Regex>> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::Regex::new(
            r"(?i)(?:item with id|referenced reasoning item) '?([A-Za-z]+_[A-Za-z0-9_-]+)'?(?:\s|[.,:]|$)",
        )
        .ok()
    });
    let id = pattern.as_ref()?.captures(message)?.get(1)?.as_str();
    (id.len() <= 256).then(|| id.to_owned())
}

#[derive(Default)]
pub(crate) struct Recovery {
    retries: u8,
}

impl Recovery {
    /// Only explicit HTTP rejections before publication authorize another ask.
    /// Oversized, stalled and broken error streams remain intact and are not retried.
    pub(crate) async fn inspect(
        &mut self,
        response: ProxyResponse,
    ) -> (ProxyResponse, Option<String>) {
        let status = response.status();
        if self.retries >= 3 || !matches!(status.as_u16(), 400 | 404) {
            return (response, None);
        }
        let headers = response.headers().clone();
        let mut stream = Box::pin(response.bytes_stream());
        let mut chunks = Vec::new();
        let mut bytes = Vec::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let complete = loop {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(None) => break true,
                Ok(Some(Ok(chunk))) => {
                    let fits = bytes.len().saturating_add(chunk.len()) <= 64 * 1024;
                    if fits {
                        bytes.extend_from_slice(&chunk);
                    }
                    chunks.push(Ok(chunk));
                    if !fits {
                        break false;
                    }
                }
                Ok(Some(Err(error))) => {
                    chunks.push(Err(error));
                    break false;
                }
                Err(_) => break false,
            }
        };
        let id = complete.then(|| unreadable_id(&bytes)).flatten();
        if id.is_some() {
            self.retries += 1;
        }
        (
            ProxyResponse::streamed(status, headers, futures::stream::iter(chunks).chain(stream)),
            id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn official_filter_preserves_valid_seals_stored_and_malformed_history() {
        let mut body = json!({"store":false,"input":[
            {"type":"reasoning","id":"rs_bare","summary":[],"encrypted_content":null},
            {"type":"reasoning","id":"rs_text","content":[{"type":"reasoning_text","text":"Synthetic thought"}],"encrypted_content":"synthetic-foreign"},
            {"type":"reasoning","id":"rs_native","summary":[],"content":[],"encrypted_content":"synthetic-native"},
            {"type":"reasoning","encrypted_content":42},
            {"type":"reasoning","content":"synthetic-unsupported"},
            {"type":"function_call","call_id":"call_synthetic","name":"synthetic","arguments":"{}"},
            {"type":"function_call_output","call_id":"call_synthetic","output":"synthetic"},
            {"type":"compaction","encrypted_content":"synthetic-checkpoint"},
            {"role":"user","content":"synthetic"}
        ]});
        let original = body.clone();
        normalize_official(&mut body);
        assert_eq!(
            body["input"],
            Value::Array(original["input"].as_array().unwrap_or(&Vec::new())[2..].to_vec())
        );
        let mut stored = original;
        stored["store"] = json!(true);
        let before = stored.clone();
        normalize_official(&mut stored);
        assert_eq!(stored, before);
    }

    #[test]
    fn raw_rewrites_preserve_numeric_lexemes_and_tool_records() {
        let raw = br#"{"model":"synthetic","store":false,"n":1.234567890123456789,"huge":9007199254740993,"input":[{"type":"reasoning","id":"rs_0","summary":[]},{"type":"reasoning","id":"rs_sealed","encrypted_content":"synthetic","summary":[]},{"type":"function_call","id":"fc_fixture","call_id":"call_fixture","name":"fixture","arguments":"{}"},{"type":"function_call_output","call_id":"call_fixture","output":"synthetic"},{"role":"user","content":"synthetic","number":1e9999}]}"#;
        for rewritten in [normalize_official_wire(raw), remove_wire(raw, "rs_0")] {
            let bytes = rewritten.as_deref().unwrap_or_default();
            let text = String::from_utf8_lossy(bytes);
            for wanted in [
                "1.234567890123456789",
                "9007199254740993",
                "1e9999",
                "fc_fixture",
                "rs_sealed",
            ] {
                assert!(
                    text.contains(wanted),
                    "synthetic replay lost preserved field"
                );
            }
            assert!(!text.contains("rs_0"));
        }
        assert!(remove_wire(raw, "fc_fixture").is_none());
        assert!(remove_wire(raw, "rs_missing").is_none());
    }

    #[test]
    fn checked_removal_never_bypasses_history_validation() {
        let mut body = json!({"input":[{"type":"reasoning","id":"rs_fixture"},{"type":"function_call_output","call_id":"call_fixture","output":"synthetic"}]});
        let before = body.clone();
        assert!(!remove_checked(&mut body, "rs_fixture", |_| false));
        assert_eq!(body, before);
        assert!(remove_checked(&mut body, "rs_fixture", |_| true));
        assert_eq!(body["input"], json!([before["input"][1]]));
    }

    #[test]
    fn unsupported_and_ambiguous_input_is_preserved() {
        let long =
            json!({"error":{"message":format!("Item with id 'rs_{}' not found", "x".repeat(300))}});
        assert!(unreadable_id(long.to_string().as_bytes()).is_none());
        let unicode = json!({"error":{"message":"Item with id 'rs_0é' not found"}});
        assert!(unreadable_id(unicode.to_string().as_bytes()).is_none());
        for mut body in [
            json!({"model":"synthetic"}),
            json!({"input":"synthetic"}),
            json!({"input":null}),
        ] {
            let before = body.clone();
            normalize_official(&mut body);
            assert_eq!(body, before);
            assert!(!remove_checked(&mut body, "rs_0", |_| true));
            assert_eq!(body, before);
        }
        let raw = br#"{"input":[{"type":"reasoning","id":"rs_0","encrypted_content":"synthetic"},{"type":"message","id":"rs_0","role":"user","content":"synthetic"}]}"#;
        assert!(remove_wire(raw, "rs_0").is_none());
        let mut body: Value = serde_json::from_slice(raw).ok().unwrap_or(Value::Null);
        let before = body.clone();
        assert!(!remove_checked(&mut body, "rs_0", |_| true));
        assert_eq!(body, before);
        let protected = br#"{"input":[{"type":"compaction","id":"cmp_fixture","encrypted_content":"synthetic-checkpoint"},{"type":"function_call","id":"fc_fixture","call_id":"call_fixture","name":"fixture","arguments":"{}"},{"type":"function_call_output","id":"out_fixture","call_id":"call_fixture","output":"synthetic"},{"type":"message","id":"msg_fixture","role":"user","content":"synthetic"}]}"#;
        for id in ["cmp_fixture", "fc_fixture", "out_fixture", "msg_fixture"] {
            assert!(remove_wire(protected, id).is_none());
        }
        for raw in [
            br#"{"store":true,"input":[{"type":"reasoning","id":"rs_0"}]}"#.as_slice(),
            b"invalid JSON",
            br#"{"input":[{"type":"reasoning","encrypted_content":5}]}"#,
        ] {
            assert!(normalize_official_wire(raw).is_none());
        }
    }

    #[tokio::test]
    async fn oversized_or_broken_error_stream_is_preserved_without_retry(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use bytes::Bytes;
        let error = Bytes::from_static(br#"{"error":{"message":"Item with id 'rs_0' not found"}}"#);
        let headers = http::HeaderMap::from_iter([(
            http::header::RETRY_AFTER,
            http::HeaderValue::from_static("7"),
        )]);
        let oversized = Bytes::from([error.as_ref(), vec![b' '; 65536].as_slice()].concat());
        let response = ProxyResponse::buffered(
            http::StatusCode::NOT_FOUND,
            headers.clone(),
            oversized.clone(),
        );
        let (response, id) = Recovery::default().inspect(response).await;
        assert!(id.is_none());
        assert_eq!(response.headers(), &headers);
        assert_eq!(response.bytes_with_limit(128 * 1024).await?, oversized);

        let response = ProxyResponse::streamed(
            http::StatusCode::NOT_FOUND,
            headers,
            futures::stream::iter(vec![
                Ok(error.clone()),
                Err(std::io::Error::other("synthetic interrupted stream")),
            ]),
        );
        let (response, id) = Recovery::default().inspect(response).await;
        assert!(id.is_none());
        let chunks = response.bytes_stream().collect::<Vec<_>>().await;
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().ok(), Some(&error));
        assert!(chunks[1].is_err());
        Ok(())
    }

    #[tokio::test]
    async fn recovery_requires_named_reasoning_refusal_and_has_a_bound() {
        let error = br#"{"error":{"message":"Item with id 'rs_0' not found. Items are not persisted when `store` is set to false."}}"#;
        let mut recovery = Recovery::default();
        for status in [200, 401, 403, 409, 429, 500, 502] {
            let response = ProxyResponse::buffered(
                http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::OK),
                Default::default(),
                error.as_slice().into(),
            );
            let (_, id) = recovery.inspect(response).await;
            assert!(id.is_none());
        }
        for attempt in 0..4 {
            let response = ProxyResponse::buffered(
                http::StatusCode::NOT_FOUND,
                Default::default(),
                error.as_slice().into(),
            );
            let (response, id) = recovery.inspect(response).await;
            assert_eq!(id.as_deref(), (attempt < 3).then_some("rs_0"));
            assert_eq!(
                response.bytes_with_limit(65536).await.ok().as_deref(),
                Some(error.as_slice())
            );
        }
        for error in [
            br#"{"error":{"message":"The model does not exist"}}"#.as_slice(),
            b"not JSON",
            br#"{"error":{"message":"Item with id 'rs_0' is invalid"}}"#,
        ] {
            assert!(unreadable_id(error).is_none());
        }
    }
}
