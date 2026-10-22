use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::Duration,
};

use chrono::{FixedOffset, NaiveDate, TimeZone, Utc};
use reqwest::{header::RETRY_AFTER, Client, StatusCode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const STATE_SCHEMA_VERSION: u8 = 2;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ATTEMPTS: usize = 3;
const STATISTICS_TZ_OFFSET_SECONDS: i32 = 8 * 60 * 60;
const TELEMETRY_BASE_URL: &str = match option_env!("VARYAI_ANALYTICS_BASE_URL") {
    Some(value) => value,
    None => "",
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct TelemetryState {
    schema_version: u8,
    pub(crate) enabled: bool,
    #[serde(skip)]
    consent_revision: u64,
    pub(crate) installation_id: Option<String>,
    pub(crate) last_observed_date: Option<String>,
    pub(crate) last_reported_version: Option<String>,
}

pub(crate) struct TelemetryManager {
    path: Option<PathBuf>,
    state: Mutex<TelemetryState>,
    consent_changed: tokio::sync::watch::Sender<u64>,
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
            .unwrap_or_default();
        if state.schema_version != STATE_SCHEMA_VERSION
            || state.installation_id.as_deref().is_some_and(|id| {
                Uuid::parse_str(id)
                    .map(|uuid| uuid.get_version_num() != 4)
                    .unwrap_or(true)
            })
        {
            state = TelemetryState::default();
        }
        state.schema_version = STATE_SCHEMA_VERSION;
        // A missing consent field is not consent, including files written by
        // older versions that reported automatically. Disabling also forgets ID.
        if !state.enabled {
            state.installation_id = None;
            state.last_observed_date = None;
            state.last_reported_version = None;
        } else if state.installation_id.is_none() {
            state.installation_id = Some(Uuid::new_v4().to_string());
        }
        // Fresh, opted-out installations need no identifier or state file.
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
        }
    }

    pub(crate) fn preferences(&self) -> TelemetryPreferences {
        TelemetryPreferences {
            enabled: self.snapshot().enabled,
            available: telemetry_endpoint().is_some() && supported_platform().is_some(),
        }
    }

    pub(crate) fn set_enabled(&self, enabled: bool) -> Result<TelemetryPreferences, String> {
        let mut state = lock_state(&self.state);
        if state.enabled != enabled {
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
        Ok(self.preferences())
    }

    pub(crate) fn start_worker(self: Arc<Self>) {
        if self.preferences().available {
            tauri::async_runtime::spawn(telemetry_worker(Arc::downgrade(&self)));
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
        persist_state(self.path.as_deref(), &next)?;
        *current = next;
        Ok(true)
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
            || state.last_reported_version.as_deref() != Some(app_version))
}

#[derive(Serialize)]
struct LaunchRequest<'a> {
    installation_id: &'a str,
    app_version: &'a str,
    os: &'a str,
    arch: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchResponse {
    observed_date: String,
}

enum RequestOutcome {
    Success(String),
    Retry(Option<Duration>),
    Stop,
}

async fn telemetry_worker(manager: Weak<TelemetryManager>) {
    let Some(owner) = manager.upgrade() else {
        return;
    };
    let mut changes = owner.consent_changed.subscribe();
    drop(owner);
    loop {
        let Some(owner) = manager.upgrade() else {
            return;
        };
        let state = owner.snapshot();
        drop(owner);
        if !state.enabled {
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
        if report_due(&state, &today_in_statistics_timezone(), version) {
            if let Some(date) = send_round(&manager, &state, version, &mut changes).await {
                if let Some(owner) = manager.upgrade() {
                    if owner.record_success(&state, date, version).is_err() {
                        log::warn!("[Telemetry] Could not persist the reporting date");
                    }
                }
            }
        }
        if !manager
            .upgrade()
            .is_some_and(|owner| owner.permits_report(&state))
        {
            continue;
        }
        tokio::select! {
            _ = changes.changed() => {},
            _ = tokio::time::sleep(until_next_statistics_day() + Duration::from_secs(jitter_seconds(121))) => {}
        }
    }
}

async fn send_round(
    manager: &Weak<TelemetryManager>,
    state: &TelemetryState,
    app_version: &str,
    changes: &mut tokio::sync::watch::Receiver<u64>,
) -> Option<String> {
    if !state.enabled {
        return None;
    }
    let installation_id = state.installation_id.as_deref()?;
    let endpoint = telemetry_endpoint()?;
    let (os, arch) = supported_platform()?;
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .https_only(true)
        .build()
        .ok()?;
    let payload = LaunchRequest {
        installation_id,
        app_version,
        os,
        arch,
    };
    for attempt in 0..MAX_ATTEMPTS {
        if !manager.upgrade()?.permits_report(state) {
            return None;
        }
        let result = tokio::select! {
            biased;
            _ = changes.changed() => return None,
            result = async { classify_response(client.post(endpoint.clone()).json(&payload).send().await).await } => result,
        };
        match result {
            RequestOutcome::Success(date) => return Some(date),
            RequestOutcome::Stop => return None,
            RequestOutcome::Retry(retry_after) if attempt + 1 < MAX_ATTEMPTS => {
                let backoff = retry_after.unwrap_or_else(|| {
                    Duration::from_secs(1u64 << attempt) + Duration::from_millis(jitter_millis(501))
                });
                tokio::select! {
                    _ = changes.changed() => return None,
                    _ = tokio::time::sleep(backoff) => {}
                }
            }
            RequestOutcome::Retry(_) => return None,
        }
    }
    None
}

async fn classify_response(result: Result<reqwest::Response, reqwest::Error>) -> RequestOutcome {
    let response = match result {
        Ok(response) => response,
        Err(_) => return RequestOutcome::Retry(None),
    };
    let status = response.status();
    if status == StatusCode::OK {
        const MAX_RESPONSE_BYTES: usize = 1024;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return RequestOutcome::Retry(None);
        }
        let mut response = response;
        let mut bytes = Vec::with_capacity(MAX_RESPONSE_BYTES);
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if bytes.len() + chunk.len() <= MAX_RESPONSE_BYTES => {
                    bytes.extend_from_slice(&chunk);
                }
                Ok(Some(_)) => return RequestOutcome::Retry(None),
                Ok(None) => break,
                Err(_) => return RequestOutcome::Retry(None),
            }
        }
        let body = match serde_json::from_slice::<LaunchResponse>(&bytes) {
            Ok(body) => body,
            Err(_) => return RequestOutcome::Retry(None),
        };
        return if NaiveDate::parse_from_str(&body.observed_date, "%Y-%m-%d")
            .map(|date| date.format("%Y-%m-%d").to_string() == body.observed_date)
            .unwrap_or(false)
        {
            RequestOutcome::Success(body.observed_date)
        } else {
            RequestOutcome::Retry(None)
        };
    }
    if status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        return RequestOutcome::Retry(
            response
                .headers()
                .get(RETRY_AFTER)
                .and_then(parse_retry_after),
        );
    }
    RequestOutcome::Stop
}

fn parse_retry_after(value: &reqwest::header::HeaderValue) -> Option<Duration> {
    let value = value.to_str().ok()?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let retry_at = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    Some(
        (retry_at.with_timezone(&Utc) - Utc::now())
            .to_std()
            .unwrap_or_default(),
    )
}

fn telemetry_endpoint() -> Option<reqwest::Url> {
    let mut base = reqwest::Url::parse(TELEMETRY_BASE_URL).ok()?;
    if base.scheme() != "https"
        || base.host_str().is_none()
        || base.username() != ""
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || (base.path() != "/" && !base.path().is_empty())
    {
        return None;
    }
    base.set_path("/api/v1/telemetry/launch");
    Some(base)
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

fn until_next_statistics_day() -> Duration {
    let offset = FixedOffset::east_opt(STATISTICS_TZ_OFFSET_SECONDS)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("UTC offset is valid"));
    let now = Utc::now();
    let local_now = now.with_timezone(&offset);
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

fn jitter_millis(range: u64) -> u64 {
    if range <= 1 {
        return 0;
    }
    let bytes = Uuid::new_v4();
    u64::from(u16::from_be_bytes([
        bytes.as_bytes()[0],
        bytes.as_bytes()[1],
    ])) % range
}

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;
