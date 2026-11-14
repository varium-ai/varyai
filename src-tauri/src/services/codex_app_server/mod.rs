//! Follow settled Codex routing changes through the desktop app's own app-server.
//! Catalog changes only offer a manual refresh. Windows desktop recovery is used
//! only when an ended app-server has no replacement; every step waits for idle.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

mod activity;
pub(crate) mod freshness;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(any(windows, test))]
mod windows;
#[cfg(windows)]
use windows as platform;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Instance {
    pid: i32,
    started_us: u64,
}

pub(crate) use super::agent_connections::ReloadState;

struct Server {
    identity: Instance,
    route: [u8; 32],
    catalog: Option<[u8; 32]>,
    catalog_stale: bool,
    attempted: bool,
    attempts: u8,
}

#[derive(Default)]
struct Monitor {
    route: Option<[u8; 32]>,
    catalog: Option<[u8; 32]>,
    running: Vec<Server>,
    pending: Option<Pending>,
    changed_at: u64,
    failed: bool,
}

struct Pending {
    old: Vec<Instance>,
    manual: Vec<Instance>,
    last_attempt: Vec<Instance>,
    deadline: Option<u64>,
    replacement_deadline: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    None,
    Restart(Vec<Instance>),
    Recovered,
    TimedOut,
}

impl Monitor {
    fn defer(&mut self) {
        if let Some(pending) = &mut self.pending {
            for server in &mut self.running {
                if pending.last_attempt.contains(&server.identity) {
                    server.attempts = server.attempts.saturating_sub(1);
                    server.attempted = server.attempts > 0;
                }
            }
            pending.last_attempt.clear();
            if !self.running.iter().any(|server| server.attempted) {
                pending.deadline = None;
            }
        }
    }

    fn observe_catalog(&mut self, catalog: Option<[u8; 32]>, modified_us: Option<u64>) {
        if let Some(catalog) = catalog {
            self.catalog = Some(catalog);
            for server in &mut self.running {
                if server.catalog.is_none() {
                    server.catalog = Some(catalog);
                    // At startup the previous catalog digest isn't known. A file
                    // definitely newer than this server is a manual-refresh hint.
                    server.catalog_stale = modified_us.is_some_and(|modified| {
                        modified > server.identity.started_us.saturating_add(2_000_000)
                    });
                }
            }
        }
    }

    fn snapshot(&self) -> ReloadState {
        if let Some(pending) = &self.pending {
            if self
                .running
                .iter()
                .any(|server| pending.old.contains(&server.identity) && server.attempted)
                || pending.deadline.is_some()
                || pending.replacement_deadline.is_some()
            {
                return ReloadState::Restarting;
            }
            return ReloadState::WaitingForIdle;
        }
        if self.failed {
            return ReloadState::Failed;
        }
        if self.running.is_empty() {
            return ReloadState::Stopped;
        }
        if self.catalog.is_some()
            && self
                .running
                .iter()
                .any(|server| server.catalog_stale || server.catalog != self.catalog)
        {
            return ReloadState::StaleCatalog;
        }
        ReloadState::Current
    }

    fn request_restart(&mut self, now: u64) {
        if self.running.is_empty() {
            return;
        }
        let old: Vec<_> = self.running.iter().map(|s| s.identity.clone()).collect();
        for server in &mut self.running {
            server.attempted = false;
            server.attempts = 0;
        }
        self.pending = Some(Pending {
            manual: old.clone(),
            old,
            last_attempt: Vec::new(),
            deadline: None,
            replacement_deadline: None,
        });
        self.failed = false;
        self.changed_at = now;
    }

