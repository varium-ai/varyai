//! Windows lifecycle selection and orchestration are tested without native processes.
use super::{phase_error, State, CHANGED, OPEN_FAILED, QUIT_FAILED, RUNNING};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Process {
    pid: u32,
    parent: u32,
    image: String,
    args: Vec<String>,
    created: u64,
    aumid: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Closed {
    Exited,
    Remains,
}

trait Runtime {
    fn processes(&self) -> Result<Vec<Process>, String>;
    fn open_installed(&self, valid: impl Fn() -> bool) -> Result<(), String>;
    fn close(&self, processes: &[Process]) -> Result<(), String>;
    fn wait_closed(&self, processes: &[Process]) -> Result<Closed, String>;
    fn end_tree(&self, main: &Process) -> Result<(), String>;
    fn wait_gone(&self) -> Result<(), String>;
    fn launch(&self, main: &Process) -> Result<(), String>;
    fn wait_back(&self) -> Result<(), String>;
}

fn normalize(path: &str) -> String {
    path.replace('/', "\\").to_ascii_lowercase()
}

fn installed_images(app: &str, local: &str, programs: &str) -> Vec<String> {
    let mut images = Vec::new();
    if app == "t3code" {
        for root in [
            (!local.is_empty()).then(|| format!(r"{local}\Programs")),
            (!programs.is_empty()).then(|| programs.to_owned()),
        ]
        .into_iter()
        .flatten()
        {
            for (folder, executable) in [
                ("T3 Code", "T3 Code.exe"),
                ("T3 Code (Alpha)", "T3 Code (Alpha).exe"),
                ("T3 Code (Beta)", "T3 Code (Beta).exe"),
                ("t3code", "T3 Code.exe"),
            ] {
                images.push(format!(r"{root}\{folder}\{executable}"));
            }
        }
        return images;
    }
    if !programs.is_empty() {
        let relative: &[&str] = match app {
            "workbuddy" => &[r"WorkBuddy\WorkBuddy.exe"],
            "workbuddy-ai" => &[r"WorkBuddy AI\WorkBuddy.exe", r"WorkBuddy-AI\WorkBuddy.exe"],
            "qoder" => &[r"Qoder CN\Qoder CN.exe"],
            "trae" => &[r"Trae CN\Trae CN.exe"],
            "trae-intl" => &[r"Trae\Trae.exe"],
            "zcode" => &[r"ZCode\ZCode.exe"],
            "deepseek-harness" => &[r"DeepSeek Harness\DeepSeek Harness.exe"],
            _ => &[],
        };
        images.extend(relative.iter().map(|path| format!(r"{programs}\{path}")));
    }
    if !local.is_empty() {
        let relative = match app {
            "qoder" => Some(r"Programs\Qoder CN\Qoder CN.exe"),
            "trae" => Some(r"Programs\Trae CN\Trae CN.exe"),
            "trae-intl" => Some(r"Programs\Trae\Trae.exe"),
            "claude-desktop" => Some(r"AnthropicClaude\claude.exe"),
            "zcode" => Some(r"Programs\ZCode\ZCode.exe"),
            "deepseek-harness" => Some(r"Programs\DeepSeek Harness\DeepSeek Harness.exe"),
            _ => None,
        };
        if let Some(path) = relative {
            images.push(format!(r"{local}\{path}"));
        }
    }
    images
}

fn matches_image(app: &str, image: &str, local: &str, programs: &str) -> bool {
    let image = normalize(image);
    if matches!(app, "t3code" | "zcode" | "deepseek-harness") {
        return installed_images(app, local, programs)
            .iter()
            .any(|path| normalize(path) == image);
    }
    let programs = normalize(programs);
    if app == "workbuddy" {
        return !programs.is_empty() && image == format!("{programs}\\workbuddy\\workbuddy.exe");
    }
    if app == "workbuddy-ai" {
        return !programs.is_empty()
            && (image == format!("{programs}\\workbuddy ai\\workbuddy.exe")
                || image == format!("{programs}\\workbuddy-ai\\workbuddy.exe"));
    }
    if matches!(app, "trae" | "trae-intl") {
        let local = normalize(local);
        let (folder, exe) = if app == "trae" {
            ("trae cn", "trae cn.exe")
        } else {
            ("trae", "trae.exe")
        };
        return (!programs.is_empty() && image == format!("{programs}\\{folder}\\{exe}"))
            || (!local.is_empty() && image == format!("{local}\\programs\\{folder}\\{exe}"));
    }
    if app == "qoder" {
        let local = normalize(local);
        return (!programs.is_empty() && image == format!("{programs}\\qoder cn\\qoder cn.exe"))
            || (!local.is_empty()
                && image == format!("{local}\\programs\\qoder cn\\qoder cn.exe"));
    }
    if app == "codex" {
        return !programs.is_empty()
            && image
                .strip_prefix(&format!("{programs}\\windowsapps\\"))
                .is_some_and(|relative| {
                    let pieces = relative.split('\\').collect::<Vec<_>>();
                    pieces.len() == 3
                        && pieces[0].starts_with("openai.codex_")
                        && pieces[1] == "app"
                        && pieces[2] == "chatgpt.exe"
                });
    }
    if app != "claude-desktop" {
        return false;
    }
    let local = normalize(local);
    if !local.is_empty() {
        let prefix = format!("{local}\\anthropicclaude\\");
        if let Some(relative) = image.strip_prefix(&prefix) {
            let pieces = relative.split('\\').collect::<Vec<_>>();
            if pieces == ["claude.exe"]
                || (pieces.len() == 2 && pieces[0].starts_with("app-") && pieces[1] == "claude.exe")
            {
                return true;
            }
        }
    }
    if programs.is_empty() {
        return false;
    }
    let prefix = format!("{programs}\\windowsapps\\");
    let Some(relative) = image.strip_prefix(&prefix) else {
        return false;
    };
    let pieces = relative.split('\\').collect::<Vec<_>>();
    (pieces.len() == 2 || (pieces.len() == 3 && pieces[1] == "app"))
        && (pieces[0].starts_with("claude_") || pieces[0].starts_with("anthropicpbc.claude_"))
        && pieces.last() == Some(&"claude.exe")
}

fn runs_script(args: &[String]) -> bool {
    if args.iter().skip(1).any(|arg| {
        let arg = arg.to_ascii_lowercase();
        [".js", ".cjs", ".mjs"]
            .iter()
            .any(|suffix| arg.ends_with(suffix))
    }) {
        return true;
    }
    args.iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .is_some_and(|arg| {
            let arg = normalize(arg);
            !arg.starts_with("\\prefetch:")
                && (arg.starts_with('\\')
                    || arg
                        .as_bytes()
                        .get(1..3)
                        .is_some_and(|bytes| bytes == b":\\"))
        })
}

fn main_process(processes: &[Process]) -> Result<&Process, String> {
    if processes.is_empty() || processes.iter().any(|process| process.created == 0) {
        return Err(CHANGED.into());
    }
    let roots = processes
        .iter()
        .filter(|process| !processes.iter().any(|parent| parent.pid == process.parent))
        .collect::<Vec<_>>();
    if roots.len() != 1
        || processes
            .iter()
            .any(|process| normalize(&process.image) != normalize(&roots[0].image))
    {
        return Err(CHANGED.into());
    }
    Ok(roots[0])
}

fn freshness(processes: &[Process], applied_ms: Option<i64>) -> State {
    if processes.is_empty() {
        return State::Stopped;
    }
    let Some(applied_ms) = applied_ms else {
        return State::Unknown;
    };
    // FILETIME's epoch starts 11644473600 seconds before Unix time.
    let launched = |process: &Process| {
        process
            .created
            .checked_sub(116_444_736_000_000_000)
            .map(|ticks| (ticks / 10_000) as i64)
    };
    if processes.iter().any(|process| {
        launched(process).is_some_and(|launched| super::started_before_change(launched, applied_ms))
    }) {
        State::RestartRequired
    } else if processes.iter().any(|process| launched(process).is_none()) {
        State::Unknown
    } else {
        State::Current
    }
}

fn restart_with(
    runtime: &impl Runtime,
    applied_ms: Option<i64>,
    valid: impl Fn() -> bool,
) -> Result<(), String> {
    if !valid() {
        return Err(super::super::CONFLICT.into());
    }
    let processes = runtime.processes()?;
    restart_processes(runtime, processes, applied_ms, valid)
}

fn launch_aumid<'a>(app: &str, observed: Option<&'a str>) -> Option<&'a str> {
    observed.or_else(|| (app == "codex").then_some("OpenAI.Codex_2p2nqsd0c76g0!App"))
}

