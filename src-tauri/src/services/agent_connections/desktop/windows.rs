//! Windows lifecycle selection and orchestration are tested without native processes.
use super::{State, CHANGED, RUNNING};

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

fn matches_image(app: &str, image: &str, local: &str, programs: &str) -> bool {
    let image = normalize(image);
    let programs = normalize(programs);
    if app == "workbuddy" {
        return !programs.is_empty() && image == format!("{programs}\\workbuddy\\workbuddy.exe");
    }
    if app == "workbuddy-ai" {
        return !programs.is_empty()
            && (image == format!("{programs}\\workbuddy ai\\workbuddy.exe")
                || image == format!("{programs}\\workbuddy-ai\\workbuddy.exe"));
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
    if processes
        .iter()
        .any(|process| launched(process).is_some_and(|launched| launched <= applied_ms))
    {
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
    if freshness(&processes, applied_ms) != State::RestartRequired {
        return Err(CHANGED.into());
    }
    let main = main_process(&processes)?;
    if !valid() || runtime.processes()? != processes {
        return Err(CHANGED.into());
    }
    runtime.close(&processes)?;
    if runtime.wait_closed(&processes)? == Closed::Remains {
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
    if !runtime.processes()?.is_empty() {
        return Err(CHANGED.into());
    }
    runtime.launch(main)?;
    runtime.wait_back()
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

#[cfg(target_os = "windows")]
pub(super) fn supported(app: &str) -> bool {
    // Only apps with verified Windows process identity can restart here.
    // Other desktop agents keep macOS support via bundle identifiers.
    match app {
        "claude-desktop" | "codex" | "workbuddy-ai" | "qoder" => true,
        "workbuddy" => crate::workbuddy_config::get_workbuddy_dir()
            .file_name()
            .is_some_and(|name| name == ".workbuddy"),
        _ => false,
    }
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
    restart_with(&native::Native { app }, applied_ms, valid)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            created: 116_444_736_010_000_000,
            aumid: None,
        }
    }
    struct Fake {
        snapshots: RefCell<VecDeque<Vec<Process>>>,
        events: RefCell<Vec<&'static str>>,
        closed: Closed,
        fail: Option<&'static str>,
    }
    impl Fake {
        fn new(snapshots: Vec<Vec<Process>>, closed: Closed) -> Self {
            Self {
                snapshots: RefCell::new(snapshots.into()),
                events: RefCell::new(vec![]),
                closed,
                fail: None,
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
        fn close(&self, _: &[Process]) -> Result<(), String> {
            self.event("close")
        }
        fn wait_closed(&self, _: &[Process]) -> Result<Closed, String> {
            self.event("wait_closed")?;
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
                Err(RUNNING.into())
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
