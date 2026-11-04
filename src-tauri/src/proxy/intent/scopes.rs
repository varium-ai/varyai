//! Bounded consumer/conversation isolation; active streams retain their own lease.
use super::super::providers::{
    codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore,
};
use super::*;

const MAX_CONVERSATIONS: usize = 1024;
const CONVERSATION_TTL: Duration = Duration::from_secs(3600);
#[derive(Default)]
pub(super) struct Conversations(std::sync::Mutex<Vec<Entry>>);
struct Entry {
    consumer: String,
    identity: Arc<tokio::sync::Mutex<Bindings>>,
    name: Option<String>,
    touched: Instant,
    state: Arc<Conversation>,
}
#[derive(Default)]
pub(super) struct Conversation {
    pub bindings: Arc<tokio::sync::Mutex<Bindings>>,
    pub history: Arc<CodexChatHistoryStore>,
    pub shadow: Arc<GeminiShadowStore>,
}

pub(super) struct Durable {
    store: Arc<continuation::Store>,
    scope: String,
    id: String,
    targets: Vec<String>,
}
impl Durable {
    pub fn persist(&self, bindings: &Bindings) {
        let map = |values: &HashMap<String, usize>| {
            values
                .iter()
                .map(|(key, index)| (key.clone(), self.targets.get(*index).cloned()))
                .collect()
        };
        self.store.record(
            &self.scope,
            &self.id,
            continuation::Snapshot {
                targets: self.targets.clone(),
                responses: map(&bindings.responses),
                calls: map(&bindings.calls),
                sessions: map(&bindings.sessions),
                opaque: map(&bindings.opaque),
                items: map(&bindings.items),
            },
        );
    }
}

fn references_with(body: &Value, contains: impl Fn(&str, &str) -> bool) -> bool {
    if body["previous_response_id"]
        .as_str()
        .is_some_and(|id| contains("response", id))
    {
        return true;
    }
    body["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["call_id"]
                .as_str()
                .is_some_and(|id| contains("call", id))
                || item["id"].as_str().is_some_and(|id| contains("item", id))
                || item["encrypted_content"]
                    .as_str()
                    .is_some_and(|text| contains("opaque", text))
        })
    })
}
fn references(snapshot: &continuation::Snapshot, body: &Value) -> bool {
    references_with(body, |kind, id| {
        let map = match kind {
            "response" => &snapshot.responses,
            "call" => &snapshot.calls,
            "item" => &snapshot.items,
            _ => &snapshot.opaque,
        };
        map.contains_key(&continuation::hash(kind, id))
    })
}

