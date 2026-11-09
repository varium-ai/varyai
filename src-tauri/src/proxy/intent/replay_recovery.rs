// Adapted from Magpie's encrypted-state refusal recovery (MIT License).
// Copyright (c) 2026 yetone
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use regex::bytes::Regex;
use serde_json::value::RawValue;
#[cfg(test)]
use serde_json::Value;
use std::{collections::BTreeMap, sync::OnceLock};

// Magpie c7ebb6f8d91693b80f863f9667879060d18820f4, affinity.go:
// foreignReasoning, withoutSeals and withoutCompaction. Keep the refusal
// vocabulary and the two recovery stages aligned with that reference.
const FOREIGN_REASONING: &str = r"(?i)invalid_encrypted_content|invalid[ _]encrypted[ _](summary|content)|encrypted[ _]content.{0,80}could not be (verified|decrypted)|could not (decrypt|verify).{0,40}encrypted[ _]content|could not (decode|decrypt).{0,20}compaction[ _]blob";
static FOREIGN_REASONING_REGEX: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();

/// One request's recovery budget. The caller must invoke recovery only before
/// releasing a response to the client, then retry the same upstream identity.
#[derive(Default)]
pub(super) struct Recovery {
    stage: u8,
}

impl Recovery {
    /// Preserve every untouched JSON value, including its numeric lexemes.
    /// The caller must protect native compaction/checkpoints and retry before
    /// returning any upstream response to the client.
    pub(super) fn recover_wire(
        &mut self,
        status: u16,
        error: &[u8],
        payload: &[u8],
        allow_compaction: bool,
    ) -> Option<Vec<u8>> {
        if status < 400 || self.stage >= 2 {
            return None;
        }
        let refusal = FOREIGN_REASONING_REGEX
            .get_or_init(|| Regex::new(FOREIGN_REASONING))
            .as_ref()
            .ok()?;
        if !refusal.is_match(error) {
            return None;
        }
        if self.stage == 0 {
            self.stage = 1;
            if let Some(recovered) = rewrite_wire(payload, false) {
                return Some(recovered);
            }
        }
        if allow_compaction {
            self.stage = 2;
            return rewrite_wire(payload, true);
        }
        None
    }

    /// `allow_compaction` must be false for a native compaction or checkpoint.
    /// Unrelated errors and requests that cannot change consume no retry.
    #[cfg(test)]
    pub(super) fn recover(
        &mut self,
        status: u16,
        error: &[u8],
        body: &Value,
        allow_compaction: bool,
    ) -> Option<Value> {
        let payload = serde_json::to_vec(body).ok()?;
        let recovered = self.recover_wire(status, error, &payload, allow_compaction)?;
        serde_json::from_slice(&recovered).ok()
    }
}

type RawObject = BTreeMap<String, Box<RawValue>>;

fn raw_string(object: &RawObject, key: &str) -> Option<String> {
    serde_json::from_str(object.get(key)?.get()).ok()
}

fn rewrite_wire(payload: &[u8], compaction: bool) -> Option<Vec<u8>> {
    let mut root: RawObject = serde_json::from_slice(payload).ok()?;
    let items: Vec<Box<RawValue>> = serde_json::from_str(root.get("input")?.get()).ok()?;
    let is_compaction = |object: &RawObject| {
        matches!(
            raw_string(object, "type").as_deref(),
            Some("compaction" | "compaction_summary")
        )
    };
    // As in Magpie, the second stage only runs when at least one compaction
    // has a nonempty seal; then all items of these two kinds are removed.
    if compaction
        && !items.iter().any(|item| {
            let Ok(object) = serde_json::from_str::<RawObject>(item.get()) else {
                return false;
            };
            is_compaction(&object)
                && raw_string(&object, "encrypted_content").is_some_and(|seal| !seal.is_empty())
        })
    {
        return None;
    }
    let mut changed = false;
    let mut kept = Vec::with_capacity(items.len());
    for item in items {
        let Ok(mut object) = serde_json::from_str::<RawObject>(item.get()) else {
            kept.push(item);
            continue;
        };
        let kind = raw_string(&object, "type");
        if (compaction && is_compaction(&object))
            || (!compaction && kind.as_deref() == Some("reasoning"))
        {
            changed = true;
            continue;
        }
        if !compaction
            && !matches!(
                kind.as_deref(),
                Some("compaction" | "compaction_summary" | "context_compaction")
            )
            && raw_string(&object, "encrypted_content").is_some_and(|seal| !seal.is_empty())
        {
            object.remove("encrypted_content");
            kept.push(serde_json::value::to_raw_value(&object).ok()?);
            changed = true;
        } else {
            kept.push(item);
        }
    }
    if !changed {
        return None;
    }
    root.insert("input".into(), serde_json::value::to_raw_value(&kept).ok()?);
    serde_json::to_vec(&root).ok()
}

