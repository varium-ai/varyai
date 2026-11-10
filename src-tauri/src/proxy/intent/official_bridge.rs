//! Shared wire adaptation for explicitly selected official Responses targets.
//! Account selection, token refresh and transport remain in the intent executor.
use super::super::{
    protocol_bridge::{ir, Protocol},
    ProxyError,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};

const MAX_HISTORY_BYTES: usize = 2 * 1024 * 1024;
const MAX_HISTORY_REPLIES: usize = 64;

#[derive(Default)]
pub(super) struct History(Mutex<VecDeque<Stored>>);
struct Stored {
    target: usize,
    calls: Vec<String>,
    reasoning: Vec<Value>,
    visible: String,
    bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_deferred_tool_discovery_stays_callable_on_official_responses(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let body = json!({
            "model":"synthetic-model",
            "messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":"synthetic-search","name":"ToolSearch","input":{"query":"lookup"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-search","content":[{"type":"tool_reference","tool_name":"lookup"}]}]}
            ],
            "tools":[
                {"name":"ToolSearch","input_schema":{"type":"object"}},
                {"name":"lookup","defer_loading":true,"input_schema":{"type":"object","properties":{"query":{"type":"string"}}}}
            ]
        });
        let wire = request_wire(
            &serde_json::to_vec(&body)?,
            Protocol::Anthropic,
            "/v1/messages",
            "synthetic-model",
            &History::default(),
            0,
            false,
        )?;
        let sent: Value = serde_json::from_slice(&wire)?;
        assert_eq!(sent["tools"][1]["name"], "lookup");
        assert_eq!(
            sent["tools"][1]["parameters"],
            body["tools"][1]["input_schema"]
        );
        assert!(sent["tools"][1].get("defer_loading").is_none());
        assert!(sent["input"].to_string().contains("Tool available: lookup"));

        let parsed = ir::parse::request(Protocol::Anthropic, &body, "/v1/messages")?;
        assert!(
            ir::build::request(&parsed, Protocol::Responses)?["tools"][1]
                .get("defer_loading")
                .is_none()
        );
        assert_eq!(
            ir::build::request(&parsed, Protocol::Anthropic)?["tools"][1]["defer_loading"],
            true
        );
        Ok(())
    }

    #[test]
    fn native_responses_keep_their_explicit_deferred_tool_search_contract(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let body = json!({
            "model":"synthetic-model","input":"Synthetic question",
            "tools":[
                {"type":"tool_search"},
                {"type":"function","name":"lookup","defer_loading":true,"parameters":{"type":"object"}}
            ]
        });
        let wire = request_wire(
            &serde_json::to_vec(&body)?,
            Protocol::Responses,
            "/v1/responses",
            "synthetic-model",
            &History::default(),
            0,
            false,
        )?;
        let sent: Value = serde_json::from_slice(&wire)?;
        assert_eq!(sent["tools"][0], body["tools"][0]);
        assert_eq!(sent["tools"][1]["defer_loading"], true);
        assert_eq!(
            sent["tools"][1]["parameters"],
            body["tools"][1]["parameters"]
        );
        Ok(())
    }

