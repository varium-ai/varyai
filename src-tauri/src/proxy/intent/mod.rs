//! Global group execution and explicit Codex projections, isolated from provider switching.
#[cfg(test)]
mod capacity_tests;
mod checkpoint;
mod compaction;
mod continuation;
pub(super) mod length;
pub(crate) mod native;
#[cfg(test)]
mod resume_tests;
pub(crate) mod scheduling;
mod scopes;
pub(crate) mod trace;
#[cfg(test)]
mod trace_tests;
use super::protocol_bridge::Protocol;
use crate::services::intent_routing::{classifier, RoutingGroup};
use axum::{
    body::Body,
    extract::{Request, State},
    response::Response,
};
use futures::StreamExt;
use http::{header, StatusCode};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Runtime {
    pub(crate) traces: Arc<trace::Store>,
    pub(crate) connections: super::connections::Connections,
    sessions: RwLock<HashMap<String, TemporarySession>>,
    menu: RwLock<Option<Arc<Menu>>>,
    global: RwLock<Option<Arc<Menu>>>,
    global_generation: std::sync::atomic::AtomicU64,
    reserved: RwLock<std::collections::HashSet<String>>,
    conversations: scopes::Conversations,
    pub(crate) native: native::Native,
    continuations: RwLock<Option<Arc<continuation::Store>>>,
}
pub(crate) const TEMPORARY_SESSION_TTL: Duration = Duration::from_secs(3600);
struct TemporarySession {
    snapshot: Arc<Session>,
    _catalog: Option<tempfile::TempPath>,
    expiry: tokio::task::AbortHandle,
}
impl Drop for TemporarySession {
    fn drop(&mut self) {
        self.expiry.abort();
    }
}
#[derive(Clone)]
pub(crate) struct MenuModel {
    pub alias: String,
    pub label: String,
    pub description: &'static str,
    pub order: usize,
}
#[derive(Clone)]
pub(crate) struct Menu {
    pub token: String,
    pub models: HashMap<String, MenuModel>,
    pub default_model: Option<String>,
    pub omitted_entries: usize,
    pub context_settings: crate::services::intent_routing::context::Settings,
    pub groups: HashMap<String, Arc<Session>>,
}
impl Menu {
    pub(crate) fn stand_in_direct(&self, alias: &str) -> Option<&str> {
        if !alias.starts_with("auto/") || self.models.values().any(|model| model.alias == alias) {
            return None;
        }
        let session = self.groups.get(alias)?;
        if session.targets.len() != 1 || !session.group.auto_generated {
            return None;
        }
        let target = session.targets.first()?;
        self.models.values().find_map(|model| {
            if !model.alias.starts_with("vary-provider/") {
                return None;
            }
            let direct = self.groups.get(&model.alias)?.targets.first()?;
            (target.provider_id == direct.provider_id && target.model == direct.model)
                .then_some(model.alias.as_str())
        })
    }
    pub(crate) fn listed_aliases(&self) -> impl Iterator<Item = &String> {
        // Legacy internally constructed menus did not carry display metadata.
        let mut aliases = self
            .groups
            .keys()
            .filter(|alias| {
                self.models.is_empty() || self.models.values().any(|model| &model.alias == *alias)
            })
            .collect::<Vec<_>>();
        aliases.sort_by_key(|alias| (self.order_of(alias), *alias));
        aliases.into_iter()
    }
    pub(crate) fn order_of(&self, alias: &str) -> usize {
        self.models
            .values()
            .find(|model| model.alias == alias)
            .map_or(usize::MAX, |model| model.order)
    }
    fn share_scheduling(&mut self, previous: &Self) {
        let mut carried: Vec<_> = previous
            .groups
            .values()
            .flat_map(|s| s.targets.iter().cloned())
            .collect();
        for session in self.groups.values_mut() {
            for target in &mut Arc::make_mut(session).targets {
                if let Some(old) = carried.iter().find(|old| old.same_destination(target)) {
                    old.scheduling.merge_quota(&target.scheduling);
                    target.scheduling = old.scheduling.clone();
                } else {
                    if let Some(old) = carried.iter().find(|old| old.same_account(target)) {
                        old.scheduling.merge_quota(&target.scheduling);
                        target.scheduling = Arc::new(old.scheduling.for_model());
                    }
                    carried.push(target.clone());
                }
            }
        }
    }
    pub(crate) fn preserve_from(&mut self, previous: &Self) {
        self.token = previous.token.clone();
        let mut carried: Vec<_> = previous
            .groups
            .values()
            .flat_map(|s| s.targets.iter().cloned())
            .collect();
        for (alias, session) in &mut self.groups {
            let next = Arc::make_mut(session);
            if let Some(old) = previous.groups.get(alias).filter(|old| {
                old.group
                    .members
                    .iter()
                    .map(|m| (&m.id, &m.provider_id, &m.model_id, &m.effort))
                    .eq(next
                        .group
                        .members
                        .iter()
                        .map(|m| (&m.id, &m.provider_id, &m.model_id, &m.effort)))
                    && old.targets.len() == next.targets.len()
                    && old
                        .targets
                        .iter()
                        .zip(&next.targets)
                        .all(|(a, b)| a.same_destination(b))
            }) {
                next.bindings = old.bindings.clone();
            }
            for target in &mut next.targets {
                if let Some(old) = carried.iter().find(|old| old.same_destination(target)) {
                    old.scheduling.merge_quota(&target.scheduling);
                    target.scheduling = old.scheduling.clone();
                } else {
                    if let Some(old) = carried.iter().find(|old| old.same_account(target)) {
                        old.scheduling.merge_quota(&target.scheduling);
                        target.scheduling = Arc::new(old.scheduling.for_model());
                    }
                    carried.push(target.clone());
                }
            }
        }
    }
}
#[derive(Clone)]
pub(crate) struct Target {
    pub provider_id: String,
    pub model: String,
    pub fixed_effort: Option<String>,
    pub upstream: Option<crate::provider::Provider>,
    pub effort: crate::services::intent_routing::effort::Capability,
    pub url: reqwest::Url,
    pub headers: http::HeaderMap,
    pub official: bool,
    pub credentials: Option<Arc<crate::services::intent_routing::catalog::ExecutionCredentials>>,
    pub scheduling: Arc<scheduling::Scheduling>,
}
impl Target {
    fn matches_member(&self, member: &crate::services::intent_routing::Member) -> bool {
        self.provider_id == member.provider_id
            && self.model == member.model_id
            && self.fixed_effort == member.effort
    }
    fn continuation_destination(&self) -> Option<String> {
        use sha2::{Digest, Sha256};
        let mut headers = self
            .headers
            .iter()
            .filter(|(name, _)| !self.official || name.as_str() != "authorization")
            .map(|(name, value)| (name.as_str(), value.as_bytes()))
            .collect::<Vec<_>>();
        headers.sort();
        let account = match &self.credentials {
            Some(credentials) => Some(credentials.continuation_identity()?),
            None if self.official => return None,
            None => None,
        };
        let provider = self
            .upstream
            .as_ref()
            .map(|p| (&p.settings_config, &p.meta));
        let mut snapshot = serde_json::to_value((
            "continuation-destination-v1",
            &self.provider_id,
            &self.model,
            self.url.as_str(),
            self.official,
            account,
            provider,
            headers,
        ))
        .ok()?;
        // Keep pre-existing continuation identities for ordinary, unfixed seats.
        if let (Some(fixed), Some(fields)) = (&self.fixed_effort, snapshot.as_array_mut()) {
            fields.push(json!({"fixedEffort": fixed}));
        }
        snapshot.sort_all_objects();
        let bytes = serde_json::to_vec(&snapshot).ok()?;
        Some(format!("{:x}", Sha256::digest(bytes)))
    }
    pub(crate) fn same_destination(&self, other: &Self) -> bool {
        self.provider_id == other.provider_id
            && self.model == other.model
            && self.fixed_effort == other.fixed_effort
            && self.url == other.url
            && match (&self.upstream, &other.upstream) {
                (Some(a), Some(b)) => {
                    a.settings_config == b.settings_config
                        && match (serde_json::to_value(&a.meta), serde_json::to_value(&b.meta)) {
                            (Ok(a), Ok(b)) => a == b,
                            _ => false,
                        }
                }
                (None, None) => true,
                _ => false,
            }
            && self.same_account(other)
    }
    pub(crate) fn same_account(&self, other: &Self) -> bool {
        let same_url = if let (Some(a), Some(b)) = (&self.upstream, &other.upstream) {
            let gemini = |provider: &crate::provider::Provider| {
                provider
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.api_format.as_deref())
                    .and_then(Protocol::from_format)
                    == Some(Protocol::Gemini)
            };
            let account_url = |url: &reqwest::Url| {
                let mut url = url.clone();
                if let Some((base, _)) = url.path().split_once("/models/") {
                    let path = base.to_owned();
                    url.set_path(&path);
                }
                url
            };
            if gemini(a) && gemini(b) {
                a.settings_config == b.settings_config
                    && account_url(&self.url) == account_url(&other.url)
            } else {
                self.url == other.url
            }
        } else {
            self.url == other.url
        };
        same_url
            && self.official == other.official
            && match (&self.credentials, &other.credentials) {
                (Some(a), Some(b)) => a.same_identity(b),
                (None, None) => self.headers == other.headers,
                _ => false,
            }
    }
}
#[derive(Clone)]
pub(crate) struct Session {
    pub group: RoutingGroup,
    pub targets: Vec<Target>,
    pub classifier: Option<crate::provider::Provider>,
    pub client: reqwest::Client,
    pub created: Instant,
    pub bindings: Arc<tokio::sync::Mutex<Bindings>>,
}
impl Session {
    pub(crate) fn default_reasoning_level(&self) -> Option<String> {
        let mut defaults = self
            .menu_targets()
            .map(|target| target.effort.default_level.as_deref());
        let level = defaults.next()??;
        defaults
            .all(|other| other == Some(level))
            .then(|| level.to_owned())
    }
    pub(crate) fn offered_levels(&self) -> Vec<String> {
        use crate::services::intent_routing::effort;
        if !self.group.levels.is_empty() {
            return effort::offered(&self.group, std::iter::empty());
        }
        let unfixed = self
            .menu_targets()
            .filter(|target| target.fixed_effort.is_none())
            .collect::<Vec<_>>();
        let shared = effort::shared(unfixed.iter().map(|target| &target.effort));
        if unfixed
            .iter()
            .any(|target| !target.effort.quiet && !target.effort.levels.is_empty())
        {
            return shared;
        }
        let fixed = self
            .menu_targets()
            .filter(|target| !target.effort.quiet && !target.effort.levels.is_empty())
            .filter_map(|target| target.fixed_effort.as_deref())
            .collect::<std::collections::HashSet<_>>();
        ["none", "minimal", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .filter(|level| fixed.contains(level))
            .map(str::to_owned)
            .collect()
    }

    pub(crate) fn model_entry(&self, alias: &str) -> Value {
        let context = crate::services::intent_routing::context::Metadata::for_session(self);
        let mut model = json!({"id":alias,"object":"model","owned_by":"varyai","context_window":context.context_window});
        crate::services::intent_routing::effort::advertise(
            &mut model,
            &self.offered_levels(),
            self.default_reasoning_level().as_deref().unwrap_or(""),
        );
        model
    }
    fn target_indices(&self, member: &crate::services::intent_routing::Member) -> Vec<usize> {
        self.targets
            .iter()
            .enumerate()
            .filter_map(|(index, target)| target.matches_member(member).then_some(index))
            .collect()
    }
    pub(crate) fn menu_targets(&self) -> impl Iterator<Item = &Target> {
        self.targets.iter().filter(|target| {
            self.group
                .members
                .iter()
                .any(|member| target.matches_member(member) && self.group.includes_in_menu(member))
        })
    }
}
impl Runtime {
    pub(crate) async fn prepare_continuations(&self, path: std::path::PathBuf) {
        if self.continuations.read().is_ok_and(|store| store.is_some()) {
            return;
        }
        let key = path.with_extension("key");
        match continuation::Store::load(path, key).await {
            Ok(store) => {
                if let Ok(mut slot) = self.continuations.write() {
                    if slot.is_none() {
                        *slot = Some(store);
                    }
                }
            }
            Err(_) => log::warn!("[Routing] Continuation recovery unavailable"),
        }
    }
    pub(crate) async fn flush_continuations(&self) {
        let store = self
            .continuations
            .read()
            .ok()
            .and_then(|store| store.clone());
        if let Some(store) = store {
            if store.flush().await.is_err() {
                log::warn!("[Routing] Continuation persistence unavailable");
            }
        }
    }
    pub(crate) fn global_snapshot(&self) -> Result<Option<Arc<Menu>>, String> {
        self.global
            .read()
            .map(|menu| menu.clone())
            .map_err(|_| "intentRouting.failed".into())
    }
    pub(crate) fn reserve_global_aliases(
        &self,
        aliases: impl IntoIterator<Item = String>,
    ) -> Result<(), String> {
        let mut reserved = self.reserved.write().map_err(|_| "intentRouting.failed")?;
        for alias in aliases {
            if !reserved.contains(&alias) && reserved.len() >= 16384 {
                return Err("intentRouting.limit".into());
            }
            reserved.insert(alias);
        }
        Ok(())
    }
    pub(crate) fn global_alias_reserved(&self, alias: &str) -> Result<bool, String> {
        if alias.starts_with("vary-auto/")
            || alias.starts_with("vary-provider/")
            || alias.starts_with("auto/")
        {
            return Ok(true);
        }
        self.reserved
            .read()
            .map(|set| set.contains(alias))
            .map_err(|_| "intentRouting.failed".into())
    }
    pub(crate) fn global_current(&self, generation: u64) -> bool {
        self.global.read().is_ok_and(|slot| {
            slot.is_none()
                || self
                    .global_generation
                    .load(std::sync::atomic::Ordering::Acquire)
                    == generation
        })
    }
    pub(crate) fn set_global(&self, mut menu: Menu, generation: u64) -> Result<(), String> {
        self.reserve_global_aliases(menu.groups.keys().cloned())?;
        let mut slot = self.global.write().map_err(|_| "intentRouting.failed")?;
        if let Some(previous) = slot.as_ref() {
            menu.preserve_from(previous);
        }
        if let Some(local) = self.menu_snapshot()? {
            menu.share_scheduling(&local);
        }
        *slot = Some(Arc::new(menu));
        self.global_generation
            .store(generation, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    pub(crate) fn clear_global(&self) {
        if let Ok(mut slot) = self.global.write() {
            *slot = None;
        }
    }
    pub(crate) fn menu_snapshot(&self) -> Result<Option<Arc<Menu>>, String> {
        self.menu
            .read()
            .map(|menu| menu.clone())
            .map_err(|_| "intentRouting.failed".into())
    }
    #[cfg(test)]
    pub(crate) fn menu_images(&self, token: &str) -> Result<HashMap<String, bool>, String> {
        let menu = self.menu.read().map_err(|_| "intentRouting.failed")?;
        Ok(menu
            .as_ref()
            .filter(|m| m.token == token)
            .map(|menu| {
                menu.groups
                    .values()
                    .map(|session| {
                        (
                            session.group.id.clone(),
                            session
                                .menu_targets()
                                .any(|t| t.effort.images != Some(false)),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    #[cfg(test)]
    pub(crate) fn menu_efforts(&self, token: &str) -> Result<HashMap<String, Vec<String>>, String> {
        let menu = self.menu.read().map_err(|_| "intentRouting.failed")?;
        Ok(menu
            .as_ref()
            .filter(|m| m.token == token)
            .map(|menu| {
                menu.groups
                    .values()
                    .map(|session| (session.group.id.clone(), session.offered_levels()))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn menu_active(&self) -> bool {
        self.menu.read().map(|v| v.is_some()).unwrap_or(false)
    }
    pub fn clear_menu(&self) {
        self.native.revoke();
        self.native.clear_menu();
        if let Ok(mut v) = self.menu.write() {
            *v = None;
        }
    }
    pub fn set_menu(&self, menu: Arc<Menu>) -> Result<Option<Arc<Menu>>, String> {
        let global = self.global_snapshot()?;
        let menu = if let Some(global) = global {
            let mut menu = (*menu).clone();
            menu.share_scheduling(&global);
            Arc::new(menu)
        } else {
            menu
        };
        let mut slot = self.menu.write().map_err(|_| "intentRouting.failed")?;
        Ok(slot.replace(menu))
    }
    pub fn restore_menu(&self, menu: Option<Arc<Menu>>) {
        if let Ok(mut slot) = self.menu.write() {
            *slot = menu;
        }
    }

    pub fn install(
        self: &Arc<Self>,
        session: Session,
        catalog: Option<tempfile::TempPath>,
    ) -> Result<String, String> {
        let mut sessions = self.sessions.write().map_err(|_| "intentRouting.failed")?;
        sessions.retain(|_, s| s.snapshot.created.elapsed() < TEMPORARY_SESSION_TTL);
        if sessions.len() >= 16 {
            return Err("intentRouting.limit".into());
        }
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let deadline = session.created + TEMPORARY_SESSION_TTL;
        // The timer must not keep the runtime or its temporary files alive at shutdown.
        let runtime = Arc::downgrade(self);
        let expiring_token = token.clone();
        let expiry = tokio::spawn(async move {
            tokio::time::sleep_until(deadline.into()).await;
            if let Some(runtime) = runtime.upgrade() {
                runtime.revoke(&expiring_token);
            }
        });
        sessions.insert(
            token.clone(),
            TemporarySession {
                snapshot: Arc::new(session),
                _catalog: catalog,
                expiry: expiry.abort_handle(),
            },
        );
        Ok(token)
    }
    pub fn revoke(&self, token: &str) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.remove(token);
        }
    }
}
#[derive(Default)]
pub(crate) struct Bindings {
    responses: HashMap<String, usize>,
    calls: HashMap<String, usize>,
    sessions: HashMap<String, usize>,
    opaque: HashMap<String, usize>,
    items: HashMap<String, usize>,
    pool: scheduling::Pool,
    durable: Option<scopes::Durable>,
    completed_sessions: std::collections::HashSet<String>,
    held_rules: Vec<String>,
    last_input_tokens: u64,
}
fn remember(map: &mut HashMap<String, usize>, kind: &str, key: &str, target: usize) -> bool {
    let key = continuation::hash(kind, key);
    match map.entry(key) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(target);
            true
        }
        std::collections::hash_map::Entry::Occupied(mut entry)
            if *entry.get() != target && *entry.get() != usize::MAX =>
        {
            entry.insert(usize::MAX);
            true
        }
        _ => false,
    }
}
impl Bindings {
    fn persist(&self) {
        if let Some(durable) = &self.durable {
            durable.persist(self);
        }
    }
    fn bind_session(&mut self, key: &str, target: usize) {
        // A received retryable failure may advance an unbound request's plan.
        self.sessions
            .insert(continuation::hash("session", key), target);
        self.completed_sessions
            .remove(&continuation::hash("session", key));
        self.persist();
    }
    fn complete_session(&mut self, key: &str) {
        self.completed_sessions
            .insert(continuation::hash("session", key));
        self.persist();
    }
    fn session(&self, key: &str) -> Option<usize> {
        self.sessions
            .get(&continuation::hash("session", key))
            .copied()
    }
    fn resolve(&self, body: &Value, session: Option<&str>) -> Result<Option<usize>, ()> {
        let mut selected = session.and_then(|s| self.session(s));
        if selected == Some(usize::MAX) {
            return Err(());
        }
        let mut merge = |candidate: Option<usize>| -> Result<(), ()> {
            let candidate = candidate.filter(|v| *v != usize::MAX).ok_or(())?;
            if selected.is_some_and(|s| s != candidate) {
                return Err(());
            }
            selected = Some(candidate);
            Ok(())
        };
        if let Some(previous) = body.get("previous_response_id").filter(|v| !v.is_null()) {
            merge(
                self.responses
                    .get(&continuation::hash(
                        "response",
                        previous.as_str().ok_or(())?,
                    ))
                    .copied(),
            )?;
        }
        if let Some(items) = body.get("input").and_then(Value::as_array) {
            for item in items {
                if matches!(
                    item["type"].as_str(),
                    Some("function_call_output" | "function_call")
                ) {
                    merge(
                        self.calls
                            .get(&continuation::hash(
                                "call",
                                item["call_id"].as_str().ok_or(())?,
                            ))
                            .copied(),
                    )?;
                }
                if let Some(ciphertext) = item.get("encrypted_content").and_then(Value::as_str) {
                    let key = continuation::hash("opaque", ciphertext);
                    if let Some(index) = self.opaque.get(&key) {
                        merge(Some(*index))?;
                    }
                }
                if matches!(
                    item["type"].as_str(),
                    Some("item_reference" | "reasoning" | "compaction" | "compaction_summary")
                ) && item.get("encrypted_content").is_none()
                {
                    merge(
                        self.items
                            .get(&continuation::hash("item", item["id"].as_str().ok_or(())?))
                            .copied(),
                    )?;
                }
            }
            if items.iter().any(|v| {
                v.get("encrypted_content").is_some()
                    || !matches!(
                        v.get("type").and_then(Value::as_str),
                        None | Some("message" | "function_call_output")
                    )
            }) && selected.is_none()
            {
                return Err(());
            }
        }
        Ok(selected)
    }
    fn known_opaque(&self, body: &Value) -> bool {
        body.get("input")
            .and_then(Value::as_array)
            .is_none_or(|items| {
                items.iter().all(|item| {
                    if let Some(ciphertext) = item.get("encrypted_content").and_then(Value::as_str)
                    {
                        let key = continuation::hash("opaque", ciphertext);
                        return self
                            .opaque
                            .get(&key)
                            .is_some_and(|index| *index != usize::MAX);
                    }
                    if matches!(
                        item["type"].as_str(),
                        Some("reasoning" | "compaction" | "compaction_summary" | "item_reference")
                    ) {
                        return item["id"]
                            .as_str()
                            .and_then(|id| self.items.get(&continuation::hash("item", id)))
                            .is_some_and(|index| *index != usize::MAX);
                    }
                    true
                })
            })
    }
    fn full(&self) -> bool {
        self.responses.len()
            + self.calls.len()
            + self.sessions.len()
            + self.opaque.len()
            + self.items.len()
            >= 4096
    }
    fn observe(&mut self, value: &Value, target: usize) {
        if self.full() {
            return;
        }
        let mut changed = false;
        let response = value.get("response").unwrap_or(value);
        if let Some(id) = response
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| id.len() <= 256)
        {
            changed |= remember(&mut self.responses, "response", id, target);
        }
        let native_calls = response
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "tool_use")
            .filter_map(|item| item["id"].as_str())
            .chain(
                value
                    .get("content_block")
                    .filter(|item| item["type"] == "tool_use")
                    .and_then(|item| item["id"].as_str()),
            );
        for id in native_calls.filter(|id| id.len() <= 256) {
            if self.full() {
                break;
            }
            changed |= remember(&mut self.calls, "call", id, target);
        }
        if let Some(choices) = response.get("choices").and_then(Value::as_array) {
            for call in choices
                .iter()
                .flat_map(|choice| choice.get("message").or_else(|| choice.get("delta")))
                .flat_map(|message| message.get("tool_calls").and_then(Value::as_array))
                .flatten()
            {
                if self.full() {
                    break;
                }
                if let Some(id) = call["id"].as_str().filter(|id| id.len() <= 256) {
                    changed |= remember(&mut self.calls, "call", id, target);
                }
            }
        }
        let items = response
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(value.get("item"));
        for item in items {
            if self.full() {
                break;
            }
            if let Some(id) = item["id"].as_str().filter(|id| id.len() <= 256) {
                changed |= remember(&mut self.items, "item", id, target);
            }
            if let Some(ciphertext) = item["encrypted_content"]
                .as_str()
                .filter(|text| text.len() <= 256 * 1024)
            {
                changed |= remember(&mut self.opaque, "opaque", ciphertext, target);
            }
            if item["type"] == "function_call" {
                if let Some(id) = item["call_id"].as_str().filter(|id| id.len() <= 256) {
                    changed |= remember(&mut self.calls, "call", id, target);
                }
            }
        }
        if changed {
            self.persist();
        }
    }
}
#[derive(Default)]
struct Observer {
    scheduling: Option<Arc<scheduling::Scheduling>>,
    started: Option<Instant>,
    first_content: Option<Instant>,
    first_text: Option<Instant>,
    last_observed: Option<Instant>,
    flow: Vec<(Instant, u64)>,
    flow_bytes: u64,
    flow_overflow: bool,
    tokens: u64,
    output_tokens: u32,
    reasoning_tokens: u32,
    buffer: String,
    remainder: Vec<u8>,
    oversized: bool,
    completed: bool,
    trace_terminal: bool,
    failed: bool,
    pending_tools: bool,
}
impl Observer {
    fn push(&mut self, bytes: &[u8], bindings: &mut Bindings, target: usize) {
        if self.oversized {
            return;
        }
        if self.buffer.len() + bytes.len() > 2 * 1024 * 1024 {
            self.oversized = true;
            self.buffer.clear();
            return;
        }
        super::sse::append_utf8_safe(&mut self.buffer, &mut self.remainder, bytes);
        while let Some(block) = super::sse::take_sse_block(&mut self.buffer) {
            self.failed |= block.lines().any(|line| {
                super::sse::strip_sse_field(line, "event").is_some_and(|event| {
                    matches!(event, "error" | "response.failed" | "response.incomplete")
                })
            });
            let data = block
                .lines()
                .filter_map(|l| super::sse::strip_sse_field(l, "data"))
                .collect::<Vec<_>>()
                .join("\n");
            if data == "[DONE]" {
                self.completed = true;
            }
            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                self.observe_at(&value, bindings, target, Instant::now());
            }
        }
    }
    fn finish(&mut self, bindings: &mut Bindings, target: usize) {
        if let Ok(value) = serde_json::from_str::<Value>(&self.buffer) {
            self.completed = value.get("error").is_none_or(Value::is_null);
            self.observe_at(&value, bindings, target, Instant::now());
        }
    }
    fn observe_at(&mut self, value: &Value, bindings: &mut Bindings, target: usize, now: Instant) {
        self.last_observed = Some(now);
        if let Some(scheduling) = &self.scheduling {
            scheduling.observe_event(value, chrono::Utc::now().timestamp());
        }
        bindings.observe(value, target);
        let kind = value["type"].as_str().unwrap_or("");
        self.pending_tools |= value.get("response").unwrap_or(value)["output"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["type"] == "function_call"))
            || value["item"]["type"] == "function_call"
            || value["content_block"]["type"] == "tool_use"
            || value["content"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["type"] == "tool_use"))
            || value["choices"].as_array().is_some_and(|choices| {
                choices.iter().any(|choice| {
                    ["message", "delta"].iter().any(|key| {
                        choice[*key]["tool_calls"]
                            .as_array()
                            .is_some_and(|calls| !calls.is_empty())
                    })
                })
            });
        self.completed |= matches!(kind, "response.completed" | "message_stop");
        self.trace_terminal |= value["choices"].as_array().is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice["finish_reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty())
            })
        }) || value["candidates"].as_array().is_some_and(|candidates| {
            candidates.iter().any(|candidate| {
                candidate["finishReason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty())
            })
        });
        self.failed |= value.get("error").is_some_and(|error| !error.is_null())
            || matches!(value["status"].as_str(), Some("failed" | "incomplete"))
            || matches!(kind, "error" | "response.failed" | "response.incomplete")
            || matches!(
                value["response"]["status"].as_str(),
                Some("failed" | "incomplete")
            );
        if self.started.is_some() {
            let nonempty = |value: &Value| value.as_str().is_some_and(|s| !s.is_empty());
            let mut text_bytes = 0u64;
            let mut content = false;
            if nonempty(&value["delta"]) {
                if matches!(
                    kind,
                    "response.output_text.delta" | "response.function_call_arguments.delta"
                ) {
                    text_bytes += value["delta"].as_str().map_or(0, str::len) as u64;
                }
                content |= matches!(
                    kind,
                    "response.output_text.delta"
                        | "response.function_call_arguments.delta"
                        | "response.reasoning_text.delta"
                        | "response.reasoning_summary_text.delta"
                );
            }
            if kind == "content_block_delta" {
                text_bytes += value["delta"]["text"].as_str().map_or(0, str::len) as u64;
                content |= ["text", "thinking", "partial_json"]
                    .iter()
                    .any(|key| nonempty(&value["delta"][*key]));
            }
            content |=
                kind == "content_block_start" && value["content_block"]["type"] == "tool_use";
            content |=
                kind == "response.output_item.added" && value["item"]["type"] == "function_call";
            if let Some(choices) = value["choices"].as_array() {
                for choice in choices {
                    let delta = &choice["delta"];
                    text_bytes += delta["content"].as_str().map_or(0, str::len) as u64;
                    content |= nonempty(&delta["content"])
                        || nonempty(&delta["reasoning_content"])
                        || delta["tool_calls"]
                            .as_array()
                            .is_some_and(|calls| !calls.is_empty());
                }
            }
            if let Some(candidates) = value["candidates"].as_array() {
                for part in candidates
                    .iter()
                    .filter_map(|candidate| candidate["content"]["parts"].as_array())
                    .flatten()
                {
                    content |= nonempty(&part["text"]) || part["functionCall"].is_object();
                    if part["thought"] != true {
                        text_bytes += part["text"].as_str().map_or(0, str::len) as u64;
                    }
                }
            }
            if content {
                self.first_content.get_or_insert(now);
            }
            if text_bytes > 0 {
                self.first_text.get_or_insert(now);
                self.flow_bytes = self.flow_bytes.saturating_add(text_bytes);
                if let Some((_, bytes)) = self.flow.last_mut().filter(|(at, _)| *at == now) {
                    *bytes = self.flow_bytes;
                } else if self.flow.len() < 65536 {
                    self.flow.push((now, self.flow_bytes));
                } else {
                    self.flow_overflow = true;
                }
            }
        }
        let usage = &value.get("response").unwrap_or(value)["usage"];
        let input = usage["input_tokens"]
            .as_u64()
            .or_else(|| usage["prompt_tokens"].as_u64())
            .or_else(|| value["message"]["usage"]["input_tokens"].as_u64())
            .or_else(|| value["usageMetadata"]["promptTokenCount"].as_u64())
            .unwrap_or(0);
        let output = usage["output_tokens"]
            .as_u64()
            .or_else(|| usage["completion_tokens"].as_u64())
            .or_else(|| value["usageMetadata"]["candidatesTokenCount"].as_u64())
            .unwrap_or(0);
        self.tokens = self.tokens.max(input.saturating_add(output));
        bindings.last_input_tokens = bindings.last_input_tokens.max(input);
        self.output_tokens = self
            .output_tokens
            .max(output.min(u64::from(u32::MAX)) as u32);
        self.reasoning_tokens = self.reasoning_tokens.max(
            usage["output_tokens_details"]["reasoning_tokens"]
                .as_u64()
                .unwrap_or(0)
                .min(u64::from(u32::MAX)) as u32,
        );
    }
    fn sample(&self) -> (u64, f64, f64, u32) {
        let ttft = self
            .started
            .zip(self.first_content)
            .map(|(a, b)| b.saturating_duration_since(a).as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        // Middle 80% of content, scaled to the full reply, detects a vendor-held burst.
        let flow = if self.flow_overflow || self.flow_bytes == 0 {
            0.0
        } else {
            let mark = |share: u64| {
                self.flow
                    .iter()
                    .find(|(_, bytes)| {
                        bytes.saturating_mul(100) >= self.flow_bytes.saturating_mul(share)
                    })
                    .map(|(at, _)| *at)
            };
            mark(10)
                .zip(mark(90))
                .map(|(a, b)| b.saturating_duration_since(a).as_secs_f64() * 1250.0)
                .unwrap_or(0.0)
        };
        let mut decode = self
            .first_text
            .zip(self.last_observed)
            .map(|(a, b)| b.saturating_duration_since(a).as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let output = self.output_tokens.saturating_sub(self.reasoning_tokens);
        // Do not learn a decode rate from a held/burst reply or hidden reasoning.
        let thinking = self
            .first_content
            .zip(self.first_text)
            .map(|(a, b)| b.saturating_duration_since(a).as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        if decode < 100.0
            || flow <= 0.0
            || flow * 4.0 < decode
            || f64::from(output) * 1000.0 > 10000.0 * decode
            || (self.reasoning_tokens > 0
                && f64::from(output) * thinking > 20.0 * f64::from(self.reasoning_tokens) * decode)
        {
            decode = 0.0;
        }
        (self.tokens, ttft, decode, output)
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        if let Some(scheduling) = &self.scheduling {
            let (tokens, ttft, decode, output) = self.sample();
            scheduling.served(tokens, ttft, decode, output, chrono::Utc::now().timestamp());
        }
    }
}
fn error(status: StatusCode, code: &'static str) -> Response {
    let mut response = Response::new(Body::from(
        json!({"error":{"type":"invalid_request_error","code":code,"message":code}}).to_string(),
    ));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    response
}
pub(crate) async fn handle(
    State(state): State<super::server::ProxyState>,
    request: Request,
) -> Response {
    let started = Instant::now();
    let fail = error;
    if request
        .extensions()
        .get::<std::net::SocketAddr>()
        .is_some_and(|peer| !peer.ip().is_loopback())
        || request.headers().contains_key("origin")
        || request.headers().contains_key("sec-fetch-site")
    {
        return fail(StatusCode::FORBIDDEN, "routing_local_clients_only");
    }
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    let menu = state.intent_runtime.menu.read().ok().and_then(|slot| {
        slot.as_ref()
            .filter(|m| token == Some(m.token.as_str()))
            .cloned()
    });
    let session = state
        .intent_runtime
        .sessions
        .read()
        .ok()
        .and_then(|s| token.and_then(|t| s.get(t).map(|entry| entry.snapshot.clone())))
        .filter(|s| s.created.elapsed() < TEMPORARY_SESSION_TTL);
    if session.is_none() && menu.is_none() {
        return fail(StatusCode::UNAUTHORIZED, "routing_session_expired");
    }
    crate::codex_client::observe(request.headers());
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "routing_query_unsupported");
    }
    if request.method() == http::Method::GET && request.uri().path() == "/intent/v1/models" {
        let aliases = if let Some(menu) = &menu {
            menu.listed_aliases().cloned().collect::<Vec<_>>()
        } else {
            session.iter().map(|s| s.group.alias()).collect()
        };
        let data = aliases
            .into_iter()
            .filter_map(|id| {
                menu.as_ref()
                    .and_then(|menu| menu.groups.get(&id))
                    .or(session.as_ref())
                    .map(|session| session.model_entry(&id))
            })
            .collect::<Vec<_>>();
        return axum::response::IntoResponse::into_response(axum::Json(
            json!({"object":"list","data":data}),
        ));
    }
    if request.method() != http::Method::POST || request.uri().path() != "/intent/v1/responses" {
        return fail(StatusCode::NOT_FOUND, "routing_endpoint_unsupported");
    }
    let (parts, body) = request.into_parts();
    let bytes = match axum::body::to_bytes(body, 8 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::PAYLOAD_TOO_LARGE, "routing_body_limit"),
    };
    let original = match super::pi::json::parse(&bytes) {
        Ok(v) if v.is_object() => v,
        _ => return fail(StatusCode::BAD_REQUEST, "routing_invalid_json"),
    };
    let session = menu
        .as_ref()
        .and_then(|m| {
            original["model"]
                .as_str()
                .and_then(|id| m.groups.get(id))
                .cloned()
        })
        .or(session);
    let Some(session) = session else {
        return fail(StatusCode::NOT_FOUND, "routing_model_unavailable");
    };
    if menu.is_none()
        && original["model"] != session.group.alias()
        && original["model"] != format!("vary-auto/{}", session.group.id)
    {
        return fail(StatusCode::NOT_FOUND, "routing_model_unavailable");
    }
    execute(
        state,
        session,
        parts,
        bytes,
        original,
        Protocol::Responses,
        "/v1/responses".into(),
        "local-codex".into(),
        menu.is_some(),
        false,
        None,
        started,
    )
    .await
}

pub(crate) fn catalog_stale() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "routing_catalog_stale")
}
pub(crate) async fn handle_global(
    state: super::server::ProxyState,
    request: Request,
    consumer: String,
    alias: &str,
) -> Response {
    let generation = state.db.routing_generation();
    if !state.intent_runtime.global_current(generation) {
        return error(StatusCode::SERVICE_UNAVAILABLE, "routing_catalog_stale");
    }
    let session = match state.intent_runtime.global_snapshot() {
        Ok(Some(menu)) => menu.groups.get(alias).cloned(),
        Ok(None) => None,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "routing_catalog_unavailable",
            )
        }
    };
    let Some(session) = session else {
        return error(StatusCode::NOT_FOUND, "routing_model_unavailable");
    };
    let (parts, body) = request.into_parts();
    let endpoint = parts
        .uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(parts.uri.path())
        .to_owned();
    let protocol = if parts.uri.path() == "/v1/messages" {
        Protocol::Anthropic
    } else if parts.uri.path() == "/v1/chat/completions" {
        Protocol::Chat
    } else if parts.uri.path().starts_with("/v1beta/models/") {
        Protocol::Gemini
    } else {
        Protocol::Responses
    };
    if !super::protocol_bridge::supported_query(parts.uri.path(), parts.uri.query()) {
        return error(StatusCode::BAD_REQUEST, "routing_query_unsupported");
    }
    let bytes = match axum::body::to_bytes(body, 8 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return error(StatusCode::PAYLOAD_TOO_LARGE, "routing_body_limit"),
    };
    let original = match super::pi::json::parse(&bytes) {
        Ok(value) if value.is_object() => value,
        _ => return error(StatusCode::BAD_REQUEST, "routing_invalid_json"),
    };
    let external = parts
        .extensions
        .get::<super::connections::Agent>()
        .is_none();
    execute(
        state,
        session,
        parts,
        bytes,
        original,
        protocol,
        endpoint,
        consumer,
        true,
        external,
        Some(generation),
        Instant::now(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    state: super::server::ProxyState,
    session: Arc<Session>,
    parts: http::request::Parts,
    bytes: bytes::Bytes,
    original: Value,
    protocol: Protocol,
    endpoint: String,
    consumer: String,
    refresh_credentials: bool,
    external: bool,
    generation: Option<u64>,
    started: Instant,
) -> Response {
    let agent = parts
        .extensions
        .get::<super::connections::Agent>()
        .map(|agent| agent.0)
        .unwrap_or(if external { "external" } else { "codex" });
    let trace = state.intent_runtime.traces.begin(
        agent,
        &session
            .group
            .model_alias
            .clone()
            .unwrap_or_else(|| format!("vary-auto/{}", session.group.id)),
        &session.group.name,
        match session.group.routing {
            crate::services::intent_routing::RoutingStrategy::Smart => "smart",
            crate::services::intent_routing::RoutingStrategy::Ordered => "ordered",
            crate::services::intent_routing::RoutingStrategy::RoundRobin => "round_robin",
            crate::services::intent_routing::RoutingStrategy::LeastUsed => "least_used",
            crate::services::intent_routing::RoutingStrategy::Pace => "pace",
            crate::services::intent_routing::RoutingStrategy::FreeFirst => "free_first",
            crate::services::intent_routing::RoutingStrategy::Fastest => "fastest",
            crate::services::intent_routing::RoutingStrategy::Weighted => "weighted",
            crate::services::intent_routing::RoutingStrategy::Manual => "manual",
        },
        started,
    );
    let compact = parts.extensions.get::<native::Compact>().copied();
    trace.purpose(&parts.headers, compact.is_some());
    let fail = |status: StatusCode, code| {
        trace.finish(status.as_u16(), trace::State::Failed);
        error(status, code)
    };
    let native_lease = parts.extensions.get::<Arc<native::Lease>>().cloned();
    if native_lease.as_ref().is_some_and(|lease| !lease.valid()) {
        return fail(StatusCode::UNAUTHORIZED, "routing_native_unavailable");
    }
    let streaming = original["stream"] == true
        || (protocol == Protocol::Gemini
            && endpoint
                .split('?')
                .next()
                .is_some_and(|p| p.ends_with(":streamGenerateContent")));
    let projection = match super::group_forward::canonical_request(protocol, &original, &endpoint) {
        Ok(value) => match compact.filter(|c| c.native) {
            Some(compact) => match compaction::history(&value, compact.operation) {
                Ok(history) => history,
                Err(code) => return fail(StatusCode::BAD_REQUEST, code),
            },
            None => value,
        },
        Err(_) => return fail(StatusCode::BAD_REQUEST, "routing_invalid_input"),
    };
    let conversation = original["prompt_cache_key"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .or_else(|| {
            [
                "session_id",
                "session-id",
                "thread-id",
                "thread_id",
                "x-conversation-id",
            ]
            .into_iter()
            .find_map(|name| {
                parts
                    .headers
                    .get(name)
                    .and_then(|s| s.to_str().ok())
                    .filter(|s| !s.is_empty() && s.len() <= 256)
            })
        })
        .map(str::to_owned);
    if let Some(key) = &conversation {
        trace.session(key, &consumer);
    }
    let durable = if external || refresh_credentials {
        state
            .intent_runtime
            .continuations
            .read()
            .ok()
            .and_then(|store| store.clone())
    } else {
        None
    };
    let scoped = match state.intent_runtime.conversations.acquire_durable(
        &consumer,
        &session,
        conversation.as_deref(),
        &projection,
        durable,
    ) {
        Ok(scoped) => scoped,
        Err(code) => return fail(StatusCode::CONFLICT, code),
    };
    let mut bindings = match scoped.bindings.clone().try_lock_owned() {
        Ok(bindings) => bindings,
        Err(_) => return fail(StatusCode::CONFLICT, "routing_session_busy"),
    };
    if bindings.full() {
        return fail(StatusCode::CONFLICT, "routing_session_capacity");
    }
    let resolution = if protocol != Protocol::Responses {
        let pinned = conversation
            .as_ref()
            .and_then(|name| bindings.session(name));
        let tools = projection
            .get("input")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    matches!(
                        item["type"].as_str(),
                        Some("function_call" | "function_call_output")
                    )
                })
            });
        if (tools || opaque_continuation(protocol, &original)) && pinned.is_none() {
            Err(())
        } else {
            Ok(pinned)
        }
    } else {
        bindings.resolve(&projection, conversation.as_deref())
    };
    let mut pinned = match resolution {
        Ok(p) => p,
        Err(_) => return fail(StatusCode::CONFLICT, "routing_unknown_continuation"),
    };
    let mut affinity = None;
    if compact.is_none()
        && session.group.routing != crate::services::intent_routing::RoutingStrategy::Manual
        && length::portable(&projection)
        && !opaque_continuation(protocol, &original)
        && conversation.as_ref().is_some_and(|key| {
            bindings
                .completed_sessions
                .contains(&continuation::hash("session", key))
        })
    {
        match session.group.affinity.as_str() {
            "strict" => {}
            "off" | "turn" => {
                pinned = None;
            }
            _ => {
                affinity = pinned.take();
            }
        }
    }
    if native_lease.is_some() && !bindings.known_opaque(&projection) {
        return fail(StatusCode::CONFLICT, "routing_unknown_continuation");
    }
    if let Some(forced) = parts.extensions.get::<native::ForcedTarget>() {
        if pinned.is_some_and(|index| index != forced.0) {
            return fail(StatusCode::CONFLICT, "routing_checkpoint_conflict");
        }
        pinned = Some(forced.0);
    }
    if pinned.is_some_and(|index| {
        session.targets.get(index).is_none_or(|target| {
            session.group.members.iter().any(|member| {
                target.matches_member(member) && session.group.off.contains(&member.id)
            })
        })
    }) {
        return fail(StatusCode::CONFLICT, "routing_continuation_target_disabled");
    }
    let mut body = original.clone();
    let consumer_app = parts
        .extensions
        .get::<super::connections::Agent>()
        .map(|agent| agent.0);
    let wire_app = match protocol {
        Protocol::Responses | Protocol::Chat => crate::app_config::AppType::Codex,
        Protocol::Anthropic => crate::app_config::AppType::Claude,
        Protocol::Gemini => crate::app_config::AppType::Gemini,
    };
    let inspection_app = consumer_app
        .and_then(|app| app.parse::<crate::app_config::AppType>().ok())
        .unwrap_or_else(|| wire_app.clone());
    let inspection = if external {
        super::security::inspect_external(&wire_app, &consumer, &mut body, &parts.headers)
    } else {
        super::security::inspect_with_context(
            &inspection_app,
            &wire_app,
            "",
            None,
            &mut body,
            &parts.headers,
        )
    };
    match inspection {
        Ok(Some(blocked)) => {
            trace.finish(blocked.status().as_u16(), trace::State::Failed);
            return blocked;
        }
        Err(_) => return fail(StatusCode::BAD_REQUEST, "routing_inspection_failed"),
        _ => {}
    }
    let inspected_projection =
        match super::group_forward::canonical_request(protocol, &body, &endpoint) {
            Ok(value) => value,
            Err(_) => return fail(StatusCode::BAD_REQUEST, "routing_invalid_input"),
        };
    let estimated_tokens = length::estimate(&inspected_projection, &body);
    let has_images = request_has_images(protocol, &body, &inspected_projection);
    let requested_effort =
        crate::services::intent_routing::rules::wire_effort(&body, protocol.format());
    let folder = crate::services::intent_routing::folders::declared(&inspected_projection);
    let facts = crate::services::intent_routing::rules::Facts {
        tokens: estimated_tokens
            .map(|estimate| estimate.max(bindings.last_input_tokens))
            .or((bindings.last_input_tokens > 0).then_some(bindings.last_input_tokens)),
        images: has_images,
        effort: requested_effort.as_deref(),
        agent,
        compact: compact.is_some(),
        at: chrono::Utc::now(),
        folder: &folder,
    };
    let paused = session
        .group
        .rules
        .iter()
        .filter(|rule| {
            rule.enabled
                && rule.pause
                && rule
                    .conditions
                    .as_ref()
                    .is_some_and(|conditions| conditions.matches(&facts))
        })
        .map(|rule| rule.target_member_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let is_paused =
        |index: usize| {
            session.targets.get(index).is_some_and(|target| {
                session.group.members.iter().any(|member| {
                    target.matches_member(member) && paused.contains(member.id.as_str())
                })
            })
        };
    if pinned.is_some_and(is_paused) {
        if length::portable(&projection)
            && !opaque_continuation(protocol, &original)
            && native_lease.is_none()
        {
            pinned = None;
        } else {
            return fail(StatusCode::CONFLICT, "routing_continuation_target_paused");
        }
    }
    if affinity.is_some_and(is_paused) {
        affinity = None;
    }
    let classifier_candidates =
        crate::services::intent_routing::rules::classifier_candidates(&session.group, &facts);
    let classified = if inspected_projection["_group_projection_incomplete"] != true
        && pinned.is_none()
        && session.group.uses_classifier()
        && !classifier_candidates.is_empty()
    {
        if let (Some(text), Some(provider)) = (
            classifier::extract_user_text(&inspected_projection),
            session.classifier.clone(),
        ) {
            let mut eligible = session.group.clone();
            eligible
                .rules
                .retain(|rule| classifier_candidates.contains(&rule.id));
            classifier::trial_prepared(eligible, provider, text)
                .await
                .ok()
                .and_then(|r| r.intent_id)
        } else {
            None
        }
    } else {
        None
    };
    let matched_rules =
        if session.group.routing == crate::services::intent_routing::RoutingStrategy::Manual {
            Vec::new()
        } else if pinned.is_some() {
            bindings.held_rules.clone()
        } else {
            let rules = crate::services::intent_routing::rules::matching(
                &session.group,
                &facts,
                classified.as_deref(),
            );
            bindings.held_rules = rules.clone();
            rules
        };
    let approved = match session.group.plan_prepared(None) {
        Ok(p) => p
            .candidates
            .iter()
            .flat_map(|m| session.target_indices(m))
            .filter(|index| !is_paused(*index))
            .collect::<Vec<_>>(),
        Err(_) => return fail(StatusCode::BAD_REQUEST, "routing_invalid_plan"),
    };
    if approved.is_empty() && !paused.is_empty() {
        trace.step(trace::Kind::Paused, None, None);
        return fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "routing_all_members_paused",
        );
    }
    let states = session
        .targets
        .iter()
        .map(|target| {
            let mut health = target.scheduling.snapshot();
            if let Some(member) = session
                .group
                .members
                .iter()
                .find(|m| target.matches_member(m))
            {
                health.weight = member.weight;
                health.free = member.free.or(target.effort.free).unwrap_or(false);
            }
            health
        })
        .collect::<Vec<_>>();
    let mut pool = session.bindings.lock().await;
    let mut candidates = if pinned.is_none()
        || session.group.routing == crate::services::intent_routing::RoutingStrategy::Weighted
    {
        scheduling::arrange(
            session.group.routing,
            &approved,
            &states,
            &mut pool.pool,
            chrono::Utc::now().timestamp(),
        )
    } else {
        approved
    };
    drop(pool);
    if pinned.is_none() {
        // An account pool keeps its configured order, but spends another
        // account's known allowance before retrying an exhausted one.
        let now = chrono::Utc::now().timestamp();
        let mut pools = HashMap::<_, Vec<usize>>::new();
        for (at, index) in candidates.iter().enumerate() {
            if let Some(t) = session.targets.get(*index).filter(|t| t.official) {
                pools
                    .entry((&t.provider_id, &t.model, &t.fixed_effort))
                    .or_default()
                    .push(at);
            }
        }
        for positions in pools.into_values().filter(|p| p.len() > 1) {
            let mut pool = positions
                .iter()
                .map(|at| candidates[*at])
                .collect::<Vec<_>>();
            pool.sort_by_key(|index| (session.targets[*index].scheduling.exhausted(now), *index));
            for (at, index) in positions.into_iter().zip(pool) {
                candidates[at] = index;
            }
        }
    }
    if pinned.is_none() && session.group.sink {
        scheduling::sink_last(
            session.group.routing,
            &mut candidates,
            &states,
            chrono::Utc::now().timestamp(),
        );
    }
    if let Some(index) = pinned {
        candidates = vec![index];
    } else if let Some(index) = affinity {
        if let Some(at) = candidates.iter().position(|candidate| *candidate == index) {
            candidates.remove(at);
            candidates.insert(0, index);
        }
    }
    let mut overflowed = false;
    let mut overflow_tokens = None;
    let mut refused_models = std::collections::HashSet::new();
    if pinned.is_none() && !matched_rules.is_empty() {
        // A rule's ordered seats stay together ahead of the strategy's tail.
        let seats = matched_rules
            .iter()
            .filter_map(|id| session.group.rules.iter().find(|rule| &rule.id == id))
            .flat_map(|rule| {
                std::iter::once(&rule.target_member_id).chain(rule.fallback_member_ids.iter())
            });
        let mut first = Vec::new();
        for id in seats {
            if let Some(index) = session
                .group
                .members
                .iter()
                .find(|m| &m.id == id)
                .map(|m| session.target_indices(m))
            {
                for index in index {
                    if let Some(position) = candidates.iter().position(|i| *i == index) {
                        candidates.remove(position);
                        first.push(index);
                    }
                }
            }
        }
        first.append(&mut candidates);
        candidates = first;
    }
    if pinned.is_none() && candidates.len() > 1 {
        if let Some(tokens) = estimated_tokens {
            let roomy = candidates
                .iter()
                .copied()
                .filter(|index| {
                    session.targets[*index]
                        .effort
                        .context
                        .window()
                        .is_none_or(|window| tokens < window)
                })
                .collect::<Vec<_>>();
            if !roomy.is_empty() {
                // A too-small last resort can still tell the client to compact
                // when every roomy destination is rate-limited.
                let small = candidates
                    .iter()
                    .copied()
                    .filter(|index| !roomy.contains(index))
                    .collect::<Vec<_>>();
                candidates = roomy;
                candidates.extend(small);
            }
        }
    }
    trace.plan(
        candidates
            .iter()
            .filter_map(|index| session.targets.get(*index))
            .map(|target| state.intent_runtime.traces.candidate(target))
            .collect(),
        estimated_tokens,
    );
    if pinned.is_some() {
        trace.step(trace::Kind::Pinned, None, None);
    } else if affinity.is_some() {
        trace.step(trace::Kind::Affinity, None, None);
    }
    if !matched_rules.is_empty() {
        trace.step(trace::Kind::Rule, None, None);
    }
    if !paused.is_empty() {
        trace.step(trace::Kind::Paused, None, None);
    }
    let mut attempted = false;
    let retry_seconds = match session.group.patience {
        value if value < 0 => 0,
        0 => 60,
        value => u64::from(value as u16),
    };
    let mut attempts = candidates
        .into_iter()
        .enumerate()
        .collect::<std::collections::VecDeque<_>>();
    let mut transient: Vec<(usize, usize)> = Vec::new();
    let mut retry_deadline = None;
    loop {
        if attempts.is_empty() && !transient.is_empty() && retry_seconds > 0 {
            let deadline = *retry_deadline
                .get_or_insert_with(|| Instant::now() + Duration::from_secs(retry_seconds));
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let now = chrono::Utc::now().timestamp();
            let until_ready = transient
                .iter()
                .filter_map(|(_, index)| session.targets.get(*index))
                .map(|target| target.scheduling.rest_until().saturating_sub(now).max(0) as u64)
                .min()
                .unwrap_or(1);
            let pause = left.min(Duration::from_secs(until_ready.max(1)));
            if let Some(lease) = &native_lease {
                tokio::select! {
                    _ = lease.cancelled() => return fail(StatusCode::UNAUTHORIZED, "routing_native_unavailable"),
                    _ = tokio::time::sleep(pause) => {}
                }
            } else {
                tokio::time::sleep(pause).await;
            }
            if Instant::now() >= deadline {
                break;
            }
            trace.step(trace::Kind::Retry, None, None);
            let now = chrono::Utc::now().timestamp();
            transient.retain(|&(candidate, index)| {
                if session
                    .targets
                    .get(index)
                    .is_some_and(|target| target.scheduling.rest_until() <= now)
                {
                    attempts.push_back((candidate, index));
                    false
                } else {
                    true
                }
            });
            if attempts.is_empty() {
                continue;
            }
        }
        let Some((candidate, index)) = attempts.pop_front() else {
            break;
        };
        if retry_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        let Some(target) = session.targets.get(index) else {
            return fail(StatusCode::BAD_REQUEST, "routing_invalid_target");
        };
        if refused_models.contains(&(target.provider_id.clone(), target.model.clone()))
            || overflow_tokens.is_some_and(|tokens| {
                target
                    .effort
                    .context
                    .window()
                    .is_some_and(|window| tokens >= window)
            })
        {
            trace.step(trace::Kind::Context, Some(candidate), None);
            continue;
        }
        if has_images && target.effort.images == Some(false) {
            trace.step(trace::Kind::Images, Some(candidate), None);
            if pinned.is_some() {
                return fail(
                    StatusCode::BAD_REQUEST,
                    "routing_target_does_not_support_images",
                );
            }
            continue;
        }
        let output = target
            .upstream
            .as_ref()
            .and_then(|provider| provider.meta.as_ref())
            .and_then(|meta| meta.api_format.as_deref())
            .and_then(Protocol::from_format)
            .unwrap_or(Protocol::Responses);
        if target.official && protocol != Protocol::Responses {
            return fail(
                StatusCode::BAD_REQUEST,
                "routing_official_protocol_unsupported",
            );
        }
        if target.official
            && !compact.is_some_and(|c| c.native && c.operation == compaction::Operation::Legacy)
            && (body["stream"] != true || body["store"] != false)
        {
            return fail(
                StatusCode::BAD_REQUEST,
                "routing_official_requires_stream_and_store_false",
            );
        }
        if generation.is_some_and(|generation| generation != state.db.routing_generation()) {
            return fail(StatusCode::SERVICE_UNAVAILABLE, "routing_catalog_stale");
        }
        let mut selected_body = body.clone();
        // Native Gemini selects its exact model in the URL, not in JSON.
        if protocol != Protocol::Gemini {
            selected_body["model"] = Value::String(target.model.clone());
        }
        let payload = match super::pi::json::encode_redacted(&bytes, &original, &selected_body) {
            Ok(payload) => payload,
            Err(_) => return fail(StatusCode::BAD_REQUEST, "routing_unsafe_payload_edit"),
        };
        let payload = {
            let rule_effort = matched_rules
                .iter()
                .filter_map(|id| session.group.rules.iter().find(|rule| &rule.id == id))
                .find(|rule| {
                    !rule.use_effort.is_empty()
                        && session.group.members.iter().any(|member| {
                            target.matches_member(member)
                                && (member.id == rule.target_member_id
                                    || rule.fallback_member_ids.contains(&member.id))
                        })
                })
                .map(|rule| rule.use_effort.as_str());
            match crate::services::intent_routing::effort::adapt_member_wire(
                &payload,
                protocol.format(),
                &target.effort,
                target.fixed_effort.as_deref().or(rule_effort),
            ) {
                Ok(payload) => payload,
                Err(code) => return fail(StatusCode::BAD_REQUEST, code),
            }
        };
        let mut headers = target.headers.clone();
        if target.official && crate::codex_client::apply_identity(&mut headers).is_err() {
            return fail(
                StatusCode::BAD_GATEWAY,
                "routing_client_identity_unavailable",
            );
        }
        if refresh_credentials {
            if let Some(credentials) = &target.credentials {
                let refreshed =
                    tokio::time::timeout(Duration::from_secs(30), credentials.token()).await;
                let Ok(Some(token)) = refreshed else {
                    if pinned.is_none()
                        && session.targets.iter().any(|other| {
                            other.official
                                && other.provider_id == target.provider_id
                                && other.model == target.model
                                && other.fixed_effort == target.fixed_effort
                                && !other.same_account(target)
                        })
                    {
                        continue;
                    }
                    return fail(
                        StatusCode::UNAUTHORIZED,
                        "routing_account_changed_or_expired",
                    );
                };
                let Ok(value) = http::HeaderValue::from_str(&format!("Bearer {token}")) else {
                    return fail(StatusCode::UNAUTHORIZED, "routing_account_unavailable");
                };
                headers.insert(header::AUTHORIZATION, value);
            }
        }
        super::protocol_bridge::copy_caller_headers(output, &parts.headers, &mut headers);
        headers.insert(
            header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("identity"),
        );
        if generation.is_some_and(|generation| generation != state.db.routing_generation()) {
            return fail(StatusCode::SERVICE_UNAVAILABLE, "routing_catalog_stale");
        }
        if native_lease.as_ref().is_some_and(|lease| !lease.valid()) {
            return fail(StatusCode::UNAUTHORIZED, "routing_native_unavailable");
        }
        // Once dispatched, a named conversation cannot move after an ambiguous failure.
        if let Some(key) = conversation.as_ref() {
            bindings.bind_session(key, index);
        }
        if let Some(selected) = parts.extensions.get::<native::SelectedTarget>() {
            let Ok(mut selected) = selected.0.write() else {
                return fail(StatusCode::SERVICE_UNAVAILABLE, "routing_state_unavailable");
            };
            *selected = Some(index);
        }
        attempted = true;
        trace.observe_account(candidate, &target.scheduling);
        trace.step(trace::Kind::Attempt, Some(candidate), None);
        let attempt_started = Instant::now();
        let first_content_deadline = (streaming
            && session.group.first_token > 0
            && !attempts.is_empty()
            && pinned.is_none()
            && session.group.routing != crate::services::intent_routing::RoutingStrategy::Manual)
            .then(|| attempt_started + Duration::from_secs(u64::from(session.group.first_token)));
        let attempt_deadline = first_content_deadline
            .into_iter()
            .chain(retry_deadline)
            .min();
        let response = if protocol != output {
            let Some(upstream) = &target.upstream else {
                return fail(StatusCode::BAD_REQUEST, "routing_conversion_unsupported");
            };
            let mut bridge_body = match super::pi::json::parse(&payload) {
                Ok(body) => body,
                Err(_) => return fail(StatusCode::BAD_REQUEST, "routing_invalid_json"),
            };
            bridge_body["model"] = Value::String(target.model.clone());
            // Protocol helpers see only allowed caller protocol metadata, never caller credentials.
            let mut caller_headers = http::HeaderMap::new();
            super::protocol_bridge::copy_caller_headers(
                protocol,
                &parts.headers,
                &mut caller_headers,
            );
            let forward = super::group_forward::forward(
                &state,
                protocol,
                upstream,
                &parts.method,
                &endpoint,
                &bridge_body,
                &caller_headers,
                &parts.extensions,
                conversation.as_deref(),
                scoped.history.clone(),
                scoped.shadow.clone(),
            );
            let forwarded =
                match wait_for_attempt(attempt_deadline, native_lease.as_deref(), forward).await {
                    Ok(result) => result,
                    Err(AttemptWait::Revoked) => {
                        return fail(StatusCode::UNAUTHORIZED, "routing_native_unavailable")
                    }
                    Err(AttemptWait::Deadline) => {
                        if retry_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                            break;
                        }
                        trace.step(trace::Kind::FirstTokenTimeout, Some(candidate), None);
                        continue;
                    }
                };
            match forwarded {
                Ok(response) => response,
                Err(super::ProxyError::UpstreamError {
                    status,
                    body: error_body,
                }) if length::overflow(status, error_body.as_deref().unwrap_or("").as_bytes()) => {
                    trace.step(trace::Kind::Context, Some(candidate), Some(status));
                    if pinned.is_some()
                        || session.group.routing
                            == crate::services::intent_routing::RoutingStrategy::Manual
                    {
                        return fail(StatusCode::BAD_REQUEST, "context_length_exceeded");
                    }
                    overflowed = true;
                    overflow_tokens = if status == 413 {
                        None
                    } else {
                        estimated_tokens
                    };
                    refused_models.insert((target.provider_id.clone(), target.model.clone()));
                    continue;
                }
                Err(super::ProxyError::UpstreamError { status, .. })
                    if pinned.is_none()
                        && session.group.routing
                            != crate::services::intent_routing::RoutingStrategy::Manual
                        && matches!(status, 429 | 502 | 503 | 504) =>
                {
                    trace.step(trace::Kind::Rejected, Some(candidate), Some(status));
                    if status == 429 {
                        target
                            .scheduling
                            .rate_limited(chrono::Utc::now().timestamp());
                    }
                    target
                        .scheduling
                        .failed(&http::HeaderMap::new(), chrono::Utc::now().timestamp());
                    transient.push((candidate, index));
                    continue;
                }
                Err(
                    super::ProxyError::InvalidRequest(_)
                    | super::ProxyError::TransformError(_)
                    | super::ProxyError::ConfigError(_),
                ) => {
                    return fail(StatusCode::BAD_REQUEST, "routing_conversion_unsupported");
                }
                Err(_) => {
                    return fail(
                        StatusCode::BAD_GATEWAY,
                        "routing_upstream_connection_failed",
                    )
                }
            }
        } else {
            let mut url = target.url.clone();
            if protocol == Protocol::Anthropic
                && parts.uri.query() == Some("beta=true")
                && parts
                    .headers
                    .get(header::USER_AGENT)
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.starts_with("claude-cli/"))
            {
                url.query_pairs_mut().append_pair("beta", "true");
            }
            if compact.is_some_and(|c| c.native && c.operation == compaction::Operation::Legacy) {
                let Some(base) = url.path().strip_suffix("/responses") else {
                    return fail(StatusCode::BAD_REQUEST, "routing_compaction_unsupported");
                };
                let path = format!("{base}/responses/compact");
                url.set_path(&path);
            }
            if protocol == Protocol::Gemini
                && endpoint
                    .split('?')
                    .next()
                    .is_some_and(|p| p.ends_with(":streamGenerateContent"))
            {
                let path = url
                    .path()
                    .strip_suffix(":generateContent")
                    .map(|p| format!("{p}:streamGenerateContent"));
                if let Some(path) = path {
                    url.set_path(&path);
                }
                url.query_pairs_mut().append_pair("alt", "sse");
            }
            let dispatch = session
                .client
                .post(url)
                .headers(headers)
                .body(payload)
                .send();
            let dispatched =
                match wait_for_attempt(attempt_deadline, native_lease.as_deref(), dispatch).await {
                    Ok(result) => result,
                    Err(AttemptWait::Revoked) => {
                        return fail(StatusCode::UNAUTHORIZED, "routing_native_unavailable")
                    }
                    Err(AttemptWait::Deadline) => {
                        if retry_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                            break;
                        }
                        trace.step(trace::Kind::FirstTokenTimeout, Some(candidate), None);
                        continue;
                    }
                };
            match dispatched {
                Ok(response) => super::hyper_client::ProxyResponse::Reqwest(response),
                // Ambiguous network failures are never replayed at another account.
                Err(_) => {
                    return fail(
                        StatusCode::BAD_GATEWAY,
                        "routing_upstream_connection_failed",
                    )
                }
            }
        };
        let status = response.status();
        trace.step(
            if status.is_success() {
                trace::Kind::Connected
            } else {
                trace::Kind::Rejected
            },
            Some(candidate),
            Some(status.as_u16()),
        );
        target
            .scheduling
            .observe_headers(response.headers(), chrono::Utc::now().timestamp());
        if matches!(status.as_u16(), 429 | 502 | 503 | 504) {
            if status.as_u16() == 429 {
                target
                    .scheduling
                    .rate_limited(chrono::Utc::now().timestamp());
            }
            target
                .scheduling
                .failed(response.headers(), chrono::Utc::now().timestamp());
        }
        trace.observe_account(candidate, &target.scheduling);
        if pinned.is_none()
            && session.group.routing != crate::services::intent_routing::RoutingStrategy::Manual
            && matches!(status.as_u16(), 429 | 502 | 503 | 504)
        {
            transient.push((candidate, index));
            continue;
        }
        if !status.is_success() {
            if matches!(status.as_u16(), 400 | 413 | 422) {
                let upstream_error = tokio::time::timeout(
                    Duration::from_secs(5),
                    response.bytes_with_limit(64 * 1024),
                )
                .await;
                if upstream_error
                    .ok()
                    .and_then(Result::ok)
                    .is_some_and(|bytes| length::overflow(status.as_u16(), &bytes))
                {
                    if pinned.is_some()
                        || session.group.routing
                            == crate::services::intent_routing::RoutingStrategy::Manual
                    {
                        return fail(StatusCode::BAD_REQUEST, "context_length_exceeded");
                    }
                    overflowed = true;
                    overflow_tokens = if status.as_u16() == 413 {
                        None
                    } else {
                        estimated_tokens
                    };
                    refused_models.insert((target.provider_id.clone(), target.model.clone()));
                    continue;
                }
            }
            return fail(StatusCode::BAD_GATEWAY, "routing_upstream_rejected");
        }
        if response
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_some_and(|v| v != "identity")
        {
            return fail(
                StatusCode::BAD_GATEWAY,
                "routing_upstream_encoding_unsupported",
            );
        }
        if let Some(trace) = parts
            .extensions
            .get::<super::gateway::GatewayUpstreamModel>()
        {
            trace.record(Some(&target.model));
        }
        let response_headers = public_response_headers(response.headers());
        let mut upstream = Box::pin(response.bytes_stream());
        let mut buffered = Vec::new();
        let mut observed_first_content = None;
        if let Some(deadline) = first_content_deadline {
            let deadline = deadline.min(retry_deadline.unwrap_or(deadline));
            let mut probe = Observer::default();
            probe.started = Some(attempt_started);
            let mut probe_bindings = Bindings::default();
            let mut buffered_bytes = 0usize;
            let mut timed_out = false;
            loop {
                let next =
                    wait_for_attempt(Some(deadline), native_lease.as_deref(), upstream.next())
                        .await;
                match next {
                    Err(AttemptWait::Revoked) => {
                        return fail(StatusCode::UNAUTHORIZED, "routing_native_unavailable")
                    }
                    Err(AttemptWait::Deadline) => {
                        timed_out = true;
                        break;
                    }
                    Ok(None) => break,
                    Ok(Some(Err(_))) => {
                        return fail(
                            StatusCode::BAD_GATEWAY,
                            "routing_upstream_connection_failed",
                        )
                    }
                    Ok(Some(Ok(bytes))) => {
                        buffered_bytes = buffered_bytes.saturating_add(bytes.len());
                        probe.push(&bytes, &mut probe_bindings, index);
                        buffered.push(bytes);
                        // Keep the pre-content buffer bounded; never discard a partial reply
                        // after it has been handed to the client.
                        if probe.first_content.is_some()
                            || probe.failed
                            || probe.completed
                            || probe.trace_terminal
                            || buffered_bytes >= 2 * 1024 * 1024
                        {
                            break;
                        }
                    }
                }
            }
            if timed_out {
                if retry_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    break;
                }
                trace.step(trace::Kind::FirstTokenTimeout, Some(candidate), None);
                continue;
            }
            observed_first_content = probe.first_content;
        }
        let mut upstream = futures::stream::iter(buffered.into_iter().map(Ok)).chain(upstream);
        target.scheduling.succeeded();
        trace.observe_account(candidate, &target.scheduling);
        if session.group.routing == crate::services::intent_routing::RoutingStrategy::Weighted {
            session.bindings.lock().await.pool.reassign(index);
        }
        if let Some(key) = conversation.as_ref() {
            bindings.bind_session(key, index);
        }
        let mut observer = Observer::default();
        observer.scheduling = Some(target.scheduling.clone());
        observer.started = streaming.then_some(attempt_started);
        observer.first_content = observed_first_content;
        let mut collector = super::pi::usage::Collector::new(match protocol {
            Protocol::Responses => crate::services::pi_routing::catalog::Protocol::Responses,
            Protocol::Chat => crate::services::pi_routing::catalog::Protocol::Chat,
            Protocol::Anthropic => crate::services::pi_routing::catalog::Protocol::Anthropic,
            Protocol::Gemini => crate::services::pi_routing::catalog::Protocol::Gemini,
        });
        let mut job = super::pi::usage::Job {
            app_type: if external {
                "external"
            } else {
                consumer_app.unwrap_or("codex")
            },
            db: state.db.clone(),
            provider_id: target.provider_id.clone(),
            request_id: trace.id.clone(),
            request_model: target.model.clone(),
            session_id: None,
            usage: Default::default(),
            latency_ms: 0,
            status: status.as_u16(),
            streaming,
        };
        let metadata = super::tool_observation::mount::metadata(
            match protocol {
                Protocol::Responses => super::tool_observation::parsers::PROTOCOL_OPENAI_RESPONSES,
                Protocol::Chat => super::tool_observation::parsers::PROTOCOL_OPENAI_CHAT,
                Protocol::Anthropic => super::tool_observation::parsers::PROTOCOL_ANTHROPIC,
                Protocol::Gemini => super::tool_observation::parsers::PROTOCOL_GEMINI,
            },
            if external {
                "external"
            } else {
                consumer_app.unwrap_or("codex")
            },
            if job.streaming {
                super::tool_observation::channel::Transport::Sse
            } else {
                super::tool_observation::channel::Transport::Json
            },
            if protocol == output {
                super::tool_observation::channel::OutputQuality::Native
            } else {
                super::tool_observation::channel::OutputQuality::ConvertedUnknown
            },
            super::tool_observation::mount::collect_tool_schemas(
                &body,
                if protocol == Protocol::Chat {
                    "openai"
                } else {
                    wire_app.as_str()
                },
            ),
        );
        let account_scheduling = target.scheduling.clone();
        let stream = async_stream::stream! {
            let _lease = scoped;
            let mut content_timed = false;
            // Keep the conversation guard until EOF/drop, including cancellation.
            loop {
                let next = if let Some(lease) = &native_lease {
                    tokio::select! {
                        biased;
                        _ = lease.cancelled() => {
                            trace.finish(401, trace::State::Failed);
                            yield Err(std::io::Error::other("Routing authorization revoked"));
                            return;
                        }
                        next = upstream.next() => next,
                    }
                } else { upstream.next().await };
                let Some(next) = next else { break; };
                match next {
                    Ok(bytes) => { observer.push(&bytes, &mut bindings, index);
                        if !content_timed {
                            if let Some(at) = observer.first_content {
                                trace.first_content(at);
                                content_timed = true;
                            }
                        }
                        collector.push(&bytes); yield Ok::<_, std::io::Error>(bytes); }
                    Err(_) => {
                        trace.finish(502, trace::State::Failed);
                        yield Err(std::io::Error::other("Routing upstream stream failed")); return;
                    }
                }
            }
            observer.finish(&mut bindings, index);
            if observer.completed && !observer.failed && !observer.pending_tools && !observer.oversized {
                if let Some(key) = conversation.as_ref() { bindings.complete_session(key); }
            }
            collector.finish();
            if collector.observed {
                trace.usage(&collector.usage, observer.first_content);
            }
            let (_, _, decode_ms, output_tokens) = observer.sample();
            trace.speed(decode_ms, output_tokens);
            trace.observe_account(candidate, &account_scheduling);
            let trace_failed = observer.failed || (streaming && !observer.completed && !observer.trace_terminal && !observer.oversized);
            trace.finish(
                if trace_failed { 502 } else { status.as_u16() },
                if trace_failed { trace::State::Failed } else { trace::State::Completed },
            );
            if collector.observed && !external {
                job.usage = collector.usage;
                job.latency_ms = started.elapsed().as_millis() as u64;
                super::pi::usage::emit(job);
            }
        };
        let mut response = Response::new(Body::from_stream(
            super::tool_observation::mount::observe_stream(stream, metadata),
        ));
        *response.status_mut() = status;
        *response.headers_mut() = response_headers;
        return response;
    }
    if has_images && !attempted {
        return fail(StatusCode::BAD_REQUEST, "routing_no_image_candidate");
    }
    if overflowed {
        return fail(StatusCode::BAD_REQUEST, "context_length_exceeded");
    }
    fail(StatusCode::BAD_GATEWAY, "routing_candidates_unavailable")
}
enum AttemptWait {
    Deadline,
    Revoked,
}
async fn wait_for_attempt<T>(
    deadline: Option<Instant>,
    lease: Option<&native::Lease>,
    future: impl std::future::Future<Output = T>,
) -> Result<T, AttemptWait> {
    let wait = async {
        match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline.into(), future)
                .await
                .map_err(|_| AttemptWait::Deadline),
            None => Ok(future.await),
        }
    };
    match lease {
        Some(lease) => tokio::select! {
            biased;
            _ = lease.cancelled() => Err(AttemptWait::Revoked),
            result = wait => result,
        },
        None => wait.await,
    }
}
fn public_response_headers(source: &http::HeaderMap) -> http::HeaderMap {
    let mut output = http::HeaderMap::new();
    for name in [
        "content-type",
        "cache-control",
        "retry-after",
        "request-id",
        "x-request-id",
        "x-ratelimit-limit-requests",
        "x-ratelimit-limit-tokens",
        "x-ratelimit-remaining-requests",
        "x-ratelimit-remaining-tokens",
        "x-ratelimit-reset-requests",
        "x-ratelimit-reset-tokens",
        "anthropic-ratelimit-requests-limit",
        "anthropic-ratelimit-requests-remaining",
        "anthropic-ratelimit-requests-reset",
        "anthropic-ratelimit-tokens-limit",
        "anthropic-ratelimit-tokens-remaining",
        "anthropic-ratelimit-tokens-reset",
    ] {
        for value in source
            .get_all(name)
            .iter()
            .filter(|value| value.as_bytes().len() <= 1024)
            .take(4)
        {
            output.append(http::HeaderName::from_static(name), value.clone());
        }
    }
    output
}
fn opaque_continuation(protocol: Protocol, body: &Value) -> bool {
    match protocol {
        Protocol::Anthropic => body["messages"].as_array().is_some_and(|messages| {
            messages.iter().any(|message| {
                message["content"].as_array().is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        matches!(
                            block["type"].as_str(),
                            Some("thinking" | "redacted_thinking")
                        ) || block.get("signature").is_some()
                    })
                })
            })
        }),
        Protocol::Chat => body["messages"].as_array().is_some_and(|messages| {
            messages.iter().any(|message| {
                message.get("reasoning_content").is_some() || message.get("reasoning").is_some()
            })
        }),
        Protocol::Gemini => body["contents"].as_array().is_some_and(|contents| {
            contents.iter().any(|content| {
                content["parts"].as_array().is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part.get("thoughtSignature").is_some()
                            || part["thought"] == true
                            || part.get("functionResponse").is_some()
                    })
                })
            })
        }),
        Protocol::Responses => false,
    }
}
fn request_has_images(protocol: Protocol, body: &Value, projection: &Value) -> bool {
    contains_input_image(projection)
        || match protocol {
            Protocol::Anthropic | Protocol::Chat => {
                body["messages"].as_array().is_some_and(|messages| {
                    messages.iter().any(|message| {
                        message["content"].as_array().is_some_and(|content| {
                            content.iter().any(|part| {
                                matches!(part["type"].as_str(), Some("image" | "image_url"))
                            })
                        })
                    })
                })
            }
            Protocol::Gemini => body["contents"].as_array().is_some_and(|contents| {
                contents.iter().any(|content| {
                    content["parts"].as_array().is_some_and(|parts| {
                        parts.iter().any(|part| {
                            part.get("inlineData").is_some() || part.get("fileData").is_some()
                        })
                    })
                })
            }),
            Protocol::Responses => false,
        }
}
fn contains_input_image(body: &Value) -> bool {
    body.get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("input_image")
                    || item
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(|content| {
                            content.iter().any(|part| {
                                part.get("type").and_then(Value::as_str) == Some("input_image")
                            })
                        })
            })
        })
}

#[cfg(test)]
mod global_tests;
#[cfg(test)]
mod tests;
