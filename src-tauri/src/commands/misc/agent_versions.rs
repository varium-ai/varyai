use super::*;

const AGENTS: &[&str] = &[
    "claude-desktop",
    "codex",
    "workbuddy",
    "workbuddy-ai",
    "qoder",
    "doubaowork",
    "trae",
    "trae-intl",
    "kimi",
    "zcode",
    "deepseek-harness",
    "t3code",
];

#[derive(Debug, serde::Serialize)]
pub struct AgentVersion {
    pub supported: bool,
    pub installed: bool,
    pub version: Option<String>,
    pub latest_version: Option<String>,
    pub source: Option<&'static str>,
}

pub(super) fn version_number(text: &str) -> Option<String> {
    VERSION_RE.find(text).map(|found| found.as_str().to_owned())
}

#[cfg(target_os = "macos")]
fn desktop_version(bundle: &Path) -> Option<String> {
    // Read the product version, never the embedded Electron/editor package version.
    version_number(&desktop_apps::field(bundle, "CFBundleShortVersionString")?)
}

#[cfg(not(target_os = "macos"))]
fn desktop_version(_bundle: &Path) -> Option<String> {
    None
}

fn detect_in(
    app: &str,
    roots: &[PathBuf],
    cli_dirs: &[PathBuf],
    desktop_supported: bool,
) -> AgentVersion {
    // ChatGPT's bundle is legacy installation evidence, not the Codex App's version.
    let bundles = if app == "codex" {
        &["Codex.app"][..]
    } else {
        agent_desktop_bundles(app)
    };
    let cli_supported = matches!(app, "workbuddy" | "kimi" | "qwen");
    let mut result = AgentVersion {
        supported: cli_supported || (desktop_supported && !bundles.is_empty()),
        installed: false,
        version: None,
        latest_version: None,
        source: None,
    };
    if desktop_supported {
        if let Some(bundle) = desktop_apps::find_in(app, roots) {
            result.installed = true;
            result.source = Some("desktop");
            result.version = desktop_version(&bundle);
            return result;
        }
    }
    if cli_supported {
        for dir in cli_dirs {
            if let Some(path) = tool_executable_candidates(app, dir)
                .into_iter()
                .find(|path| path.is_file())
            {
                result.installed = true;
                result.source = Some("cli");
                // A WorkBuddy launcher is presence evidence, not a verified version-only CLI.
                if app == "workbuddy" {
                    return result;
                }
                #[cfg(not(target_os = "windows"))]
                let output = run_probe_version_command(
                    &path,
                    &prepend_search_dir_to_path(dir, &effective_path_os().unwrap_or_default()),
                );
                #[cfg(target_os = "windows")]
                let output = run_probe_version_command(
                    &path,
                    &format!("{};{}", dir.display(), effective_path_string()),
                );
                result.version = output
                    .ok()
                    .filter(|output| output.status.success())
                    .and_then(|output| {
                        version_number(&decode_command_output(&output.stdout))
                            .or_else(|| version_number(&decode_command_output(&output.stderr)))
                    });
                // An unreadable default install must not silently fall back to a different version.
                return result;
            }
        }
    }
    result
}

async fn latest_version(
    client: &reqwest::Client,
    app: &str,
    info: &AgentVersion,
) -> Option<String> {
    if info.source != Some("desktop") || !cfg!(target_os = "macos") {
        return None;
    }
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    agent_updates::latest(client, app, arch)
        .await
        .map(|release| release.version)
}

#[cfg(any(windows, test))]
fn apply_windows_desktop(
    app: &str,
    info: &mut AgentVersion,
    snapshot: &desktop_apps::windows::Snapshot,
) {
    info.supported |= desktop_apps::windows::supported(app);
    if let Some(desktop) = desktop_apps::windows::detect(app, snapshot) {
        info.installed = true;
        info.source = Some("desktop");
        info.version = desktop.version;
    }
}

