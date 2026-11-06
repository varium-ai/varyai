//! Windows desktop process rules are tested without inspecting live processes.
use super::Instance;

#[cfg(windows)]
#[path = "windows/native.rs"]
mod native;
#[cfg(windows)]
pub(super) use native::{instances, terminate};

struct Process {
    identity: Instance,
    parent: i32,
}

fn normalize(path: &str) -> String {
    path.replace('/', "\\")
        .trim_start_matches(r"\\?\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn package(path: &str) -> Option<&str> {
    let offset = path.find(r"\windowsapps\openai.codex_")? + r"\windowsapps\".len();
    let end = path[offset..].find('\\')? + offset;
    Some(&path[..end])
}

fn desktop_server(exe: &str, parent: &str, args: &[String]) -> bool {
    let exe = normalize(exe);
    let parent = normalize(parent);
    let Some((directory, name)) = parent.rsplit_once('\\') else {
        return false;
    };
    if !matches!(name, "chatgpt.exe" | "codex.exe") {
        return false;
    }
    let store = package(&parent);
    let classic = [
        r"\openai\codex\",
        r"\programs\codex\",
        r"\programs\chatgpt\",
        r"\program files\codex\",
        r"\program files\chatgpt\",
    ]
    .iter()
    .any(|part| parent.contains(part));
    if store.is_none() && !classic {
        return false;
    }
    let cached = exe
        .rsplit_once(r"\appdata\local\openai\codex\bin\")
        .is_some_and(|(_, suffix)| {
            let parts = suffix.split('\\').collect::<Vec<_>>();
            parts.len() == 2 && !matches!(parts[0], "" | "." | "..") && parts[1] == "codex.exe"
        });
    let bundled = store.is_some_and(|root| package(&exe) == Some(root))
        || (classic
            && exe.starts_with(&format!("{directory}\\"))
            && [r"\resources\codex.exe", r"\resources\codex-cli\codex.exe"]
                .iter()
                .any(|part| exe.ends_with(part)));
    let stdio = args
        .iter()
        .position(|arg| arg == "app-server")
        .is_some_and(|i| args.get(i + 1).is_none_or(|arg| arg.starts_with('-')))
        && !args.iter().any(|arg| {
            arg == "--listen"
                || arg.starts_with("--listen=")
                || arg.starts_with("--managed-daemon")
                || arg == "daemon"
        });
    (cached || bundled) && exe.ends_with(r"\codex.exe") && stdio
}

fn selected_home(environment: &[String]) -> Option<String> {
    let get = |key: &str| {
        environment.iter().find_map(|entry| {
            let (name, value) = entry.split_once('=')?;
            (name.eq_ignore_ascii_case(key) && !value.is_empty()).then_some(value)
        })
    };
    if let Some(home) = get("CODEX_HOME") {
        return Some(home.into());
    }
    let profile = get("USERPROFILE")
        .map(str::to_owned)
        .or_else(|| Some(format!("{}{}", get("HOMEDRIVE")?, get("HOMEPATH")?)))?;
    Some(format!("{}\\.codex", profile.trim_end_matches(['\\', '/'])))
}

fn unix_micros(filetime: u64) -> Option<u64> {
    filetime
        .checked_sub(116_444_736_000_000_000)
        .map(|ticks| ticks / 10)
}

fn descendants(processes: &[Process], root: &Instance) -> Vec<Instance> {
    let mut found = Vec::new();
    let mut parents = vec![root.clone()];
    let mut seen = std::collections::HashSet::from([root.pid]);
    while let Some(parent) = parents.pop() {
        for process in processes {
            if process.parent == parent.pid
                && process.identity.started_us >= parent.started_us
                && seen.insert(process.identity.pid)
            {
                found.push(process.identity.clone());
                parents.push(process.identity.clone());
            }
        }
    }
    found
}

fn finish_descendants(
    before: Vec<Instance>,
    after: Result<Vec<Instance>, ()>,
    mut terminate: impl FnMut(&Instance) -> Result<(), ()>,
) -> Result<(), ()> {
    let mut descendants = before;
    let mut result = Ok(());
    match after {
        Ok(after) => {
            for child in after {
                if !descendants.contains(&child) {
                    descendants.push(child);
                }
            }
        }
        Err(()) => result = Err(()),
    }
    for child in descendants.into_iter().rev() {
        if terminate(&child).is_err() {
            result = Err(());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const CACHE: &str =
        r"C:\Users\fixture\AppData\Local\OpenAI\Codex\bin\synthetic-version\codex.exe";
    const STORE: &str = r"C:\Program Files\WindowsApps\OpenAI.Codex_synthetic\app\ChatGPT.exe";

    fn args() -> Vec<String> {
        vec![
            CACHE.into(),
            "-c".into(),
            "features.code_mode_host=true".into(),
            "app-server".into(),
            "--analytics-default-enabled".into(),
        ]
    }

    #[test]
    fn windows_cached_store_and_classic_desktop_servers_are_recognized() {
        assert!(desktop_server(CACHE, STORE, &args()));
        assert!(desktop_server(
            &CACHE.to_uppercase(),
            &STORE.to_uppercase(),
            &args()
        ));
        assert!(desktop_server(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_synthetic\app\resources\codex.exe",
            STORE,
            &args()
        ));
        assert!(desktop_server(
            r"C:\Users\fixture\AppData\Local\Programs\Codex\resources\codex.exe",
            r"C:\Users\fixture\AppData\Local\Programs\Codex\Codex.exe",
            &args()
        ));
    }

    #[test]
    fn windows_editors_daemons_explicit_listeners_and_other_installations_are_excluded() {
        for extra in [
            "--listen",
            "--listen=stdio://",
            "--managed-daemon",
            "daemon",
        ] {
            let mut args = args();
            args.push(extra.into());
            assert!(!desktop_server(CACHE, STORE, &args));
        }
        assert!(!desktop_server(
            CACHE,
            r"C:\Program Files\Editor\Editor.exe",
            &args()
        ));
        assert!(!desktop_server(
            r"C:\Users\fixture\AppData\Roaming\npm\codex.exe",
            STORE,
            &args()
        ));
        assert!(!desktop_server(
            r"C:\Program Files\WindowsApps\Other.Package\codex.exe",
            STORE,
            &args()
        ));
        assert!(!desktop_server(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_other\app\resources\codex.exe",
            STORE,
            &args()
        ));
    }

    #[test]
    fn windows_chatgpt_chat_package_is_not_the_codex_desktop_app() {
        assert!(!desktop_server(
            CACHE,
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT_synthetic\app\ChatGPT.exe",
            &args(),
        ));
    }

    #[test]
    fn windows_home_uses_the_native_profile_and_respects_custom_codex_home() {
        let env = vec![
            r"HOME=C:\synthetic\git-home".into(),
            r"USERPROFILE=C:\Users\fixture".into(),
        ];
        assert_eq!(
            selected_home(&env).as_deref(),
            Some(r"C:\Users\fixture\.codex")
        );
        let mut custom = env;
        custom.push(r"CODEX_HOME=D:\synthetic\codex".into());
        assert_eq!(
            selected_home(&custom).as_deref(),
            Some(r"D:\synthetic\codex")
        );
        assert!(selected_home(&[]).is_none());
    }

    #[test]
    fn windows_filetime_is_converted_to_the_shared_activity_clock() {
        assert_eq!(
            unix_micros(116_444_736_000_000_000 + 123_456_780),
            Some(12_345_678)
        );
        assert!(unix_micros(0).is_none());
    }

    #[test]
    fn windows_cleanup_attempts_known_children_even_after_listing_or_sibling_failure() {
        let child = |pid| Instance {
            pid,
            started_us: 100,
        };
        let mut attempted = Vec::new();
        assert!(
            finish_descendants(vec![child(11), child(12)], Err(()), |target| {
                attempted.push(target.pid);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(attempted, vec![12, 11]);
        attempted.clear();
        assert!(
            finish_descendants(vec![child(11), child(12)], Ok(vec![child(13)]), |target| {
                attempted.push(target.pid);
                if target.pid == 13 {
                    Err(())
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        assert_eq!(attempted, vec![13, 12, 11]);
    }

    #[test]
    fn windows_restart_tree_excludes_reused_parent_ids_and_other_apps() {
        let root = Instance {
            pid: 10,
            started_us: 100,
        };
        let item = |pid, parent, started_us| Process {
            identity: Instance { pid, started_us },
            parent,
        };
        let processes = vec![
            item(10, 1, 100),
            item(11, 10, 110),
            item(12, 11, 120),
            item(13, 10, 90),
            item(14, 13, 140),
            item(15, 99, 150),
        ];
        let actual = descendants(&processes, &root);
        assert_eq!(
            actual,
            vec![
                Instance {
                    pid: 11,
                    started_us: 110
                },
                Instance {
                    pid: 12,
                    started_us: 120
                }
            ]
        );
    }
}