    fn observe(&mut self, route: [u8; 32], running: Vec<Instance>, busy: bool, now: u64) -> Action {
        let changed = self.route != Some(route);
        if changed {
            self.route = Some(route);
            self.changed_at = now;
        }
        self.running
            .retain(|server| running.contains(&server.identity));
        for identity in running {
            if !self
                .running
                .iter()
                .any(|server| server.identity == identity)
            {
                // A new identity reads the configuration afresh, including on PID reuse.
                self.failed = false;
                self.running.push(Server {
                    identity,
                    route,
                    catalog: None,
                    catalog_stale: false,
                    attempted: false,
                    attempts: 0,
                });
            }
        }
        let stale: Vec<_> = self
            .running
            .iter()
            .filter(|server| server.route != route && server.attempts < 3)
            .map(|server| server.identity.clone())
            .collect();
        if !stale.is_empty() {
            let pending = self.pending.get_or_insert_with(|| Pending {
                old: Vec::new(),
                manual: Vec::new(),
                last_attempt: Vec::new(),
                deadline: None,
                replacement_deadline: None,
            });
            for identity in stale {
                if !pending.old.contains(&identity) {
                    pending.old.push(identity);
                }
            }
            self.failed = false;
        }
        let Some(pending) = &mut self.pending else {
            return Action::None;
        };
        // A route reverted before any signal no longer needs a restart. Already
        // attempted identities keep their deadline across subsequent changes.
        pending.old.retain(|identity| {
            self.running
                .iter()
                .find(|server| &server.identity == identity)
                .is_none_or(|server| {
                    server.attempted || server.route != route || pending.manual.contains(identity)
                })
        });
        if pending.old.is_empty() {
            self.pending = None;
            return Action::None;
        }
        let old: Vec<_> = self
            .running
            .iter()
            .filter(|server| pending.old.contains(&server.identity))
            .collect();
        if old.is_empty() && !self.running.is_empty() {
            self.pending = None;
            self.failed = false;
            return Action::Recovered;
        }
        // Exiting the old process and receiving a replacement are separate
        // phases. A slow exit must not consume the replacement's 30-second
        // allowance or trigger Windows desktop recovery as the old server exits.
        let deadline = if old.is_empty() {
            Some(
                *pending
                    .replacement_deadline
                    .get_or_insert(now.saturating_add(30)),
            )
        } else {
            pending.replacement_deadline = None;
            pending.deadline
        };
        if deadline.is_some_and(|deadline| now >= deadline) {
            self.pending = None;
            self.failed = true;
            return Action::TimedOut;
        }
        if !old.is_empty() && !busy && now.saturating_sub(self.changed_at) >= 3 {
            let targets: Vec<_> = old
                .into_iter()
                .filter(|server| server.attempts < 3)
                .map(|server| server.identity.clone())
                .collect();
            if !targets.is_empty() {
                for server in &mut self.running {
                    if targets.contains(&server.identity) {
                        server.attempted = true;
                        server.attempts += 1;
                    }
                }
                pending.last_attempt = targets.clone();
                pending.deadline.get_or_insert(now.saturating_add(20));
                return Action::Restart(targets);
            }
        }
        Action::None
    }
}

fn terminate_targets(
    targets: &[Instance],
    mut terminate: impl FnMut(&Instance) -> Result<(), ()>,
) -> Vec<Instance> {
    targets
        .iter()
        .filter(|target| terminate(target).is_err())
        .cloned()
        .collect()
}

