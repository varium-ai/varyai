//! Bounded routing metadata, recorded at the decision sites. Never stores bodies,
//! headers, upstream errors or caller session identifiers. Observation fails open.
use serde::{Deserialize, Serialize};

mod history;
pub(crate) use history::Page as HistoryPage;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock, Weak,
    },
    time::Instant,
};

pub(crate) const CAPACITY: usize = 60;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Candidate {
    #[serde(skip)]
    pub(super) key_id: Option<String>,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed_effort: Option<String>,
    pub context_window: Option<u64>,
    pub account: Option<Account>,
    #[serde(skip)]
    pub(super) scheduling: Weak<super::scheduling::Scheduling>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Routing,
    Streaming,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Strategy,
    Pinned,
    Affinity,
    Rule,
    Paused,
    Attempt,
    Retry,
    FirstTokenTimeout,
    Context,
    Images,
    Rejected,
    Connected,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Step {
    pub kind: Kind,
    pub at_ms: u64,
    pub candidate: Option<usize>,
    pub status: Option<u16>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Route {
    pub id: String,
    pub started_at: i64,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub session: Option<String>,
    pub model: String,
    pub group_name: String,
    pub strategy: String,
    pub candidates: Vec<Candidate>,
    pub steps: Vec<Step>,
    pub state: State,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub first_token_ms: Option<u64>,
    pub output_tokens_per_second: Option<f64>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub estimated_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<String>,
}

pub(crate) struct Store {
    pub(super) rows: RwLock<VecDeque<Route>>,
    active_keys: RwLock<BTreeMap<String, ActiveKey>>,
    salt: String,
    history: Option<history::History>,
}

struct ActiveKey {
    provider: String,
    key: String,
    finished: Arc<AtomicBool>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            rows: RwLock::default(),
            active_keys: RwLock::default(),
            salt: uuid::Uuid::new_v4().to_string(),
            history: None,
        }
    }
}

fn label(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect()
}

impl Store {
    pub(crate) fn key_running(&self, provider: &str, key: &str) -> Option<usize> {
        self.active_keys
            .try_read()
            .ok()
            .filter(|active| active.len() < 8192)
            .map(|active| {
                active
                    .values()
                    .filter(|entry| {
                        entry.provider == provider
                            && entry.key == key
                            && !entry.finished.load(Ordering::Relaxed)
                    })
                    .count()
            })
    }
    pub(crate) fn with_history(root: std::path::PathBuf) -> std::io::Result<Self> {
        Ok(Self {
            history: Some(history::History::new(root)?),
            ..Self::default()
        })
    }

    pub(crate) fn history_days(&self) -> std::io::Result<Vec<String>> {
        self.history
            .as_ref()
            .map_or_else(|| Ok(Vec::new()), |history| history.days())
    }

