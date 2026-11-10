//! Desktop configuration freshness is independent of field ownership.
use serde::Serialize;
#[cfg(any(target_os = "windows", test))]
mod windows;
#[cfg(any(target_os = "macos", test))]
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    Stopped,
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    Current,
    RestartRequired,
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    Unknown,
    #[cfg(not(target_os = "macos"))]
    Unsupported,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Instance {
    pid: i32,
    bundle: PathBuf,
    launched_ms: Option<i64>,
}

#[cfg(any(target_os = "macos", test))]
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

pub(crate) fn supports_restart(app: &str) -> bool {
    // Desktop apps with a stable install identity can offer a verified quit/reopen.
    // CLI-only agents still require a new session instead of process restart.
    matches!(
        app,
        "claude-desktop"
            | "workbuddy"
            | "workbuddy-ai"
            | "codex"
            | "qoder"
            | "trae"
            | "doubaowork"
            | "kimi"
            | "zcode"
            | "deepseek-harness"
    )
}

#[cfg(any(target_os = "macos", test))]
fn bundle_ids(app: &str) -> &'static [&'static str] {
    match app {
        "claude-desktop" => &["com.anthropic.claudefordesktop"],
        "workbuddy" => &["com.tencent.workbuddy.mac"],
        "workbuddy-ai" => &["com.workbuddy.workbuddy-ai"],
        "codex" => &["com.openai.codex"],
        "qoder" => &["com.qodercn.app"],
        "trae" => &["cn.trae.app"],
        "doubaowork" => &["com.work.pc.doubao"],
        "kimi" => &["com.kimi.code.desktop"],
        "zcode" => &["dev.zcode.app"],
        "deepseek-harness" => &["com.deepseek.dsh"],
        _ => &[],
    }
}

