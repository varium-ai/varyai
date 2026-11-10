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
    pub bindings: std::sync::Mutex<Bindings>,
    pub history: Arc<CodexChatHistoryStore>,
    pub shadow: Arc<GeminiShadowStore>,
    // The scope identity freezes the exact ordered destination set.
    pub official: Arc<super::official_bridge::History>,
}

impl Conversation {
    // Like Magpie's sticks mutex, this guard is only used for in-memory
    // lookups/updates. No guard may span a network await or a stream yield.
    pub fn update<T>(&self, f: impl FnOnce(&mut Bindings) -> T) -> Result<T, &'static str> {
        let mut bindings = self
            .bindings
            .lock()
            .map_err(|_| "routing_state_unavailable")?;
        Ok(f(&mut bindings))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Name<'a> {
    pub key: Option<&'a str>,
    // A derived opening can still recover an observed continuation whose
    // original opening was omitted by its client. Explicit names stay isolated.
    pub derived: bool,
}
impl<'a> From<Option<&'a str>> for Name<'a> {
    fn from(key: Option<&'a str>) -> Self {
        Self {
            key,
            derived: false,
        }
    }
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
            if length::visible_reasoning(item) {
                return false;
            }
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
fn references(snapshot: &continuation::Snapshot, body: &Value, consumer: &str) -> bool {
    let projection = (consumer == "agent:trae").then(|| {
        trae::projection(body, |id| {
            snapshot.calls.contains_key(&continuation::hash("call", id))
        })
    });
    references_with(projection.as_ref().unwrap_or(body), |kind, id| {
        let map = match kind {
            "response" => &snapshot.responses,
            "call" => &snapshot.calls,
            "item" => &snapshot.items,
            _ => &snapshot.opaque,
        };
        map.contains_key(&continuation::hash(kind, id))
    })
}

#[derive(Clone, Copy, Default)]
pub(super) enum HistoryPolicy {
    #[default]
    Strict,
    Portable,
    ResponsesReplay,
}
impl HistoryPolicy {
    fn allows_fresh(self) -> bool {
        !matches!(self, Self::Strict)
    }
}

impl Conversations {
    #[cfg(test)]
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

