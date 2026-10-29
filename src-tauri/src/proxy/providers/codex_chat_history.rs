use super::codex_chat_common::{is_empty_value, response_item_call_id};
use crate::proxy::security::{
    chat_enrichment::{ChatEnrichmentBudget, ChatEnrichmentOwner, EnrichmentIds, EnrichmentItem},
    http_restoration::DlpForwardError,
    UpstreamPayloadOwner,
};
use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

const MAX_CACHED_RESPONSES: usize = 512;

#[derive(Clone)]
struct CachedCall {
    item: Arc<Value>,
    // Lookup snapshots share the actual immutable allocation and its copy charge.
    // Replacing a call drops only the cache's handles, not live snapshots.
    _payload_owner: Option<UpstreamPayloadOwner>,
}

impl std::fmt::Debug for CachedCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CachedCall").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default)]
struct CachedResponse {
    calls_by_id: HashMap<String, CachedCall>,
    call_order: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct CodexChatHistoryInner {
    responses: HashMap<String, CachedResponse>,
    response_order: VecDeque<String>,
    call_index: HashMap<String, VecDeque<String>>,
}

#[derive(Debug, Clone)]
struct CachedLookup {
    snapshot: Arc<CodexChatHistoryInner>,
    previous_index: Option<usize>,
    include_owned: bool,
}

/// Cross-request history needed when Codex Responses is bridged to Chat
/// Completions.
///
/// Chat providers such as DeepSeek require an assistant message with the
/// original tool call and its `reasoning_content` immediately before the tool
/// result. Codex often sends follow-up requests as
/// `previous_response_id + function_call_output`, so this store restores the
/// missing function call before the request is converted to Chat messages.
/// Some Codex flows such as subagents may omit or rewrite
/// `previous_response_id`, so the store can also fall back to a uniquely
/// cached `call_id`.
#[derive(Debug, Default)]
pub struct CodexChatHistoryStore {
    inner: RwLock<Arc<CodexChatHistoryInner>>,
}

impl CodexChatHistoryStore {
    pub async fn record_response(&self, response: &Value) -> usize {
        self.record_response_with_payload_owner(response, None)
            .await
    }

