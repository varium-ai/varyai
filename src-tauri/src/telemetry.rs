use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::Duration,
};

use chrono::{FixedOffset, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const STATE_SCHEMA_VERSION: u8 = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(40);
const MAX_ATTEMPTS: usize = 3;
const STATISTICS_TZ_OFFSET_SECONDS: i32 = 8 * 60 * 60;
const GA4_MEASUREMENT_ID: &str = match option_env!("VARYAI_GA4_MEASUREMENT_ID") {
    Some(value) => value,
    None => "G-2R2MZ76G1E",
};

#[path = "telemetry_window.rs"]
mod window;
pub(crate) use window::destroy as destroy_analytics_window;
pub(crate) use window::{guard_invoke_handler, ANALYTICS_WINDOW_LABEL};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct TelemetryState {
    schema_version: u8,
    pub(crate) enabled: bool,
    #[serde(skip)]
    consent_revision: u64,
    pub(crate) installation_id: Option<String>,
    pub(crate) last_observed_date: Option<String>,
    pub(crate) last_reported_version: Option<String>,
    last_measurement_id: Option<String>,
}

impl Default for TelemetryState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            enabled: true,
            consent_revision: 0,
            installation_id: None,
            last_observed_date: None,
            last_reported_version: None,
            last_measurement_id: None,
        }
    }
}

pub(crate) struct TelemetryManager {
    path: Option<PathBuf>,
    state: Mutex<TelemetryState>,
    consent_changed: tokio::sync::watch::Sender<u64>,
    pending: Mutex<Option<PendingReport>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TelemetryPreferences {
    pub enabled: bool,
    pub available: bool,
}

impl TelemetryManager {
    pub(crate) fn load(path: Option<PathBuf>) -> Self {
        let saved = path.as_ref().and_then(|path| fs::read(path).ok());
        let mut state = saved
            .as_ref()
            .and_then(|data| serde_json::from_slice::<TelemetryState>(data).ok())
            .unwrap_or_else(|| TelemetryState {
                enabled: saved.is_none(),
                ..TelemetryState::default()
            });
        if !matches!(state.schema_version, 1 | 2 | STATE_SCHEMA_VERSION)
            || state.installation_id.as_deref().is_some_and(|id| {
                Uuid::parse_str(id)
                    .map(|uuid| uuid.get_version_num() != 4)
                    .unwrap_or(true)
            })
        {
            state = TelemetryState {
                enabled: false,
                ..TelemetryState::default()
            };
        }
        // Backend report markers do not establish a Google Analytics dispatch.
        if state.schema_version < STATE_SCHEMA_VERSION {
            state.last_observed_date = None;
            state.last_reported_version = None;
        }
        state.schema_version = STATE_SCHEMA_VERSION;
        // Reporting is on by default. Preserve an explicit disabled choice from
        // older builds, and retain valid legacy IDs to avoid counting upgrades twice.
        if !state.enabled {
            state.installation_id = None;
            state.last_observed_date = None;
            state.last_reported_version = None;
        } else if state.installation_id.is_none() {
            state.installation_id = Some(Uuid::new_v4().to_string());
        }
        // Persist the identifier before reporting so restarts keep the same identity.
        if (saved.is_some() || state.enabled) && persist_state(path.as_deref(), &state).is_err() {
            state.enabled = false;
            state.installation_id = None;
            log::warn!("[Telemetry] Preferences storage unavailable; reporting disabled");
        }
        let (consent_changed, _) = tokio::sync::watch::channel(0);
        Self {
            path,
            state: Mutex::new(state),
            consent_changed,
            pending: Mutex::new(None),
        }
    }

    pub(crate) fn preferences(&self) -> TelemetryPreferences {
        TelemetryPreferences {
            enabled: self.snapshot().enabled,
            available: measurement_id().is_some() && supported_platform().is_some(),
        }
    }

    pub(crate) fn set_enabled(&self, enabled: bool) -> Result<TelemetryPreferences, String> {
        let mut state = lock_state(&self.state);
        let changed = state.enabled != enabled;
        if changed {
            let mut next = state.clone();
            next.enabled = enabled;
            next.installation_id = enabled.then(|| Uuid::new_v4().to_string());
            next.last_observed_date = None;
            next.last_reported_version = None;
            next.consent_revision = next.consent_revision.wrapping_add(1);
            persist_state(self.path.as_deref(), &next)?;
            *state = next;
            self.consent_changed.send_replace(state.consent_revision);
        }
        drop(state);
        if changed {
            self.cancel_pending();
        }
        Ok(self.preferences())
    }