    fn reply() -> ir::Reply {
        ir::Reply {
            id: "synthetic-official-reply".into(),
            model: "synthetic-model".into(),
            stop: "tool".into(),
            usage: Default::default(),
            parts: vec![
                ir::Part::Thinking {
                    text: "Synthetic visible thought".into(),
                    signature: None,
                    source: Protocol::Responses,
                    sealed: Some(
                        json!({"id":"synthetic-reasoning","type":"reasoning","summary":[{"type":"summary_text","text":"Synthetic visible thought"}],"encrypted_content":"synthetic-private-seal"}),
                    ),
                },
                ir::Part::ToolCall {
                    id: "synthetic-call".into(),
                    name: "synthetic_tool".into(),
                    arguments: json!({"value":7}),
                    signature: None,
                },
            ],
        }
    }
    fn payload(thought: &str, argument: u64) -> Vec<u8> {
        json!({"model":"synthetic-model","messages":[{"role":"assistant","content":null,"reasoning_content":thought,"tool_calls":[{"id":"synthetic-call","type":"function","function":{"name":"synthetic_tool","arguments":json!({"value":argument}).to_string()}}]},{"role":"tool","tool_call_id":"synthetic-call","content":"Synthetic result"}]}).to_string().into_bytes()
    }
    #[test]
    fn opaque_state_only_returns_to_its_exact_scope_target_and_inspected_call(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let history = History::default();
        history.record(&reply(), 0);
        let wire = request(
            &payload("Synthetic visible thought", 7),
            "/v1/chat/completions",
            &history,
            0,
        )?;
        let value: Value = serde_json::from_slice(&wire)?;
        assert!(value["input"]
            .as_array()
            .ok_or("missing input")?
            .iter()
            .any(|item| item["encrypted_content"] == "synthetic-private-seal"));
        for (store, target, thought, argument) in [
            (&history, 1, "Synthetic visible thought", 7),
            (&history, 0, "Synthetic inspected replacement", 7),
            (&history, 0, "Synthetic visible thought", 8),
        ] {
            let wire = request(
                &payload(thought, argument),
                "/v1/chat/completions",
                store,
                target,
            )?;
            assert!(!String::from_utf8(wire)?.contains("synthetic-private-seal"));
        }
        let wire = request(
            &payload("Synthetic visible thought", 7),
            "/v1/chat/completions",
            &History::default(),
            0,
        )?;
        assert!(!String::from_utf8(wire)?.contains("synthetic-private-seal"));
        Ok(())
    }
    #[test]
    fn conflicting_opaque_replies_do_not_guess_a_state() -> Result<(), Box<dyn std::error::Error>> {
        let history = History::default();
        let original = reply();
        history.record(&original, 0);
        let mut changed = original;
        if let ir::Part::Thinking {
            sealed: Some(item), ..
        } = &mut changed.parts[0]
        {
            item["encrypted_content"] = json!("synthetic-other-seal");
        }
        history.record(&changed, 0);
        let wire = request(
            &payload("Synthetic visible thought", 7),
            "/v1/chat/completions",
            &history,
            0,
        )?;
        assert!(!String::from_utf8(wire)?.contains("encrypted_content\":\"synthetic"));
        Ok(())
    }
    #[test]
    fn adding_a_visible_summary_does_not_restore_a_previously_hidden_one(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let history = History::default();
        let mut original = reply();
        if let ir::Part::Thinking {
            text,
            sealed: Some(item),
            ..
        } = &mut original.parts[0]
        {
            text.clear();
            item["summary"] = json!([]);
        }
        history.record(&original, 0);
        let valid = request(&payload("", 7), "/v1/chat/completions", &history, 0)?;
        assert!(String::from_utf8(valid)?.contains("synthetic-private-seal"));
        let changed = request(
            &payload("Synthetic added summary", 7),
            "/v1/chat/completions",
            &history,
            0,
        )?;
        assert!(!String::from_utf8(changed)?.contains("synthetic-private-seal"));
        Ok(())
    }
    #[test]
    fn old_private_tool_state_is_evicted_without_affecting_newer_state(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let history = History::default();
        for index in 0..65 {
            let mut next = reply();
            if let ir::Part::ToolCall { id, .. } = &mut next.parts[1] {
                *id = format!("synthetic-call-{index}");
            }
            history.record(&next, 0);
        }
        for (index, retained) in [(0, false), (64, true)] {
            let mut body: Value = serde_json::from_slice(&payload("Synthetic visible thought", 7))?;
            body["messages"][0]["tool_calls"][0]["id"] = json!(format!("synthetic-call-{index}"));
            body["messages"][1]["tool_call_id"] = json!(format!("synthetic-call-{index}"));
            let wire = request(
                &serde_json::to_vec(&body)?,
                "/v1/chat/completions",
                &history,
                0,
            )?;
            assert_eq!(
                String::from_utf8(wire)?.contains("synthetic-private-seal"),
                retained
            );
        }
        Ok(())
    }
}
fn call_key(id: &str, name: &str, arguments: &Value) -> Option<String> {
    if id.len() > 256 || name.len() > 1024 {
        return None;
    }
    let encoded = serde_json::to_string(&(id, name, arguments)).ok()?;
    (encoded.len() <= 256 * 1024).then(|| super::continuation::hash("official-call", &encoded))
}
impl History {
    pub(super) fn record(&self, reply: &ir::Reply, target: usize) {
        let mut entry = Stored {
            target,
            calls: Vec::new(),
            reasoning: Vec::new(),
            visible: String::new(),
            bytes: 0,
        };
        for part in &reply.parts {
            match part {
                ir::Part::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } => {
                    if entry.calls.len() >= 64 {
                        return;
                    }
                    let Some(key) = call_key(id, name, arguments) else {
                        return;
                    };
                    entry.calls.push(key);
                }
                ir::Part::Thinking {
                    text,
                    source: Protocol::Responses,
                    sealed,
                    ..
                } => {
                    if entry.visible.len().saturating_add(text.len()) > MAX_HISTORY_BYTES / 2 {
                        return;
                    }
                    entry.visible.push_str(text);
                    if let Some(item) = sealed
                        .as_ref()
                        .filter(|item| item["encrypted_content"].is_string())
                    {
                        // Only known opaque fields and the exact visible summary
                        // can be reintroduced after request inspection.
                        if item.as_object().is_none_or(|fields| {
                            fields.keys().any(|key| {
                                !matches!(
                                    key.as_str(),
                                    "type" | "id" | "summary" | "encrypted_content"
                                )
                            })
                        }) {
                            return;
                        }
                        if item.get("id").is_some_and(|id| !id.is_string())
                            || item["summary"].as_array().is_none_or(|parts| {
                                parts.iter().any(|part| {
                                    part["type"] != "summary_text"
                                        || !part["text"].is_string()
                                        || part.as_object().is_none_or(|fields| {
                                            fields
                                                .keys()
                                                .any(|key| !matches!(key.as_str(), "type" | "text"))
                                        })
                                })
                            })
                        {
                            return;
                        }
                        let weight = item.to_string().len();
                        entry.bytes = entry.bytes.saturating_add(weight);
                        if entry.bytes > MAX_HISTORY_BYTES / 2 {
                            return;
                        }
                        entry.reasoning.push(item.clone());
                    }
                }
                _ => {}
            }
        }
        if entry.calls.is_empty() || entry.calls.len() > 64 || entry.reasoning.is_empty() {
            return;
        }
        entry.bytes = entry
            .bytes
            .saturating_add(entry.visible.len() + entry.calls.len() * 64);
        if entry.bytes > MAX_HISTORY_BYTES {
            return;
        }
        let Ok(mut history) = self.0.lock() else {
            return;
        };
        if let Some(old) = history.iter_mut().find(|old| {
            old.target == target && old.calls.iter().any(|key| entry.calls.contains(key))
        }) {
            if old.calls != entry.calls
                || old.reasoning != entry.reasoning
                || old.visible != entry.visible
            {
                old.reasoning.clear();
            }
            return;
        }
        while history.len() >= MAX_HISTORY_REPLIES
            || history
                .iter()
                .map(|item| item.bytes)
                .sum::<usize>()
                .saturating_add(entry.bytes)
                > MAX_HISTORY_BYTES
        {
            history.pop_front();
        }
        history.push_back(entry);
    }

    fn restore(&self, body: &mut Value, target: usize) {
        let Some(input) = body["input"].as_array_mut() else {
            return;
        };
        let mut keys = HashMap::new();
        for (index, item) in input
            .iter()
            .enumerate()
            .filter(|(_, item)| item["type"] == "function_call")
        {
            let (Some(id), Some(name), Some(arguments)) = (
                item["call_id"].as_str(),
                item["name"].as_str(),
                item["arguments"].as_str(),
            ) else {
                continue;
            };
            if arguments.len() > 256 * 1024 {
                continue;
            }
            let Some(key) = serde_json::from_str::<Value>(arguments)
                .ok()
                .and_then(|arguments| call_key(id, name, &arguments))
            else {
                continue;
            };
            keys.entry(key)
                .and_modify(|old| *old = usize::MAX)
                .or_insert(index);
        }
        let Ok(history) = self.0.lock() else {
            return;
        };
        let mut edits = Vec::new();
        for entry in history
            .iter()
            .filter(|entry| entry.target == target && !entry.reasoning.is_empty())
        {
            let positions = entry
                .calls
                .iter()
                .map(|key| keys.get(key).copied().filter(|index| *index != usize::MAX))
                .collect::<Option<Vec<_>>>();
            let Some(positions) = positions else {
                continue;
            };
            let Some(&first) = positions.first() else {
                continue;
            };
            if !positions.windows(2).all(|pair| pair[0] < pair[1]) {
                continue;
            }
            let start = if entry.visible.is_empty() {
                if first > 0 && input[first - 1]["type"] == "reasoning" {
                    continue;
                }
                first
            } else {
                let visible = json!({"type":"reasoning","summary":[{"type":"summary_text","text":entry.visible}]});
                if first == 0 || input[first - 1] != visible {
                    continue;
                }
                first - 1
            };
            edits.push((start, first, entry.reasoning.clone()));
        }
        drop(history);
        edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
        for (start, end, reasoning) in edits {
            input.splice(start..end, reasoning);
        }
    }
}

