//! Desktop configuration freshness is independent of field ownership.
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Stopped,
    Current,
    RestartRequired,
    Unknown,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Instance {
    pid: i32,
    bundle: PathBuf,
    launched_ms: Option<i64>,
}

fn assess(instances: &[Instance], applied_ms: Option<i64>) -> State {
    if instances.is_empty() {
        return State::Stopped;
    }
    let Some(applied_ms) = applied_ms else {
        return State::Unknown;
    };
    if instances
        .iter()
        .any(|instance| instance.launched_ms.is_some_and(|time| time <= applied_ms))
    {
        return State::RestartRequired;
    }
    if instances
        .iter()
        .any(|instance| instance.launched_ms.is_none())
    {
        State::Unknown
    } else {
        State::Current
    }
}

pub(crate) fn state(app: &str, applied_ms: Option<i64>) -> Option<State> {
    if app != "claude-desktop" {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        Some(macos::instances().map_or(State::Unknown, |instances| assess(&instances, applied_ms)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = applied_ms;
        Some(State::Unsupported)
    }
}

const FAILED: &str = "agentConnections.restartFailed";
const CHANGED: &str = "agentConnections.restartChanged";
const RUNNING: &str = "agentConnections.restartBlocked";

trait Runtime {
    fn instances(&self) -> Result<Vec<Instance>, String>;
    fn quit(&self, instance: &Instance) -> Result<(), String>;
    fn wait_for_exit(&self, instances: &[Instance]) -> Result<(), String>;
    fn open(&self, bundle: &std::path::Path) -> Result<(), String>;
}

fn restart_with(
    runtime: &impl Runtime,
    applied_ms: Option<i64>,
    valid: impl Fn() -> bool,
) -> Result<(), String> {
    if !valid() {
        return Err(super::CONFLICT.into());
    }
    let instances = runtime.instances()?;
    if assess(&instances, applied_ms) != State::RestartRequired {
        return Err(CHANGED.into());
    }
    let target = instances.first().ok_or(CHANGED)?.bundle.clone();
    if instances.iter().any(|instance| instance.bundle != target) {
        return Err(CHANGED.into());
    }
    // Recheck launch identity before requesting a normal quit. Never terminate a replacement process.
    if !valid() || runtime.instances()? != instances {
        return Err(CHANGED.into());
    }
    for instance in &instances {
        runtime.quit(instance)?;
    }
    runtime.wait_for_exit(&instances)?;
    if !valid() {
        return Err(super::CONFLICT.into());
    }
    if !runtime.instances()?.is_empty() {
        return Err(CHANGED.into());
    }
    runtime.open(&target)
}

pub(crate) fn restart(
    app: &str,
    applied_ms: Option<i64>,
    valid: impl Fn() -> bool,
) -> Result<(), String> {
    if app != "claude-desktop" {
        return Err("agentConnections.restartUnsupported".into());
    }
    #[cfg(target_os = "macos")]
    {
        restart_with(&macos::Native, applied_ms, valid)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (applied_ms, valid);
        Err("agentConnections.restartUnsupported".into())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;

    const ID: &str = "com.anthropic.claudefordesktop";
    pub(super) struct Native;
    fn applications() -> objc2::rc::Retained<objc2_foundation::NSArray<NSRunningApplication>> {
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(ID))
    }
    fn identity(app: &NSRunningApplication) -> Result<Instance, String> {
        let path = PathBuf::from(
            app.bundleURL()
                .and_then(|url| url.path())
                .ok_or(FAILED)?
                .to_string(),
        );
        let trusted = [
            PathBuf::from("/Applications/Claude.app"),
            crate::config::get_home_dir().join("Applications/Claude.app"),
        ];
        if !trusted.contains(&path) || path.is_symlink() || !path.is_dir() {
            return Err(FAILED.into());
        }
        let date = app
            .launchDate()
            .map(|date| date.timeIntervalSince1970() * 1000.0)
            .filter(|date| date.is_finite() && *date > 0.0);
        Ok(Instance {
            pid: app.processIdentifier(),
            bundle: path,
            launched_ms: date.map(|date| date as i64),
        })
    }
    pub(super) fn instances() -> Result<Vec<Instance>, String> {
        applications()
            .iter()
            .filter(|app| !app.isTerminated())
            .map(|app| identity(&app))
            .collect()
    }
    impl Runtime for Native {
        fn instances(&self) -> Result<Vec<Instance>, String> {
            instances()
        }
        fn quit(&self, instance: &Instance) -> Result<(), String> {
            let apps = applications();
            let app = apps
                .iter()
                .find(|app| app.processIdentifier() == instance.pid)
                .ok_or(CHANGED)?;
            if identity(&app)? != *instance {
                return Err(CHANGED.into());
            }
            app.terminate().then_some(()).ok_or_else(|| RUNNING.into())
        }
        fn wait_for_exit(&self, instances: &[Instance]) -> Result<(), String> {
            let start = std::time::Instant::now();
            loop {
                let live = self.instances()?;
                if !live.iter().any(|app| instances.contains(app)) {
                    return Ok(());
                }
                if start.elapsed() > std::time::Duration::from_secs(30) {
                    return Err(RUNNING.into());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        fn open(&self, bundle: &std::path::Path) -> Result<(), String> {
            let status = std::process::Command::new("/usr/bin/open")
                .arg(bundle)
                .status()
                .map_err(|_| FAILED)?;
            status.success().then_some(()).ok_or_else(|| FAILED.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(launched_ms: Option<i64>) -> Instance {
        Instance {
            pid: 123,
            bundle: "/synthetic/Claude.app".into(),
            launched_ms,
        }
    }

    #[test]
    fn desktop_freshness_tracks_launches_instead_of_configuration_ownership() {
        assert_eq!(assess(&[], Some(2000)), State::Stopped);
        assert_eq!(
            assess(&[instance(Some(1000))], Some(2000)),
            State::RestartRequired
        );
        assert_eq!(assess(&[instance(Some(3000))], Some(2000)), State::Current);
        assert_eq!(
            assess(&[instance(Some(2000))], Some(2000)),
            State::RestartRequired
        );
        assert_eq!(assess(&[instance(None)], Some(2000)), State::Unknown);
        assert_eq!(assess(&[instance(Some(3000))], None), State::Unknown);
        assert_eq!(
            assess(&[instance(Some(1000)), instance(Some(3000))], Some(2000)),
            State::RestartRequired
        );
    }
}