    pub(crate) fn start_worker(self: Arc<Self>, app: tauri::AppHandle) {
        if self.preferences().available {
            tauri::async_runtime::spawn(telemetry_worker(Arc::downgrade(&self), app));
        }
    }

    pub(crate) fn snapshot(&self) -> TelemetryState {
        lock_state(&self.state).clone()
    }

    fn permits_report(&self, expected: &TelemetryState) -> bool {
        let state = lock_state(&self.state);
        state.enabled
            && state.installation_id.is_some()
            && state.installation_id == expected.installation_id
            && state.consent_revision == expected.consent_revision
    }

    fn record_success(
        &self,
        expected: &TelemetryState,
        observed_date: String,
        app_version: &str,
    ) -> Result<bool, String> {
        let mut current = lock_state(&self.state);
        if !current.enabled
            || current.installation_id != expected.installation_id
            || current.consent_revision != expected.consent_revision
        {
            return Ok(false);
        }
        let mut next = current.clone();
        next.last_observed_date = Some(observed_date);
        next.last_reported_version = Some(app_version.to_string());
        next.last_measurement_id = measurement_id().map(str::to_string);
        persist_state(self.path.as_deref(), &next)?;
        *current = next;
        Ok(true)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnalyticsReport {
    token: String,
    measurement_id: String,
    installation_id: String,
    app_version: String,
    os: String,
    arch: String,
    observed_date: String,
    events: Vec<&'static str>,
}

struct PendingReport {
    report: AnalyticsReport,
    expected: TelemetryState,
    completion: tokio::sync::oneshot::Sender<bool>,
}

impl TelemetryManager {
    fn begin_report(
        &self,
        expected: &TelemetryState,
        date: &str,
        version: &str,
    ) -> Option<(AnalyticsReport, tokio::sync::oneshot::Receiver<bool>)> {
        if !self.permits_report(expected) || !report_due(expected, date, version) {
            return None;
        }
        let (os, arch) = supported_platform()?;
        let report = AnalyticsReport {
            token: Uuid::new_v4().to_string(),
            measurement_id: measurement_id()?.to_string(),
            installation_id: expected.installation_id.clone()?,
            app_version: version.to_string(),
            os: os.into(),
            arch: arch.into(),
            observed_date: date.into(),
            events: report_events(expected, date, version),
        };
        let (completion, receiver) = tokio::sync::oneshot::channel();
        *self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(PendingReport {
            report: report.clone(),
            expected: expected.clone(),
            completion,
        });
        Some((report, receiver))
    }

    fn pending_report(&self) -> Option<AnalyticsReport> {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending
            .as_ref()
            .filter(|pending| self.permits_report(&pending.expected))
            .map(|pending| pending.report.clone())
    }

    pub(crate) fn complete_report(&self, token: &str, dispatched: bool) -> Result<bool, String> {
        let pending = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if pending.as_ref().is_none_or(|p| p.report.token != token) {
                return Ok(false);
            }
            pending.take()
        };
        let Some(pending) = pending else {
            return Ok(false);
        };
        let saved = if dispatched {
            self.record_success(
                &pending.expected,
                pending.report.observed_date,
                &pending.report.app_version,
            )
        } else {
            Ok(false)
        };
        let _ = pending
            .completion
            .send(saved.as_ref().copied().unwrap_or(false));
        saved
    }

    fn cancel_pending(&self) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}

fn lock_state(state: &Mutex<TelemetryState>) -> MutexGuard<'_, TelemetryState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn persist_state(path: Option<&Path>, state: &TelemetryState) -> Result<(), String> {
    let path = path.ok_or_else(|| "statistics settings storage is unavailable".to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "statistics settings storage is unavailable".to_string())?;
    fs::create_dir_all(parent).map_err(|_| "statistics settings could not be saved".to_string())?;
    let temporary = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("analytics"),
        Uuid::new_v4()
    ));
    let serialized = serde_json::to_vec(state)
        .map_err(|_| "statistics settings could not be saved".to_string())?;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| "statistics settings could not be saved".to_string())?;
    if file.write_all(&serialized).is_err() || file.sync_all().is_err() {
        let _ = fs::remove_file(&temporary);
        return Err("statistics settings could not be saved".to_string());
    }
    drop(file);
    if replace_file(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err("statistics settings could not be saved".to_string());
    }
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn report_due(state: &TelemetryState, today: &str, app_version: &str) -> bool {
    state.enabled
        && state.installation_id.is_some()
        && (state.last_observed_date.as_deref() != Some(today)
            || state.last_reported_version.as_deref() != Some(app_version)
            || state.last_measurement_id.as_deref() != measurement_id())
}