    pub(crate) async fn record_response_with_payload_owner(
        &self,
        response: &Value,
        owner: Option<UpstreamPayloadOwner>,
    ) -> usize {
        let Some(response_id) = response
            .get("id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        else {
            return 0;
        };

        let calls = response
            .get("output")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| cached_call_item(item, owner.as_ref()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if calls.is_empty() {
            return 0;
        }

        self.insert_calls(response_id, calls).await
    }

    async fn record_call_item(
        &self,
        response_id: Option<&str>,
        item: &Value,
        owner: Option<UpstreamPayloadOwner>,
    ) -> bool {
        let Some(response_id) = response_id.filter(|value| !value.is_empty()) else {
            return false;
        };
        let Some(call) = cached_call_item(item, owner.as_ref()) else {
            return false;
        };

        self.insert_calls(response_id, vec![call]).await > 0
    }

    async fn snapshot(&self) -> Arc<CodexChatHistoryInner> {
        let current = self.inner.read().await;
        Arc::clone(&current)
    }

    async fn insert_calls(&self, response_id: &str, calls: Vec<(String, CachedCall)>) -> usize {
        loop {
            let snapshot = self.snapshot().await;
            #[cfg(test)]
            tests::probe_work("build");
            // Clone only metadata and immutable item handles, never another JSON tree.
            // The handles retain the charge for each actual cached allocation.
            let mut next = snapshot.as_ref().clone();
            let inserted = next.insert_calls(response_id, calls.clone());
            let mut candidate = Arc::new(next);
            #[cfg(test)]
            tests::probe_work("prepared");
            let published = {
                let mut current = self.inner.write().await;
                if Arc::ptr_eq(&current, &snapshot) {
                    std::mem::swap(&mut *current, &mut candidate);
                    true
                } else {
                    false
                }
            };
            // Release both rejected candidates and retired snapshots after the guard.
            // Pinning the base snapshot prevents pointer reuse during the check.
            drop(candidate);
            drop(snapshot);
            if published {
                return inserted;
            }
            // A competing publication won. Rebuild against it to preserve its changes.
        }
    }

    /// Legacy count-only enrichment is restricted to ownerless cache entries.
    /// Owned entries require the fallible API and owners retained through sending.
    #[allow(dead_code)] // Retain the ownerless count-only compatibility entry point.
    pub async fn enrich_request(&self, body: &mut Value) -> usize {
        match self.enrich_request_impl(body, false).await {
            Ok((changed, _)) => changed,
            Err(_) => 0,
        }
    }

    /// Build one independently charged candidate, retaining its owner through sending.
    #[cfg(test)]
    pub(crate) async fn enrich_request_with_payload_owners(
        &self,
        body: &mut Value,
    ) -> Result<(usize, Option<ChatEnrichmentOwner>), DlpForwardError> {
        self.enrich_request_with_budget(body, None).await
    }

    pub(crate) async fn enrich_request_with_provenance(
        &self,
        body: &mut Value,
        current_budget: Option<ChatEnrichmentBudget>,
        provenance: crate::proxy::security::chat_enrichment::ChatHistoryProvenance,
    ) -> Result<(usize, Option<ChatEnrichmentOwner>), DlpForwardError> {
        if matches!(
            provenance,
            crate::proxy::security::chat_enrichment::ChatHistoryProvenance::IndependentReadable
        ) {
            return Ok((0, None));
        }
        self.enrich_request_with_budget(body, current_budget).await
    }

    pub(crate) async fn enrich_request_with_budget(
        &self,
        body: &mut Value,
        current_budget: Option<ChatEnrichmentBudget>,
    ) -> Result<(usize, Option<ChatEnrichmentOwner>), DlpForwardError> {
        let previous_id = body
            .get("previous_response_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let Some(input) = body.get("input") else {
            return Ok((0, None));
        };
        let items = match input {
            Value::Array(items) => items.as_slice(),
            Value::Object(_) => std::slice::from_ref(input),
            _ => return Ok((0, None)),
        };
        // No allocation precedes the only await. The original request stays intact.
        let lookup = self.lookup(previous_id).await;
        // Without a current budget, discover actual contributions without building
        // ID sets or a temporary selection map. A budget does not grant authority.
        let mut selected_budget = current_budget;
        if selected_budget.is_none() {
            // The first output inserts its whole restore group, in cache order.
            if let Some(first_output) = items.iter().position(is_output_item) {
                for item in &items[..first_output] {
                    if let Some(call) = enriching_call(&lookup, item) {
                        if let Some(owner) = &call._payload_owner {
                            selected_budget = Some(owner.chat_enrichment_budget()?);
                            break;
                        }
                    }
                }
                if selected_budget.is_none() {
                    let mut consider =
                        |id: &str, call: &CachedCall| -> Result<(), DlpForwardError> {
                            if selected_budget.is_none()
                                && contains_output(items, id)
                                && !contains_call(items, id)
                            {
                                if let Some(owner) = &call._payload_owner {
                                    selected_budget = Some(owner.chat_enrichment_budget()?);
                                }
                            }
                            Ok(())
                        };
                    lookup.visit_ordered_calls(&mut consider)?;
                }
            }
            if selected_budget.is_none() {
                for (index, item) in items.iter().enumerate() {
                    let call = if is_output_item(item) {
                        response_item_call_id(item)
                            .filter(|id| {
                                !items[..index].iter().any(|prior| {
                                    (is_output_item(prior) || is_call_item(prior))
                                        && response_item_call_id(prior) == Some(*id)
                                })
                            })
                            .and_then(|id| lookup.cached_call(id))
                    } else {
                        enriching_call(&lookup, item)
                    };
                    if let Some(owner) = call.and_then(|call| call._payload_owner.as_ref()) {
                        selected_budget = Some(owner.chat_enrichment_budget()?);
                        break;
                    }
                }
            }
        }
        let Some(budget) = selected_budget else {
            // The ownerless-only compatibility path cannot copy guarded entries.
            drop(lookup);
            return self
                .enrich_request_impl(body, false)
                .await
                .map(|(changed, _)| (changed, None));
        };
        let mut output_ids = EnrichmentIds::new(&budget)?;
        let mut existing_ids = EnrichmentIds::new(&budget)?;
        for item in items {
            if let Some(id) = response_item_call_id(item) {
                if is_output_item(item) {
                    output_ids.insert(id)?;
                }
                if is_call_item(item) {
                    existing_ids.insert(id)?;
                }
            }
        }
        let mut group = budget.plan()?;
        lookup.visit_ordered_calls(&mut |id, call| {
            if output_ids.contains(id) && !existing_ids.contains(id) {
                group.push((id, call))?;
            }
            Ok(())
        })?;
        let mut seen = EnrichmentIds::new(&budget)?;
        let mut plan = budget.plan()?;
        let mut inserted_group = false;
        let mut changed = 0usize;
        for item in items {
            if is_output_item(item) {
                if !inserted_group {
                    for &(id, call) in group.as_slice() {
                        plan.push(EnrichmentItem {
                            value: &call.item,
                            cached: None,
                            missing: 0,
                        })?;
                        seen.insert(id)?;
                        changed += 1;
                    }
                    inserted_group = true;
                }
                if let Some(id) = response_item_call_id(item) {
                    if !seen.contains(id) {
                        if let Some(call) = lookup.cached_call(id) {
                            plan.push(EnrichmentItem {
                                value: &call.item,
                                cached: None,
                                missing: 0,
                            })?;
                            seen.insert(id)?;
                            changed += 1;
                        }
                    }
                }
            }
            let mut projected = EnrichmentItem {
                value: item,
                cached: None,
                missing: 0,
            };
            if is_call_item(item) {
                if let Some(id) = response_item_call_id(item) {
                    if let Some(call) = lookup.cached_call(id) {
                        projected.missing = enrichment_mask(item, &call.item);
                        if projected.missing != 0 {
                            projected.cached = Some(&call.item);
                            changed += 1;
                        }
                    }
                    seen.insert(id)?;
                }
            }
            plan.push(projected)?;
        }
        if changed == 0 {
            return Ok((0, None));
        }
        // Every plan and source borrow remains frozen across both construction passes.
        // The complete candidate is built before one infallible transactional swap.
        let (candidate, owner) = budget.build(body, plan.as_slice())?;
        *body = candidate;
        Ok((changed, Some(owner)))
    }

    async fn enrich_request_impl(
        &self,
        body: &mut Value,
        _include_owned: bool,
    ) -> Result<(usize, Vec<UpstreamPayloadOwner>), DlpForwardError> {
        let previous_response_id = body
            .get("previous_response_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty());
        let Some(input) = body.get("input") else {
            return Ok((0, Vec::new()));
        };
        let items = match input {
            Value::Array(items) => items.as_slice(),
            Value::Object(_) => std::slice::from_ref(input),
            _ => return Ok((0, Vec::new())),
        };
        let output_call_ids = items
            .iter()
            .filter(|item| {
                item.get("type")
                    .and_then(|value| value.as_str())
                    .is_some_and(is_call_output_item_type)
            })
            .filter_map(response_item_call_id)
            .collect::<HashSet<_>>();
        let existing_call_ids = items
            .iter()
            .filter(|item| {
                item.get("type")
                    .and_then(|value| value.as_str())
                    .is_some_and(is_call_item_type)
            })
            .filter_map(response_item_call_id)
            .collect::<HashSet<_>>();
        // Keep the original input in place across the only cancellation point.
        let mut lookup = self.lookup(previous_response_id).await;
        lookup.exclude_owned();
        let restore_group = lookup.restore_group(&output_call_ids, &existing_call_ids);
        let restore_group_ids = restore_group
            .iter()
            .map(|(call_id, _)| *call_id)
            .collect::<HashSet<_>>();
        let mut restore_group = Some(restore_group);
        let mut seen_call_ids = HashSet::new();
        let mut changed = 0usize;
        // Owners are declared first so prepared JSON drops before its charges on error.
        let mut payload_owners = Vec::new();
        let mut prepared = Vec::new();
        for item in items {
            let mut before = Vec::new();
            let mut fields = None;
            match item.get("type").and_then(|value| value.as_str()) {
                Some(item_type) if is_call_item_type(item_type) => {
                    if let Some(call_id) = response_item_call_id(item) {
                        if let Some(cached) = lookup.cached_call(call_id) {
                            fields = prepare_call_enrichment(item, cached, &mut payload_owners)?;
                            changed += usize::from(fields.is_some());
                        }
                        seen_call_ids.insert(call_id);
                    }
                }
                Some(item_type) if is_call_output_item_type(item_type) => {
                    if let Some(group) = restore_group.take().filter(|group| !group.is_empty()) {
                        for (call_id, cached) in group {
                            before.push(cached.copy_for_request(&mut payload_owners)?);
                            seen_call_ids.insert(call_id);
                            changed += 1;
                        }
                    }
                    if let Some(call_id) = response_item_call_id(item) {
                        if !seen_call_ids.contains(call_id) && !restore_group_ids.contains(call_id)
                        {
                            if let Some(cached) = lookup.cached_call(call_id) {
                                before.push(cached.copy_for_request(&mut payload_owners)?);
                                seen_call_ids.insert(call_id);
                                changed += 1;
                            }
                        }
                    }
                }
                _ => {}
            }
            prepared.push((before, fields));
        }
        // No fallible payload copies or awaits remain after this point.
        let Some(input) = body.get_mut("input") else {
            return Ok((0, Vec::new()));
        };
        let original_input = std::mem::take(input);
        let original_was_object = original_input.is_object();
        let items = match original_input {
            Value::Array(items) => items,
            other => vec![other],
        };
        let mut new_items = Vec::new();
        for (mut item, (before, fields)) in items.into_iter().zip(prepared) {
            new_items.extend(before);
            if let (Some(object), Some(Value::Object(fields))) = (item.as_object_mut(), fields) {
                object.extend(fields);
            }
            new_items.push(item);
        }
        if changed == 0 && original_was_object {
            *input = new_items.into_iter().next().unwrap_or(Value::Null);
        } else {
            *input = Value::Array(new_items);
        }
        Ok((changed, payload_owners))
    }

    async fn lookup(&self, previous_response_id: Option<&str>) -> CachedLookup {
        let snapshot = self.snapshot().await;
        #[cfg(test)]
        tests::probe_work("lookup");
        let previous_index = previous_response_id.and_then(|id| {
            snapshot
                .response_order
                .iter()
                .position(|cached_id| cached_id == id)
        });
        CachedLookup {
            snapshot,
            previous_index,
            include_owned: true,
        }
    }
}

impl CodexChatHistoryInner {
    fn insert_calls(&mut self, response_id: &str, calls: Vec<(String, CachedCall)>) -> usize {
        #[cfg(test)]
        tests::probe_work("insert");
        if !self.responses.contains_key(response_id) {
            self.response_order.push_back(response_id.to_string());
        }

        let cached_response = self.responses.entry(response_id.to_string()).or_default();
        let mut inserted_or_updated = 0usize;
        let mut indexed_call_ids = Vec::new();
        for (call_id, item) in calls {
            if !cached_response.calls_by_id.contains_key(&call_id) {
                cached_response.call_order.push(call_id.clone());
            }
            cached_response.calls_by_id.insert(call_id.clone(), item);
            indexed_call_ids.push(call_id);
            inserted_or_updated += 1;
        }
        for call_id in indexed_call_ids {
            self.index_call(&call_id, response_id);
        }

        self.prune();
        inserted_or_updated
    }

    fn prune(&mut self) {
        #[cfg(test)]
        tests::probe_work("prune");
        while self.response_order.len() > MAX_CACHED_RESPONSES {
            let Some(response_id) = self.response_order.pop_front() else {
                break;
            };
            self.responses.remove(&response_id);
            self.remove_response_from_call_index(&response_id);
        }
    }

    fn index_call(&mut self, call_id: &str, response_id: &str) {
        #[cfg(test)]
        tests::probe_work("index");
        let response_ids = self.call_index.entry(call_id.to_string()).or_default();
        if !response_ids
            .iter()
            .any(|cached_id| cached_id == response_id)
        {
            response_ids.push_back(response_id.to_string());
        }
    }

    fn remove_response_from_call_index(&mut self, response_id: &str) {
        #[cfg(test)]
        tests::probe_work("prune_index");
        for response_ids in self.call_index.values_mut() {
            response_ids.retain(|cached_id| cached_id != response_id);
        }
        self.call_index
            .retain(|_, response_ids| !response_ids.is_empty());
    }

    fn unique_call(&self, call_id: &str) -> Option<&CachedCall> {
        let response_ids = self.call_index.get(call_id)?;
        let mut found = None;
        for response_id in response_ids {
            let Some(item) = self
                .responses
                .get(response_id)
                .and_then(|response| response.calls_by_id.get(call_id))
            else {
                continue;
            };
            if found.is_some() {
                return None;
            }
            found = Some(item);
        }
        found
    }
}

impl CachedLookup {
    fn previous(&self) -> Option<&CachedResponse> {
        let id = self.snapshot.response_order.get(self.previous_index?)?;
        self.snapshot.responses.get(id)
    }

    fn eligible<'a>(&self, call: &'a CachedCall) -> Option<&'a CachedCall> {
        (self.include_owned || call._payload_owner.is_none()).then_some(call)
    }

    fn cached_call(&self, call_id: &str) -> Option<&CachedCall> {
        #[cfg(test)]
        tests::probe_work("fallback");
        self.previous()
            .and_then(|previous| previous.calls_by_id.get(call_id))
            .or_else(|| self.snapshot.unique_call(call_id))
            .and_then(|call| self.eligible(call))
    }

    fn exclude_owned(&mut self) {
        self.include_owned = false;
    }

