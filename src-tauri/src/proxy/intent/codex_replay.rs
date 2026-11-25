//! Select a new Codex turn without treating inline, closed tool pairs as live owners.
use serde_json::Value;
use std::collections::HashMap;

pub(super) struct InlineReplay {
    pub current_turn_start: usize,
    pub new_user_turn: bool,
    pub follows_interruption: bool,
}

/// Classifies the read-only planner view. All items, including historical tool
/// pairs, still go through the usual request conversion and inspection.
pub(super) fn inline_tool_history(body: &Value) -> Option<InlineReplay> {
    if body["_group_projection_incomplete"] == true
        || !body["previous_response_id"].is_null()
        || !body["conversation"].is_null()
    {
        return None;
    }
    let items = body["input"].as_array()?;
    let start = items
        .iter()
        .rposition(|item| is_user(item) && !is_interrupt_marker(item))?;
    if !visible_message(&items[start]) {
        return None;
    }
    let mut calls = HashMap::new();
    let mut open_calls = 0usize;
    let mut awaiting_reply = false;
    let mut page_context = false;
    let mut closed_reply = false;
    let mut last_user_turn = None;
    let mut interrupted_turn = None;
    for item in &items[..start] {
        if visible_message(item) {
            if is_user(item) && is_interrupt_marker(item) {
                let turn = turn_id(item)?;
                if open_calls != 0 || last_user_turn != Some(turn) {
                    return None;
                }
                awaiting_reply = false;
                interrupted_turn = Some(turn);
            } else if is_user(item) {
                if awaiting_reply
                    || interrupted_turn.is_some_and(|turn| turn_id(item) == Some(turn))
                {
                    return None;
                }
                last_user_turn = turn_id(item);
                closed_reply = false;
            } else if item["role"] == "assistant"
                && item["phase"] != "commentary"
                && open_calls == 0
            {
                // Text emitted beside an outstanding call is visible history,
                // not its final reply. Wait for all outputs and a later reply.
                awaiting_reply = false;
                interrupted_turn = None;
                closed_reply = true;
            }
        } else if super::length::visible_reasoning(item) || reasoning_preference(item) {
            continue;
        } else if is_page_context(item) {
            page_context = true;
        } else {
            match item["type"].as_str() {
                Some("function_call") if full_call(item) => {
                    let id = call_id(item)?;
                    if calls.len() >= 4096 || calls.insert(id, true).is_some() {
                        return None;
                    }
                    open_calls += 1;
                    awaiting_reply = true;
                }
                Some("function_call_output") if full_output(item) => {
                    let open = calls.get_mut(call_id(item)?)?;
                    if !*open {
                        return None;
                    }
                    *open = false;
                    open_calls -= 1;
                }
                _ => return None,
            }
        }
    }
    let current = &items[start + 1..];
    let new_user_turn = current.iter().all(is_page_context);
    // Message IDs from a plain completed conversation can also belong to
    // several models. They are not references when the text is present.
    let plain_boundary = new_user_turn && (closed_reply || interrupted_turn.is_some());
    if open_calls != 0
        || awaiting_reply
        || (calls.is_empty()
            && !page_context
            && !current.iter().any(is_page_context)
            && !plain_boundary)
    {
        return None;
    }
    // Codex can close an interrupted turn without an assistant reply. Its own
    // abort marker and a distinct user turn identify this boundary; plain text
    // resembling the marker, or an unmatched tool output, is not sufficient.
    let follows_interruption = match interrupted_turn {
        Some(previous) if previous != turn_id(&items[start])? => true,
        Some(_) => return None,
        None => false,
    };
    Some(InlineReplay {
        current_turn_start: start,
        new_user_turn,
        follows_interruption,
    })
}

// Codex can include the model picker's reasoning preference in the transcript.
// Only this exact, inline shape has no provider-owned continuation state.
fn reasoning_preference(item: &Value) -> bool {
    item["type"] == "configuration_update"
        && item.as_object().is_some_and(|fields| fields.len() == 2)
        && item["reasoning"]
            .as_object()
            .is_some_and(|fields| fields.len() == 1)
        && matches!(
            item["reasoning"]["effort"].as_str(),
            Some("none" | "minimal" | "low" | "medium" | "high" | "xhigh")
        )
}

