//! Shared desktop presence and version discovery. Never launch an application.
use super::*;

fn ids(app: &str) -> &'static [&'static str] {
    match app {
        "workbuddy" => &["com.tencent.workbuddy.mac", "com.workbuddy.workbuddy-ai"],
        "claude-desktop" => &["com.anthropic.claudefordesktop"],
        "codex" => &["com.openai.codex"],
        "trae" => &["cn.trae.app"],
        "qoder" => &["com.qodercn.app"],
        "doubaowork" => &["com.work.pc.doubao"],
        "deepseek-harness" => &["com.deepseek.dsh"],
        _ => &[],
    }
}

pub(super) fn find_in(app: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if agent_desktop_bundles(app).is_empty() {
        return None;
    }
    for root in roots {
        for name in agent_desktop_bundles(app) {
            let path = root.join(name);
            if matches(app, &path, true) {
                return Some(path);
            }
        }
        let mut folders = vec![root.clone()];
        if let Ok(entries) = std::fs::read_dir(root) {
            let mut nested = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_dir()
                        && !path.is_symlink()
                        && path.extension().is_none_or(|ext| ext != "app")
                })
                .collect::<Vec<_>>();
            nested.sort();
            folders.extend(nested);
        }
        for folder in folders {
            let Ok(entries) = std::fs::read_dir(folder) else {
                continue;
            };
            let mut bundles = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
                .collect::<Vec<_>>();
            bundles.sort();
            for path in bundles {
                let named = path.file_name().is_some_and(|name| {
                    agent_desktop_bundles(app)
                        .iter()
                        .any(|known| name == *known)
                });
                if matches(app, &path, named) {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn matches(app: &str, path: &Path, named: bool) -> bool {
    let Ok(resolved) = path.canonicalize() else {
        return false;
    };
    if !resolved.is_dir()
        || resolved.components().any(|component| {
            component.as_os_str() == ".Trash" || component.as_os_str() == ".Trashes"
        })
    {
        return false;
    }
    match field(path, "CFBundleIdentifier") {
        Some(id) if !ids(app).is_empty() => ids(app).contains(&id.as_str()),
        // ChatGPT has historically named two different OpenAI applications.
        _ if app == "codex" && path.file_name().is_some_and(|name| name == "ChatGPT.app") => false,
        _ => named,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn field(bundle: &Path, key: &str) -> Option<String> {
    use objc2_foundation::{
        NSData, NSDictionary, NSPropertyListMutabilityOptions, NSPropertyListSerialization,
        NSString,
    };
    let bytes = std::fs::read(bundle.join("Contents/Info.plist")).ok()?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let data = NSData::from_vec(bytes);
    // A null format pointer is allowed; Foundation owns the returned immutable tree.
    let value = unsafe {
        NSPropertyListSerialization::propertyListWithData_options_format_error(
            &data,
            NSPropertyListMutabilityOptions::empty(),
            std::ptr::null_mut(),
        )
    }
    .ok()?;
    let dict = value.downcast_ref::<NSDictionary>()?;
    let value = dict.objectForKey(&NSString::from_str(key))?;
    Some(value.downcast_ref::<NSString>()?.to_string())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn field(bundle: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(bundle.join("Contents/Info.plist")).ok()?;
    let pattern = Regex::new(&format!(
        r"<key>{}</key>\s*<string>([^<]+)</string>",
        regex::escape(key)
    ))
    .ok()?;
    Some(pattern.captures(&text)?.get(1)?.as_str().into())
}

pub(super) fn find(app: &str) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let roots = [
        PathBuf::from("/Applications"),
        crate::config::get_home_dir().join("Applications"),
    ];
    find_in(app, &roots).or_else(|| spotlight(app))
}

#[cfg(all(target_os = "macos", not(test)))]
fn spotlight(app: &str) -> Option<PathBuf> {
    use std::{
        process::{Command, Stdio},
        sync::Mutex,
        time::{Duration, Instant},
    };
    struct CachedPaths {
        checked: Instant,
        paths: Vec<PathBuf>,
    }
    static CACHE: Lazy<Mutex<HashMap<String, CachedPaths>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    if std::env::var_os("CC_SWITCH_TEST_HOME").is_some() || ids(app).is_empty() {
        return None;
    }
    let cached = CACHE.lock().ok().and_then(|cache| {
        cache
            .get(app)
            .filter(|answer| answer.checked.elapsed() < Duration::from_secs(30))
            .map(|answer| answer.paths.clone())
    });
    let paths = cached.unwrap_or_else(|| {
        let query = ids(app)
            .iter()
            .map(|id| format!("kMDItemCFBundleIdentifier == '{id}'"))
            .collect::<Vec<_>>()
            .join(" || ");
        let mut paths = vec![];
        if let Ok(mut child) = Command::new("/usr/bin/mdfind")
            .args(["-0", &query])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            let start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        if let Ok(output) = child.wait_with_output() {
                            if output.status.success() && output.stdout.len() <= 256 * 1024 {
                                paths = output
                                    .stdout
                                    .split(|byte| *byte == 0)
                                    .filter(|bytes| !bytes.is_empty())
                                    .filter_map(|bytes| std::str::from_utf8(bytes).ok())
                                    .map(PathBuf::from)
                                    .collect();
                            }
                        }
                        break;
                    }
                    Ok(None) if start.elapsed() < Duration::from_millis(750) => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
        if let Ok(mut cache) = CACHE.lock() {
            cache.insert(
                app.into(),
                CachedPaths {
                    checked: Instant::now(),
                    paths: paths.clone(),
                },
            );
        }
        paths
    });
    paths.into_iter().find(|path| matches(app, path, false))
}

#[cfg(any(not(target_os = "macos"), test))]
fn spotlight(_: &str) -> Option<PathBuf> {
    None
}

pub(super) fn update_channel_matches(app: &str, bundle: &Path) -> bool {
    if app == "deepseek-harness" {
        let Ok(bytes) = std::fs::read(bundle.join("Contents/Resources/app-update.yml")) else {
            return false;
        };
        if bytes.len() > 64 * 1024 {
            return false;
        }
        let Ok(feed) = serde_yaml::from_slice::<serde_json::Value>(&bytes) else {
            return false;
        };
        return feed["provider"] == "generic"
            && feed["channel"] == "nightly"
            && feed["url"].as_str().is_some_and(|url| {
                url.trim_end_matches('/')
                    == "https://download.deepseek.com/dsh-desk/feeds/mac-arm64"
            });
    }
    app != "workbuddy"
        || (field(bundle, "CFBundleIdentifier").as_deref() != Some("com.workbuddy.workbuddy-ai")
            && bundle
                .file_name()
                .is_none_or(|name| name != "WorkBuddy AI.app"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_updates_require_the_installed_public_channel() {
        let root = tempfile::tempdir().unwrap();
        let resources = root.path().join("Contents/Resources");
        std::fs::create_dir_all(&resources).unwrap();
        let feed = "provider: generic\nchannel: nightly\nurl: https://download.deepseek.com/dsh-desk/feeds/mac-arm64/\n";
        std::fs::write(resources.join("app-update.yml"), feed).unwrap();
        assert!(update_channel_matches("deepseek-harness", root.path()));
        for changed in [
            feed.replace("nightly", "stable"),
            feed.replace("download.deepseek.com", "example.invalid"),
        ] {
            std::fs::write(resources.join("app-update.yml"), changed).unwrap();
            assert!(!update_channel_matches("deepseek-harness", root.path()));
        }
    }

    fn bundle(root: &Path, name: &str, id: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::create_dir_all(path.join("Contents")).unwrap();
        std::fs::write(path.join("Contents/Info.plist"), format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleShortVersionString</key><string>5.6.2</string></dict></plist>"#)).unwrap();
        path
    }

    #[test]
    fn every_supported_desktop_is_found_in_an_application_subfolder() {
        for (app, name) in [
            ("claude-desktop", "Claude.app"),
            ("codex", "Codex.app"),
            ("workbuddy", "WorkBuddy AI.app"),
            ("qoder", "Qoder CN.app"),
            ("doubaowork", "DoubaoWork.app"),
            ("trae", "Trae CN.app"),
            ("kimi", "Kimi Code.app"),
            ("zcode", "ZCode.app"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let nested = dir.path().join("AI Tools");
            std::fs::create_dir(&nested).unwrap();
            let path = bundle(
                &nested,
                name,
                ids(app).first().copied().unwrap_or("test.synthetic"),
            );
            assert_eq!(find_in(app, &[dir.path().into()]), Some(path), "{app}");
        }
    }

    #[test]
    fn trashed_desktop_bundles_do_not_count_as_installed() {
        let dir = tempfile::tempdir().unwrap();
        for trash in [".Trash", ".Trashes/501"] {
            let root = dir.path().join(trash);
            let path = bundle(&root, "DoubaoWork.app", "com.work.pc.doubao");
            assert!(!matches("doubaowork", &path, false));
            assert!(find_in("doubaowork", &[root]).is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn application_symlinks_to_trashed_bundles_do_not_count_as_installed() {
        let dir = tempfile::tempdir().unwrap();
        let path = bundle(
            &dir.path().join(".Trash"),
            "DoubaoWork.app",
            "com.work.pc.doubao",
        );
        let apps = dir.path().join("Applications");
        std::fs::create_dir(&apps).unwrap();
        let alias = apps.join("DoubaoWork.app");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(!matches("doubaowork", &alias, false));
        assert!(find_in("doubaowork", &[apps]).is_none());
    }

    #[test]
    fn workbuddy_builds_and_renamed_bundles_are_detected_by_identity() {
        for (name, id) in [
            ("WorkBuddy.app", "com.tencent.workbuddy.mac"),
            ("WorkBuddy AI.app", "com.workbuddy.workbuddy-ai"),
            ("Renamed.app", "com.workbuddy.workbuddy-ai"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = bundle(dir.path(), name, id);
            assert_eq!(find_in("workbuddy", &[dir.path().into()]), Some(path));
        }
    }

    #[test]
    fn workbuddy_update_channels_keep_the_international_build_separate() {
        let dir = tempfile::tempdir().unwrap();
        let international = bundle(dir.path(), "Renamed.app", "com.workbuddy.workbuddy-ai");
        assert!(!update_channel_matches("workbuddy", &international));
        let domestic = bundle(dir.path(), "WorkBuddy.app", "com.tencent.workbuddy.mac");
        assert!(update_channel_matches("workbuddy", &domestic));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn binary_metadata_and_external_renames_remain_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = bundle(dir.path(), "Renamed.app", "com.workbuddy.workbuddy-ai");
        assert!(std::process::Command::new("/usr/bin/plutil")
            .args(["-convert", "binary1"])
            .arg(path.join("Contents/Info.plist"))
            .status()
            .unwrap()
            .success());
        assert_eq!(
            find_in("workbuddy", &[dir.path().into()]),
            Some(path.clone())
        );
        assert_eq!(
            field(&path, "CFBundleShortVersionString").as_deref(),
            Some("5.6.2")
        );
        assert!(field(&path, "MissingKey").is_none());
    }

    #[test]
    fn misleading_names_chatgpt_and_leftover_configuration_do_not_count() {
        let dir = tempfile::tempdir().unwrap();
        bundle(dir.path(), "WorkBuddy.app", "test.other");
        bundle(dir.path(), "ChatGPT.app", "com.openai.chat");
        std::fs::create_dir(dir.path().join(".workbuddy-ai")).unwrap();
        assert!(find_in("workbuddy", &[dir.path().into()]).is_none());
        assert!(find_in("codex", &[dir.path().into()]).is_none());
        let path = bundle(dir.path(), "ChatGPT.app", "com.openai.codex");
        assert_eq!(find_in("codex", &[dir.path().into()]), Some(path));
    }

    #[test]
    fn arbitrary_config_directories_and_bundles_are_not_traversed() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().join("Other.app");
        bundle(&outer, "WorkBuddy AI.app", "com.workbuddy.workbuddy-ai");
        assert!(find_in("workbuddy", &[dir.path().into()]).is_none());
        let missing = dir.path().join("missing");
        assert!(find_in("workbuddy", &[missing.clone()]).is_none());
        assert!(!missing.exists());
    }
}