    fn visit_ordered_calls<'a>(
        &'a self,
        visit: &mut impl FnMut(&'a str, &'a CachedCall) -> Result<(), DlpForwardError>,
    ) -> Result<(), DlpForwardError> {
        let previous = self.previous();
        if let Some(previous) = previous {
            for id in &previous.call_order {
                if let Some(call) = previous
                    .calls_by_id
                    .get(id)
                    .and_then(|call| self.eligible(call))
                {
                    visit(id, call)?;
                }
            }
        }
        for response_id in &self.snapshot.response_order {
            let Some(response) = self.snapshot.responses.get(response_id) else {
                continue;
            };
            for id in &response.call_order {
                if previous.is_some_and(|previous| previous.calls_by_id.contains_key(id)) {
                    continue;
                }
                if let Some(call) = self
                    .snapshot
                    .unique_call(id)
                    .and_then(|call| self.eligible(call))
                {
                    visit(id, call)?;
                }
            }
        }
        Ok(())
    }

    fn restore_group(
        &self,
        output_call_ids: &HashSet<&str>,
        existing_call_ids: &HashSet<&str>,
    ) -> Vec<(&str, &CachedCall)> {
        let mut group = Vec::new();
        let mut grouped_call_ids = HashSet::new();
        let previous = self.previous();
        if let Some(previous) = previous {
            for call_id in &previous.call_order {
                if output_call_ids.contains(call_id.as_str())
                    && !existing_call_ids.contains(call_id.as_str())
                {
                    if let Some(call) = previous
                        .calls_by_id
                        .get(call_id)
                        .and_then(|call| self.eligible(call))
                    {
                        grouped_call_ids.insert(call_id.as_str());
                        group.push((call_id.as_str(), call));
                    }
                }
            }
        }
        // Preserve insertion order without materializing a selected-call map.
        for response_id in &self.snapshot.response_order {
            let Some(response) = self.snapshot.responses.get(response_id) else {
                continue;
            };
            for call_id in &response.call_order {
                if !output_call_ids.contains(call_id.as_str())
                    || existing_call_ids.contains(call_id.as_str())
                    || grouped_call_ids.contains(call_id.as_str())
                    || previous.is_some_and(|previous| previous.calls_by_id.contains_key(call_id))
                {
                    continue;
                }
                if let Some(call) = self
                    .snapshot
                    .unique_call(call_id)
                    .and_then(|call| self.eligible(call))
                {
                    grouped_call_ids.insert(call_id.as_str());
                    group.push((call_id.as_str(), call));
                }
            }
        }
        group
    }
}

pub fn record_responses_sse_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    history: Arc<CodexChatHistoryStore>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    record_responses_sse_stream_with_payload_owner(stream, history, None)
}

pub(crate) fn record_responses_sse_stream_with_payload_owner(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    history: Arc<CodexChatHistoryStore>,
    owner: Option<UpstreamPayloadOwner>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut current_response_id: Option<String> = None;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        inspect_sse_block(
                            &block,
                            &mut current_response_id,
                            history.as_ref(),
                            owner.as_ref(),
                        ).await;
                    }
                    yield Ok(bytes);
                }
                Err(err) => yield Err(err),
            }
        }
    }
}

async fn inspect_sse_block(
    block: &str,
    current_response_id: &mut Option<String>,
    history: &CodexChatHistoryStore,
    owner: Option<&UpstreamPayloadOwner>,
) {
    if block.trim().is_empty() {
        return;
    }

    let mut data_parts = Vec::new();
    for line in block.lines() {
        if let Some(data) = strip_sse_field(line, "data") {
            data_parts.push(data.to_string());
        }
    }

    let data = data_parts.join("\n");
    if data.trim().is_empty() || data.trim() == "[DONE]" {
        return;
    }

    let Ok(value) = serde_json::from_str::<Value>(&data) else {
        return;
    };

    if let Some(response_id) = value
        .pointer("/response/id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
    {
        *current_response_id = Some(response_id.to_string());
    }

    match value.get("type").and_then(|value| value.as_str()) {
        Some("response.output_item.done") => {
            if let Some(item) = value.get("item") {
                history
                    .record_call_item(current_response_id.as_deref(), item, owner.cloned())
                    .await;
            }
        }
        Some("response.completed") => {
            if let Some(response) = value.get("response") {
                history
                    .record_response_with_payload_owner(response, owner.cloned())
                    .await;
            }
        }
        _ => {}
    }
}

impl CachedCall {
    fn copy_for_request(
        &self,
        owners: &mut Vec<UpstreamPayloadOwner>,
    ) -> Result<Value, DlpForwardError> {
        match &self._payload_owner {
            Some(owner) => {
                let (copy, copy_owner) = owner.try_clone_json(&self.item)?;
                owners.push(copy_owner);
                Ok(copy)
            }
            None => Ok(self.item.as_ref().clone()),
        }
    }
}

fn cached_call_item(
    item: &Value,
    owner: Option<&UpstreamPayloadOwner>,
) -> Option<(String, CachedCall)> {
    if !item
        .get("type")
        .and_then(|value| value.as_str())
        .is_some_and(is_call_item_type)
    {
        return None;
    }
    let call_id = response_item_call_id(item)?;
    let (item, owner) = match owner {
        // Admission failure skips this optional entry; never retain an uncharged copy.
        Some(owner) => {
            let (copy, copy_owner) = owner.try_clone_json(item).ok()?;
            (copy, Some(copy_owner))
        }
        None => (item.clone(), None),
    };
    Some((
        call_id.to_string(),
        CachedCall {
            item: Arc::new(item),
            _payload_owner: owner,
        },
    ))
}

fn is_call_item_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "function_call" | "custom_tool_call" | "tool_search_call"
    )
}

fn is_call_output_item_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "function_call_output" | "custom_tool_call_output" | "tool_search_output"
    )
}

fn is_call_item(item: &Value) -> bool {
    item.get("type")
        .and_then(Value::as_str)
        .is_some_and(is_call_item_type)
}
fn is_output_item(item: &Value) -> bool {
    item.get("type")
        .and_then(Value::as_str)
        .is_some_and(is_call_output_item_type)
}
fn contains_call(items: &[Value], id: &str) -> bool {
    items
        .iter()
        .any(|item| is_call_item(item) && response_item_call_id(item) == Some(id))
}
fn contains_output(items: &[Value], id: &str) -> bool {
    items
        .iter()
        .any(|item| is_output_item(item) && response_item_call_id(item) == Some(id))
}
fn enrichment_mask(item: &Value, cached: &Value) -> u8 {
    if !item.is_object() {
        return 0;
    }
    ENRICHMENT_FIELDS
        .iter()
        .enumerate()
        .fold(0, |mask, (index, key)| {
            if !item.get(key).is_some_and(|value| !is_empty_value(value))
                && cached.get(key).is_some_and(|value| !is_empty_value(value))
            {
                mask | (1u8 << index)
            } else {
                mask
            }
        })
}
fn enriching_call<'a>(lookup: &'a CachedLookup, item: &Value) -> Option<&'a CachedCall> {
    if !is_call_item(item) {
        return None;
    }
    lookup
        .cached_call(response_item_call_id(item)?)
        .filter(|call| enrichment_mask(item, &call.item) != 0)
}

const ENRICHMENT_FIELDS: [&str; 8] = [
    "name",
    "namespace",
    "arguments",
    "input",
    "status",
    "execution",
    "reasoning_content",
    "reasoning",
];