fn item_fields(item: &Value, fields: &[&str]) -> bool {
    item.as_object().is_some_and(|object| {
        object.keys().all(|name| {
            matches!(
                name.as_str(),
                "type" | "id" | "internal_chat_message_metadata_passthrough"
            ) || fields.contains(&name.as_str())
        }) && (item["id"].is_null() || item["id"].is_string())
            && (item["internal_chat_message_metadata_passthrough"].is_null()
                || item["internal_chat_message_metadata_passthrough"].is_object())
    })
}

fn nonempty_bounded(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|text| !text.is_empty() && text.len() <= 256)
}

fn call_id(item: &Value) -> Option<&str> {
    nonempty_bounded(&item["call_id"])
}

fn turn_id(item: &Value) -> Option<&str> {
    nonempty_bounded(&item["internal_chat_message_metadata_passthrough"]["turn_id"])
}

fn is_user(item: &Value) -> bool {
    (item["type"].is_null() || item["type"] == "message") && item["role"] == "user"
}

fn is_interrupt_marker(item: &Value) -> bool {
    item["internal_chat_message_metadata_passthrough"]["content_item_kinds"]
        .as_array()
        .is_some_and(|kinds| kinds.len() == 1 && kinds[0] == "generic.turn_aborted")
        && item["content"].as_array().is_some_and(|parts| {
            parts.len() == 1
                && parts[0]["type"] == "input_text"
                && parts[0]["text"].as_str().is_some_and(|text| {
                    text.trim().starts_with("<turn_aborted>")
                        && text.trim().ends_with("</turn_aborted>")
                })
        })
}

fn text_parts(value: &Value, kinds: &[&str]) -> bool {
    value.is_string()
        || value.as_array().is_some_and(|parts| {
            !parts.is_empty()
                && parts.iter().all(|part| {
                    part["type"]
                        .as_str()
                        .is_some_and(|kind| kinds.contains(&kind))
                        && part["text"].is_string()
                        && part.as_object().is_some_and(|fields| {
                            fields.keys().all(|key| match key.as_str() {
                                "type" | "text" => true,
                                "annotations" | "logprobs" if part["type"] == "output_text" => {
                                    fields
                                        .get(key)
                                        .and_then(Value::as_array)
                                        .is_some_and(Vec::is_empty)
                                }
                                _ => false,
                            })
                        })
                })
        })
}

fn completed(item: &Value) -> bool {
    item["status"].is_null() || item["status"] == "completed"
}

fn visible_message(item: &Value) -> bool {
    (item["type"].is_null() || item["type"] == "message")
        && matches!(
            item["role"].as_str(),
            Some("user" | "assistant" | "system" | "developer")
        )
        && item_fields(item, &["role", "content", "status", "phase"])
        && (item["phase"].is_null()
            || matches!(item["phase"].as_str(), Some("commentary" | "final_answer")))
        && completed(item)
        && text_parts(&item["content"], &["text", "input_text", "output_text"])
}

fn full_call(item: &Value) -> bool {
    item_fields(
        item,
        &["call_id", "name", "namespace", "arguments", "status"],
    ) && completed(item)
        && nonempty_bounded(&item["name"]).is_some()
        && (item["namespace"].is_null() || nonempty_bounded(&item["namespace"]).is_some())
        && item["arguments"].as_str().is_some_and(|args| {
            args.len() <= 1024 * 1024
                && args.trim_start().starts_with('{')
                && serde_json::from_str::<&serde_json::value::RawValue>(args).is_ok()
        })
}

fn full_output(item: &Value) -> bool {
    item_fields(item, &["call_id", "output", "status"])
        && completed(item)
        && text_parts(&item["output"], &["input_text", "text"])
}