#[cfg(test)]
mod tests {
    use super::Recovery;
    use serde_json::json;

    const FOREIGN: &[u8] = b"invalid_encrypted_content";

    #[test]
    fn wire_recovery_preserves_numeric_lexemes_at_both_stages() {
        let payload = br#"{"huge":9007199254740993,"decimal":1.234567890123456789,"beyond_float":1e9999,"input":[{"type":"reasoning","encrypted_content":null},{"type":"message","encrypted_content":"synthetic-seal","integer":9007199254740993,"decimal":1.234567890123456789,"nested":{"exponent":1.2300e+99}},{"type":"compaction","encrypted_content":"synthetic-compaction"}]}"#;
        let mut recovery = Recovery::default();
        let first = recovery.recover_wire(400, FOREIGN, payload, true);
        assert!(first.is_some());
        let Some(first) = first else { return };
        for lexeme in [
            "9007199254740993",
            "1.234567890123456789",
            "1e9999",
            "1.2300e+99",
        ] {
            assert!(
                String::from_utf8_lossy(&first).contains(lexeme),
                "synthetic numeric lexeme changed"
            );
        }
        let second = recovery.recover_wire(400, FOREIGN, &first, true);
        assert!(second.is_some());
        let Some(second) = second else { return };
        for lexeme in [
            "9007199254740993",
            "1.234567890123456789",
            "1e9999",
            "1.2300e+99",
        ] {
            assert!(
                String::from_utf8_lossy(&second).contains(lexeme),
                "synthetic numeric lexeme changed"
            );
        }
        assert!(!String::from_utf8_lossy(&second).contains("encrypted_content"));
        assert_eq!(recovery.recover_wire(400, FOREIGN, payload, true), None);
    }

    #[test]
    fn wire_recovery_rejects_invalid_payload_without_rewrite() {
        for payload in [b"not json".as_slice(), b"null", b"[]", b"{\"input\":null}"] {
            assert_eq!(
                Recovery::default().recover_wire(400, FOREIGN, payload, true),
                None
            );
        }
    }

    #[test]
    fn first_stage_removes_reasoning_and_only_nonempty_ordinary_seals() {
        let body = json!({
            "model": "synthetic-model", "future_option": {"keep": true},
            "input": [
                {"type": "reasoning", "encrypted_content": null, "summary": []},
                {"type": "reasoning", "encrypted_content": "", "summary": []},
                {"type": "reasoning", "encrypted_content": "synthetic-reasoning"},
                {"type": "message", "role": "assistant", "content": [], "encrypted_content": null, "future": 7},
                {"type": "message", "role": "assistant", "content": "answer", "encrypted_content": "synthetic-message", "future": 8},
                {"type": "function_call", "call_id": "synthetic-call", "arguments": "{}", "encrypted_content": "synthetic-call-seal"},
                {"type": "function_call_output", "call_id": "synthetic-call", "output": "synthetic-result", "encrypted_content": ""},
                {"type": "compaction", "encrypted_content": "synthetic-compaction"},
                {"type": "context_compaction", "encrypted_content": "synthetic-context"}
            ]
        });
        let recovered = Recovery::default().recover(409, FOREIGN, &body, true);
        assert_eq!(
            recovered,
            Some(json!({
                "model": "synthetic-model", "future_option": {"keep": true},
                "input": [
                    {"type": "message", "role": "assistant", "content": [], "encrypted_content": null, "future": 7},
                    {"type": "message", "role": "assistant", "content": "answer", "future": 8},
                    {"type": "function_call", "call_id": "synthetic-call", "arguments": "{}"},
                    {"type": "function_call_output", "call_id": "synthetic-call", "output": "synthetic-result", "encrypted_content": ""},
                    {"type": "compaction", "encrypted_content": "synthetic-compaction"},
                    {"type": "context_compaction", "encrypted_content": "synthetic-context"}
                ]
            }))
        );
        assert_eq!(body["input"].as_array().map(Vec::len), Some(9));
    }

