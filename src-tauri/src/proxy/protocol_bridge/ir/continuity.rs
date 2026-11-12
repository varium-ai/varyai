//! Provider/session-scoped replay of Gemini signatures and server-tool state lost by clients.
use super::*;
use crate::proxy::providers::gemini_shadow::{GeminiShadowStore, GeminiToolCallMeta};
use serde_json::json;
use std::{
    borrow::Cow,
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
};

struct Call<'a> {
    id: &'a str,
    name: Cow<'a, str>,
    arguments: Cow<'a, Value>,
    signature: Option<&'a str>,
}
fn call(part: &Part) -> Option<Call<'_>> {
    let (id, name, arguments, signature) = match part {
        Part::ToolCall {
            id,
            name,
            arguments,
            signature,
        } => (
            id,
            Cow::Borrowed(name.as_str()),
            Cow::Borrowed(arguments),
            signature.as_deref(),
        ),
        Part::NamespacedToolCall {
            id,
            namespace,
            name,
            arguments,
            signature,
        } => (
            id,
            Cow::Owned(
                crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(
                    namespace, name,
                ),
            ),
            Cow::Borrowed(arguments),
            signature.as_deref(),
        ),
        Part::CustomToolCall {
            id,
            name,
            input,
            namespace,
        } => (
            id,
            namespace
                .as_ref()
                .map(|ns| {
                    Cow::Owned(
                        crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(
                            ns, name,
                        ),
                    )
                })
                .unwrap_or(Cow::Borrowed(name.as_str())),
            Cow::Owned(json!({"input":input})),
            None,
        ),
        _ => return None,
    };
    Some(Call {
        id,
        name,
        arguments,
        signature,
    })
}

/// Called only for a reply decoded from Gemini. Auxiliary state never rejects forwarding.
pub(crate) fn record(
    reply: &Reply,
    store: &GeminiShadowStore,
    provider: &str,
    session: Option<&str>,
) {
    let Some(session) = session.filter(|value| !value.is_empty()) else {
        return;
    };
    if provider.is_empty() {
        return;
    }
    let Ok(body) = super::reply::render(Protocol::Gemini, reply) else {
        return;
    };
    let Some(content) = body
        .pointer("/candidates/0/content")
        .filter(|v| v.is_object())
    else {
        return;
    };
    let tool_calls = reply
        .parts
        .iter()
        .filter_map(call)
        .map(|call| GeminiToolCallMeta {
            id: (!call.id.is_empty()).then(|| call.id.to_owned()),
            name: call.name.into_owned(),
            args: call.arguments.into_owned(),
            thought_signature: call.signature.map(str::to_owned),
        })
        .collect();
    store.record_assistant_turn(provider, session, content.clone(), tool_calls);
}

fn shape_hash<'a>(parts: impl IntoIterator<Item = &'a Part>) -> u64 {
    let mut fingerprints = Vec::new();
    for part in parts {
        let mut hash = DefaultHasher::new();
        if let Some(call) = call(part) {
            "call".hash(&mut hash);
            call.id.hash(&mut hash);
            call.name.hash(&mut hash);
            // Equality below also checks the structured arguments; this hash is only an index.
        } else {
            match part {
                Part::Text { text, .. } => {
                    "text".hash(&mut hash);
                    text.hash(&mut hash);
                }
                Part::Thinking { text, .. } => {
                    "thinking".hash(&mut hash);
                    text.hash(&mut hash);
                }
                other => std::mem::discriminant(other).hash(&mut hash),
            }
        }
        fingerprints.push(hash.finish());
    }
    fingerprints.sort_unstable();
    let mut hash = DefaultHasher::new();
    fingerprints.hash(&mut hash);
    hash.finish()
}
fn same_part(left: &Part, right: &Part) -> bool {
    if let (Some(a), Some(b)) = (call(left), call(right)) {
        return a.id == b.id && a.name == b.name && a.arguments == b.arguments;
    }
    match (left, right) {
        (Part::Text { text: a, .. }, Part::Text { text: b, .. }) => a == b,
        (Part::Thinking { text: a, .. }, Part::Thinking { text: b, .. }) => a == b,
        (
            Part::Search {
                query: a,
                hits: ah,
                gemini: an,
            },
            Part::Search {
                query: b,
                hits: bh,
                gemini: bn,
            },
        ) => a == b && ah == bh && (an.is_none() || bn.is_none() || an == bn),
        _ => left == right,
    }
}
fn match_parts(current: &[&Part], saved: &[Part]) -> Option<Vec<usize>> {
    if current.len() != saved.len() {
        return None;
    }
    let mut used = std::collections::HashSet::new();
    let mut mapping = Vec::with_capacity(current.len());
    for part in current {
        let mut matches = saved
            .iter()
            .enumerate()
            .filter(|(index, saved)| !used.contains(index) && same_part(part, saved));
        let (index, _) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        used.insert(index);
        mapping.push(index);
    }
    Some(mapping)
}
fn group_parts(request: &Request, range: (usize, usize)) -> Vec<&Part> {
    request.messages[range.0..range.1]
        .iter()
        .flat_map(|m| &m.parts)
        .collect()
}
fn fill_call_signature(part: &mut Part, signature: &str) {
    match part {
        Part::ToolCall {
            signature: slot, ..
        }
        | Part::NamespacedToolCall {
            signature: slot, ..
        } => {
            if slot.is_none() {
                *slot = Some(signature.to_owned());
            }
        }
        Part::CustomToolCall { .. } => {
            if let Some(call) = call(part) {
                let replacement = Part::ToolCall {
                    id: call.id.to_owned(),
                    name: call.name.into_owned(),
                    arguments: call.arguments.into_owned(),
                    signature: Some(signature.to_owned()),
                };
                *part = replacement;
            }
        }
        _ => {}
    }
}