fn is_page_context(item: &Value) -> bool {
    super::super::providers::codex_chat_common::is_unsolicited_page_context(item)
        && item_fields(item, &["name", "namespace", "call_id", "output", "status"])
        && completed(item)
        && text_parts(&item["output"], &["input_text", "text"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn closed() -> Value {
        json!({"input":[
            {"role":"user","content":"Synthetic opening"},
            {"type":"function_call","name":"synthetic_tool","call_id":"call-test","arguments":"{}"},
            {"type":"function_call_output","call_id":"call-test","output":"Synthetic result"},
            {"type":"reasoning","id":"rs-test","summary":[{"type":"summary_text","text":"Synthetic reasoning"}],"encrypted_content":null},
            {"role":"assistant","content":[{"type":"output_text","text":"Synthetic answer","annotations":[]}],"phase":"final_answer"},
            {"role":"user","content":"Synthetic next"}
        ]})
    }

    #[test]
    fn closed_pairs_define_a_boundary_but_not_an_active_tool_continuation() {
        let mut body = closed();
        let replay = inline_tool_history(&body).expect("closed history");
        assert_eq!(replay.current_turn_start, 5);
        assert!(replay.new_user_turn && !replay.follows_interruption);
        body["input"].as_array_mut().expect("fixture array").extend([
            json!({"type":"function_call","name":"synthetic_tool","call_id":"call-active","arguments":"{}"}),
            json!({"type":"function_call_output","call_id":"call-active","output":"Synthetic active result"}),
        ]);
        let active = inline_tool_history(&body).expect("older history is closed");
        assert_eq!(active.current_turn_start, 5);
        assert!(!active.new_user_turn);
    }

    #[test]
    fn plain_complete_messages_from_different_models_are_inline_history() {
        let body = json!({"input":[
            {"role":"user","content":"Synthetic opening"},
            {"role":"assistant","id":"provider-a-message","content":"First answer"},
            {"role":"user","content":"Synthetic second"},
            {"type":"message","role":"assistant","id":"provider-b-message","phase":"final_answer","status":"completed",
                "content":[{"type":"output_text","text":"Second answer","annotations":[],"logprobs":[]}]},
            {"role":"user","content":"Return to group"}
        ]});
        let history = inline_tool_history(&body).expect("inline messages");
        assert!(history.new_user_turn);
        assert_eq!(history.current_turn_start, 4);
    }

    #[test]
    fn text_before_a_tool_output_does_not_close_the_round() {
        let mut body = closed();
        let items = body["input"].as_array_mut().expect("fixture array");
        let final_reply = items.remove(4);
        items.insert(2, final_reply.clone());
        assert!(inline_tool_history(&body).is_none());
        body["input"]
            .as_array_mut()
            .expect("fixture array")
            .insert(5, final_reply);
        assert!(inline_tool_history(&body).is_some());
    }

    #[test]
    fn malformed_or_opaque_history_does_not_release_the_owner() {
        for (index, field, value) in [
            (1, "arguments", json!("{bad")),
            (1, "status", json!("in_progress")),
            (2, "call_id", json!("unmatched")),
            (
                2,
                "output",
                json!([{"type":"input_image","image_url":"synthetic"}]),
            ),
            (3, "encrypted_content", json!("synthetic-seal")),
            (4, "phase", json!("commentary")),
            (4, "type", json!("item_reference")),
            (
                4,
                "content",
                json!([{"type":"output_text","text":"Synthetic answer","logprobs":[{"token":"fixture","logprob":-1.0}]}]),
            ),
            (
                4,
                "content",
                json!([{"type":"output_text","text":"Synthetic cited answer","annotations":[{"type":"file_citation","file_id":"synthetic-file"}]}]),
            ),
        ] {
            let mut body = closed();
            body["input"][index][field] = value;
            assert!(inline_tool_history(&body).is_none(), "{index}: {field}");
        }
        for field in ["previous_response_id", "conversation"] {
            let mut body = closed();
            body[field] = json!("synthetic-cursor");
            assert!(inline_tool_history(&body).is_none(), "{field}");
        }
        for invalid in [
            json!({"type":"function_call_output","call_id":"call-test","output":"Synthetic duplicate"}),
            json!({"type":"item_reference","id":"synthetic-reference"}),
            json!({"type":"compaction","encrypted_content":"synthetic-checkpoint"}),
            json!({"type":"configuration_update","reasoning":{"effort":"high","encrypted_content":"synthetic-seal"}}),
            json!({"type":"configuration_update","reasoning":{"effort":"high"},"id":"synthetic-reference"}),
        ] {
            let mut body = closed();
            body["input"]
                .as_array_mut()
                .expect("fixture array")
                .insert(5, invalid);
            assert!(inline_tool_history(&body).is_none());
        }
    }
}