fn read(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    match fs::symlink_metadata(path) {
        Ok(meta)
            if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 8 * 1024 * 1024 =>
        {
            Err(())
        }
        Ok(_) => fs::read(path).map(Some).map_err(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

fn config_doc(config: &Path) -> Result<toml::Value, ()> {
    let bytes = read(config)?.unwrap_or_default();
    std::str::from_utf8(&bytes)
        .map_err(|_| ())?
        .parse()
        .map_err(|_| ())
}

fn selected_profile(doc: &toml::Value) -> Result<Option<&toml::Value>, ()> {
    doc.get("profile")
        .map(|profile| {
            doc.get("profiles")
                .and_then(|profiles| profiles.get(profile.as_str()?))
                .filter(|profile| profile.is_table())
                .ok_or(())
        })
        .transpose()
}

fn modified_us(path: &Path) -> Result<u64, ()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => u64::try_from(
            meta.modified()
                .map_err(|_| ())?
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map_err(|_| ())?
                .as_micros(),
        )
        .map_err(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(_) => Err(()),
    }
}

fn catalog_sample(config: &Path) -> Result<([u8; 32], u64), ()> {
    let doc = config_doc(config)?;
    let mut modified = modified_us(config)?;
    let profile = selected_profile(&doc)?;
    let catalog = profile
        .and_then(|profile| profile.get("model_catalog_json"))
        .or_else(|| doc.get("model_catalog_json"));
    let mut digest = Sha256::new();
    if let Some(catalog) = catalog {
        let path = catalog.as_str().ok_or(())?;
        let home = config.parent().ok_or(())?;
        let path_on_disk = home.join(path);
        let bytes = read(&path_on_disk)?.ok_or(())?;
        modified = modified.max(modified_us(&path_on_disk)?);
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
        if !value.get("models").is_some_and(Value::is_array) {
            return Err(());
        }
        digest.update(path.as_bytes());
        // Only the selectable model identities matter. Reordering entries or
        // refreshing descriptions, effort metadata and priorities is harmless.
        let mut models = value["models"]
            .as_array()
            .ok_or(())?
            .iter()
            .map(|model| {
                let slug = model.get("slug").and_then(Value::as_str).ok_or(())?;
                let visibility = model
                    .get("visibility")
                    .map(|value| value.as_str().ok_or(()))
                    .transpose()?
                    .unwrap_or("");
                Ok((slug, visibility))
            })
            .collect::<Result<Vec<_>, ()>>()?;
        models.sort_unstable();
        digest.update(serde_json::to_vec(&models).map_err(|_| ())?);
    }
    Ok((digest.finalize().into(), modified))
}

fn catalog_fingerprint(config: &Path) -> Result<[u8; 32], ()> {
    catalog_sample(config).map(|sample| sample.0)
}

fn files_unchanged(config: &Path, route: [u8; 32], catalog: Option<[u8; 32]>) -> bool {
    catalog.is_some_and(|catalog| {
        fingerprint(config).ok() == Some(route) && catalog_fingerprint(config).ok() == Some(catalog)
    })
}

fn fingerprint(config: &Path) -> Result<[u8; 32], ()> {
    let doc = config_doc(config)?;
    let profile = selected_profile(&doc)?;
    let get = |key: &str| profile.and_then(|p| p.get(key)).or_else(|| doc.get(key));
    let provider = match get("model_provider") {
        Some(value) => value.as_str().filter(|value| !value.is_empty()).ok_or(())?,
        None => "openai",
    };
    if get("openai_base_url").is_some_and(|url| !url.is_str()) {
        return Err(());
    }
    let mut table = doc
        .get("model_providers")
        .and_then(|p| p.get(provider))
        .cloned();
    if (provider != "openai" && table.is_none())
        || table.as_ref().is_some_and(|table| {
            !table.is_table()
                || table.get("base_url").is_some_and(|url| !url.is_str())
                || table
                    .get("requires_openai_auth")
                    .is_some_and(|auth| !auth.is_bool())
        })
    {
        return Err(());
    }
    let no_account = provider != "openai"
        && table
            .as_ref()
            .and_then(|table| table.get("requires_openai_auth"))
            .and_then(toml::Value::as_bool)
            != Some(true);
    if let Some(table) = table.as_mut().and_then(toml::Value::as_table_mut) {
        table.remove("name");
    }
    let mut digest = Sha256::new();
    // Catalogs and profile/provider display names don't determine where requests
    // go. Catalog freshness is reported separately for a confirmed manual reload.
    let route = json!({
        "provider": provider,
        "table": table,
        "native_url": get("openai_base_url"),
        "auth_store": get("cli_auth_credentials_store"),
        "login_method": get("forced_login_method"),
        "login_workspace": get("forced_chatgpt_workspace_id"),
    });
    digest.update(serde_json::to_vec(&route).map_err(|_| ())?);
    let home = config.parent().ok_or(())?;
    if let Some(bytes) = if no_account {
        None
    } else {
        read(&home.join("auth.json"))?
    } {
        let auth: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
        if !auth.is_object() {
            return Err(());
        }
        // Access/refresh token rotations keep the same account and do not need a restart.
        use base64::Engine;
        let identity = auth
            .pointer("/tokens/id_token")
            .and_then(Value::as_str)
            .and_then(|token| token.split('.').nth(1))
            .and_then(|part| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(part)
                    .ok()
            })
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        digest.update(
            serde_json::to_vec(&json!({
                "mode": auth.get("auth_mode"),
                "api_key": auth.get("OPENAI_API_KEY"),
                "account": auth.pointer("/tokens/account_id"),
                "subject": identity.as_ref().and_then(|v| v.get("sub")),
            }))
            .map_err(|_| ())?,
        );
    }
    Ok(digest.finalize().into())
}

#[cfg(any(target_os = "macos", windows))]
struct Control {
    state: ReloadState,
    requested: bool,
}

#[cfg(any(target_os = "macos", windows))]
pub(crate) struct Worker {
    task: tokio::task::AbortHandle,
    control: std::sync::Arc<std::sync::Mutex<Control>>,
}

#[cfg(any(target_os = "macos", windows))]
impl Worker {
    pub(crate) fn snapshot(&self) -> ReloadState {
        self.control
            .lock()
            .map(|control| control.state)
            .unwrap_or(ReloadState::Failed)
    }

    /// Queue an explicitly confirmed refresh; only the worker may end a server.
    pub(crate) fn request_restart(&self) -> Result<(), String> {
        let mut control = self
            .control
            .lock()
            .map_err(|_| "agentConnections.restartFailed".to_owned())?;
        if !matches!(
            control.state,
            ReloadState::StaleCatalog | ReloadState::Failed
        ) {
            return Err("agentConnections.restartChanged".into());
        }
        control.requested = true;
        control.state = ReloadState::WaitingForIdle;
        Ok(())
    }
}

#[cfg(any(target_os = "macos", windows))]
impl Drop for Worker {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(any(target_os = "macos", windows))]
pub(crate) fn start(
    manager: std::sync::Weak<super::intent_routing::client::Manager>,
    locks: crate::proxy::switch_lock::SwitchLockManager,
    requests: std::sync::Arc<crate::proxy::client_activity::Activity>,
    desktop: std::sync::Arc<super::agent_connections::Manager>,
) -> Worker {
    let control = std::sync::Arc::new(std::sync::Mutex::new(Control {
        state: ReloadState::Stopped,
        requested: false,
    }));
    let worker_control = control.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut monitor = Monitor::default();
        let clock = std::time::Instant::now();
        loop {
            let Some(manager) = manager.upgrade() else {
                break;
            };
            let guard = locks.lock_for_app("codex").await;
            let config = manager.config_path().to_owned();
            let manual = worker_control.lock().is_ok_and(|control| control.requested);
            let needs_activity = monitor.pending.is_some() || manual;
            let previous_route = monitor.route;
            let sample_config = config.clone();
            let sample_requests = requests.clone();
            let sample_desktop = desktop.clone();
            let sample = tokio::task::spawn_blocking(move || {
                let home = sample_config.parent().ok_or(())?;
                let route = fingerprint(&sample_config)?;
                let freshness = sample_desktop
                    .codex_startup_freshness(&sample_config)
                    .map_err(|_| ())?;
                let catalog = catalog_sample(&sample_config).ok().map(|(digest, _)| {
                    (
                        digest,
                        freshness.catalog_changed_us.max(freshness.route_changed_us),
                    )
                });
                let processes = platform::instances(home)?;
                let since = processes.iter().map(|p| p.started_us).min().unwrap_or(0);
                let busy = (needs_activity
                    || previous_route.is_some_and(|previous| previous != route))
                    && (catalog.is_none()
                        || sample_requests.busy()
                        || activity::busy(home, since).unwrap_or(true));
                Ok::<_, ()>((route, catalog, processes, busy))
            })
            .await;
            if let Ok(Ok((route, catalog, processes, busy))) = sample {
                let now = clock.elapsed().as_secs();
                let mut action = monitor.observe(route, processes, busy, now);
                monitor.observe_catalog(
                    catalog.map(|sample| sample.0),
                    catalog.map(|sample| sample.1),
                );
                if let Ok(mut control) = worker_control.lock() {
                    if control.requested {
                        monitor.request_restart(now);
                        control.requested = false;
                        action = Action::None;
                    }
                    // Publish before awaiting the final recheck, so another command
                    // cannot queue a second refresh against an outdated UI sample.
                    control.state = monitor.snapshot();
                }
                match action {
                    Action::Restart(targets) => {
                        let client = manager.clone();
                        let final_requests = requests.clone();
                        let result = tokio::task::spawn_blocking(move || {
                            let home = config.parent().ok_or(())?;
                            let since = targets.iter().map(|p| p.started_us).min().ok_or(())?;
                            // An external writer or a newly started turn always wins.
                            if !files_unchanged(&config, route, catalog.map(|sample| sample.0))
                                || final_requests.busy()
                                || activity::busy(home, since).unwrap_or(true)
                            {
                                return Ok(None);
                            }
                            if client.sync_threads().is_err() {
                                log::warn!("Codex conversation provider recovery remains pending");
                            }
                            if !files_unchanged(&config, route, catalog.map(|sample| sample.0))
                                || final_requests.busy()
                                || activity::busy(home, since).unwrap_or(true)
                            {
                                return Ok(None);
                            }
                            let failed = terminate_targets(&targets, |target| {
                                platform::terminate(target, home)
                            });
                            Ok::<_, ()>(Some(failed))
                        })
                        .await;
                        match result {
                            Ok(Ok(Some(failed))) => {
                                if failed.is_empty() {
                                    log::info!("Codex routing changed; waiting for the desktop app-server to restart");
                                } else {
                                    log::warn!("Codex app-server restart remains pending");
                                }
                            }
                            Ok(Ok(None)) => monitor.defer(),
                            _ => {
                                log::warn!("Codex app-server restart unavailable");
                            }
                        }
                    }
                    Action::Recovered => {
                        let _ = tokio::task::spawn_blocking(move || manager.sync_threads()).await;
                        log::info!(
                            "Codex desktop app-server replacement observed after routing change"
                        );
                    }
                    Action::TimedOut => {
                        log::warn!("Codex desktop app-server restart did not recover within the wait period");
                        #[cfg(windows)]
                        if monitor.running.is_empty() {
                            let recovery_requests = requests.clone();
                            let recovery = tokio::task::spawn_blocking(move || {
                                let home = config
                                    .parent()
                                    .ok_or_else(|| "agentConnections.restartFailed".to_owned())?;
                                super::agent_connections::desktop::recover_codex(|| {
                                    files_unchanged(&config, route, catalog.map(|sample| sample.0))
                                        && !recovery_requests.busy()
                                        && !activity::busy(home, 0).unwrap_or(true)
                                        && platform::instances(home)
                                            .is_ok_and(|servers| servers.is_empty())
                                })
                            })
                            .await;
                            if matches!(recovery, Ok(Ok(()))) {
                                monitor.failed = false;
                            } else {
                                log::warn!("Codex desktop recovery remains unavailable");
                            }
                        }
                    }
                    Action::None => {}
                }
            } else {
                // A failed read is not evidence of a stable final configuration.
                monitor.changed_at = clock.elapsed().as_secs();
            }
            if let Ok(mut control) = worker_control.lock() {
                // A command queued during this sample stays visible until consumed.
                if !control.requested {
                    control.state = monitor.snapshot();
                }
            }
            drop(guard);
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
    Worker {
        task: task.inner().abort_handle(),
        control,
    }
}