#[cfg(any(target_os = "macos", test))]
fn workbuddy_bundle_ids(dir: &std::path::Path) -> Option<&'static [&'static str]> {
    match dir.file_name()?.to_str()? {
        ".workbuddy-ai" => Some(&["com.workbuddy.workbuddy-ai"]),
        ".workbuddy" => Some(&["com.tencent.workbuddy.mac"]),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn selected_bundle_ids(app: &str) -> Result<&'static [&'static str], String> {
    if matches!(app, "workbuddy" | "workbuddy-ai") {
        let expected = bundle_ids(app);
        workbuddy_bundle_ids(&crate::workbuddy_config::get_workbuddy_dir())
            .filter(|ids| *ids == expected)
            .or_else(|| (app == "workbuddy-ai").then_some(expected))
            .ok_or_else(|| FAILED.into())
    } else {
        Ok(bundle_ids(app))
    }
}

pub(crate) fn state(app: &str, applied_ms: Option<i64>) -> Option<State> {
    if !supports_restart(app) {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        Some(
            macos::Native { app }
                .instances()
                .map_or(State::Unknown, |instances| assess(&instances, applied_ms)),
        )
    }
    #[cfg(target_os = "windows")]
    {
        Some(windows::state(app, applied_ms))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = applied_ms;
        Some(State::Unsupported)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
const FAILED: &str = "agentConnections.restartFailed";
#[cfg(any(target_os = "macos", target_os = "windows", test))]
const CHANGED: &str = "agentConnections.restartChanged";
#[cfg(any(target_os = "macos", target_os = "windows", test))]
const RUNNING: &str = "agentConnections.restartBlocked";

#[cfg(any(target_os = "macos", test))]
trait Runtime {
    fn instances(&self) -> Result<Vec<Instance>, String>;
    fn quit(&self, instance: &Instance) -> Result<(), String>;
    fn wait_for_exit(&self, instances: &[Instance]) -> Result<(), String>;
    fn open(&self, bundle: &std::path::Path) -> Result<(), String>;
}

#[cfg(any(target_os = "macos", test))]
fn restart_with(
    runtime: &impl Runtime,
    applied_ms: Option<i64>,
    valid: impl Fn() -> bool,
) -> Result<(), String> {
    if !valid() {
        return Err(super::CONFLICT.into());
    }
    let instances = runtime.instances()?;
    if assess(&instances, applied_ms) != State::RestartRequired
        || instances
            .iter()
            .any(|instance| instance.launched_ms.is_none())
    {
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
    if !supports_restart(app) {
        return Err("agentConnections.restartUnsupported".into());
    }
    #[cfg(target_os = "macos")]
    {
        // First-time TRAE/Doubao setup may only need the desktop app opened.
        if matches!(app, "trae" | "doubaowork") && state(app, applied_ms) == Some(State::Stopped) {
            return open_installed(app, valid);
        }
        restart_with(&macos::Native { app }, applied_ms, valid)
    }
    #[cfg(target_os = "windows")]
    {
        windows::restart(app, applied_ms, valid)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (applied_ms, valid);
        Err("agentConnections.restartUnsupported".into())
    }
}

#[cfg(target_os = "macos")]
fn open_installed(app: &str, valid: impl Fn() -> bool) -> Result<(), String> {
    if !valid() {
        return Err(super::CONFLICT.into());
    }
    let ids = selected_bundle_ids(app)?;
    let roots = [
        PathBuf::from("/Applications"),
        crate::config::get_home_dir().join("Applications"),
    ];
    let mut candidates = Vec::new();
    for root in &roots {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if macos::bundle_matches(&path, ids, &roots) {
                    candidates.push(path);
                }
            }
        }
    }
    candidates.sort();
    let target = candidates.into_iter().next().ok_or(FAILED)?;
    if !valid() || !macos::bundle_matches(&target, ids, &roots) {
        return Err(if valid() { CHANGED } else { super::CONFLICT }.into());
    }
    let status = std::process::Command::new("/usr/bin/open")
        .arg(&target)
        .status()
        .map_err(|_| FAILED)?;
    status.success().then_some(()).ok_or_else(|| FAILED.into())
}

// Internal app-server fallback; public desktop selection remains unchanged.
#[cfg(target_os = "windows")]
pub(crate) fn recover_codex(valid: impl Fn() -> bool) -> Result<(), String> {
    windows::recover_codex(valid)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;

    pub(super) struct Native<'a> {
        pub app: &'a str,
    }
    fn applications(
        id: &str,
    ) -> objc2::rc::Retained<objc2_foundation::NSArray<NSRunningApplication>> {
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(id))
    }
    pub(super) fn bundle_matches(path: &std::path::Path, ids: &[&str], roots: &[PathBuf]) -> bool {
        use objc2_foundation::{
            NSData, NSDictionary, NSPropertyListMutabilityOptions, NSPropertyListSerialization,
        };
        if !roots.iter().any(|root| path.starts_with(root))
            || path.extension().is_none_or(|ext| ext != "app")
            || path.canonicalize().ok().as_deref() != Some(path)
            || !path.is_dir()
        {
            return false;
        }
        let Ok(bytes) = std::fs::read(path.join("Contents/Info.plist")) else {
            return false;
        };
        if bytes.len() > 1024 * 1024 {
            return false;
        }
        let data = NSData::from_vec(bytes);
        // Foundation parses immutable XML or binary plist bytes without NSBundle's metadata cache.
        let Ok(value) = (unsafe {
            NSPropertyListSerialization::propertyListWithData_options_format_error(
                &data,
                NSPropertyListMutabilityOptions::empty(),
                std::ptr::null_mut(),
            )
        }) else {
            return false;
        };
        value
            .downcast_ref::<NSDictionary>()
            .and_then(|dict| dict.objectForKey(&NSString::from_str("CFBundleIdentifier")))
            .and_then(|value| {
                value
                    .downcast_ref::<NSString>()
                    .map(|id| ids.contains(&id.to_string().as_str()))
            })
            .unwrap_or(false)
    }
    fn identity(app: &NSRunningApplication, ids: &[&str]) -> Result<Instance, String> {
        let id = app.bundleIdentifier().ok_or(FAILED)?.to_string();
        if !ids.contains(&id.as_str()) {
            return Err(FAILED.into());
        }
        let path = PathBuf::from(
            app.bundleURL()
                .and_then(|url| url.path())
                .ok_or(FAILED)?
                .to_string(),
        );
        let roots = [
            PathBuf::from("/Applications"),
            crate::config::get_home_dir().join("Applications"),
        ];
        // Accept renamed/nested bundles only when both the running identity and
        // the bundle on disk identify the selected app. Reject symlink escapes.
        if !bundle_matches(&path, &[&id], &roots) {
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
    impl Runtime for Native<'_> {
        fn instances(&self) -> Result<Vec<Instance>, String> {
            if cfg!(test) || std::env::var_os("CC_SWITCH_TEST_HOME").is_some() {
                return Err(FAILED.into());
            }
            let ids = selected_bundle_ids(self.app)?;
            let mut found = Vec::new();
            for id in ids {
                for app in applications(id).iter().filter(|app| !app.isTerminated()) {
                    found.push(identity(&app, ids)?);
                }
            }
            found.sort_by_key(|instance| instance.pid);
            Ok(found)
        }
        fn quit(&self, instance: &Instance) -> Result<(), String> {
            let ids = selected_bundle_ids(self.app)?;
            let apps = ids
                .iter()
                .flat_map(|id| applications(id).to_vec())
                .collect::<Vec<_>>();
            let app = apps
                .iter()
                .find(|app| app.processIdentifier() == instance.pid)
                .ok_or(CHANGED)?;
            if identity(app, ids)? != *instance {
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
            let roots = [
                PathBuf::from("/Applications"),
                crate::config::get_home_dir().join("Applications"),
            ];
            if !bundle_matches(bundle, selected_bundle_ids(self.app)?, &roots) {
                return Err(CHANGED.into());
            }
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

    use std::{cell::RefCell, collections::VecDeque};

    struct FakeRuntime {
        snapshots: RefCell<VecDeque<Vec<Instance>>>,
        events: RefCell<Vec<&'static str>>,
        quit_error: Option<&'static str>,
        wait_error: Option<&'static str>,
    }

    impl FakeRuntime {
        fn new(snapshots: Vec<Vec<Instance>>) -> Self {
            Self {
                snapshots: RefCell::new(snapshots.into()),
                events: RefCell::new(vec![]),
                quit_error: None,
                wait_error: None,
            }
        }
    }

    impl Runtime for FakeRuntime {
        fn instances(&self) -> Result<Vec<Instance>, String> {
            self.events.borrow_mut().push("instances");
            Ok(self
                .snapshots
                .borrow_mut()
                .pop_front()
                .expect("unexpected discovery"))
        }
        fn quit(&self, _: &Instance) -> Result<(), String> {
            self.events.borrow_mut().push("quit");
            self.quit_error.map_or(Ok(()), |error| Err(error.into()))
        }
        fn wait_for_exit(&self, _: &[Instance]) -> Result<(), String> {
            self.events.borrow_mut().push("wait");
            self.wait_error.map_or(Ok(()), |error| Err(error.into()))
        }
        fn open(&self, bundle: &std::path::Path) -> Result<(), String> {
            assert_eq!(bundle, std::path::Path::new("/synthetic/Claude.app"));
            self.events.borrow_mut().push("open");
            Ok(())
        }
    }

    #[test]
    fn supported_desktop_selection_includes_verified_desktop_agents() {
        assert!(supports_restart("claude-desktop"));
        assert!(supports_restart("workbuddy"));
        assert!(supports_restart("workbuddy-ai"));
        assert!(supports_restart("codex"));
        assert!(supports_restart("qoder"));
        assert!(supports_restart("trae"));
        assert!(supports_restart("doubaowork"));
        assert!(supports_restart("kimi"));
        assert!(supports_restart("zcode"));
        assert!(supports_restart("deepseek-harness"));
        assert_eq!(bundle_ids("workbuddy"), &["com.tencent.workbuddy.mac"]);
        assert_eq!(bundle_ids("workbuddy-ai"), &["com.workbuddy.workbuddy-ai"]);
        assert_eq!(
            bundle_ids("claude-desktop"),
            &["com.anthropic.claudefordesktop"]
        );
        assert_eq!(bundle_ids("codex"), &["com.openai.codex"]);
        assert_eq!(bundle_ids("qoder"), &["com.qodercn.app"]);
        assert_eq!(bundle_ids("trae"), &["cn.trae.app"]);
        assert_eq!(bundle_ids("doubaowork"), &["com.work.pc.doubao"]);
        assert_eq!(bundle_ids("kimi"), &["com.kimi.code.desktop"]);
        assert_eq!(bundle_ids("zcode"), &["dev.zcode.app"]);
        assert_eq!(bundle_ids("deepseek-harness"), &["com.deepseek.dsh"]);
        for app in ["claude", "gemini", "opencode", "pi", "unknown"] {
            assert!(!supports_restart(app), "{app}");
            assert!(bundle_ids(app).is_empty(), "{app}");
        }
    }

    #[test]
    fn workbuddy_selection_follows_the_managed_config_variant() {
        for (dir, id) in [
            (".workbuddy-ai", "com.workbuddy.workbuddy-ai"),
            (".workbuddy", "com.tencent.workbuddy.mac"),
        ] {
            assert_eq!(
                workbuddy_bundle_ids(std::path::Path::new(dir)),
                Some(&[id][..])
            );
        }
        assert_eq!(
            workbuddy_bundle_ids(std::path::Path::new("/custom/settings")),
            None
        );
        assert_eq!(
            workbuddy_bundle_ids(std::path::Path::new(".codebuddy")),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bundle_identity_is_read_from_disk_again_before_reopen() {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("Renamed.app");
        std::fs::create_dir_all(bundle.join("Contents")).unwrap();
        let plist = bundle.join("Contents/Info.plist");
        let write_id = |id: &str| {
            std::fs::write(&plist, format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{id}</string></dict></plist>"#)).unwrap()
        };
        let roots = [root.path().canonicalize().unwrap()];
        let bundle = bundle.canonicalize().unwrap();
        write_id("com.workbuddy.workbuddy-ai");
        assert!(macos::bundle_matches(
            &bundle,
            &["com.workbuddy.workbuddy-ai"],
            &roots
        ));
        assert!(!macos::bundle_matches(
            &bundle,
            &["com.tencent.workbuddy.mac"],
            &roots
        ));
        write_id("test.replacement");
        assert!(!macos::bundle_matches(
            &bundle,
            &["com.workbuddy.workbuddy-ai"],
            &roots
        ));
    }

    #[test]
    fn restart_normally_quits_waits_and_reopens_the_verified_bundle() {
        let old = vec![instance(Some(1000))];
        let runtime = FakeRuntime::new(vec![old.clone(), old, vec![]]);
        assert_eq!(restart_with(&runtime, Some(2000), || true), Ok(()));
        assert_eq!(
            *runtime.events.borrow(),
            [
                "instances",
                "instances",
                "quit",
                "wait",
                "instances",
                "open"
            ]
        );
    }

    #[test]
    fn restart_refuses_stopped_current_unknown_and_mixed_bundles() {
        let mut other = instance(Some(1000));
        other.bundle = "/synthetic/Other.app".into();
        for instances in [
            vec![],
            vec![instance(Some(3000))],
            vec![instance(None)],
            vec![instance(Some(1000)), instance(None)],
            vec![instance(Some(1000)), other],
        ] {
            let runtime = FakeRuntime::new(vec![instances]);
            assert_eq!(
                restart_with(&runtime, Some(2000), || true),
                Err(CHANGED.into())
            );
            assert_eq!(*runtime.events.borrow(), ["instances"]);
        }
    }

    #[test]
    fn restart_rechecks_configuration_before_quit_and_before_reopen() {
        for (checks, expected, events) in [
            (vec![false], super::super::CONFLICT, vec![]),
            (vec![true, false], CHANGED, vec!["instances"]),
            (
                vec![true, true, false],
                super::super::CONFLICT,
                vec!["instances", "instances", "quit", "wait"],
            ),
        ] {
            let old = vec![instance(Some(1000))];
            let runtime = FakeRuntime::new(vec![old.clone(), old]);
            let checks = RefCell::new(VecDeque::from(checks));
            assert_eq!(
                restart_with(&runtime, Some(2000), || checks
                    .borrow_mut()
                    .pop_front()
                    .expect("unexpected validation")),
                Err(expected.into())
            );
            assert_eq!(*runtime.events.borrow(), events);
        }
    }

    #[test]
    fn restart_never_quits_a_replacement_or_reopens_over_a_new_instance() {
        let old = vec![instance(Some(1000))];
        for replacement in [
            Instance {
                pid: 456,
                ..old[0].clone()
            },
            Instance {
                launched_ms: Some(1500),
                ..old[0].clone()
            },
            Instance {
                bundle: "/synthetic/Other.app".into(),
                ..old[0].clone()
            },
        ] {
            let runtime = FakeRuntime::new(vec![old.clone(), vec![replacement.clone()]]);
            assert_eq!(
                restart_with(&runtime, Some(2000), || true),
                Err(CHANGED.into())
            );
            assert_eq!(*runtime.events.borrow(), ["instances", "instances"]);
            let runtime = FakeRuntime::new(vec![old.clone(), old.clone(), vec![replacement]]);
            assert_eq!(
                restart_with(&runtime, Some(2000), || true),
                Err(CHANGED.into())
            );
            assert_eq!(
                *runtime.events.borrow(),
                ["instances", "instances", "quit", "wait", "instances"]
            );
        }
    }

    #[test]
    fn refused_normal_quit_and_exit_timeout_never_reopen() {
        let old = vec![instance(Some(1000))];
        for refuse_quit in [true, false] {
            let mut runtime = FakeRuntime::new(vec![old.clone(), old.clone()]);
            if refuse_quit {
                runtime.quit_error = Some(RUNNING);
            } else {
                runtime.wait_error = Some(RUNNING);
            }
            assert_eq!(
                restart_with(&runtime, Some(2000), || true),
                Err(RUNNING.into())
            );
            assert!(!runtime.events.borrow().contains(&"open"));
            assert_eq!(runtime.events.borrow().contains(&"wait"), !refuse_quit);
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

    #[test]
    fn non_desktop_apps_have_no_desktop_state_and_cannot_restart() {
        assert_eq!(state("claude", Some(2000)), None);
        assert_eq!(
            restart("claude", Some(2000), || panic!(
                "must not validate an unsupported app"
            )),
            Err("agentConnections.restartUnsupported".into())
        );
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn desktop_restart_is_unsupported_without_validating_configuration() {
        let state = state("claude-desktop", Some(2000));
        assert_eq!(state, Some(State::Unsupported));
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!("unsupported")
        );
        assert_eq!(
            restart("claude-desktop", Some(2000), || {
                panic!("must not validate an unsupported platform")
            }),
            Err("agentConnections.restartUnsupported".into())
        );
    }
}