fn report_events(state: &TelemetryState, today: &str, version: &str) -> Vec<&'static str> {
    if state.last_observed_date.is_none()
        || state.last_measurement_id.as_deref() != measurement_id()
    {
        vec!["desktop_install_observed", "desktop_daily_active"]
    } else if state.last_observed_date.as_deref() != Some(today) {
        vec!["desktop_daily_active"]
    } else if state.last_reported_version.as_deref() != Some(version) {
        vec!["desktop_version_changed"]
    } else {
        vec![]
    }
}

async fn telemetry_worker(manager: Weak<TelemetryManager>, app: tauri::AppHandle) {
    let Some(owner) = manager.upgrade() else {
        return;
    };
    let mut changes = owner.consent_changed.subscribe();
    drop(owner);
    'worker: loop {
        let Some(owner) = manager.upgrade() else {
            window::destroy(&app);
            return;
        };
        let state = owner.snapshot();
        drop(owner);
        if !state.enabled {
            window::destroy(&app);
            if changes.changed().await.is_err() {
                return;
            }
            continue;
        }
        tokio::select! {
            biased;
            _ = changes.changed() => continue,
            _ = tokio::time::sleep(Duration::from_secs(30 + jitter_seconds(61))) => {}
        }
        let version = env!("CARGO_PKG_VERSION");
        let date = today_in_statistics_timezone();
        if report_due(&state, &date, version) {
            for attempt in 0..MAX_ATTEMPTS {
                let Some(owner) = manager.upgrade() else {
                    return;
                };
                let Some((report, completion)) = owner.begin_report(&state, &date, version) else {
                    break;
                };
                drop(owner);
                let dispatched = if window::show_report(&app, manager.clone(), &report).is_ok() {
                    tokio::select! {
                        biased;
                        _ = changes.changed() => {
                            if let Some(owner) = manager.upgrade() { owner.cancel_pending(); }
                            window::destroy(&app);
                            continue 'worker;
                        },
                        result = tokio::time::timeout(REQUEST_TIMEOUT, completion) => result.ok().and_then(Result::ok).unwrap_or(false),
                    }
                } else {
                    false
                };
                if dispatched {
                    break;
                }
                if let Some(owner) = manager.upgrade() {
                    owner.cancel_pending();
                }
                window::destroy(&app);
                if attempt + 1 < MAX_ATTEMPTS {
                    tokio::select! {
                        _ = changes.changed() => continue 'worker,
                        _ = tokio::time::sleep(Duration::from_secs(1 << attempt)) => {}
                    }
                }
            }
        }
        tokio::select! {
            _ = changes.changed() => {},
            _ = tokio::time::sleep(until_next_statistics_day(&date) + Duration::from_secs(jitter_seconds(121))) => {}
        }
    }
}

fn measurement_id() -> Option<&'static str> {
    GA4_MEASUREMENT_ID
        .strip_prefix("G-")
        .filter(|suffix| {
            !suffix.is_empty()
                && suffix.len() <= 62
                && suffix
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        })
        .map(|_| GA4_MEASUREMENT_ID)
}

fn supported_platform() -> Option<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        _ => return None,
    };
    Some((os, arch))
}

fn today_in_statistics_timezone() -> String {
    let offset = FixedOffset::east_opt(STATISTICS_TZ_OFFSET_SECONDS)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("UTC offset is valid"));
    Utc::now()
        .with_timezone(&offset)
        .format("%Y-%m-%d")
        .to_string()
}

fn until_next_statistics_day(dispatch_day: &str) -> Duration {
    until_next_statistics_day_at(Utc::now(), dispatch_day)
}

fn until_next_statistics_day_at(now: chrono::DateTime<Utc>, dispatch_day: &str) -> Duration {
    let offset = FixedOffset::east_opt(STATISTICS_TZ_OFFSET_SECONDS)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("UTC offset is valid"));
    let local_now = now.with_timezone(&offset);
    if local_now.format("%Y-%m-%d").to_string() != dispatch_day {
        return Duration::ZERO;
    }
    let next_day = local_now
        .date_naive()
        .succ_opt()
        .unwrap_or_else(|| local_now.date_naive());
    let Some(midnight) = next_day.and_hms_opt(0, 0, 0) else {
        return Duration::from_secs(24 * 60 * 60);
    };
    let Some(next_local) = offset.from_local_datetime(&midnight).single() else {
        return Duration::from_secs(24 * 60 * 60);
    };
    (next_local.with_timezone(&Utc) - now)
        .to_std()
        .unwrap_or(Duration::from_secs(1))
}

fn jitter_seconds(range: u64) -> u64 {
    if range <= 1 {
        return 0;
    }
    u64::from(Uuid::new_v4().as_bytes()[0]) % range
}

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;
