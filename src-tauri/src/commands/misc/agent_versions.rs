use super::*;

const AGENTS: &[&str] = &[
    "claude-desktop",
    "workbuddy",
    "qoder",
    "doubaowork",
    "trae",
    "kimi",
    "zcode",
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
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
        .arg(bundle.join("Contents/Info.plist"))
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| version_number(&decode_command_output(&output.stdout)))
        .flatten()
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
    let cli_supported = matches!(app, "workbuddy" | "kimi" | "qwen");
    let mut result = AgentVersion {
        supported: cli_supported || (desktop_supported && !agent_desktop_bundles(app).is_empty()),
        installed: false,
        version: None,
        latest_version: None,
        source: None,
    };
    if desktop_supported {
        if let Some(bundle) = roots
            .iter()
            .flat_map(|root| {
                agent_desktop_bundles(app)
                    .iter()
                    .map(move |name| root.join(name))
            })
            .find(|path| path.is_dir())
        {
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

pub(super) async fn get_versions() -> Result<HashMap<String, AgentVersion>, String> {
    futures::future::try_join_all(AGENTS.iter().map(|&app| async move {
        let mut info = tokio::task::spawn_blocking(move || {
            let roots = if cfg!(target_os = "macos") {
                vec![
                    PathBuf::from("/Applications"),
                    crate::config::get_home_dir().join("Applications"),
                ]
            } else {
                vec![]
            };
            detect_in(
                app,
                &roots,
                &build_tool_search_paths(app),
                cfg!(target_os = "macos"),
            )
        })
        .await
        .map_err(|_| "Agent version check failed".to_owned())?;
        let client = crate::proxy::http_client::get();
        info.latest_version = drop_latest_behind_local(
            latest_version(&client, app, &info).await,
            info.version.as_deref(),
        );
        Ok((app.to_owned(), info))
    }))
    .await
    .map(|versions| versions.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