    #[cfg(test)]
    pub fn acquire_durable_with_history(
        &self,
        consumer: &str,
        session: &Session,
        name: Option<&str>,
        body: &Value,
        store: Option<Arc<continuation::Store>>,
        portable_history: bool,
    ) -> Result<Arc<Conversation>, &'static str> {
        self.acquire_durable_with_policy(
            consumer,
            session,
            name,
            body,
            store,
            if portable_history {
                HistoryPolicy::Portable
            } else {
                HistoryPolicy::Strict
            },
        )
    }

    #[cfg(test)]
    pub fn acquire_durable_with_policy(
        &self,
        consumer: &str,
        session: &Session,
        name: Option<&str>,
        body: &Value,
        store: Option<Arc<continuation::Store>>,
        policy: HistoryPolicy,
    ) -> Result<Arc<Conversation>, &'static str> {
        self.acquire_named(consumer, session, name.into(), body, store, policy)
    }

    pub fn acquire_named(
        &self,
        consumer: &str,
        session: &Session,
        name: Name<'_>,
        body: &Value,
        store: Option<Arc<continuation::Store>>,
        policy: HistoryPolicy,
    ) -> Result<Arc<Conversation>, &'static str> {
        let Some(store) = store else {
            return self.acquire_with_history(consumer, &session.bindings, name, body, policy);
        };
        let Some(targets) = session
            .targets
            .iter()
            .map(Target::continuation_destination)
            .collect::<Option<Vec<_>>>()
        else {
            return self.acquire_with_history(consumer, &session.bindings, name, body, policy);
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
        let hashed_name = name
            .key
            .map(|name| continuation::hash("conversation", name));
        let name = Name {
            key: hashed_name.as_deref(),
            derived: name.derived,
        };
        let mut expected = targets.clone();
        expected.sort();
        for (id, snapshot) in store.scope(&scope) {
            if !name.derived && name.key.is_some_and(|name| name != id) {
                continue;
            }
            if name.key != Some(id.as_str()) && !references(&snapshot, body, consumer) {
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
            if saved.is_empty() && matches!(policy, HistoryPolicy::ResponsesReplay) {
                // A stale affinity snapshot cannot veto a new Codex request.
                continue;
            }
            if (saved != expected || saved.is_empty())
                && !matches!(policy, HistoryPolicy::ResponsesReplay)
            {
                return Err("routing_unknown_continuation");
            }
            if entries.len() >= MAX_CONVERSATIONS {
                return Err("routing_session_capacity");
            }
            let remap = |map: std::collections::BTreeMap<String, Option<String>>| -> Result<HashMap<String, usize>, &'static str> {
                let mut restored = HashMap::new();
                for (key, target) in map {
                    let index = match target {
                        None => usize::MAX,
                        Some(target) => match targets.iter().position(|value| value == &target) {
                            Some(index) => index,
                            None if matches!(policy, HistoryPolicy::ResponsesReplay) => continue,
                            None => return Err("routing_unknown_continuation"),
                        },
                    };
                    restored.insert(key, index);
                }
                Ok(restored)
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
                    bindings: std::sync::Mutex::new(bindings),
                    ..Default::default()
                }),
            });
        }
        let state = Self::select(
            &mut entries,
            consumer,
            &session.bindings,
            name,
            body,
            policy,
        )?;
        {
            let mut bindings = state
                .bindings
                .lock()
                .map_err(|_| "routing_state_unavailable")?;
            if bindings.durable.is_none() {
                let id = name.key.map(str::to_owned).unwrap_or_else(|| {
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

    #[cfg(test)]
    pub fn acquire(
        &self,
        consumer: &str,
        identity: &Arc<tokio::sync::Mutex<Bindings>>,
        name: Option<&str>,
        body: &Value,
    ) -> Result<Arc<Conversation>, &'static str> {
        self.acquire_with_history(consumer, identity, name.into(), body, HistoryPolicy::Strict)
    }

    fn acquire_with_history(
        &self,
        consumer: &str,
        identity: &Arc<tokio::sync::Mutex<Bindings>>,
        name: Name<'_>,
        body: &Value,
        policy: HistoryPolicy,
    ) -> Result<Arc<Conversation>, &'static str> {
        let mut entries = self.0.lock().map_err(|_| "routing_state_unavailable")?;
        entries
            .retain(|e| e.touched.elapsed() < CONVERSATION_TTL || Arc::strong_count(&e.state) > 1);
        let hashed_name = name
            .key
            .map(|name| continuation::hash("conversation", name));
        let name = Name {
            key: hashed_name.as_deref(),
            derived: name.derived,
        };
        Self::select(&mut entries, consumer, identity, name, body, policy)
    }

    fn select(
        entries: &mut Vec<Entry>,
        consumer: &str,
        identity: &Arc<tokio::sync::Mutex<Bindings>>,
        name: Name<'_>,
        body: &Value,
        policy: HistoryPolicy,
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
                        !length::visible_reasoning(i)
                            && matches!(
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
        let continuation = continuation
            || (matches!(policy, HistoryPolicy::ResponsesReplay)
                && references_with(body, |_, _| true));
        let named = name.key.and_then(|key| {
            entries
                .iter()
                .position(|e| matches(e) && e.name.as_deref() == Some(key))
        });
        let selected = if named.is_some() || (name.key.is_some() && !name.derived) {
            named
        } else if continuation {
            let mut found = None;
            for (index, entry) in entries.iter().enumerate().filter(|(_, e)| matches(e)) {
                {
                    let bindings = entry
                        .state
                        .bindings
                        .lock()
                        .map_err(|_| "routing_state_unavailable")?;
                    let projection = bindings.continuation_projection(consumer, body);
                    let evidence = references_with(&projection, |kind, id| {
                        let map = match kind {
                            "response" => &bindings.responses,
                            "call" => &bindings.calls,
                            "item" => &bindings.items,
                            _ => &bindings.opaque,
                        };
                        map.contains_key(&continuation::hash(kind, id))
                    });
                    let resolution = if matches!(policy, HistoryPolicy::ResponsesReplay) {
                        bindings.resolve_replay(&projection, None)
                    } else {
                        bindings.resolve(&projection, None)
                    };
                    match resolution {
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
        // Responses replay can also begin with vendor-owned state that this
        // process has never observed; the vendor validates it on forwarding.
        if continuation && name.key.is_none() && !policy.allows_fresh() {
            return Err("routing_unknown_continuation");
        }
        if entries.len() >= MAX_CONVERSATIONS {
            return Err("routing_session_capacity");
        }
        let state = Arc::new(Conversation::default());
        entries.push(Entry {
            consumer: consumer.into(),
            identity: identity.clone(),
            name: name.key.map(str::to_owned),
            touched: Instant::now(),
            state: state.clone(),
        });
        Ok(state)
    }
}
