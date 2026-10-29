//! Explicit, short-lived Codex group sessions, isolated from provider switching.
pub(crate) mod scheduling;
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
    sessions: RwLock<HashMap<String, TemporarySession>>,
    menu: RwLock<Option<Arc<Menu>>>,
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
}
pub(crate) struct Menu {
    pub token: String,
    pub models: HashMap<String, MenuModel>,
    pub default_model: Option<String>,
    pub omitted_entries: usize,
    pub groups: HashMap<String, Arc<Session>>,
}
impl Menu {
    pub(crate) fn preserve_from(&mut self, previous: &Self) {
        self.token = previous.token.clone();
        let mut carried: Vec<_> = previous
            .groups
            .values()
            .flat_map(|s| s.targets.iter().cloned())
            .collect();
        for (alias, session) in &mut self.groups {
            let Some(next) = Arc::get_mut(session) else {
                continue;
            };
            if let Some(old) = previous.groups.get(alias).filter(|old| {
                old.group
                    .members
                    .iter()
                    .map(|m| (&m.id, &m.provider_id, &m.model_id))
                    .eq(next
                        .group
                        .members
                        .iter()
                        .map(|m| (&m.id, &m.provider_id, &m.model_id)))
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
    pub effort: crate::services::intent_routing::effort::Capability,
    pub url: reqwest::Url,
    pub headers: http::HeaderMap,
    pub official: bool,
    pub credentials: Option<Arc<crate::services::intent_routing::catalog::ExecutionCredentials>>,
    pub scheduling: Arc<scheduling::Scheduling>,
}
impl Target {
    pub(crate) fn same_destination(&self, other: &Self) -> bool {
        self.provider_id == other.provider_id
            && self.model == other.model
            && self.same_account(other)
    }
    pub(crate) fn same_account(&self, other: &Self) -> bool {
        self.url == other.url
            && self.official == other.official
            && match (&self.credentials, &other.credentials) {
                (Some(a), Some(b)) => a.same_identity(b),
                (None, None) => self.headers == other.headers,
                _ => false,
            }
    }
}
pub(crate) struct Session {
    pub group: RoutingGroup,
    pub targets: Vec<Target>,
    pub classifier: Option<crate::provider::Provider>,
    pub client: reqwest::Client,
    pub created: Instant,
    pub bindings: Arc<tokio::sync::Mutex<Bindings>>,
}
impl Session {
    fn target_index(&self, member: &crate::services::intent_routing::Member) -> Option<usize> {
        self.targets.iter().position(|target| {
            target.provider_id == member.provider_id && target.model == member.model_id
        })
    }
    pub(crate) fn menu_targets(&self) -> impl Iterator<Item = &Target> {
        self.targets.iter().filter(|target| {
            self.group.members.iter().any(|member| {
                member.provider_id == target.provider_id
                    && member.model_id == target.model
                    && self.group.includes_in_menu(member)
            })
        })
    }
}
impl Runtime {
    pub(crate) fn menu_snapshot(&self) -> Result<Option<Arc<Menu>>, String> {
        self.menu
            .read()
            .map(|menu| menu.clone())
            .map_err(|_| "intentRouting.failed".into())
    }
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

    pub(crate) fn menu_efforts(&self, token: &str) -> Result<HashMap<String, Vec<String>>, String> {
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
                            crate::services::intent_routing::effort::shared(
                                session.menu_targets().map(|t| &t.effort),
                            ),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn menu_active(&self) -> bool {
        self.menu.read().map(|v| v.is_some()).unwrap_or(false)
    }
    pub fn clear_menu(&self) {
        if let Ok(mut v) = self.menu.write() {
            *v = None;
        }
    }
    pub fn set_menu(&self, menu: Arc<Menu>) -> Result<Option<Arc<Menu>>, String> {
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
    pool: scheduling::Pool,
}
impl Bindings {
    fn resolve(&self, body: &Value, session: Option<&str>) -> Result<Option<usize>, ()> {
        let mut selected = session.and_then(|s| self.sessions.get(s)).copied();
        let mut merge = |candidate: Option<usize>| -> Result<(), ()> {
            let candidate = candidate.filter(|v| *v != usize::MAX).ok_or(())?;
            if selected.is_some_and(|s| s != candidate) {
                return Err(());
            }
            selected = Some(candidate);
            Ok(())
        };
        if let Some(previous) = body.get("previous_response_id").filter(|v| !v.is_null()) {
            merge(self.responses.get(previous.as_str().ok_or(())?).copied())?;
        }
        if let Some(items) = body.get("input").and_then(Value::as_array) {
            for item in items {
                if matches!(
                    item["type"].as_str(),
                    Some("function_call_output" | "function_call")
                ) {
                    merge(self.calls.get(item["call_id"].as_str().ok_or(())?).copied())?;
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
    fn full(&self) -> bool {
        self.responses.len() + self.calls.len() + self.sessions.len() >= 4096
    }
    fn observe(&mut self, value: &Value, target: usize) {
        if self.full() {
            return;
        }
        let response = value.get("response").unwrap_or(value);
        if let Some(id) = response
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| id.len() <= 256)
        {
            self.responses
                .entry(id.into())
                .and_modify(|old| {
                    if *old != target {
                        *old = usize::MAX;
                    }
                })
                .or_insert(target);
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
            if item["type"] == "function_call" {
                if let Some(id) = item["call_id"].as_str().filter(|id| id.len() <= 256) {
                    self.calls
                        .entry(id.into())
                        .and_modify(|old| {
                            if *old != target {
                                *old = usize::MAX;
                            }
                        })
                        .or_insert(target);
                }
            }
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
            let data = block
                .lines()
                .filter_map(|l| super::sse::strip_sse_field(l, "data"))
                .collect::<Vec<_>>()
                .join("\n");
            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                self.observe_at(&value, bindings, target, Instant::now());
            }
        }
    }
    fn finish(&mut self, bindings: &mut Bindings, target: usize) {
        if let Ok(value) = serde_json::from_str::<Value>(&self.buffer) {
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
        if self.started.is_some() && value["delta"].as_str().is_some_and(|s| !s.is_empty()) {
            let text = matches!(
                kind,
                "response.output_text.delta" | "response.function_call_arguments.delta"
            );
            if text
                || matches!(
                    kind,
                    "response.reasoning_text.delta" | "response.reasoning_summary_text.delta"
                )
            {
                self.first_content.get_or_insert(now);
            }
            if text {
                self.first_text.get_or_insert(now);
                self.flow_bytes = self
                    .flow_bytes
                    .saturating_add(value["delta"].as_str().map_or(0, |s| s.len() as u64));
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
        let input = usage["input_tokens"].as_u64().unwrap_or(0);
        let output = usage["output_tokens"].as_u64().unwrap_or(0);
        self.tokens = self.tokens.max(input.saturating_add(output));
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
        let mut aliases = if let Some(menu) = &menu {
            menu.groups.keys().cloned().collect::<Vec<_>>()
        } else {
            session.iter().map(|s| s.group.alias()).collect()
        };
        aliases.sort();
        let data = aliases
            .into_iter()
            .map(|id| json!({"id":id,"object":"model","owned_by":"varyai"}))
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
    let mut bindings = match session.bindings.clone().try_lock_owned() {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::CONFLICT, "routing_session_busy"),
    };
    if bindings.full() {
        return fail(StatusCode::CONFLICT, "routing_session_capacity");
    }
    let conversation = original["prompt_cache_key"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .or_else(|| {
            parts
                .headers
                .get("session-id")
                .and_then(|s| s.to_str().ok())
                .filter(|s| !s.is_empty() && s.len() <= 256)
        })
        .map(str::to_owned);
    let pinned = match bindings.resolve(&original, conversation.as_deref()) {
        Ok(p) => p,
        Err(_) => return fail(StatusCode::CONFLICT, "routing_unknown_continuation"),
    };
    let mut body = original.clone();
    match super::security::inspect_and_intercept_mut_with_headers(
        &crate::app_config::AppType::Codex,
        &mut body,
        &parts.headers,
    ) {
        Ok(Some(blocked)) => return blocked,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "routing_inspection_failed"),
        _ => {}
    }
    let preferred = if pinned.is_none() && session.group.uses_classifier() {
        if let (Some(text), Some(provider)) = (
            classifier::extract_user_text(&body),
            session.classifier.clone(),
        ) {
            classifier::trial(session.group.clone(), provider, text)
                .await
                .ok()
                .and_then(|r| r.intent_id)
        } else {
            None
        }
    } else {
        None
    };
    let approved = match session.group.preview(preferred.as_deref()) {
        Ok(p) => p
            .candidates
            .iter()
            .filter_map(|m| session.target_index(m))
            .collect::<Vec<_>>(),
        Err(_) => return fail(StatusCode::BAD_REQUEST, "routing_invalid_plan"),
    };
    let states = session
        .targets
        .iter()
        .map(|target| {
            let mut health = target.scheduling.snapshot();
            if let Some(member) = session
                .group
                .members
                .iter()
                .find(|m| m.provider_id == target.provider_id && m.model_id == target.model)
            {
                health.weight = member.weight;
                health.free = member.free.or(target.effort.free).unwrap_or(false);
            }
            health
        })
        .collect::<Vec<_>>();
    let mut candidates = if pinned.is_none()
        || session.group.routing == crate::services::intent_routing::RoutingStrategy::Weighted
    {
        scheduling::arrange(
            session.group.routing,
            &approved,
            &states,
            &mut bindings.pool,
            chrono::Utc::now().timestamp(),
        )
    } else {
        approved
    };
    if let Some(index) = pinned {
        candidates = vec![index];
    }
    if pinned.is_none() {
        if let Some(member) = preferred
            .as_ref()
            .and_then(|id| session.group.rules.iter().find(|r| &r.id == id))
            .and_then(|r| {
                session
                    .group
                    .members
                    .iter()
                    .find(|m| m.id == r.target_member_id)
                    .and_then(|m| session.target_index(m))
            })
        {
            if let Some(position) = candidates.iter().position(|i| *i == member) {
                candidates.remove(position);
                candidates.insert(0, member);
            }
        }
    }
    let has_images = contains_input_image(&body);
    let mut attempted = false;
    for index in candidates {
        let Some(target) = session.targets.get(index) else {
            return fail(StatusCode::BAD_REQUEST, "routing_invalid_target");
        };
        if has_images && target.effort.images == Some(false) {
            if pinned.is_some() {
                return fail(
                    StatusCode::BAD_REQUEST,
                    "routing_target_does_not_support_images",
                );
            }
            continue;
        }
        // Official native Responses requires SSE and non-persistent requests. Do not
        // silently rewrite client semantics or drop unsupported parameters.
        if target.official && (body["stream"] != true || body["store"] != false) {
            return fail(
                StatusCode::BAD_REQUEST,
                "routing_official_requires_stream_and_store_false",
            );
        }
        body["model"] = Value::String(target.model.clone());
        let payload = match super::pi::json::encode_redacted(&bytes, &original, &body) {
            Ok(p) => p,
            Err(_) => return fail(StatusCode::BAD_REQUEST, "routing_unsafe_payload_edit"),
        };
        let payload = match crate::services::intent_routing::effort::adapt(&payload, &target.effort)
        {
            Ok(payload) => payload,
            Err(code) => return fail(StatusCode::BAD_REQUEST, code),
        };
        // Allowlist protocol headers; never relay caller credentials, cookies or workspace identity.
        let mut headers = target.headers.clone();
        if target.official && crate::codex_client::apply_identity(&mut headers).is_err() {
            return fail(
                StatusCode::BAD_GATEWAY,
                "routing_client_identity_unavailable",
            );
        }
        if menu.is_some() {
            if let Some(credentials) = &target.credentials {
                let refreshed =
                    tokio::time::timeout(Duration::from_secs(30), credentials.token()).await;
                let Ok(Some(token)) = refreshed else {
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
        for name in [
            "openai-beta",
            "x-client-request-id",
            "session-id",
            "thread-id",
        ] {
            if let Some(value) = parts.headers.get(name) {
                headers.insert(name, value.clone());
            }
        }
        headers.insert(
            header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("identity"),
        );
        attempted = true;
        let attempt_started = Instant::now();
        let response = session
            .client
            .post(target.url.clone())
            .headers(headers)
            .body(payload)
            .send()
            .await;
        // Ambiguous network failures are not replayed at another account.
        let response = match response {
            Ok(r) => r,
            Err(_) => {
                return fail(
                    StatusCode::BAD_GATEWAY,
                    "routing_upstream_connection_failed",
                )
            }
        };
        let status = response.status();
        target
            .scheduling
            .observe_headers(response.headers(), chrono::Utc::now().timestamp());
        if matches!(status.as_u16(), 429 | 502 | 503 | 504) {
            target
                .scheduling
                .failed(response.headers(), chrono::Utc::now().timestamp());
        }
        if pinned.is_none()
            && session.group.routing != crate::services::intent_routing::RoutingStrategy::Manual
            && matches!(status.as_u16(), 429 | 502 | 503 | 504)
        {
            continue;
        }
        if !status.is_success() {
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
        target.scheduling.succeeded();
        if session.group.routing == crate::services::intent_routing::RoutingStrategy::Weighted {
            bindings.pool.reassign(index);
        }
        if let Some(key) = conversation.as_ref() {
            bindings.sessions.insert(key.clone(), index);
        }
        let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
        let mut upstream = response.bytes_stream();
        let mut observer = Observer::default();
        observer.scheduling = Some(target.scheduling.clone());
        observer.started = (body["stream"] == true).then_some(attempt_started);
        let mut collector = super::pi::usage::Collector::new(
            crate::services::pi_routing::catalog::Protocol::Responses,
        );
        let mut job = super::pi::usage::Job {
            app_type: "codex",
            db: state.db.clone(),
            provider_id: target.provider_id.clone(),
            request_id: uuid::Uuid::new_v4().to_string(),
            request_model: target.model.clone(),
            session_id: None,
            usage: Default::default(),
            latency_ms: 0,
            status: status.as_u16(),
            streaming: body["stream"] == true,
        };
        let metadata = super::tool_observation::mount::metadata(
            super::tool_observation::parsers::PROTOCOL_OPENAI_RESPONSES,
            "codex",
            if job.streaming {
                super::tool_observation::channel::Transport::Sse
            } else {
                super::tool_observation::channel::Transport::Json
            },
            super::tool_observation::channel::OutputQuality::Native,
            super::tool_observation::mount::collect_tool_schemas(&body, "codex"),
        );
        let stream = async_stream::stream! {
            // Keep the binding guard until EOF/drop, including client cancellation.
            while let Some(next) = upstream.next().await {
                match next {
                    Ok(bytes) => { observer.push(&bytes, &mut bindings, index);
                        collector.push(&bytes); yield Ok::<_, std::io::Error>(bytes); }
                    Err(_) => { yield Err(std::io::Error::other("Routing upstream stream failed")); return; }
                }
            }
            observer.finish(&mut bindings, index);
            collector.finish();
            if collector.observed {
                job.usage = collector.usage;
                job.latency_ms = started.elapsed().as_millis() as u64;
                super::pi::usage::emit(job);
            }
        };
        let mut response = Response::new(Body::from_stream(
            super::tool_observation::mount::observe_stream(stream, metadata),
        ));
        *response.status_mut() = status;
        if let Some(value) = content_type {
            response.headers_mut().insert(header::CONTENT_TYPE, value);
        }
        return response;
    }
    if has_images && !attempted {
        return fail(StatusCode::BAD_REQUEST, "routing_no_image_candidate");
    }
    fail(StatusCode::BAD_GATEWAY, "routing_candidates_unavailable")
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
mod tests;