    #[test]
    fn retries_are_bounded_to_two_stages() {
        let mut recovery = Recovery::default();
        let body = json!({"input": [
            {"type": "reasoning", "encrypted_content": "synthetic-reasoning"},
            {"type": "compaction", "encrypted_content": "synthetic-compaction"},
            {"type": "compaction_summary", "encrypted_content": null},
            {"type": "message", "role": "user", "content": "hello"}
        ]});
        let first = recovery.recover(400, FOREIGN, &body, true);
        let second = first
            .as_ref()
            .and_then(|body| recovery.recover(400, FOREIGN, body, true));
        assert_eq!(
            second,
            Some(json!({"input": [{"type": "message", "role": "user", "content": "hello"}]}))
        );
        assert_eq!(recovery.recover(400, FOREIGN, &body, true), None);
    }

    #[test]
    fn native_compaction_or_checkpoint_disables_second_stage() {
        let mut recovery = Recovery::default();
        let body = json!({"input": [
            {"type": "reasoning", "encrypted_content": "synthetic-reasoning"},
            {"type": "compaction", "encrypted_content": "synthetic-compaction"}
        ]});
        let first = recovery.recover(400, FOREIGN, &body, false);
        assert_eq!(
            first,
            Some(
                json!({"input": [{"type": "compaction", "encrypted_content": "synthetic-compaction"}]})
            )
        );
        assert_eq!(
            first
                .as_ref()
                .and_then(|body| recovery.recover(400, FOREIGN, body, false)),
            None
        );
    }

    #[test]
    fn skips_directly_to_compaction_when_first_stage_cannot_change_body() {
        let body = json!({"input": [{"type": "compaction_summary", "encrypted_content": "synthetic-compaction"}]});
        let mut recovery = Recovery::default();
        assert_eq!(
            recovery.recover(400, FOREIGN, &body, true),
            Some(json!({"input": []}))
        );
        assert_eq!(recovery.recover(400, FOREIGN, &body, true), None);
        assert_eq!(
            Recovery::default().recover(400, FOREIGN, &body, false),
            None
        );
    }

    #[test]
    fn requires_error_status_matching_refusal_and_a_real_mutation() {
        let body = json!({"input": [{"type": "reasoning", "encrypted_content": null}]});
        for status in [200, 399] {
            assert_eq!(
                Recovery::default().recover(status, FOREIGN, &body, true),
                None
            );
        }
        for error in [
            b"Conflict".as_slice(),
            b"new session required",
            b"invalid request",
            b"could not decrypt another field",
        ] {
            assert_eq!(Recovery::default().recover(409, error, &body, true), None);
        }
        for body in [
            json!({"input": "hello"}),
            json!({"input": []}),
            json!({"input": [{"type": "message", "encrypted_content": null}]}),
            json!({"input": [{"type": "compaction", "encrypted_content": ""}]}),
        ] {
            assert_eq!(Recovery::default().recover(400, FOREIGN, &body, true), None);
        }
    }

    #[test]
    fn recognizes_pinned_magpie_refusal_spellings() {
        let body = json!({"input": [{"type": "reasoning", "encrypted_content": null}]});
        for error in [
            "invalid_encrypted_content",
            "INVALID ENCRYPTED SUMMARY",
            "invalid_encrypted_summary",
            "The encrypted content for item rs_synthetic could not be verified",
            "Encrypted content could not be decrypted or parsed.",
            "Could not decrypt the provided encrypted_content.",
            "Could not verify this encrypted content",
            "Could not decode the compaction blob",
            "Could not decrypt compaction_blob",
        ] {
            assert!(
                Recovery::default()
                    .recover(400, error.as_bytes(), &body, true)
                    .is_some(),
                "synthetic refusal was not recognized"
            );
        }
    }
}