impl Conversations {
    pub fn acquire_durable(
        &self,
        consumer: &str,
        session: &Session,
        name: Option<&str>,
        body: &Value,
        store: Option<Arc<continuation::Store>>,
    ) -> Result<Arc<Conversation>, &'static str> {
        self.acquire_durable_with_history(consumer, session, name, body, store, false)
    }

    pub fn acquire_durable_with_history(
        &self,
        consumer: &str,
        session: &Session,
        name: Option<&str>,
        body: &Value,
        store: Option<Arc<continuation::Store>>,
        portable_history: bool,
    ) -> Result<Arc<Conversation>, &'static str> {
        let Some(store) = store else {
            if !portable_history {
                return self.acquire(consumer, &session.bindings, name, body);
            }
            return self.acquire_with_history(
                consumer,
                &session.bindings,
                name,
                body,
                portable_history,
            );
        };
        let Some(targets) = session
            .targets
            .iter()
            .map(Target::continuation_destination)
            .collect::<Option<Vec<_>>>()
        else {
            return self.acquire_with_history(
                consumer,
                &session.bindings,
                name,
                body,
                portable_history,
            );
        };
        if targets
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != targets.len()
        {
            return Err("routing_ambiguous_continuation");
        }
        // Keep the scope independent of target ordering, but validate the complete
        // destination set before restoring any individual binding below.
        let scope = continuation::hash(
            "scope",
            &serde_json::to_string(&(consumer, &session.group.id, session.group.alias()))
                .map_err(|_| "routing_state_unavailable")?,
        );
        let mut entries = self.0.lock().map_err(|_| "routing_state_unavailable")?;
        entries
            .retain(|e| e.touched.elapsed() < CONVERSATION_TTL || Arc::strong_count(&e.state) > 1);
        let matches =
            |e: &Entry| e.consumer == consumer && Arc::ptr_eq(&e.identity, &session.bindings);
        let name = name.map(|name| continuation::hash("conversation", name));
        let mut expected = targets.clone();
        expected.sort();
        for (id, snapshot) in store.scope(&scope) {
            if name.as_ref().is_some_and(|name| name != &id) {
                continue;
            }
            if name.is_none() && !references(&snapshot, body) {
                continue;
            }
            if entries
                .iter()
                .any(|e| matches(e) && e.name.as_ref() == Some(&id))
            {
                continue;
            }
            let mut saved = snapshot.targets.clone();
            saved.sort();
            if saved != expected || saved.is_empty() {
                return Err("routing_unknown_continuation");
            }
            if entries.len() >= MAX_CONVERSATIONS {
                return Err("routing_session_capacity");
            }
            let remap = |map: std::collections::BTreeMap<String, Option<String>>| -> Result<HashMap<String, usize>, &'static str> {
                map.into_iter().map(|(key, target)| {
                    let index = match target {
                        None => usize::MAX,
                        Some(target) => targets.iter().position(|value| value == &target)
                            .ok_or("routing_unknown_continuation")?,
                    };
                    Ok((key, index))
                }).collect()
            };
            let bindings = Bindings {
                responses: remap(snapshot.responses)?,
                calls: remap(snapshot.calls)?,
                sessions: remap(snapshot.sessions)?,
                opaque: remap(snapshot.opaque)?,
                items: remap(snapshot.items)?,
                durable: Some(Durable {
                    store: store.clone(),
                    scope: scope.clone(),
                    id: id.clone(),
                    targets: targets.clone(),
                }),
                ..Default::default()
            };
            entries.push(Entry {
                consumer: consumer.into(),
                identity: session.bindings.clone(),
                name: Some(id),
                touched: Instant::now(),
                state: Arc::new(Conversation {
                    bindings: Arc::new(tokio::sync::Mutex::new(bindings)),
                    ..Default::default()
                }),
            });
        }
        let state = Self::select(
            &mut entries,
            consumer,
            &session.bindings,
            name.as_deref(),
            body,
            portable_history,
        )?;
        if let Ok(mut bindings) = state.bindings.try_lock() {
            if bindings.durable.is_none() {
                let id = name.unwrap_or_else(|| {
                    continuation::hash("unnamed", &uuid::Uuid::new_v4().to_string())
                });
                if let Some(entry) = entries
                    .iter_mut()
                    .find(|entry| Arc::ptr_eq(&entry.state, &state))
                {
                    entry.name = Some(id.clone());
                }
                bindings.durable = Some(Durable {
                    store,
                    scope,
                    id,
                    targets,
                });
            }
        }
        Ok(state)
    }

    pub fn acquire(
        &self,
        consumer: &str,
        identity: &Arc<tokio::sync::Mutex<Bindings>>,
        name: Option<&str>,
        body: &Value,
    ) -> Result<Arc<Conversation>, &'static str> {
        self.acquire_with_history(consumer, identity, name, body, false)
    }

    fn acquire_with_history(
        &self,
        consumer: &str,
        identity: &Arc<tokio::sync::Mutex<Bindings>>,
        name: Option<&str>,
        body: &Value,
        portable_history: bool,
    ) -> Result<Arc<Conversation>, &'static str> {
        let mut entries = self.0.lock().map_err(|_| "routing_state_unavailable")?;
        entries
            .retain(|e| e.touched.elapsed() < CONVERSATION_TTL || Arc::strong_count(&e.state) > 1);
        let name = name.map(|name| continuation::hash("conversation", name));
        Self::select(
            &mut entries,
            consumer,
            identity,
            name.as_deref(),
            body,
            portable_history,
        )
    }

    fn select(
        entries: &mut Vec<Entry>,
        consumer: &str,
        identity: &Arc<tokio::sync::Mutex<Bindings>>,
        name: Option<&str>,
        body: &Value,
        portable_history: bool,
    ) -> Result<Arc<Conversation>, &'static str> {
        let matches = |e: &Entry| e.consumer == consumer && Arc::ptr_eq(&e.identity, identity);
        let continuation = body
            .get("previous_response_id")
            .filter(|v| !v.is_null())
            .is_some()
            || body
                .get("input")
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items.iter().any(|i| {
                        matches!(
                            i["type"].as_str(),
                            Some(
                                "function_call"
                                    | "function_call_output"
                                    | "reasoning"
                                    | "compaction"
                                    | "compaction_summary"
                                    | "item_reference"
                            )
                        )
                    })
                });
        let selected = if let Some(name) = name {
            entries
                .iter()
                .position(|e| matches(e) && e.name.as_deref() == Some(name))
        } else if continuation {
            let mut found = None;
            for (index, entry) in entries.iter().enumerate().filter(|(_, e)| matches(e)) {
                if let Ok(bindings) = entry.state.bindings.try_lock() {
                    let evidence = references_with(body, |kind, id| {
                        let map = match kind {
                            "response" => &bindings.responses,
                            "call" => &bindings.calls,
                            "item" => &bindings.items,
                            _ => &bindings.opaque,
                        };
                        map.contains_key(&continuation::hash(kind, id))
                    });
                    match bindings.resolve(body, None) {
                        Ok(Some(_)) => {
                            if found.is_some() {
                                return Err("routing_ambiguous_continuation");
                            }
                            found = Some(index);
                        }
                        Err(()) if evidence => {
                            return Err("routing_ambiguous_continuation");
                        }
                        _ => {}
                    }
                } else {
                    return Err("routing_session_busy");
                }
            }
            found
        } else {
            None
        };
        if let Some(index) = selected {
            entries[index].touched = Instant::now();
            return Ok(entries[index].state.clone());
        }
        // Full ownership discovery and durable destination checks run first.
        // Only a self-contained transcript with no known owner may start fresh.
        if continuation && name.is_none() && !portable_history {
            return Err("routing_unknown_continuation");
        }
        if entries.len() >= MAX_CONVERSATIONS {
            return Err("routing_session_capacity");
        }
        let state = Arc::new(Conversation::default());
        entries.push(Entry {
            consumer: consumer.into(),
            identity: identity.clone(),
            name: name.map(str::to_owned),
            touched: Instant::now(),
            state: state.clone(),
        });
        Ok(state)
    }
}