pub(super) async fn get_versions() -> Result<HashMap<String, AgentVersion>, String> {
    #[cfg(target_os = "windows")]
    let snapshot = std::sync::Arc::new(
        tokio::task::spawn_blocking(desktop_apps::windows::snapshot)
            .await
            .map_err(|_| "Agent version check failed".to_owned())?,
    );
    futures::future::try_join_all(AGENTS.iter().map(|&app| {
        #[cfg(target_os = "windows")]
        let snapshot = std::sync::Arc::clone(&snapshot);
        async move {
            let (mut info, channel_matches) = tokio::task::spawn_blocking(move || {
                let bundle = desktop_apps::find(app);
                let roots = bundle
                    .as_ref()
                    .and_then(|bundle| bundle.parent().map(Path::to_path_buf))
                    .into_iter()
                    .collect::<Vec<_>>();
                let info = detect_in(
                    app,
                    &roots,
                    &build_tool_search_paths(app),
                    cfg!(target_os = "macos"),
                );
                #[cfg(target_os = "windows")]
                let info = {
                    let mut info = info;
                    apply_windows_desktop(app, &mut info, &snapshot);
                    info
                };
                let channel_matches =
                    bundle.is_none_or(|bundle| desktop_apps::update_channel_matches(app, &bundle));
                (info, channel_matches)
            })
            .await
            .map_err(|_| "Agent version check failed".to_owned())?;
            let client = crate::proxy::http_client::get();
            if channel_matches {
                info.latest_version = drop_latest_behind_local(
                    latest_version(&client, app, &info).await,
                    info.version.as_deref(),
                );
            }
            Ok((app.to_owned(), info))
        }
    }))
    .await
    .map(|versions| versions.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_desktop_versions_and_presence_share_the_same_installation_evidence() {
        for app in AGENTS {
            let mut info = detect_in(app, &[], &[], false);
            apply_windows_desktop(app, &mut info, &desktop_apps::windows::Snapshot::default());
            assert!(info.supported, "{app}");
            assert!(!info.installed, "{app}");
        }
        let snapshot = desktop_apps::windows::Snapshot {
            packages: vec!["OpenAI.Codex_26.1002.7124.0_x64__2p2nqsd0c76g0".into()],
            ..Default::default()
        };
        let mut info = detect_in("codex", &[], &[], false);
        apply_windows_desktop("codex", &mut info, &snapshot);
        assert!(info.supported && info.installed);
        assert_eq!(info.source, Some("desktop"));
        assert_eq!(info.version.as_deref(), Some("26.1002.7124.0"));
        assert!(info.latest_version.is_none());
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("WorkBuddy")).unwrap();
        std::fs::write(root.path().join("WorkBuddy/WorkBuddy.exe"), "fixture").unwrap();
        let snapshot = desktop_apps::windows::Snapshot {
            roots: vec![root.path().into()],
            ..Default::default()
        };
        info.version = Some("1.2.3".into());
        info.source = Some("cli");
        apply_windows_desktop("workbuddy", &mut info, &snapshot);
        assert!(info.installed);
        assert_eq!(info.source, Some("desktop"));
        assert!(info.version.is_none());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn codex_app_version_is_read_separately_and_chatgpt_is_not_codex() {
        assert!(AGENTS.contains(&"codex"));
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("ChatGPT.app")).unwrap();
        assert!(!detect_in("codex", &[root.path().to_owned()], &[], true).installed);
        let contents = root.path().join("Codex.app/Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(contents.join("Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleShortVersionString</key><string>26.1000.100</string></dict></plist>"#).unwrap();
        let info = detect_in("codex", &[root.path().to_owned()], &[], true);
        assert_eq!(info.version.as_deref(), Some("26.1000.100"));
        assert_eq!(info.source, Some("desktop"));
        assert!(info.latest_version.is_none());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn desktop_uses_product_version_and_never_launches_the_application() {
        let root = tempfile::tempdir().unwrap();
        let contents = root.path().join("Trae CN.app/Contents");
        std::fs::create_dir_all(contents.join("Resources/app")).unwrap();
        std::fs::write(contents.join("Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleShortVersionString</key><string>3.4.9</string></dict></plist>"#).unwrap();
        std::fs::write(
            contents.join("Resources/app/package.json"),
            r#"{"version":"1.107.1"}"#,
        )
        .unwrap();
        let info = detect_in("trae", &[root.path().to_owned()], &[], true);
        assert!(info.supported && info.installed);
        assert_eq!(info.version.as_deref(), Some("3.4.9"));
        assert_eq!(info.source, Some("desktop"));
        assert!(info.latest_version.is_none());
    }

    #[test]
    fn missing_metadata_does_not_report_an_installed_app_as_missing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Qoder CN.app")).unwrap();
        let info = detect_in("qoder", &[root.path().to_owned()], &[], true);
        assert!(info.installed);
        assert!(info.version.is_none());
        let unsupported = detect_in("qoder", &[], &[], false);
        assert!(!unsupported.supported);
        let missing = detect_in("qwen", &[], &[], false);
        assert!(missing.supported && !missing.installed);
    }

    #[test]
    #[cfg(unix)]
    fn cli_version_failure_retains_presence_without_exposing_output() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("qwen");
        for (script, expected) in [
            ("#!/bin/sh\nprintf 'Qwen Code 0.25.2\\n'\n", Some("0.25.2")),
            (
                "#!/bin/sh\nprintf 'synthetic-private-error' >&2\nexit 1\n",
                None,
            ),
        ] {
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            let info = detect_in("qwen", &[], &[root.path().to_owned()], false);
            assert!(info.installed);
            assert_eq!(info.version.as_deref(), expected);
            assert!(!serde_json::to_string(&info)
                .unwrap()
                .contains("synthetic-private-error"));
        }
    }

    #[test]
    fn workbuddy_launcher_is_not_executed_to_discover_a_version() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join(if cfg!(windows) {
            "workbuddy.exe"
        } else {
            "workbuddy"
        });
        std::fs::write(file, "synthetic non-CLI launcher").unwrap();
        let info = detect_in("workbuddy", &[], &[root.path().to_owned()], false);
        assert!(info.installed && info.version.is_none());
    }
}