    pub(crate) fn history(&self, day: Option<&str>) -> std::io::Result<HistoryPage> {
        match (&self.history, day) {
            (Some(history), Some(day)) => history.read(day),
            (Some(history), None) => {
                history.days()?;
                Ok(HistoryPage::default())
            }
            _ => Ok(HistoryPage::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn flush_history(&self) -> std::io::Result<()> {
        self.history
            .as_ref()
            .map_or(Ok(()), |history| history.flush())
    }

    pub(crate) fn candidate(&self, target: &super::Target) -> Candidate {
        // A runtime-salted identity groups models without exposing a key, URL,
        // workspace or credential-derived stable identifier to the renderer.
        let mut hash = Sha256::new();
        let mut part = |bytes: &[u8]| {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        };
        part(self.salt.as_bytes());
        part(target.provider_id.as_bytes());
        part(&[u8::from(target.official)]);
        let mut url = target.url.clone();
        if target
            .upstream
            .as_ref()
            .and_then(|provider| provider.meta.as_ref())
            .and_then(|meta| meta.api_format.as_deref())
            .and_then(super::Protocol::from_format)
            == Some(super::Protocol::Gemini)
        {
            if let Some((base, _)) = url.path().split_once("/models/") {
                let base = base.to_owned();
                url.set_path(&base);
            }
        }
        part(url.as_str().as_bytes());
        if let Some(credentials) = &target.credentials {
            if let Some(identity) = credentials.continuation_identity() {
                part(identity.as_bytes());
            } else {
                part(&credentials.fingerprint());
            }
        } else {
            let mut headers = target.headers.iter().collect::<Vec<_>>();
            headers.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
            for (name, value) in headers {
                part(name.as_str().as_bytes());
                part(value.as_bytes());
            }
        }
        let provider_name = label(
            target
                .upstream
                .as_ref()
                .map(|p| p.name.as_str())
                .or_else(|| target.credentials.as_ref().map(|c| c.provider_name()))
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(if target.official { "Codex" } else { "Provider" }),
        );
        let account = Account {
            id: format!("{:x}", hash.finalize()),
            provider_id: target.provider_id.clone(),
            provider_name: provider_name.clone(),
            kind: if target.official {
                AccountKind::Account
            } else if target.upstream.is_some()
                || target.headers.contains_key("authorization")
                || target.headers.contains_key("x-api-key")
            {
                AccountKind::Key
            } else {
                AccountKind::Provider
            },
            model: label(&target.model),
            telemetry: target.scheduling.trace_account().unwrap_or_default(),
        };
        Candidate {
            key_id: (!target.official).then(|| {
                let key = target
                    .upstream
                    .as_ref()
                    .map(|provider| {
                        provider
                            .resolve_usage_credentials(&crate::app_config::AppType::Codex)
                            .1
                    })
                    .unwrap_or_default();
                crate::services::routing_upstreams::key_observation(&key)
            }),
            provider_id: target.provider_id.clone(),
            provider_name,
            model: label(&target.model),
            fixed_effort: target.fixed_effort.clone(),
            context_window: target.effort.context.window(),
            account: Some(account),
            scheduling: Arc::downgrade(&target.scheduling),
        }
    }

    pub(crate) fn live_accounts(&self, requests: &[Route]) -> Vec<Account> {
        let mut accounts = BTreeMap::new();
        for candidate in requests.iter().flat_map(|row| &row.candidates) {
            if let Some(mut account) = candidate.account.clone() {
                if let Some(current) = candidate
                    .scheduling
                    .upgrade()
                    .and_then(|s| s.trace_account())
                {
                    account.telemetry = current;
                } else {
                    account.telemetry.windows.clear();
                }
                accounts
                    .entry((account.id.clone(), account.model.clone()))
                    .or_insert(account);
            }
        }
        accounts.into_values().collect()
    }

    pub(crate) fn begin(
        self: &Arc<Self>,
        agent: &str,
        model: &str,
        group_name: &str,
        strategy: &str,
        started: Instant,
    ) -> Request {
        let id = uuid::Uuid::new_v4().to_string();
        if let Ok(mut rows) = self.rows.try_write() {
            rows.push_front(Route {
                id: id.clone(),
                started_at: chrono::Utc::now()
                    .timestamp_millis()
                    .saturating_sub(started.elapsed().as_millis() as i64),
                agent: label(agent),
                kind: None,
                session: None,
                model: label(model),
                group_name: label(group_name),
                strategy: label(strategy),
                candidates: Vec::new(),
                steps: Vec::new(),
                state: State::Routing,
                status: None,
                duration_ms: 0,
                first_token_ms: None,
                output_tokens_per_second: None,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                estimated_tokens: None,
                cost_usd: None,
            });
            rows.truncate(CAPACITY);
        }
        Request {
            store: self.clone(),
            id,
            started,
            finished: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<Route> {
        self.rows
            .try_read()
            .map(|rows| {
                rows.iter()
                    .cloned()
                    .map(|mut row| {
                        if matches!(row.state, State::Routing | State::Streaming) {
                            row.duration_ms = chrono::Utc::now()
                                .timestamp_millis()
                                .saturating_sub(row.started_at)
                                .max(0) as u64;
                        }
                        row
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub(crate) struct Request {
    store: Arc<Store>,
    pub id: String,
    started: Instant,
    finished: Arc<AtomicBool>,
}

impl Request {
    pub(crate) fn auxiliary_purpose(&self, kind: &'static str) {
        self.update(|row| row.kind = Some(kind.into()));
    }

    fn update(&self, update: impl FnOnce(&mut Route)) {
        if let Ok(mut rows) = self.store.rows.try_write() {
            if let Some(row) = rows.iter_mut().find(|row| row.id == self.id) {
                row.duration_ms = self.started.elapsed().as_millis() as u64;
                update(row);
            }
        }
    }

    pub(crate) fn purpose(&self, headers: &http::HeaderMap, compact: bool) {
        let kind = if compact {
            Some("compact")
        } else if let Some(marker) = headers
            .get("x-openai-subagent")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
        {
            Some(known_kind(marker).unwrap_or("unknown"))
        } else {
            #[derive(serde::Deserialize)]
            struct Metadata {
                thread_source: Option<String>,
            }
            headers
                .get("x-codex-turn-metadata")
                .filter(|value| value.as_bytes().len() <= 8192)
                .and_then(|value| serde_json::from_slice::<Metadata>(value.as_bytes()).ok())
                .and_then(|metadata| metadata.thread_source)
                .filter(|source| !matches!(source.as_str(), "" | "user" | "subagent"))
                .map(|source| known_kind(&source).unwrap_or("unknown"))
        };
        self.update(|row| row.kind = kind.map(str::to_owned));
    }

    pub(crate) fn session(&self, session: &str, consumer: &str) {
        let mut hash = Sha256::new();
        for part in [&self.store.salt, consumer, session] {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        let digest = format!("{:x}", hash.finalize());
        self.update(|row| row.session = Some(digest));
    }

    pub(crate) fn plan(&self, candidates: Vec<Candidate>, estimated: Option<u64>) {
        self.update(|row| {
            row.candidates = candidates.into_iter().take(256).collect();
            row.estimated_tokens = estimated;
        });
        self.step(Kind::Strategy, None, None);
    }

    pub(crate) fn observe_account(
        &self,
        candidate: usize,
        scheduling: &super::scheduling::Scheduling,
    ) {
        if let Some(telemetry) = scheduling.trace_account() {
            self.update(|row| {
                if let Some(account) = row
                    .candidates
                    .get_mut(candidate)
                    .and_then(|c| c.account.as_mut())
                {
                    account.telemetry = telemetry;
                }
            });
        }
    }

    pub(crate) fn step(&self, kind: Kind, candidate: Option<usize>, status: Option<u16>) {
        let mut active_key = None;
        self.update(|row| {
            if kind == Kind::Attempt {
                active_key = candidate
                    .and_then(|index| row.candidates.get(index))
                    .and_then(|candidate| {
                        candidate
                            .key_id
                            .as_ref()
                            .map(|key| (candidate.provider_id.clone(), key.clone()))
                    });
            }
            if kind == Kind::Connected {
                row.state = State::Streaming;
            }
            // Keep the first decisions; finish reserves the final state.
            if row.steps.len() < 1023
                && (candidate.is_none_or(|index| index < row.candidates.len()))
            {
                row.steps.push(Step {
                    kind,
                    at_ms: row.duration_ms,
                    candidate,
                    status,
                });
            }
        });
        if kind == Kind::Attempt {
            if let Ok(mut active) = self.store.active_keys.try_write() {
                active.retain(|_, entry| !entry.finished.load(Ordering::Relaxed));
                active.remove(&self.id);
                if active.len() < 8192 {
                    if let Some((provider, key)) = active_key {
                        active.insert(
                            self.id.clone(),
                            ActiveKey {
                                provider,
                                key,
                                finished: self.finished.clone(),
                            },
                        );
                    }
                }
            }
        }
    }

    pub(crate) fn usage(
        &self,
        usage: &super::super::usage::parser::TokenUsage,
        first_content: Option<Instant>,
    ) {
        self.update(|row| {
            row.input_tokens = Some(usage.input_tokens);
            row.output_tokens = Some(usage.output_tokens);
            row.cache_read_tokens = Some(usage.cache_read_tokens);
            row.first_token_ms = first_content
                .map(|at| at.saturating_duration_since(self.started).as_millis() as u64);
        });
    }

    pub(crate) fn first_content(&self, at: Instant) {
        self.update(|row| {
            row.first_token_ms.get_or_insert_with(|| {
                at.saturating_duration_since(self.started).as_millis() as u64
            });
        });
    }

    pub(crate) fn speed(&self, decode_ms: f64, output_tokens: u32) {
        if decode_ms > 0.0 && output_tokens > 0 {
            let speed = f64::from(output_tokens) * 1000.0 / decode_ms;
            if speed.is_finite() {
                self.update(|row| row.output_tokens_per_second = Some(speed));
            }
        }
    }

    pub(crate) fn finish(&self, status: u16, state: State) {
        if self.finished.swap(true, Ordering::Relaxed) {
            return;
        }
        if let Ok(mut active) = self.store.active_keys.try_write() {
            active.remove(&self.id);
        }
        let kind = match state {
            State::Completed => Kind::Completed,
            State::Cancelled => Kind::Cancelled,
            _ => Kind::Failed,
        };
        let mut completed = None;
        self.update(|row| {
            row.state = state;
            row.status = Some(status);
            row.steps.truncate(1023);
            let candidate = row
                .steps
                .iter()
                .rev()
                .find(|step| step.kind == Kind::Attempt)
                .and_then(|step| step.candidate);
            row.steps.push(Step {
                kind,
                at_ms: row.duration_ms,
                candidate,
                status: Some(status),
            });
            if self.store.history.is_some() {
                completed = Some(row.clone());
            }
        });
        if let (Some(history), Some(row)) = (&self.store.history, completed) {
            history.save(row);
        }
    }
}

// Keep only catalog tokens, never arbitrary client metadata or header text.
fn known_kind(marker: &str) -> Option<&'static str> {
    match marker {
        "guardian" => Some("guardian"),
        "auto_review" => Some("auto_review"),
        "guardian_review" => Some("guardian_review"),
        "review" => Some("review"),
        "compact" => Some("compact"),
        "memory_consolidation" => Some("memory_consolidation"),
        "memgen" => Some("memgen"),
        "memory" => Some("memory"),
        "thread_title" => Some("thread_title"),
        "thread_title_reconsideration" => Some("thread_title_reconsideration"),
        "title_generation" => Some("title_generation"),
        "title" => Some("title"),
        "thread_description" => Some("thread_description"),
        "composer_predictions" => Some("composer_predictions"),
        "collab_spawn" => Some("collab_spawn"),
        "thread_spawn" => Some("thread_spawn"),
        "agent_job" => Some("agent_job"),
        "luna_reserve" => Some("luna_reserve"),
        "ambient_suggestions" => Some("ambient_suggestions"),
        "ambient_suggestion_safety" => Some("ambient_suggestion_safety"),
        "web_search" => Some("web_search"),
        "vision" => Some("vision"),
        "routing" => Some("routing"),
        "provider_test" => Some("provider_test"),
        "api_detect" => Some("api_detect"),
        "model_list" => Some("model_list"),
        "mcp" => Some("mcp"),
        _ => None,
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        self.finish(499, State::Cancelled);
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AccountKind {
    Account,
    Key,
    Provider,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    pub id: String,
    pub kind: AccountKind,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    #[serde(flatten)]
    pub telemetry: Telemetry,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Telemetry {
    pub rest_until: i64,
    pub sunk_at: Option<i64>,
    pub windows: Vec<Allowance>,
    pub first_token_ms: Option<f64>,
    pub output_tokens_per_second: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Allowance {
    pub used_percent: f64,
    pub window_seconds: i64,
    pub resets_at: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub requests: Vec<Route>,
    pub accounts: Vec<Account>,
    pub capacity: usize,
    pub running: bool,
    pub days: Vec<String>,
    pub history_error: bool,
}