fn prepare_call_enrichment(
    item: &Value,
    cached: &CachedCall,
    owners: &mut Vec<UpstreamPayloadOwner>,
) -> Result<Option<Value>, DlpForwardError> {
    let missing = |key: &str| {
        !item.get(key).is_some_and(|value| !is_empty_value(value))
            && cached
                .item
                .get(key)
                .is_some_and(|value| !is_empty_value(value))
    };
    if !item.is_object() || !ENRICHMENT_FIELDS.iter().any(|key| missing(key)) {
        return Ok(None);
    }
    // Copy once under a full-tree charge, then move only the missing fields.
    // Retaining the conservative full charge also covers the copied field keys.
    let mut fields = cached.copy_for_request(owners)?;
    if let Some(object) = fields.as_object_mut() {
        object.retain(|key, _| ENRICHMENT_FIELDS.contains(&key.as_str()) && missing(key));
    }
    Ok(Some(fields))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use serde_json::json;

    const SYNTHETIC_TOKEN: &str = "vary_cu_00000000000000000000_ai";

    fn tokenized_response(response_id: &str, call_id: &str) -> Value {
        json!({
            "id": response_id,
            "output": [{
                "type": "function_call",
                "call_id": call_id,
                "name": "synthetic_tool",
                "arguments": json!({"value": SYNTHETIC_TOKEN}).to_string(),
                "reasoning_content": SYNTHETIC_TOKEN
            }]
        })
    }

    fn payload_owner_fixture() -> Result<(UpstreamPayloadOwner, impl Fn() -> usize), std::io::Error>
    {
        UpstreamPayloadOwner::test_fixture().map_err(|error| std::io::Error::other(error.code()))
    }

    type WorkHook = std::rc::Rc<dyn Fn(&'static str)>;
    type WorkStages = std::rc::Rc<std::cell::RefCell<HashSet<&'static str>>>;

    thread_local! {
        static WORK_HOOK: std::cell::RefCell<Option<WorkHook>> = const {
            std::cell::RefCell::new(None)
        };
    }

    pub(super) fn probe_work(stage: &'static str) {
        let hook = WORK_HOOK.with(|slot| slot.borrow().clone());
        if let Some(hook) = hook {
            hook(stage);
        }
    }

    struct WorkProbe;

    impl WorkProbe {
        fn install(hook: impl Fn(&'static str) + 'static) -> Self {
            WORK_HOOK.with(|slot| {
                assert!(slot.borrow_mut().replace(std::rc::Rc::new(hook)).is_none());
            });
            Self
        }
    }
    #[test]
    fn borrowed_history_call_ids_preserve_precedence_and_trim_rules() {
        for (item, expected) in [
            (
                json!({"call_id":" call_primary ","id":"call_fallback"}),
                Some("call_primary"),
            ),
            (json!({"id":" call_fallback "}), Some("call_fallback")),
            (json!({"call_id":" ","id":"call_fallback"}), None),
            (json!({"call_id":null,"id":"call_fallback"}), None),
            (json!({"call_id":7,"id":"call_fallback"}), None),
            (json!({}), None),
        ] {
            assert_eq!(response_item_call_id(&item), expected);
        }
    }

    #[tokio::test]
    async fn lookup_borrowed_groups_preserve_order_ambiguity_and_owner_filter(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let mut previous = tokenized_response("resp_previous", "call_previous_z");
        previous["output"]
            .as_array_mut()
            .expect("synthetic output")
            .push(tokenized_response("ignored", "call_existing")["output"][0].clone());
        history.record_response(&previous).await;
        let (owner, _) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_previous", "call_owned"),
                Some(owner),
            )
            .await;
        for (response_id, call_id) in [
            ("resp_b", "call_fallback_b"),
            ("resp_a", "call_fallback_a"),
            ("resp_a", "call_previous_z"),
            ("resp_a", "call_ambiguous"),
            ("resp_other", "call_ambiguous"),
        ] {
            history
                .record_response(&tokenized_response(response_id, call_id))
                .await;
        }
        let snapshot = history.snapshot().await;
        let outputs = HashSet::from([
            "call_previous_z",
            "call_existing",
            "call_owned",
            "call_fallback_b",
            "call_fallback_a",
            "call_ambiguous",
        ]);
        let existing = HashSet::from(["call_existing"]);
        let mut lookup = history.lookup(Some("resp_previous")).await;
        let group = lookup.restore_group(&outputs, &existing);
        assert_eq!(
            group.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [
                "call_previous_z",
                "call_owned",
                "call_fallback_b",
                "call_fallback_a"
            ]
        );
        for (id, call) in &group {
            let response_id = match *id {
                "call_fallback_b" => "resp_b",
                "call_fallback_a" => "resp_a",
                _ => "resp_previous",
            };
            let response = &snapshot.responses[response_id];
            let original_id = response
                .call_order
                .iter()
                .find(|cached| cached.as_str() == *id)
                .expect("synthetic ordered id");
            assert_eq!(
                id.as_ptr(),
                original_id.as_ptr(),
                "group must borrow the actual cached ID"
            );
            assert!(
                std::ptr::eq(*call, &response.calls_by_id[*id]),
                "group must borrow actual call metadata"
            );
        }
        assert!(lookup.cached_call("call_ambiguous").is_none());
        drop(group);
        lookup.exclude_owned();
        assert!(lookup.cached_call("call_owned").is_none());
        let group = lookup.restore_group(&outputs, &existing);
        assert_eq!(
            group.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            ["call_previous_z", "call_fallback_b", "call_fallback_a"]
        );
        Ok(())
    }

    #[tokio::test]
    async fn lookup_snapshot_replacement_keeps_old_borrowed_metadata_valid() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&tokenized_response("resp_previous", "call_previous"))
            .await;
        let old = history.lookup(Some("resp_previous")).await;
        let cloned = old.clone();
        let mut replacement = tokenized_response("resp_previous", "call_previous");
        replacement["output"][0]["name"] = json!("synthetic_replacement");
        history.record_response(&replacement).await;
        let current = history.lookup(Some("resp_previous")).await;
        let original = old
            .cached_call("call_previous")
            .expect("old synthetic call");
        let cloned_call = cloned
            .cached_call("call_previous")
            .expect("cloned synthetic call");
        let updated = current
            .cached_call("call_previous")
            .expect("updated synthetic call");
        assert!(std::ptr::eq(original, cloned_call));
        assert!(!std::ptr::eq(original, updated));
        assert_eq!(original.item["name"], "synthetic_tool");
        assert_eq!(updated.item["name"], "synthetic_replacement");
        drop(history);
        assert_eq!(
            old.cached_call("call_previous")
                .expect("pinned synthetic call")
                .item["name"],
            "synthetic_tool"
        );
    }

    #[test]
    fn history_call_id_extraction_borrows_trimmed_input() {
        let item = json!({"call_id":"  call_synthetic  ", "id":"ignored_synthetic"});
        let source = item["call_id"].as_str().expect("synthetic id").trim();
        let extracted = response_item_call_id(&item).expect("extracted synthetic id");
        assert_eq!(extracted, source);
        assert_eq!(
            extracted.as_ptr(),
            source.as_ptr(),
            "history IDs must borrow the original trimmed input, not allocate independently"
        );
    }

    impl Drop for WorkProbe {
        fn drop(&mut self) {
            WORK_HOOK.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }

    impl Drop for CachedCall {
        fn drop(&mut self) {
            probe_work("drop_call");
        }
    }

    impl Drop for CodexChatHistoryInner {
        fn drop(&mut self) {
            probe_work("drop_snapshot");
        }
    }

    fn lock_probe(history: &Arc<CodexChatHistoryStore>) -> (WorkProbe, WorkStages, WorkStages) {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(HashSet::new()));
        let blocked = std::rc::Rc::new(std::cell::RefCell::new(HashSet::new()));
        let observed = std::rc::Rc::clone(&seen);
        let failures = std::rc::Rc::clone(&blocked);
        let history = Arc::downgrade(history);
        let probe = WorkProbe::install(move |stage| {
            observed.borrow_mut().insert(stage);
            if let Some(history) = history.upgrade() {
                if history.inner.try_write().is_err() {
                    failures.borrow_mut().insert(stage);
                }
            }
        });
        (probe, seen, blocked)
    }

    #[tokio::test]
    async fn lookup_previous_borrows_actual_snapshot_metadata() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&tokenized_response("resp_previous", "call_previous"))
            .await;
        let snapshot = history.snapshot().await;
        let lookup = history.lookup(Some("resp_previous")).await;
        let selected = lookup
            .cached_call("call_previous")
            .expect("synthetic previous call");
        let original = &snapshot.responses["resp_previous"].calls_by_id["call_previous"];
        assert!(
            std::ptr::eq(selected, original),
            "lookup must borrow actual snapshot metadata, not independently clone it"
        );
    }

    #[tokio::test]
    async fn lookup_fallback_borrows_actual_snapshot_metadata() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&tokenized_response("resp_unique", "call_unique"))
            .await;
        let snapshot = history.snapshot().await;
        let lookup = history.lookup(None).await;
        let selected = lookup
            .cached_call("call_unique")
            .expect("synthetic fallback call");
        let original = &snapshot.responses["resp_unique"].calls_by_id["call_unique"];
        assert!(
            std::ptr::eq(selected, original),
            "fallback must borrow actual snapshot metadata, not independently clone it"
        );
    }

    #[tokio::test]
    async fn runtime_lookup_and_fallback_work_is_lock_free() {
        let history = Arc::new(CodexChatHistoryStore::default());
        for (response_id, call_id) in [
            ("resp_previous", "call_previous"),
            ("resp_unique", "call_unique"),
            ("resp_ambiguous_a", "call_ambiguous"),
            ("resp_ambiguous_b", "call_ambiguous"),
        ] {
            history
                .record_response(&tokenized_response(response_id, call_id))
                .await;
        }
        let (_probe, seen, blocked) = lock_probe(&history);
        let lookup = history.lookup(Some("resp_previous")).await;
        assert!(lookup.cached_call("call_previous").is_some());
        assert!(lookup.cached_call("call_unique").is_some());
        assert!(lookup.cached_call("call_ambiguous").is_none());
        drop(lookup);
        for stage in ["lookup", "fallback"] {
            assert!(seen.borrow().contains(stage), "missing work probe: {stage}");
        }
        assert!(
            blocked.borrow().is_empty(),
            "history lock held during runtime work: {:?}",
            blocked.borrow()
        );
    }

    #[tokio::test]
    async fn runtime_index_prune_and_drop_work_is_lock_free() -> Result<(), std::io::Error> {
        let history = Arc::new(CodexChatHistoryStore::default());
        let (owner, used) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_evicted", "call_evicted"),
                Some(owner),
            )
            .await;
        for index in 1..MAX_CACHED_RESPONSES {
            history
                .record_response(&tokenized_response(&format!("resp_{index}"), "call_shared"))
                .await;
        }
        let (_probe, seen, blocked) = lock_probe(&history);
        for _ in 0..2 {
            assert_eq!(
                history
                    .record_response(&tokenized_response("resp_new", "call_new"))
                    .await,
                1
            );
        }
        assert_eq!(used(), 0, "eviction must release the old copy charge");
        let inner = history.inner.read().await;
        assert_eq!(inner.responses.len(), MAX_CACHED_RESPONSES);
        assert!(!inner.call_index.contains_key("call_evicted"));
        drop(inner);
        for stage in [
            "build",
            "insert",
            "index",
            "prune",
            "prune_index",
            "drop_call",
            "drop_snapshot",
        ] {
            assert!(seen.borrow().contains(stage), "missing work probe: {stage}");
        }
        assert!(
            blocked.borrow().is_empty(),
            "history lock held during runtime work: {:?}",
            blocked.borrow()
        );
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_snapshot_writers_preserve_responses_and_appended_calls(
    ) -> Result<(), std::io::Error> {
        // Separate OS threads let a synchronous probe force two prepared writes.
        for same_response in [false, true] {
            let history = Arc::new(CodexChatHistoryStore::default());
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let mut writers = Vec::new();
            for index in 0..2 {
                let history = Arc::clone(&history);
                let barrier = Arc::clone(&barrier);
                writers.push(std::thread::spawn(
                    move || -> Result<(usize, usize), std::io::Error> {
                        let attempts = std::rc::Rc::new(std::cell::Cell::new(0));
                        let observed = std::rc::Rc::clone(&attempts);
                        let _probe = WorkProbe::install(move |stage| {
                            if stage == "prepared" {
                                let attempt = observed.get();
                                observed.set(attempt + 1);
                                if attempt == 0 {
                                    barrier.wait();
                                }
                            }
                        });
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()?;
                        let response_id = if same_response || index == 0 {
                            "resp_first"
                        } else {
                            "resp_second"
                        };
                        let call_id = if index == 0 {
                            "call_first"
                        } else {
                            "call_second"
                        };
                        let response = tokenized_response(response_id, call_id);
                        runtime.block_on(async {
                            if index == 0 {
                                Ok((history.record_response(&response).await, attempts.get()))
                            } else {
                                Ok((
                                    usize::from(
                                        history
                                            .record_call_item(
                                                Some(response_id),
                                                &response["output"][0],
                                                None,
                                            )
                                            .await,
                                    ),
                                    attempts.get(),
                                ))
                            }
                        })
                    },
                ));
            }
            let mut total_attempts = 0;
            for writer in writers {
                let (recorded, attempts) = writer
                    .join()
                    .map_err(|_| std::io::Error::other("synthetic history writer failed"))??;
                assert_eq!(recorded, 1);
                total_attempts += attempts;
            }
            assert!(
                total_attempts >= 3,
                "a stale prepared snapshot must be retried"
            );
            let lookup = history.lookup(None).await;
            assert!(lookup.cached_call("call_first").is_some());
            assert!(lookup.cached_call("call_second").is_some());
            let inner = history.inner.read().await;
            assert_eq!(inner.responses.len(), if same_response { 1 } else { 2 });
            assert_eq!(inner.call_index.len(), 2);
            if same_response {
                assert_eq!(inner.responses["resp_first"].call_order.len(), 2);
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn cancelling_history_publication_keeps_snapshot_and_releases_candidate_copies(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&tokenized_response("resp_existing", "call_existing"))
            .await;
        let (owner, used) = payload_owner_fixture()?;
        let retained_charge = used();
        let response = tokenized_response("resp_cancelled", "call_cancelled");
        // Readers permit preparing a snapshot, but prevent publishing it.
        let lock = history.inner.read().await;
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(10),
            history.record_response_with_payload_owner(&response, Some(owner.clone()))
        )
        .await
        .is_err());
        assert_eq!(used(), retained_charge);
        assert_eq!(lock.responses.len(), 1);
        assert!(!lock.responses.contains_key("resp_cancelled"));
        drop(lock);
        let lookup = history.lookup(None).await;
        assert!(lookup.cached_call("call_existing").is_some());
        assert!(lookup.cached_call("call_cancelled").is_none());
        drop(owner);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn chat_enrichment_budget_current_is_independent_of_exhausted_source(
    ) -> Result<(), std::io::Error> {
        let response = tokenized_response("resp_owned", "call_owned");
        let probe_history = CodexChatHistoryStore::default();
        let (probe, probe_used) = payload_owner_fixture()?;
        probe_history
            .record_response_with_payload_owner(&response, Some(probe))
            .await;
        let retained = probe_used();
        drop(probe_history);
        let history = CodexChatHistoryStore::default();
        let (source, source_used) = UpstreamPayloadOwner::test_fixture_with_limit(retained)
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(
            history
                .record_response_with_payload_owner(&response, Some(source))
                .await,
            1
        );
        let (current, current_used) = payload_owner_fixture()?;
        let current_base = current_used();
        let mut request =
            json!({"input":[{"type":"function_call_output","call_id":"call_owned","output":"ok"}]});
        let (changed, candidate) = history
            .enrich_request_with_budget(
                &mut request,
                Some(
                    current
                        .chat_enrichment_budget()
                        .map_err(|error| std::io::Error::other(error.code()))?,
                ),
            )
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 1);
        let candidate = candidate.expect("synthetic candidate");
        assert_eq!(current_used(), current_base + candidate.charged_for_test());
        assert_eq!(source_used(), retained);
        drop(history);
        assert_eq!(
            source_used(),
            0,
            "source allocation need not be pinned by an independent candidate"
        );
        drop(request);
        drop(candidate);
        assert_eq!(current_used(), current_base);
        drop(current);
        assert_eq!(current_used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn chat_enrichment_budget_full_current_never_falls_back_to_source(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (source, source_used) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_owned"),
                Some(source),
            )
            .await;
        let retained = source_used();
        let (current, current_used) = UpstreamPayloadOwner::test_fixture_with_limit(64 * 1024)
            .map_err(|error| std::io::Error::other(error.code()))?;
        let mut request =
            json!({"input":[{"type":"function_call_output","call_id":"call_owned","output":"ok"}]});
        let original = request.clone();
        let result = history
            .enrich_request_with_budget(
                &mut request,
                Some(
                    current
                        .chat_enrichment_budget()
                        .map_err(|error| std::io::Error::other(error.code()))?,
                ),
            )
            .await;
        assert!(matches!(result, Err(DlpForwardError::CapacityExceeded)));
        assert_eq!(request, original);
        assert_eq!(source_used(), retained);
        assert_eq!(current_used(), 64 * 1024);
        drop(current);
        drop(history);
        assert_eq!(source_used(), 0);
        assert_eq!(current_used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn chat_enrichment_budget_ownerless_legacy_and_unused_guarded_have_no_new_buffer(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&tokenized_response("resp_legacy", "call_legacy"))
            .await;
        let (source, used) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_owned"),
                Some(source),
            )
            .await;
        let retained = used();
        let mut request = json!({"input":[{"type":"function_call_output","call_id":"call_legacy","output":"ok"}]});
        let (changed, candidate) = history
            .enrich_request_with_payload_owners(&mut request)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 1);
        assert!(candidate.is_none());
        assert_eq!(used(), retained);
        let mut no_change = tokenized_response("resp_owned", "call_owned");
        no_change["input"] = no_change["output"].take();
        let (changed, candidate) = history
            .enrich_request_with_payload_owners(&mut no_change)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 0);
        assert!(candidate.is_none());
        assert_eq!(used(), retained);
        drop(history);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn chat_enrichment_budget_mixed_candidate_keeps_only_one_full_charge(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&tokenized_response("resp_mixed", "call_legacy"))
            .await;
        let (source, used) = payload_owner_fixture()?;
        let (other, other_used) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_mixed", "call_owned"),
                Some(source),
            )
            .await;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_mixed", "call_other"),
                Some(other),
            )
            .await;
        let mut request = json!({"previous_response_id":"resp_mixed", "input":[
            {"type":"function_call_output","call_id":"call_other","output":"ok"},
            {"type":"function_call_output","call_id":"call_legacy","output":"ok"},
            {"type":"function_call_output","call_id":"call_owned","output":"ok"}
        ]});
        let (changed, candidate) = history
            .enrich_request_with_payload_owners(&mut request)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 3);
        assert_eq!(request["input"][0]["call_id"], "call_legacy");
        assert_eq!(request["input"][1]["call_id"], "call_owned");
        assert_eq!(request["input"][2]["call_id"], "call_other");
        drop(history);
        assert_eq!(
            used(),
            candidate
                .as_ref()
                .expect("synthetic candidate")
                .charged_for_test()
        );
        assert_eq!(other_used(), 0);
        drop(request);
        drop(candidate);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn chat_enrichment_plan_refuses_when_only_cached_copy_fits() -> Result<(), std::io::Error>
    {
        let response = tokenized_response("resp_owned", "call_owned");
        let probe_history = CodexChatHistoryStore::default();
        let (probe, probe_used) = payload_owner_fixture()?;
        let workspace = probe_used();
        probe_history
            .record_response_with_payload_owner(&response, Some(probe))
            .await;
        let retained = probe_used();
        let copy_charge = retained - workspace;
        drop(probe_history);
        assert_eq!(probe_used(), 0);
        let history = CodexChatHistoryStore::default();
        let (owner, used) = UpstreamPayloadOwner::test_fixture_with_limit(retained + copy_charge)
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(
            history
                .record_response_with_payload_owner(&response, Some(owner))
                .await,
            1
        );
        // Many borrowed IDs require independently charged plan backing, even
        // though only one immutable cached call is selected for insertion.
        let mut request = json!({"input": (0..128).map(|_| json!({
            "type": "function_call_output", "call_id": "call_owned", "output": "ok"
        })).collect::<Vec<_>>()});
        let original = request.clone();
        let result = history
            .enrich_request_with_payload_owners(&mut request)
            .await;
        assert!(
            matches!(result, Err(DlpForwardError::CapacityExceeded)),
            "plan backing must be charged independently of cached JSON copies"
        );
        assert_eq!(request, original);
        assert_eq!(used(), retained);
        drop(history);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn chat_enrichment_candidate_refuses_unaccounted_original_body_copy(
    ) -> Result<(), std::io::Error> {
        let response = tokenized_response("resp_owned", "call_owned");
        let history = CodexChatHistoryStore::default();
        let (owner, used) = UpstreamPayloadOwner::test_fixture_with_limit(128 * 1024)
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(
            history
                .record_response_with_payload_owner(&response, Some(owner))
                .await,
            1
        );
        let retained = used();
        let mut request = json!({
            "instructions": "synthetic-padding-".repeat(8192),
            "input": [{"type": "function_call_output", "call_id": "call_owned", "output": "ok"}]
        });
        let original = request.clone();
        let result = history
            .enrich_request_with_payload_owners(&mut request)
            .await;
        assert!(
            matches!(result, Err(DlpForwardError::CapacityExceeded)),
            "the complete independent candidate must reserve before construction"
        );
        assert_eq!(request, original);
        assert_eq!(used(), retained);
        drop(history);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn chat_enrichment_lifetime_retains_complete_candidate_charge(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (owner, used) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_owned"),
                Some(owner.clone()),
            )
            .await;
        let retained = used();
        let mut request = json!({"instructions": "synthetic context", "input": [
            {"type": "function_call_output", "call_id": "call_owned", "output": "ok"}
        ]});
        let (changed, owners) = history
            .enrich_request_with_payload_owners(&mut request)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 1);
        let candidate_charge = used() - retained;
        let (comparison, comparison_owner) = owner
            .try_clone_json(&request)
            .map_err(|error| std::io::Error::other(error.code()))?;
        let full_copy_charge = used() - retained - candidate_charge;
        drop(comparison);
        drop(comparison_owner);
        assert_eq!(
            candidate_charge, full_copy_charge,
            "candidate owner must retain the complete independently built tree charge"
        );
        drop(request);
        drop(owners);
        assert_eq!(used(), retained);
        drop(history);
        drop(owner);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn owned_cache_admission_and_enrichment_capacity_fail_without_unbudgeted_copies(
    ) -> Result<(), std::io::Error> {
        let response = tokenized_response("resp_owned", "call_owned");
        let (probe, probe_used) = payload_owner_fixture()?;
        let workspace_charge = probe_used();
        let history = CodexChatHistoryStore::default();
        history
            .record_response_with_payload_owner(&response, Some(probe))
            .await;
        let cache_copy_charge = probe_used() - workspace_charge;
        drop(history);
        assert_eq!(probe_used(), 0);

        let history = CodexChatHistoryStore::default();
        let (owner, used) = UpstreamPayloadOwner::test_fixture_with_limit(workspace_charge)
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(
            history
                .record_response_with_payload_owner(&response, Some(owner))
                .await,
            0
        );
        assert_eq!(used(), 0);

        let history = CodexChatHistoryStore::default();
        let (owner, used) =
            UpstreamPayloadOwner::test_fixture_with_limit(workspace_charge + cache_copy_charge)
                .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(
            history
                .record_response_with_payload_owner(&response, Some(owner))
                .await,
            1
        );
        for input in [
            json!([{"type": "function_call_output", "call_id": "call_owned", "output": "ok"}]),
            json!({"type": "function_call", "call_id": "call_owned"}),
        ] {
            let mut request = json!({"previous_response_id": "resp_owned", "input": input});
            let original = request.clone();
            let charge = used();
            let result = history
                .enrich_request_with_payload_owners(&mut request)
                .await;
            assert!(matches!(result, Err(DlpForwardError::CapacityExceeded)));
            assert_eq!(
                request, original,
                "capacity failure must not mutate the request"
            );
            assert_eq!(used(), charge);
        }
        drop(history);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn enrichment_releases_prepared_copies_if_a_later_copy_exceeds_capacity(
    ) -> Result<(), std::io::Error> {
        let mut response = tokenized_response("resp_owned", "call_first");
        response["output"]
            .as_array_mut()
            .ok_or_else(|| std::io::Error::other("missing output"))?
            .push(tokenized_response("resp_owned", "call_other")["output"][0].clone());
        let probe_history = CodexChatHistoryStore::default();
        let (probe, probe_used) = payload_owner_fixture()?;
        let workspace_charge = probe_used();
        probe_history
            .record_response_with_payload_owner(&response, Some(probe))
            .await;
        let retained_charge = probe_used();
        let copy_charge = (retained_charge - workspace_charge) / 2;
        drop(probe_history);
        assert_eq!(probe_used(), 0);
        let history = CodexChatHistoryStore::default();
        let (owner, used) =
            UpstreamPayloadOwner::test_fixture_with_limit(retained_charge + copy_charge)
                .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(
            history
                .record_response_with_payload_owner(&response, Some(owner))
                .await,
            2
        );
        let mut request = json!({
            "previous_response_id": "resp_owned",
            "input": [
                {"type": "function_call", "call_id": "call_first"},
                {"type": "function_call_output", "call_id": "call_other", "output": "ok"}
            ]
        });
        let original = request.clone();
        let result = history
            .enrich_request_with_payload_owners(&mut request)
            .await;
        assert!(matches!(result, Err(DlpForwardError::CapacityExceeded)));
        assert_eq!(request, original);
        assert_eq!(
            used(),
            retained_charge,
            "failed plans must release earlier copies too"
        );
        drop(history);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn count_only_enrichment_does_not_copy_owned_cache_items() -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (owner, used) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_owned"),
                Some(owner),
            )
            .await;
        let mut request = json!({
            "previous_response_id": "resp_owned",
            "input": [{"type": "function_call_output", "call_id": "call_owned", "output": "ok"}]
        });
        let original = request.clone();
        let charge = used();
        assert_eq!(history.enrich_request(&mut request).await, 0);
        assert_eq!(request, original);
        assert_eq!(used(), charge);
        drop(history);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn cancelling_enrichment_during_lookup_leaves_request_unchanged() {
        let history = CodexChatHistoryStore::default();
        let mut request = json!({
            "input": [{"type": "function_call_output", "call_id": "call_owned", "output": "ok"}]
        });
        let original = request.clone();
        let _lock = history.inner.write().await;
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(10),
            history.enrich_request_with_payload_owners(&mut request)
        )
        .await
        .is_err());
        assert_eq!(request, original);
    }

    #[tokio::test]
    async fn independent_enrichment_copies_reserve_and_release_separate_payload_charges(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (owner, used) = payload_owner_fixture()?;
        let upstream_charge = used();
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_owned"),
                Some(owner),
            )
            .await;
        let cached_charge = used();
        let make_request = || {
            json!({
                "previous_response_id": "resp_owned",
                "input": [{"type": "function_call_output", "call_id": "call_owned", "output": "ok"}]
            })
        };
        let mut first = make_request();
        let (first_changed, first_owners) = history
            .enrich_request_with_payload_owners(&mut first)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(first_changed, 1);
        let first_charge = used();
        assert!(
            first_charge > cached_charge,
            "a retained request JSON copy needs a distinct reservation"
        );
        let mut second = make_request();
        let (second_changed, second_owners) = history
            .enrich_request_with_payload_owners(&mut second)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(second_changed, 1);
        let second_charge = used();
        assert_eq!(second_charge - first_charge, first_charge - cached_charge);
        assert!(
            cached_charge > upstream_charge,
            "the cache copy must be charged too"
        );
        assert_eq!(first["input"][0]["reasoning_content"], SYNTHETIC_TOKEN);
        assert_eq!(second["input"][0]["reasoning_content"], SYNTHETIC_TOKEN);
        drop(first);
        drop(first_owners);
        assert_eq!(used(), first_charge);
        drop(second);
        drop(second_owners);
        assert_eq!(used(), cached_charge);
        drop(history);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn enrichment_payload_owners_pin_restored_calls_after_replacement_or_eviction(
    ) -> Result<(), std::io::Error> {
        for replace in [true, false] {
            let history = CodexChatHistoryStore::default();
            let (owner, used) = payload_owner_fixture()?;
            history
                .record_response_with_payload_owner(
                    &tokenized_response("resp_owned", "call_owned"),
                    Some(owner),
                )
                .await;
            let mut request = json!({
                "input": [{"type": "function_call_output", "call_id": "call_owned", "output": "ok"}]
            });
            if replace {
                request["previous_response_id"] = json!("resp_owned");
            }
            let (changed, owners) = history
                .enrich_request_with_payload_owners(&mut request)
                .await
                .map_err(|error| std::io::Error::other(error.code()))?;
            assert_eq!(changed, 1);
            assert_eq!(request["input"][0]["reasoning_content"], SYNTHETIC_TOKEN);

            if replace {
                history
                    .record_response(&tokenized_response("resp_owned", "call_owned"))
                    .await;
            } else {
                for index in 0..MAX_CACHED_RESPONSES {
                    history
                        .record_response(&tokenized_response(
                            &format!("resp_{index}"),
                            "call_other",
                        ))
                        .await;
                }
            }
            assert_eq!(
                used(),
                owners.as_ref().expect("synthetic owner").charged_for_test(),
                "the complete candidate stays charged without pinning the retired workspace"
            );
            assert!(owners.is_some());
            assert_eq!(request["input"][0]["reasoning_content"], SYNTHETIC_TOKEN);
            drop(request);
            drop(owners);
            assert_eq!(used(), 0);
        }
        Ok(())
    }

    #[tokio::test]
    async fn enrichment_payload_owners_keep_only_used_independent_copy_charges(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (shared, shared_used) = payload_owner_fixture()?;
        let (restored, restored_used) = payload_owner_fixture()?;
        let (unchanged, unchanged_used) = payload_owner_fixture()?;
        let (unrelated, unrelated_used) = payload_owner_fixture()?;
        for (call_id, owner) in [
            ("call_enriched", &shared),
            ("call_shared", &shared),
            ("call_restored", &restored),
            ("call_unchanged", &unchanged),
            ("call_unrelated", &unrelated),
        ] {
            history
                .record_response_with_payload_owner(
                    &tokenized_response("resp_owned", call_id),
                    Some(owner.clone()),
                )
                .await;
        }
        let mut request = json!({
            "previous_response_id": "resp_owned",
            "input": [
                {"type": "function_call", "call_id": "call_enriched"},
                tokenized_response("resp_owned", "call_unchanged")["output"][0],
                {"type": "function_call_output", "call_id": "call_shared", "output": "ok"},
                {"type": "function_call_output", "call_id": "call_restored", "output": "ok"}
            ]
        });
        let (changed, owners) = history
            .enrich_request_with_payload_owners(&mut request)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 3);
        assert!(
            owners.is_some(),
            "one owner retains the complete candidate, not per-field copies"
        );
        drop((shared, restored, unchanged, unrelated));
        drop(history);
        assert_eq!(
            shared_used(),
            owners.as_ref().expect("synthetic owner").charged_for_test()
        );
        assert_eq!(
            restored_used(),
            0,
            "candidate no longer pins the source workspace"
        );
        assert_eq!(unchanged_used(), 0);
        assert_eq!(unrelated_used(), 0);
        drop(request);
        drop(owners);
        assert_eq!(shared_used(), 0);
        assert_eq!(restored_used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn enrichment_payload_owners_pin_existing_call_without_tool_outputs(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (owner, used) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_owned"),
                Some(owner),
            )
            .await;
        let mut request = json!({"input": {"type": "function_call", "call_id": "call_owned"}});
        let (changed, owners) = history
            .enrich_request_with_payload_owners(&mut request)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 1);
        assert_eq!(request["input"][0]["reasoning_content"], SYNTHETIC_TOKEN);
        let (changed_again, unused_owners) = history
            .enrich_request_with_payload_owners(&mut request)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed_again, 0);
        assert!(
            unused_owners.is_none(),
            "unchanged calls must not retain owners"
        );
        drop(history);
        assert_eq!(
            used(),
            owners.as_ref().expect("synthetic owner").charged_for_test(),
            "the complete existing-call candidate retains its independent reservation"
        );
        drop(request);
        drop(owners);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn enrichment_payload_owners_are_empty_without_owned_cache_changes(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (owner, used) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_owned"),
                Some(owner),
            )
            .await;
        for mut request in [
            json!({"previous_response_id": "resp_owned"}),
            json!({"previous_response_id": "resp_owned", "input": null}),
            json!({"previous_response_id": "resp_owned", "input": "text"}),
            json!({"previous_response_id": "resp_owned", "input": []}),
            json!({"previous_response_id": "resp_owned", "input": {"type": "message"}}),
            json!({"previous_response_id": "resp_owned", "input": [tokenized_response("resp_owned", "call_owned")["output"][0]]}),
            json!({"input": [{"type": "function_call_output", "call_id": "call_missing", "output": "ok"}]}),
        ] {
            let original = request.clone();
            let (changed, owners) = history
                .enrich_request_with_payload_owners(&mut request)
                .await
                .map_err(|error| std::io::Error::other(error.code()))?;
            assert_eq!(changed, 0);
            assert!(owners.is_none());
            assert_eq!(request, original);
        }
        history
            .record_response(&tokenized_response("resp_other", "call_owned"))
            .await;
        let mut ambiguous = json!({"input": [{"type": "function_call_output", "call_id": "call_owned", "output": "ok"}]});
        let (changed, owners) = history
            .enrich_request_with_payload_owners(&mut ambiguous)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 0);
        assert!(owners.is_none(), "ambiguous calls must not retain owners");
        let mut ownerless = json!({"previous_response_id": "resp_other", "input": [{"type": "function_call_output", "call_id": "call_owned", "output": "ok"}]});
        let (changed, owners) = history
            .enrich_request_with_payload_owners(&mut ownerless)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 1);
        assert!(
            owners.is_none(),
            "legacy cache entries need no owner handles"
        );
        drop(history);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn payload_owner_survives_cached_lookup_clones_and_tokenized_enrichment(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (owner, used) = payload_owner_fixture()?;
        let charge = used();
        assert!(charge > 0);
        assert_eq!(
            history
                .record_response_with_payload_owner(
                    &tokenized_response("resp_owned", "call_owned"),
                    Some(owner),
                )
                .await,
            1
        );
        assert!(
            used() > charge,
            "cache admission must charge its independent copy"
        );
        let cached_charge = used();

        let previous = history.lookup(Some("resp_owned")).await;
        let fallback = history.lookup(None).await;
        let cloned = fallback.clone();
        assert_eq!(
            used(),
            cached_charge,
            "lookup snapshots share the actual tree"
        );
        let previous_call = previous
            .cached_call("call_owned")
            .ok_or_else(|| std::io::Error::other("missing previous call"))?;
        let fallback_call = fallback
            .cached_call("call_owned")
            .ok_or_else(|| std::io::Error::other("missing fallback call"))?;
        let cloned_call = cloned
            .cached_call("call_owned")
            .ok_or_else(|| std::io::Error::other("missing cloned call"))?;
        assert!(Arc::ptr_eq(&previous_call.item, &fallback_call.item));
        assert!(Arc::ptr_eq(&fallback_call.item, &cloned_call.item));
        let mut request = json!({
            "input": [{"type": "function_call_output", "call_id": "call_owned", "output": "ok"}]
        });
        let (changed, owners) = history
            .enrich_request_with_payload_owners(&mut request)
            .await
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert_eq!(changed, 1);
        assert_eq!(request["input"][0]["reasoning_content"], SYNTHETIC_TOKEN);
        assert_eq!(
            request["input"][0]["arguments"],
            json!({"value": SYNTHETIC_TOKEN}).to_string()
        );

        drop(request);
        drop(owners);
        drop(history);
        assert_eq!(
            used(),
            cached_charge,
            "lookup snapshots must keep their charge"
        );
        drop(previous);
        drop(fallback);
        assert_eq!(
            used(),
            cached_charge,
            "fallback clones must keep their charge"
        );
        drop(cloned);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn payload_owner_append_and_replacement_keep_only_live_call_charges(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (first, first_used) = payload_owner_fixture()?;
        let (second, second_used) = payload_owner_fixture()?;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_first"),
                Some(first),
            )
            .await;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_second"),
                Some(second.clone()),
            )
            .await;
        assert!(first_used() > 0, "appending must retain older call owners");
        assert!(second_used() > 0);

        let old = history.lookup(Some("resp_owned")).await;
        history
            .record_response_with_payload_owner(
                &tokenized_response("resp_owned", "call_first"),
                Some(second),
            )
            .await;
        assert!(first_used() > 0, "a replaced call snapshot is still live");
        drop(old);
        assert_eq!(first_used(), 0, "fully replaced owners must not accumulate");
        assert!(second_used() > 0);

        history
            .record_response(&tokenized_response("resp_owned", "call_first"))
            .await;
        assert!(
            second_used() > 0,
            "another cached call still needs this owner"
        );
        history
            .record_response(&tokenized_response("resp_owned", "call_second"))
            .await;
        assert_eq!(second_used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn payload_owner_repeated_updates_and_eviction_release_the_charge(
    ) -> Result<(), std::io::Error> {
        let history = CodexChatHistoryStore::default();
        let (owner, used) = payload_owner_fixture()?;
        let charge = used();
        let mut retained_charge = None;
        for _ in 0..32 {
            history
                .record_response_with_payload_owner(
                    &tokenized_response("resp_owned", "call_owned"),
                    Some(owner.clone()),
                )
                .await;
            if let Some(expected) = retained_charge {
                assert_eq!(
                    used(),
                    expected,
                    "updates must release the replaced copy charge"
                );
            }
            retained_charge = Some(used());
        }
        drop(owner);
        assert!(
            used() > charge,
            "only the workspace and current cache copy remain"
        );
        for index in 0..MAX_CACHED_RESPONSES {
            history
                .record_response(&tokenized_response(&format!("resp_{index}"), "call_other"))
                .await;
        }
        assert_eq!(used(), 0, "eviction must release the last retained owner");
        Ok(())
    }

    #[tokio::test]
    async fn payload_owner_is_not_retained_without_a_cacheable_call() -> Result<(), std::io::Error>
    {
        let history = CodexChatHistoryStore::default();
        for response in [
            json!({"output": tokenized_response("unused", "call_unused")["output"]}),
            json!({"id": "resp_empty", "output": []}),
            json!({"id": "resp_text", "output": [{"type": "message"}]}),
        ] {
            let (owner, used) = payload_owner_fixture()?;
            assert_eq!(
                history
                    .record_response_with_payload_owner(&response, Some(owner))
                    .await,
                0
            );
            assert_eq!(used(), 0);
        }
        Ok(())
    }

    #[tokio::test]
    async fn payload_owner_sse_done_and_completed_keep_tokenized_calls_charged(
    ) -> Result<(), std::io::Error> {
        for completed in [false, true] {
            let history = Arc::new(CodexChatHistoryStore::default());
            let (owner, used) = payload_owner_fixture()?;
            let charge = used();
            let response = tokenized_response("resp_sse", "call_sse");
            let event = if completed {
                json!({"type": "response.completed", "response": response})
            } else {
                json!({"type": "response.output_item.done", "item": response["output"][0]})
            };
            let chunks = vec![
                Bytes::from_static(
                    b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_sse\"}}\n\n",
                ),
                Bytes::from(format!("data: {event}\n\n")),
            ];
            let stream = futures::stream::iter(chunks.clone().into_iter().map(Ok));
            let recorded = record_responses_sse_stream_with_payload_owner(
                stream,
                history.clone(),
                Some(owner),
            );
            let output = recorded.collect::<Vec<_>>().await;
            assert_eq!(output.len(), chunks.len());
            for (actual, expected) in output.into_iter().zip(chunks) {
                assert_eq!(actual?, expected);
            }
            assert!(
                used() > charge,
                "SSE history retains workspace and cache copy charge"
            );
            let mut request = json!({
                "previous_response_id": "resp_sse",
                "input": [{"type": "function_call_output", "call_id": "call_sse", "output": "ok"}]
            });
            let (changed, owners) = history
                .enrich_request_with_payload_owners(&mut request)
                .await
                .map_err(|error| std::io::Error::other(error.code()))?;
            assert_eq!(changed, 1);
            assert_eq!(request["input"][0]["reasoning_content"], SYNTHETIC_TOKEN);
            drop(history);
            assert_eq!(
                used(),
                owners.as_ref().expect("synthetic owner").charged_for_test()
            );
            drop(request);
            drop(owners);
            assert_eq!(used(), 0);
        }
        Ok(())
    }

    #[tokio::test]
    async fn enriches_tool_output_with_cached_function_call_from_previous_response() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&json!({
                "id": "resp_1",
                "output": [
                    {
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "read_file",
                        "arguments": "{\"path\":\"README.md\"}",
                        "reasoning_content": "Need to inspect the file."
                    }
                ]
            }))
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request).await, 1);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["reasoning_content"], "Need to inspect the file.");
        assert_eq!(input[1]["type"], "function_call_output");
    }

    #[tokio::test]
    async fn restores_unique_call_id_without_matching_previous_response() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&json!({
                "id": "resp_1",
                "output": [
                    {
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "read_file",
                        "arguments": "{}",
                        "reasoning_content": "This is the only cached call."
                    }
                ]
            }))
            .await;

        let mut missing_previous = json!({
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });
        assert_eq!(history.enrich_request(&mut missing_previous).await, 1);
        assert_eq!(missing_previous["input"][0]["type"], "function_call");
        assert_eq!(
            missing_previous["input"][0]["reasoning_content"],
            "This is the only cached call."
        );
        assert_eq!(missing_previous["input"][1]["type"], "function_call_output");

        let mut different_previous = json!({
            "previous_response_id": "resp_2",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });
        assert_eq!(history.enrich_request(&mut different_previous).await, 1);
        assert_eq!(different_previous["input"][0]["type"], "function_call");
        assert_eq!(
            different_previous["input"][0]["reasoning_content"],
            "This is the only cached call."
        );
        assert_eq!(
            different_previous["input"][1]["type"],
            "function_call_output"
        );
    }

    #[tokio::test]
    async fn does_not_restore_ambiguous_call_id_without_previous_response() {
        let history = CodexChatHistoryStore::default();
        for (response_id, reasoning) in [
            ("resp_1", "This belongs to the first response."),
            ("resp_2", "This belongs to the second response."),
        ] {
            history
                .record_response(&json!({
                    "id": response_id,
                    "output": [
                        {
                            "type": "function_call",
                            "call_id": "call_1",
                            "name": "read_file",
                            "arguments": "{}",
                            "reasoning_content": reasoning
                        }
                    ]
                }))
                .await;
        }

        let mut missing_previous = json!({
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });
        assert_eq!(history.enrich_request(&mut missing_previous).await, 0);
        assert_eq!(missing_previous["input"][0]["type"], "function_call_output");

        let mut different_previous = json!({
            "previous_response_id": "resp_missing",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });
        assert_eq!(history.enrich_request(&mut different_previous).await, 0);
        assert_eq!(
            different_previous["input"][0]["type"],
            "function_call_output"
        );
    }

    #[tokio::test]
    async fn enriches_existing_function_call_missing_reasoning() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&json!({
                "id": "resp_1",
                "output": [
                    {
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "read_file",
                        "arguments": "{}",
                        "reasoning_content": "Need to inspect the file."
                    }
                ]
            }))
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request).await, 1);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["reasoning_content"], "Need to inspect the file.");
        assert_eq!(input.len(), 2);
    }

    #[tokio::test]
    async fn enriches_existing_function_call_missing_name_and_arguments() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&json!({
                "id": "resp_1",
                "output": [
                    {
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "read_file",
                        "arguments": "{\"path\":\"README.md\"}",
                        "reasoning_content": "Need to inspect the file."
                    }
                ]
            }))
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request).await, 1);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["name"], "read_file");
        assert_eq!(input[0]["arguments"], "{\"path\":\"README.md\"}");
        assert_eq!(input[0]["reasoning_content"], "Need to inspect the file.");
        assert_eq!(input[1]["type"], "function_call_output");
    }

    #[tokio::test]
    async fn restores_parallel_tool_calls_as_one_assistant_group() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&json!({
                "id": "resp_1",
                "output": [
                    {
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "first",
                        "arguments": "{}",
                        "reasoning_content": "Need both tools."
                    },
                    {
                        "type": "function_call",
                        "call_id": "call_2",
                        "name": "second",
                        "arguments": "{}",
                        "reasoning_content": "Need both tools."
                    }
                ]
            }))
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "one"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_2",
                    "output": "two"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request).await, 2);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_2");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[3]["type"], "function_call_output");
    }

    #[tokio::test]
    async fn restores_custom_and_tool_search_calls_from_previous_response() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&json!({
                "id": "resp_1",
                "output": [
                    {
                        "type": "custom_tool_call",
                        "call_id": "call_patch",
                        "name": "apply_patch",
                        "input": "*** Begin Patch\n*** End Patch",
                        "reasoning_content": "Need to patch the file."
                    },
                    {
                        "type": "tool_search_call",
                        "call_id": "call_search",
                        "status": "completed",
                        "execution": "client",
                        "arguments": {"query": "Gmail tools"},
                        "reasoning_content": "Need to discover tools."
                    }
                ]
            }))
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_patch",
                    "output": "patched"
                },
                {
                    "type": "tool_search_output",
                    "call_id": "call_search",
                    "tools": []
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request).await, 2);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "custom_tool_call");
        assert_eq!(input[0]["call_id"], "call_patch");
        assert_eq!(input[1]["type"], "tool_search_call");
        assert_eq!(input[1]["call_id"], "call_search");
        assert_eq!(input[2]["type"], "custom_tool_call_output");
        assert_eq!(input[3]["type"], "tool_search_output");
    }

    #[tokio::test]
    async fn records_streamed_function_call_done_items() {
        let history = Arc::new(CodexChatHistoryStore::default());
        let stream = futures::stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_stream\"}}\n\n",
            )),
            Ok(Bytes::from_static(
                b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"read_file\",\"arguments\":\"{}\",\"reasoning_content\":\"Need a file.\"}}\n\n",
            )),
        ]);

        let output = record_responses_sse_stream(stream, history.clone())
            .collect::<Vec<_>>()
            .await;
        assert_eq!(output.len(), 2);

        let mut request = json!({
            "previous_response_id": "resp_stream",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request).await, 1);
        assert_eq!(request["input"][0]["reasoning_content"], "Need a file.");
    }
}