#[cfg(test)]
fn request(
    payload: &[u8],
    endpoint: &str,
    history: &History,
    target: usize,
) -> Result<Vec<u8>, ProxyError> {
    request_wire(
        payload,
        Protocol::Chat,
        endpoint,
        "synthetic-model",
        history,
        target,
        true,
    )
}

pub(super) fn request_wire(
    payload: &[u8],
    protocol: Protocol,
    endpoint: &str,
    model: &str,
    history: &History,
    target: usize,
    workbuddy: bool,
) -> Result<Vec<u8>, ProxyError> {
    let mut body = super::super::pi::json::parse(payload)
        .map_err(|_| ProxyError::InvalidRequest("Invalid Chat request".into()))?;
    if workbuddy && protocol == Protocol::Chat {
        super::super::workbuddy::normalize_chat_for_conversion(&mut body);
    }
    let mut parsed = ir::parse::request(protocol, &body, endpoint)?;
    parsed.model = model.to_owned();
    let mut instructions = Vec::new();
    for part in std::mem::take(&mut parsed.system) {
        match part {
            ir::Part::Text {
                text,
                signature: None,
            } => instructions.push(text),
            _ => {
                return Err(ProxyError::InvalidRequest(
                    "Unsupported official instructions".into(),
                ))
            }
        }
    }
    let mut messages = Vec::new();
    for message in std::mem::take(&mut parsed.messages) {
        if matches!(message.role.as_str(), "system" | "developer") {
            for part in message.parts {
                match part {
                    ir::Part::Text {
                        text,
                        signature: None,
                    } => instructions.push(text),
                    _ => {
                        return Err(ProxyError::InvalidRequest(
                            "Unsupported official instructions".into(),
                        ))
                    }
                }
            }
        } else {
            messages.push(message);
        }
    }
    parsed.messages = messages;
    let mut body = ir::build::request(&parsed, Protocol::Responses)?;
    body["instructions"] = json!(instructions.join("\n\n"));
    // Match the official transport contract already used by the OAuth adapter.
    body["store"] = json!(false);
    body["stream"] = json!(true);
    if let Some(fields) = body.as_object_mut() {
        // Anthropic metadata is not accepted by the official Codex endpoint.
        for field in ["max_output_tokens", "temperature", "top_p", "metadata"] {
            fields.remove(field);
        }
        fields.entry("tools").or_insert_with(|| json!([]));
        fields
            .entry("parallel_tool_calls")
            .or_insert_with(|| json!(true));
    }
    let mut include = body["include"].as_array().cloned().unwrap_or_default();
    if !include
        .iter()
        .any(|value| value == "reasoning.encrypted_content")
    {
        include.push(json!("reasoning.encrypted_content"));
    }
    body["include"] = json!(include);
    history.restore(&mut body, target);
    serde_json::to_vec(&body)
        .map_err(|_| ProxyError::InvalidRequest("Invalid official request".into()))
}