/// Restore absent native state only within its provider/session scope and matching history.
pub(crate) fn restore(
    request: &mut Request,
    store: &GeminiShadowStore,
    provider: &str,
    session: Option<&str>,
) {
    let Some(session) = session.filter(|value| !value.is_empty()) else {
        return;
    };
    if provider.is_empty() {
        return;
    }
    let Some(snapshot) = store.get_session(provider, session) else {
        return;
    };
    let mut request_ids = HashMap::<String, usize>::new();
    let mut groups = Vec::new();
    let mut start = None;
    for (index, message) in request.messages.iter().enumerate() {
        if message.role == "assistant" {
            if start.is_none() {
                start = Some(index);
            }
        } else if let Some(start) = start.take() {
            groups.push((start, index));
        }
        for call in message.parts.iter().filter_map(call) {
            if message.role == "assistant" {
                *request_ids.entry(call.id.to_owned()).or_default() += 1;
            }
        }
    }
    if let Some(start) = start {
        groups.push((start, request.messages.len()));
    }
    let mut request_shapes = HashMap::<u64, Vec<(usize, usize)>>::new();
    for range in &groups {
        request_shapes
            .entry(shape_hash(group_parts(request, *range)))
            .or_default()
            .push(*range);
    }
    let stored_parts = snapshot
        .turns
        .iter()
        .filter_map(|turn| {
            super::reply::parse(
                Protocol::Gemini,
                &json!({"candidates":[{"content":turn.assistant_content}]}),
            )
            .ok()
            .map(|reply| reply.parts)
        })
        .collect::<Vec<_>>();
    for range in groups {
        let current = group_parts(request, range);
        let shape = shape_hash(current.iter().copied());
        let mut turns = stored_parts.iter().filter_map(|parts| {
            if shape_hash(parts) != shape {
                return None;
            }
            match_parts(&current, parts).map(|mapping| (parts, mapping))
        });
        let first = turns.next();
        let anchor = first.filter(|(parts, _)| {
            turns.next().is_none()
                && request_shapes.get(&shape).is_some_and(|ranges| {
                    ranges
                        .iter()
                        .filter(|other| {
                            match_parts(&group_parts(request, **other), parts).is_some()
                        })
                        .take(2)
                        .count()
                        == 1
                })
        });
        for (position, part) in request.messages[range.0..range.1]
            .iter_mut()
            .flat_map(|m| &mut m.parts)
            .enumerate()
        {
            if let Some(call) = call(part).filter(|call| {
                call.signature.is_none()
                    && !call.id.is_empty()
                    && request_ids.get(call.id) == Some(&1)
            }) {
                let mut matches = snapshot
                    .turns
                    .iter()
                    .flat_map(|turn| &turn.tool_calls)
                    .filter(|saved| {
                        saved.id.as_deref() == Some(call.id)
                            && saved.name == call.name
                            && saved.args == *call.arguments
                    });
                let first = matches.next();
                if matches.next().is_none() {
                    if let Some(signature) = first
                        .and_then(|saved| saved.thought_signature.as_deref())
                        .filter(|value| !value.is_empty())
                    {
                        fill_call_signature(part, signature);
                    }
                }
            }
            if let Some(saved) = anchor.as_ref().and_then(|(parts, mapping)| {
                mapping.get(position).and_then(|index| parts.get(*index))
            }) {
                match (part, saved) {
                    (
                        Part::Search { gemini: slot, .. },
                        Part::Search {
                            gemini: Some(native),
                            ..
                        },
                    ) if slot.is_none() => {
                        *slot = Some(native.clone());
                    }
                    (
                        Part::Text {
                            signature: slot, ..
                        },
                        Part::Text {
                            signature: Some(signature),
                            ..
                        },
                    ) if slot.is_none() => *slot = Some(signature.clone()),
                    (
                        Part::Thinking {
                            signature: slot,
                            source,
                            sealed,
                            ..
                        },
                        Part::Thinking {
                            signature: Some(signature),
                            source: Protocol::Gemini,
                            ..
                        },
                    ) if slot.is_none()
                        && sealed
                            .as_ref()
                            .is_none_or(|value| value.get("encrypted_content").is_none()) =>
                    {
                        *slot = Some(signature.clone());
                        *source = Protocol::Gemini;
                        *sealed = None;
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn native_reply(id: &str, signature: &str) -> Reply {
        super::super::reply::parse(Protocol::Gemini,&json!({"responseId":id,"modelVersion":"gemini-3.8-flash","candidates":[{"content":{"role":"model","parts":[
            {"text":"Synthetic thought","thought":true,"thoughtSignature":"synthetic-thinking-signature"},
            {"text":"Synthetic answer","thoughtSignature":"synthetic-text-signature"},
            {"functionCall":{"id":"synthetic-call","name":"lookup","args":{"agent":"synthetic"}},"thoughtSignature":signature}
        ]},"finishReason":"STOP"}]})).unwrap()
    }
    fn through_client(reply: &Reply, client: Protocol) -> Request {
        let wire = super::super::reply::render(client, reply).unwrap();
        let body = match client {
            Protocol::Chat => {
                json!({"model":"gemini-3.8-flash","messages":[wire["choices"][0]["message"].clone()]})
            }
            Protocol::Responses => json!({"model":"gemini-3.8-flash","input":wire["output"]}),
            _ => unreachable!(),
        };
        let mut parsed = super::super::parse::request(client, &body, "/v1/responses").unwrap();
        // Some clients omit even a supported optional signature carrier when replaying history.
        for message in &mut parsed.messages {
            for part in &mut message.parts {
                match part {
                    Part::ToolCall { signature, .. }
                    | Part::NamespacedToolCall { signature, .. }
                    | Part::Text { signature, .. }
                    | Part::Thinking { signature, .. } => *signature = None,
                    _ => {}
                }
            }
        }
        parsed
    }
    fn call_signature(request: &Request) -> Option<&str> {
        request
            .messages
            .iter()
            .flat_map(|m| &m.parts)
            .find_map(|part| match part {
                Part::ToolCall { signature, .. } => signature.as_deref(),
                _ => None,
            })
    }
    #[test]
    fn gemini_combined_search_state_is_restored_only_to_exact_scoped_history() {
        let store = GeminiShadowStore::with_limits(2, 4);
        let native = super::super::reply::gemini_search_fixture();
        let reply = super::super::reply::parse(Protocol::Gemini, &native).unwrap();
        record(
            &reply,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        for (provider, session, changed) in [
            ("synthetic-provider", "synthetic-session", false),
            ("other-provider", "synthetic-session", false),
            ("synthetic-provider", "other-session", false),
            ("synthetic-provider", "synthetic-session", true),
        ] {
            let mut request = through_client(&reply, Protocol::Responses);
            if changed {
                for part in request.messages.iter_mut().flat_map(|m| &mut m.parts) {
                    if let Part::Search { query, .. } = part {
                        query.push_str(" changed");
                    }
                }
            }
            restore(&mut request, &store, provider, Some(session));
            let body = super::super::build::request(&request, Protocol::Gemini).unwrap();
            if provider == "synthetic-provider" && session == "synthetic-session" && !changed {
                assert_eq!(
                    body["contents"][0]["parts"],
                    native["candidates"][0]["content"]["parts"]
                );
            } else {
                assert!(!body.to_string().contains("synthetic-call-signature"));
                assert!(!body.to_string().contains("toolResponse"));
            }
        }
    }

    #[test]
    fn gemini_call_signature_survives_chat_and_responses_history_replay() {
        for client in [Protocol::Chat, Protocol::Responses] {
            let store = GeminiShadowStore::with_limits(2, 4);
            let reply = native_reply("synthetic-response", "synthetic-call-signature");
            record(
                &reply,
                &store,
                "synthetic-provider",
                Some("synthetic-session"),
            );
            let mut request = through_client(&reply, client);
            restore(
                &mut request,
                &store,
                "synthetic-provider",
                Some("synthetic-session"),
            );
            assert_eq!(call_signature(&request), Some("synthetic-call-signature"));
            let native = super::super::build::request(&request, Protocol::Gemini).unwrap();
            assert!(native["contents"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|m| m["parts"].as_array().unwrap())
                .any(|p| p["functionCall"]["id"] == "synthetic-call"
                    && p["thoughtSignature"] == "synthetic-call-signature"));
        }
    }
    #[test]
    fn complete_assistant_turns_restore_thinking_and_text_across_client_packing() {
        for client in [Protocol::Chat, Protocol::Responses] {
            let store = GeminiShadowStore::with_limits(2, 4);
            let reply = native_reply("synthetic-response", "synthetic-call-signature");
            record(
                &reply,
                &store,
                "synthetic-provider",
                Some("synthetic-session"),
            );
            let mut request = through_client(&reply, client);
            restore(
                &mut request,
                &store,
                "synthetic-provider",
                Some("synthetic-session"),
            );
            let parts = request
                .messages
                .iter()
                .flat_map(|message| &message.parts)
                .collect::<Vec<_>>();
            assert!(parts.iter().any(|part|matches!(part,Part::Text{signature:Some(value),..} if value=="synthetic-text-signature")));
            assert!(parts.iter().any(|part|matches!(part,Part::Thinking{signature:Some(value),source:Protocol::Gemini,..} if value=="synthetic-thinking-signature")));
        }
    }
    #[test]
    fn namespace_and_custom_context_keep_the_signed_upstream_call_identity() {
        for custom in [false, true] {
            let store = GeminiShadowStore::with_limits(2, 4);
            let input = "Synthetic raw input\n  preserve spaces";
            let name = if custom { "patch" } else { "ns__lookup" };
            let args = if custom {
                json!({"input":input})
            } else {
                json!({"agent":"synthetic"})
            };
            let source = Reply {
                id: "synthetic-context".into(),
                model: "gemini-3.8-flash".into(),
                parts: vec![Part::ToolCall {
                    id: "synthetic-context-call".into(),
                    name: name.into(),
                    arguments: args.clone(),
                    signature: Some("synthetic-context-signature".into()),
                }],
                stop: "tool".into(),
                usage: Usage::default(),
            };
            record(
                &source,
                &store,
                "synthetic-provider",
                Some("synthetic-session"),
            );
            let mut request = through_client(&source, Protocol::Responses);
            request.messages[0].parts[0] = if custom {
                Part::CustomToolCall {
                    id: "synthetic-context-call".into(),
                    name: "patch".into(),
                    input: input.into(),
                    namespace: None,
                }
            } else {
                Part::NamespacedToolCall {
                    id: "synthetic-context-call".into(),
                    namespace: "ns".into(),
                    name: "lookup".into(),
                    arguments: args,
                    signature: None,
                }
            };
            restore(
                &mut request,
                &store,
                "synthetic-provider",
                Some("synthetic-session"),
            );
            let built = super::super::build::request(&request, Protocol::Gemini).unwrap();
            assert_eq!(
                built["contents"][0]["parts"][0]["functionCall"]["name"],
                name
            );
            assert_eq!(
                built["contents"][0]["parts"][0]["thoughtSignature"],
                "synthetic-context-signature"
            );
        }
    }
    #[test]
    fn mismatched_call_identity_or_scope_never_restores_a_signature() {
        let store = GeminiShadowStore::with_limits(2, 4);
        let reply = native_reply("synthetic-response", "synthetic-call-signature");
        record(
            &reply,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        for mismatch in ["id", "name", "arguments", "provider", "session"] {
            let mut request = through_client(&reply, Protocol::Chat);
            for part in request.messages.iter_mut().flat_map(|m| &mut m.parts) {
                if let Part::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } = part
                {
                    match mismatch {
                        "id" => *id = "foreign-id".into(),
                        "name" => *name = "foreign-name".into(),
                        "arguments" => *arguments = json!({"agent":"foreign"}),
                        _ => {}
                    }
                }
            }
            restore(
                &mut request,
                &store,
                if mismatch == "provider" {
                    "foreign-provider"
                } else {
                    "synthetic-provider"
                },
                Some(if mismatch == "session" {
                    "foreign-session"
                } else {
                    "synthetic-session"
                }),
            );
            assert_eq!(call_signature(&request), None);
        }
    }
    #[test]
    fn ambiguous_or_duplicate_call_ids_do_not_choose_a_cached_signature() {
        let store = GeminiShadowStore::with_limits(2, 4);
        let first = native_reply("synthetic-first", "synthetic-first-signature");
        let second = native_reply("synthetic-second", "synthetic-second-signature");
        record(
            &first,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        record(
            &second,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        let mut request = through_client(&second, Protocol::Chat);
        restore(
            &mut request,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        assert_eq!(call_signature(&request), None);
    }
    #[test]
    fn missing_sessions_do_not_create_or_reuse_memory_scope() {
        let store = GeminiShadowStore::with_limits(2, 4);
        let reply = native_reply("synthetic-response", "synthetic-call-signature");
        record(&reply, &store, "synthetic-provider", None);
        record(&reply, &store, "synthetic-provider", Some(""));
        assert_eq!(store.session_count(), 0);
        record(
            &reply,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        let mut request = through_client(&reply, Protocol::Chat);
        restore(&mut request, &store, "synthetic-provider", None);
        assert_eq!(call_signature(&request), None);
    }
    #[test]
    fn existing_caller_signature_is_never_replaced() {
        let store = GeminiShadowStore::with_limits(2, 4);
        let reply = native_reply("synthetic-response", "synthetic-call-signature");
        record(
            &reply,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        let mut request = through_client(&reply, Protocol::Chat);
        for part in request.messages.iter_mut().flat_map(|m| &mut m.parts) {
            if let Part::ToolCall { signature, .. } = part {
                *signature = Some("synthetic-caller-signature".into());
            }
        }
        restore(
            &mut request,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        assert_eq!(call_signature(&request), Some("synthetic-caller-signature"));
    }
    #[test]
    fn text_signatures_require_a_unique_whole_turn_not_a_matching_phrase() {
        let store = GeminiShadowStore::with_limits(2, 4);
        let signed = Reply {
            id: "synthetic-text".into(),
            model: "gemini-3.8-flash".into(),
            parts: vec![Part::Text {
                text: "Synthetic phrase".into(),
                signature: Some("synthetic-text-signature".into()),
            }],
            stop: "stop".into(),
            usage: Usage::default(),
        };
        record(
            &signed,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        let mut unique = through_client(&signed, Protocol::Chat);
        restore(
            &mut unique,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        assert!(
            matches!(&unique.messages[0].parts[0],Part::Text{signature:Some(s),..} if s=="synthetic-text-signature")
        );
        let mut duplicate = through_client(&signed, Protocol::Chat);
        duplicate.messages.push(duplicate.messages[0].clone());
        restore(
            &mut duplicate,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        assert!(duplicate.messages.iter().all(|m| matches!(
            &m.parts[0],
            Part::Text {
                signature: None,
                ..
            }
        )));
        let mut modified = through_client(&signed, Protocol::Chat);
        modified.messages[0].parts.push(Part::Text {
            text: "Synthetic extra text".into(),
            signature: None,
        });
        restore(
            &mut modified,
            &store,
            "synthetic-provider",
            Some("synthetic-session"),
        );
        assert!(matches!(
            &modified.messages[0].parts[0],
            Part::Text {
                signature: None,
                ..
            }
        ));
    }
}