fn recover_with(runtime: &impl Runtime, valid: impl Fn() -> bool) -> Result<(), String> {
    if !valid() {
        return Err(super::super::CONFLICT.into());
    }
    let processes = runtime.processes()?;
    if processes.is_empty() {
        return Ok(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| CHANGED)?
        .as_millis();
    let applied_ms = i64::try_from(now).map_err(|_| CHANGED)?;
    restart_processes(runtime, processes, Some(applied_ms), valid)
}

fn restart_processes(
    runtime: &impl Runtime,
    processes: Vec<Process>,
    applied_ms: Option<i64>,
    valid: impl Fn() -> bool,
) -> Result<(), String> {
    restart_verified_processes(runtime, processes, applied_ms, valid, false)
}

fn verified_replacement(live: &[Process], previous: &[Process]) -> Result<bool, String> {
    if live.is_empty() {
        return Ok(false);
    }
    Ok(
        normalize(&main_process(live)?.image) == normalize(&main_process(previous)?.image)
            && !live.iter().any(|process| {
                previous
                    .iter()
                    .any(|old| old.pid == process.pid && old.created == process.created)
            }),
    )
}

fn restart_verified_processes(
    runtime: &impl Runtime,
    mut processes: Vec<Process>,
    applied_ms: Option<i64>,
    valid: impl Fn() -> bool,
    manual: bool,
) -> Result<(), String> {
    for attempt in 0..3 {
        if manual && processes.is_empty() {
            return runtime.open_installed(valid);
        }
        let state = freshness(&processes, applied_ms);
        if !manual && state != State::RestartRequired {
            return Err(CHANGED.into());
        }
        main_process(&processes)?;
        if !valid() {
            return Err(CHANGED.into());
        }
        let latest = runtime.processes()?;
        if latest == processes {
            break;
        }
        if !manual || attempt == 2 {
            return Err(CHANGED.into());
        }
        processes = latest;
    }
    let main = main_process(&processes)?;
    runtime
        .close(&processes)
        .map_err(|error| phase_error(error, QUIT_FAILED))?;
    let closed = match runtime.wait_closed(&processes) {
        Ok(closed) => closed,
        Err(error) if manual && error == CHANGED => {
            if !valid() {
                return Err(super::super::CONFLICT.into());
            }
            if verified_replacement(&runtime.processes()?, &processes)? {
                return Ok(());
            }
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    if closed == Closed::Remains {
        if !valid() {
            return Err(super::super::CONFLICT.into());
        }
        let remaining = runtime.processes()?;
        if !remaining.contains(main) || remaining.iter().any(|process| !processes.contains(process))
        {
            return Err(CHANGED.into());
        }
        runtime.end_tree(main)?;
        runtime.wait_gone()?;
    }
    if !valid() {
        return Err(super::super::CONFLICT.into());
    }
    let live = runtime.processes()?;
    if !live.is_empty() {
        if manual && verified_replacement(&live, &processes)? {
            return Ok(());
        }
        return Err(CHANGED.into());
    }
    runtime
        .launch(main)
        .map_err(|error| phase_error(error, OPEN_FAILED))?;
    runtime
        .wait_back()
        .map_err(|error| phase_error(error, OPEN_FAILED))
}

fn complete_tree<T>(
    entries: &[T],
    mut end: impl FnMut(&T) -> Result<(), String>,
) -> Result<(), String> {
    let mut failure = None;
    for entry in entries {
        if let Err(error) = end(entry) {
            failure.get_or_insert(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

#[cfg(target_os = "windows")]
#[path = "windows/native.rs"]
mod native;
#[cfg(target_os = "windows")]
pub(super) fn recover_codex(valid: impl Fn() -> bool) -> Result<(), String> {
    recover_with(&native::Native { app: "codex" }, valid)
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn supported(app: &str) -> bool {
    // Only apps with verified Windows process identity can restart here.
    // Other desktop agents keep macOS support via bundle identifiers.
    matches!(
        app,
        "claude-desktop"
            | "codex"
            | "workbuddy"
            | "workbuddy-ai"
            | "qoder"
            | "trae"
            | "trae-intl"
            | "t3code"
            | "zcode"
            | "deepseek-harness"
    )
}
#[cfg(target_os = "windows")]
pub(super) fn running_executable(app: &str) -> Result<Option<std::path::PathBuf>, String> {
    let processes = native::Native { app }.processes()?;
    if processes.is_empty() {
        return Ok(None);
    }
    Ok(Some(std::path::PathBuf::from(
        main_process(&processes)?.image.clone(),
    )))
}
#[cfg(target_os = "windows")]
pub(super) fn state(app: &str, applied_ms: Option<i64>) -> State {
    if !supported(app) {
        return State::Unsupported;
    }
    native::Native { app }
        .processes()
        .map_or(State::Unknown, |processes| {
            freshness(&processes, applied_ms)
        })
}

#[cfg(target_os = "windows")]
pub(super) fn launched_since(app: &str, since_ms: i64) -> Option<bool> {
    if !supported(app) {
        return None;
    }
    let processes = native::Native { app }.processes().ok()?;
    super::launched_since_times(
        processes.iter().map(|process| {
            process
                .created
                .checked_sub(116_444_736_000_000_000)
                .map(|ticks| (ticks / 10_000) as i64)
        }),
        since_ms,
    )
}
#[cfg(target_os = "windows")]
pub(super) fn restart_manual(app: &str) -> Result<(), String> {
    if !supported(app) {
        return Err("agentConnections.restartUnsupported".into());
    }
    restart_manual_with(&native::Native { app })
}

fn restart_manual_with(runtime: &impl Runtime) -> Result<(), String> {
    let processes = runtime.processes()?;
    restart_verified_processes(runtime, processes, None, || true, true)
}

#[cfg(target_os = "windows")]
pub(super) fn restart(
    app: &str,
    applied_ms: Option<i64>,
    valid: impl Fn() -> bool,
) -> Result<(), String> {
    if !supported(app) {
        return Err("agentConnections.restartUnsupported".into());
    }
    if super::super::model_control::authorized(app) && state(app, applied_ms) == State::Stopped {
        return native::open_model_control(app, valid);
    }
    let runtime = native::Native { app };
    if app == "codex" {
        let processes = runtime.processes()?;
        restart_verified_processes(&runtime, processes, applied_ms, valid, true)
    } else {
        restart_with(&runtime, applied_ms, valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_launch_clock_precision_matches_macos() {
        for started in [4000, 4001, 4999, 5000, 5001] {
            let mut copy = process();
            copy.created = 116_444_736_000_000_000 + started * 10_000;
            assert_eq!(freshness(&[copy], Some(5000)), State::Current);
        }
        let mut copy = process();
        copy.created = 116_444_736_000_000_000 + 3999 * 10_000;
        assert_eq!(freshness(&[copy], Some(5000)), State::RestartRequired);
    }

    #[test]
    fn confirmed_chatgpt_restart_reopens_a_current_windows_app() {
        let processes = vec![process()];
        let runtime = Fake::new(vec![processes.clone(), vec![]], Closed::Exited);
        assert_eq!(
            restart_verified_processes(&runtime, processes, Some(0), || true, true),
            Ok(())
        );
        assert!(runtime.events.borrow().contains(&"launch"));
    }

    #[test]
    fn confirmed_restart_accepts_a_current_verified_windows_desktop() {
        let processes = vec![process()];
        let runtime = Fake::new(vec![processes.clone(), processes, vec![]], Closed::Exited);
        assert_eq!(restart_manual_with(&runtime), Ok(()));
        assert!(runtime.events.borrow().contains(&"launch"));
        assert!(runtime.events.borrow().contains(&"wait_back"));
    }

    #[test]
    fn manual_restart_opens_a_stopped_windows_desktop() {
        let runtime = Fake::new(vec![vec![]], Closed::Exited);
        assert_eq!(restart_manual_with(&runtime), Ok(()));
        assert_eq!(*runtime.events.borrow(), ["open_installed"]);
    }

    #[test]
    fn manual_restart_resamples_windows_processes_before_close() {
        let old = vec![process()];
        let replacement = vec![Process {
            pid: 456,
            ..process()
        }];
        let runtime = Fake::new(
            vec![old, replacement.clone(), replacement, vec![]],
            Closed::Exited,
        );
        assert_eq!(restart_manual_with(&runtime), Ok(()));
        assert_eq!(
            *runtime.events.borrow(),
            ["close", "wait_closed", "launch", "wait_back"]
        );
    }

    #[test]
    fn manual_restart_accepts_a_verified_windows_relaunch_during_close() {
        let old = vec![process()];
        let replacement = vec![Process {
            pid: 456,
            created: process().created + 10000,
            ..process()
        }];
        let runtime = Fake::new(vec![old.clone(), old, replacement], Closed::Exited);
        assert_eq!(restart_manual_with(&runtime), Ok(()));
        assert_eq!(*runtime.events.borrow(), ["close", "wait_closed"]);
    }

    #[test]
    fn manual_restart_handles_native_windows_replacement_discovery() {
        let old = vec![process()];
        let replacement = vec![Process {
            pid: 456,
            created: process().created + 10000,
            ..process()
        }];
        let mut runtime = Fake::new(vec![old.clone(), old, replacement], Closed::Exited);
        runtime.wait_changed = true;
        assert_eq!(restart_manual_with(&runtime), Ok(()));
        assert_eq!(*runtime.events.borrow(), ["close", "wait_closed"]);
    }

    #[test]
    fn manual_restart_never_accepts_a_live_original_windows_process_as_replacement() {
        let old = vec![process()];
        let altered = vec![Process {
            args: vec!["changed".into()],
            ..process()
        }];
        let mut runtime = Fake::new(vec![old.clone(), old, altered], Closed::Exited);
        runtime.wait_changed = true;
        assert_eq!(restart_manual_with(&runtime), Err(CHANGED.into()));
        assert_eq!(*runtime.events.borrow(), ["close", "wait_closed"]);
    }

    #[test]
    fn manual_restart_bounds_windows_discovery_races_without_closing() {
        let snapshots = (1..=4)
            .map(|pid| vec![Process { pid, ..process() }])
            .collect();
        let runtime = Fake::new(snapshots, Closed::Exited);
        assert_eq!(restart_manual_with(&runtime), Err(CHANGED.into()));
        assert!(runtime.events.borrow().is_empty());
    }

    #[test]
    fn manual_restart_rejects_unidentified_or_other_windows_replacements() {
        for replacement in [
            Process {
                created: 0,
                ..process()
            },
            Process {
                image: r"C:\Program Files\Other\Other.exe".into(),
                ..process()
            },
        ] {
            let old = vec![process()];
            let runtime = Fake::new(vec![old.clone(), old, vec![replacement]], Closed::Exited);
            assert_eq!(restart_manual_with(&runtime), Err(CHANGED.into()));
            assert_eq!(*runtime.events.borrow(), ["close", "wait_closed"]);
        }
    }

    #[test]
    fn stopped_windows_restart_reports_an_unsuccessful_launch() {
        let mut runtime = Fake::new(vec![vec![]], Closed::Exited);
        runtime.fail = Some("open_installed");
        assert_eq!(restart_manual_with(&runtime), Err(RUNNING.into()));
        assert_eq!(*runtime.events.borrow(), ["open_installed"]);
    }

    #[test]
    fn stopped_windows_install_candidates_retain_agent_identity() {
        let local = r"C:\Users\synthetic\AppData\Local";
        let programs = r"C:\Program Files";
        for app in [
            "workbuddy",
            "workbuddy-ai",
            "qoder",
            "trae",
            "trae-intl",
            "claude-desktop",
            "t3code",
            "zcode",
        ] {
            let images = installed_images(app, local, programs);
            assert!(!images.is_empty(), "{app}");
            assert!(
                images
                    .iter()
                    .all(|image| matches_image(app, image, local, programs)),
                "{app}"
            );
            assert!(installed_images(app, "", "").is_empty());
        }
        assert!(installed_images("unknown", local, programs).is_empty());
    }

    #[test]
    fn zcode_windows_identity_uses_known_install_paths() {
        let local = r"C:\Users\synthetic\AppData\Local";
        let programs = r"C:\Program Files";
        for image in [
            r"C:\Program Files\ZCode\ZCode.exe",
            r"C:\Users\synthetic\AppData\Local\Programs\ZCode\ZCode.exe",
        ] {
            assert!(matches_image("zcode", image, local, programs));
        }
        for image in [
            r"C:\Downloads\ZCode.exe",
            r"C:\Program Files\ZCode\other.exe",
            r"C:\Program Files\Other\ZCode.exe",
        ] {
            assert!(!matches_image("zcode", image, local, programs));
        }
    }

    #[test]
    fn deepseek_harness_restart_requires_a_verified_stale_desktop() {
        let local = r"C:\Users\synthetic\AppData\Local";
        let programs = r"C:\Program Files";
        assert!(supported("deepseek-harness"));
        let images = installed_images("deepseek-harness", local, programs);
        assert_eq!(images.len(), 2);
        for image in images {
            assert!(matches_image("deepseek-harness", &image, local, programs));
            let mut running = process();
            running.image = image;
            assert_eq!(freshness(&[running], Some(2000)), State::RestartRequired);
        }
        assert!(!matches_image(
            "deepseek-harness",
            r"C:\Downloads\DeepSeek Harness.exe",
            local,
            programs,
        ));
        assert!(!matches_image(
            "deepseek-harness",
            r"C:\Program Files\DeepSeek Harness\dsh.exe",
            local,
            programs,
        ));
        assert_eq!(freshness(&[], Some(2000)), State::Stopped);
    }

    #[test]
    fn t3_windows_identity_rejects_foreign_paths_and_codex_processes() {
        let local = r"C:\Users\synthetic\AppData\Local";
        let programs = r"C:\Program Files";
        for image in [
            r"C:\Downloads\T3 Code.exe",
            r"C:\Program Files\Other\T3 Code.exe",
            r"C:\Program Files\T3 Code\codex.exe",
            r"C:\Program Files\T3 Code (Beta)\T3 Code.exe",
        ] {
            assert!(!matches_image("t3code", image, local, programs));
        }
    }

    #[test]
    fn windows_restart_reports_the_failed_phase() {
        for (phase, expected) in [
            ("close", "agentConnections.restartQuitFailed"),
            ("launch", "agentConnections.restartOpenFailed"),
            ("wait_back", "agentConnections.restartOpenFailed"),
        ] {
            let processes = vec![process()];
            let mut runtime = Fake::new(vec![processes.clone(), vec![]], Closed::Exited);
            runtime.fail = Some(phase);
            assert_eq!(
                restart_verified_processes(&runtime, processes, Some(0), || true, true),
                Err(expected.into())
            );
        }
    }

    #[test]
    fn windows_restart_requires_a_replacement_after_a_successful_launch() {
        let processes = vec![process()];
        let mut runtime = Fake::new(vec![processes.clone(), vec![]], Closed::Exited);
        runtime.fail = Some("wait_back");
        assert_eq!(
            restart_verified_processes(&runtime, processes, Some(0), || true, true),
            Err(OPEN_FAILED.into())
        );
        assert_eq!(
            *runtime.events.borrow(),
            ["close", "wait_closed", "launch", "wait_back"]
        );
    }
    use std::{cell::RefCell, collections::VecDeque};
    #[test]
    fn qoder_windows_identity_rejects_other_install_roots_and_other_products() {
        assert!(matches_image(
            "qoder",
            r"C:\Program Files\Qoder CN\Qoder CN.exe",
            r"C:\Users\synthetic\AppData\Local",
            r"C:\Program Files"
        ));
        assert!(matches_image(
            "qoder",
            r"C:\Users\synthetic\AppData\Local\Programs\Qoder CN\Qoder CN.exe",
            r"C:\Users\synthetic\AppData\Local",
            r"C:\Program Files"
        ));
        assert!(!matches_image(
            "qoder",
            r"C:\Downloads\Qoder CN.exe",
            r"C:\Users\synthetic\AppData\Local",
            r"C:\Program Files"
        ));
        assert!(!matches_image(
            "qoder",
            r"C:\Program Files\Qoder\Qoder.exe",
            r"C:\Users\synthetic\AppData\Local",
            r"C:\Program Files"
        ));
    }
    fn process() -> Process {
        Process {
            pid: 10,
            parent: 4,
            image: r"C:\Program Files\WorkBuddy\WorkBuddy.exe".into(),
            args: vec!["WorkBuddy.exe".into()],
            created: 116_444_736_005_000_000,
            aumid: None,
        }
    }
    struct Fake {
        snapshots: RefCell<VecDeque<Vec<Process>>>,
        events: RefCell<Vec<&'static str>>,
        closed: Closed,
        fail: Option<&'static str>,
        wait_changed: bool,
    }
    impl Fake {
        fn new(snapshots: Vec<Vec<Process>>, closed: Closed) -> Self {
            Self {
                snapshots: RefCell::new(snapshots.into()),
                events: RefCell::new(vec![]),
                closed,
                fail: None,
                wait_changed: false,
            }
        }
        fn event(&self, name: &'static str) -> Result<(), String> {
            self.events.borrow_mut().push(name);
            if self.fail == Some(name) {
                Err(RUNNING.into())
            } else {
                Ok(())
            }
        }
    }
    impl Runtime for Fake {
        fn processes(&self) -> Result<Vec<Process>, String> {
            Ok(self
                .snapshots
                .borrow_mut()
                .pop_front()
                .expect("unexpected discovery"))
        }
        fn open_installed(&self, valid: impl Fn() -> bool) -> Result<(), String> {
            assert!(valid());
            self.event("open_installed")
        }
        fn close(&self, _: &[Process]) -> Result<(), String> {
            self.event("close")
        }
        fn wait_closed(&self, _: &[Process]) -> Result<Closed, String> {
            self.event("wait_closed")?;
            if self.wait_changed {
                return Err(CHANGED.into());
            }
            Ok(self.closed)
        }
        fn end_tree(&self, _: &Process) -> Result<(), String> {
            self.event("end")
        }
        fn wait_gone(&self) -> Result<(), String> {
            self.event("wait_gone")
        }
        fn launch(&self, _: &Process) -> Result<(), String> {
            self.event("launch")
        }
        fn wait_back(&self) -> Result<(), String> {
            self.event("wait_back")
        }
    }
    #[test]
    fn windows_selection_accepts_only_verified_install_paths_and_excludes_scripts() {
        let local = r"C:\Users\synthetic\AppData\Local";
        let programs = r"C:\Program Files";
        for image in [
            r"C:\Program Files\WorkBuddy\WorkBuddy.exe",
            r"c:/program files/workbuddy/WORKBUDDY.EXE",
        ] {
            assert!(matches_image("workbuddy", image, local, programs));
        }
        for image in [
            r"C:\Program Files\WorkBuddy AI\WorkBuddy.exe",
            r"C:\Program Files\WorkBuddy-AI\WorkBuddy.exe",
            r"C:\Temp\WorkBuddy.exe",
        ] {
            assert!(!matches_image("workbuddy", image, local, programs));
        }
        for image in [
            format!(r"{local}\AnthropicClaude\claude.exe"),
            format!(r"{local}\AnthropicClaude\app-2.0\claude.exe"),
            format!(r"{programs}\WindowsApps\Claude_2.0_x64\claude.exe"),
            format!(r"{programs}\WindowsApps\Claude_2.0_x64\app\claude.exe"),
            format!(r"{programs}\WindowsApps\AnthropicPBC.Claude_2.0_x64\claude.exe"),
        ] {
            assert!(matches_image("claude-desktop", &image, local, programs));
        }
        assert!(!matches_image(
            "claude-desktop",
            r"C:\Users\synthetic\.local\bin\claude.exe",
            local,
            programs
        ));
        for args in [
            vec![
                "WorkBuddy.exe",
                "--disable-warning=ExperimentalWarning",
                r"C:\resources\app.asar\main\daemon-app-server-entry.js",
                "--stdio",
            ],
            vec![
                "WorkBuddy.exe",
                r"C:\resources\cli\bin\codebuddy",
                "--prewarm",
            ],
        ] {
            assert!(runs_script(
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
            ));
        }
        for args in [
            vec!["WorkBuddy.exe", "--type=renderer", "/prefetch:1"],
            vec!["WorkBuddy.exe", "workbuddy://open"],
        ] {
            assert!(!runs_script(
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
            ));
        }
    }
    #[test]
    fn codex_recovery_is_internal_and_never_launches_a_closed_app() {
        let runtime = Fake::new(vec![vec![]], Closed::Remains);
        assert_eq!(recover_with(&runtime, || true), Ok(()));
        assert!(runtime.events.borrow().is_empty());
        let runtime = Fake::new(vec![], Closed::Remains);
        assert_eq!(
            recover_with(&runtime, || false),
            Err(super::super::super::CONFLICT.into())
        );
        let codex = Process {
            image: r"C:\Program Files\WindowsApps\OpenAI.Codex_1.0_x64\app\ChatGPT.exe".into(),
            ..process()
        };
        let old = vec![codex];
        let runtime = Fake::new(vec![old.clone(), old.clone(), old, vec![]], Closed::Remains);
        assert_eq!(recover_with(&runtime, || true), Ok(()));
        assert_eq!(
            *runtime.events.borrow(),
            [
                "close",
                "wait_closed",
                "end",
                "wait_gone",
                "launch",
                "wait_back"
            ]
        );
        assert_eq!(
            launch_aumid("codex", None),
            Some("OpenAI.Codex_2p2nqsd0c76g0!App")
        );
        assert_eq!(
            launch_aumid("codex", Some("observed!App")),
            Some("observed!App")
        );
        assert_eq!(launch_aumid("workbuddy", None), None);
        assert!(matches_image(
            "codex",
            r"C:\Program Files\WindowsApps\OpenAI.Codex_1.0_x64\app\ChatGPT.exe",
            "",
            r"C:\Program Files"
        ));
        for image in [
            r"C:\Program Files\ChatGPT\ChatGPT.exe",
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT_1.0_x64\app\ChatGPT.exe",
        ] {
            assert!(!matches_image("codex", image, "", r"C:\Program Files"));
        }
    }

    #[test]
    fn windows_tree_cleanup_attempts_every_retained_child_after_an_error() {
        let mut attempted = Vec::new();
        assert_eq!(
            complete_tree(&[10, 11, 12], |pid| {
                attempted.push(*pid);
                if *pid == 11 {
                    Err(RUNNING.into())
                } else {
                    Ok(())
                }
            }),
            Err(RUNNING.into())
        );
        assert_eq!(attempted, [10, 11, 12]);
    }

    #[test]
    fn windows_normal_close_and_confirmed_tray_tree_exit_reopen_once() {
        let old = vec![process()];
        let runtime = Fake::new(vec![old.clone(), old.clone(), vec![]], Closed::Exited);
        assert_eq!(restart_with(&runtime, Some(2000), || true), Ok(()));
        assert_eq!(
            *runtime.events.borrow(),
            ["close", "wait_closed", "launch", "wait_back"]
        );
        let runtime = Fake::new(vec![old.clone(), old.clone(), old, vec![]], Closed::Remains);
        assert_eq!(restart_with(&runtime, Some(2000), || true), Ok(()));
        assert_eq!(
            *runtime.events.borrow(),
            [
                "close",
                "wait_closed",
                "end",
                "wait_gone",
                "launch",
                "wait_back"
            ]
        );
    }
    #[test]
    fn windows_refuses_replacement_and_configuration_conflicts_before_force_end() {
        let old = vec![process()];
        let replacement = vec![Process {
            created: process().created + 1,
            ..process()
        }];
        let runtime = Fake::new(vec![old.clone(), replacement.clone()], Closed::Remains);
        assert_eq!(
            restart_with(&runtime, Some(2000), || true),
            Err(CHANGED.into())
        );
        assert!(runtime.events.borrow().is_empty());
        let runtime = Fake::new(vec![old.clone(), old.clone(), replacement], Closed::Remains);
        assert_eq!(
            restart_with(&runtime, Some(2000), || true),
            Err(CHANGED.into())
        );
        assert_eq!(*runtime.events.borrow(), ["close", "wait_closed"]);
        let runtime = Fake::new(vec![old.clone(), old], Closed::Remains);
        let checks = RefCell::new(VecDeque::from([true, true, false]));
        assert_eq!(
            restart_with(&runtime, Some(2000), || checks
                .borrow_mut()
                .pop_front()
                .unwrap()),
            Err(super::super::super::CONFLICT.into())
        );
        assert_eq!(*runtime.events.borrow(), ["close", "wait_closed"]);
    }
    #[test]
    fn windows_refusal_timeout_and_new_instance_never_reopen() {
        let old = vec![process()];
        for fail in ["close", "wait_closed", "end", "wait_gone"] {
            let mut runtime =
                Fake::new(vec![old.clone(), old.clone(), old.clone()], Closed::Remains);
            runtime.fail = Some(fail);
            assert_eq!(
                restart_with(&runtime, Some(2000), || true),
                Err(if fail == "close" {
                    QUIT_FAILED
                } else {
                    RUNNING
                }
                .into())
            );
            assert!(!runtime.events.borrow().contains(&"launch"));
        }
        let runtime = Fake::new(vec![old.clone(), old.clone(), old], Closed::Exited);
        assert_eq!(
            restart_with(&runtime, Some(2000), || true),
            Err(CHANGED.into())
        );
        assert!(!runtime.events.borrow().contains(&"launch"));
        let ambiguous = vec![
            process(),
            Process {
                pid: 20,
                ..process()
            },
        ];
        let runtime = Fake::new(vec![ambiguous], Closed::Remains);
        assert_eq!(
            restart_with(&runtime, Some(2000), || true),
            Err(CHANGED.into())
        );
        assert!(runtime.events.borrow().is_empty());
    }
}
