//! Presence-only Windows discovery; registry values are data, never commands.
use std::path::{Path, PathBuf};

#[cfg(test)]
#[path = "windows/tests.rs"]
mod tests;

#[derive(Default)]
pub(crate) struct Registration {
    pub name: String,
    pub location: String,
    pub icon: String,
    pub uninstall: String,
    pub version: String,
}

#[derive(Default)]
pub(crate) struct Snapshot {
    pub roots: Vec<PathBuf>,
    pub local: Option<PathBuf>,
    pub registrations: Vec<Registration>,
    pub packages: Vec<String>,
}

pub(crate) struct Installation {
    pub version: Option<String>,
}

fn layouts(app: &str) -> &'static [(&'static str, &'static str)] {
    match app {
        "claude-desktop" => &[("Claude", "Claude.exe")],
        "codex" => &[("Codex", "Codex.exe")],
        "workbuddy" => &[("WorkBuddy", "WorkBuddy.exe")],
        "workbuddy-ai" => &[
            ("WorkBuddy AI", "WorkBuddy.exe"),
            ("WorkBuddy-AI", "WorkBuddy.exe"),
            ("WorkBuddyAI", "WorkBuddy.exe"),
        ],
        "qoder" => &[("Qoder CN", "Qoder CN.exe")],
        "doubaowork" => &[("DoubaoWork", "DoubaoWork.exe")],
        "trae" => &[("Trae CN", "Trae CN.exe")],
        "trae-intl" => &[("Trae", "Trae.exe")],
        "kimi" => &[("Kimi Code", "Kimi Code.exe")],
        "zcode" => &[("ZCode", "ZCode.exe")],
        "deepseek-harness" => &[("DeepSeek Harness", "DeepSeek Harness.exe")],
        "t3code" => &[
            ("T3 Code", "T3 Code.exe"),
            ("T3 Code (Alpha)", "T3 Code (Alpha).exe"),
            ("T3 Code (Beta)", "T3 Code (Beta).exe"),
            ("t3code", "T3 Code.exe"),
        ],
        _ => &[],
    }
}

pub(crate) fn supported(app: &str) -> bool {
    !layouts(app).is_empty()
}

fn registration_matches(app: &str, name: &str) -> bool {
    let name = name.trim();
    let name = [" (User)", " (System)"].iter().fold(name, |name, suffix| {
        name.strip_suffix(suffix).unwrap_or(name)
    });
    let name = name.to_ascii_lowercase();
    match app {
        // The China installer has been observed with versioned and branded
        // DisplayName values while sharing `WorkBuddy.exe` with the
        // international edition. Accept the CN family only when the name does
        // not identify the AI/international edition.
        "workbuddy" => {
            let international = name.starts_with("workbuddy ai")
                || name.starts_with("workbuddy-ai")
                || name.starts_with("workbuddyai")
                || name.contains("international");
            let china = name == "workbuddy"
                || name.starts_with("workbuddy ")
                || name.starts_with("workbuddy(")
                || name.starts_with("workbuddy-");
            china && !international
        }
        "workbuddy-ai" => {
            name.starts_with("workbuddy ai")
                || name.starts_with("workbuddy-ai")
                || name.starts_with("workbuddyai")
                || name.contains("international")
        }
        _ => layouts(app)
            .iter()
            .any(|(folder, _)| name.eq_ignore_ascii_case(folder)),
    }
}

fn numeric_version(value: &str) -> Option<Vec<u32>> {
    let parts = value.split('.').collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len())
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    parts.into_iter().map(|part| part.parse().ok()).collect()
}

// Handles a quoted executable, an icon index and uninstaller arguments without
// passing any value through a shell. Paths with spaces may be unquoted.
fn registered_executable(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let executable = if let Some(quoted) = value.strip_prefix('"') {
        quoted.split_once('"')?.0
    } else {
        let end = value.to_ascii_lowercase().rfind(".exe")? + 4;
        &value[..end]
    };
    executable
        .to_ascii_lowercase()
        .ends_with(".exe")
        .then(|| PathBuf::from(executable))
}

fn installed_file(path: &Path) -> bool {
    path.is_absolute() && path.is_file()
}

fn registered_file(app: &str, record: &Registration) -> Option<PathBuf> {
    if !registration_matches(app, &record.name) {
        return None;
    }
    let location = PathBuf::from(record.location.trim().trim_matches('"'));
    let icon = registered_executable(&record.icon);
    let uninstall = registered_executable(&record.uninstall);
    let directories = std::iter::once(location).chain(
        icon.iter()
            .chain(uninstall.iter())
            .filter_map(|path| path.parent().map(Path::to_path_buf)),
    );
    for directory in directories.filter(|path| path.is_absolute()) {
        for (_, executable) in layouts(app) {
            let path = directory.join(executable);
            if installed_file(&path) {
                return Some(path);
            }
        }
    }
    None
}

fn store_version(app: &str, package: &str) -> Option<Vec<u32>> {
    let parts = package.split('_').collect::<Vec<_>>();
    if parts.len() != 5
        || !parts[3].is_empty()
        || !matches!(parts[2], "x64" | "x86" | "arm64" | "neutral")
    {
        return None;
    }
    let identity = match app {
        "codex" => parts[0] == "OpenAI.Codex" && parts[4] == "2p2nqsd0c76g0",
        "claude-desktop" => {
            matches!(parts[0], "Claude" | "AnthropicPBC.Claude") && !parts[4].is_empty()
        }
        _ => false,
    };
    identity
        .then(|| numeric_version(parts[1]))
        .flatten()
        .filter(|v| v.len() == 4)
}

pub(crate) fn detect(app: &str, snapshot: &Snapshot) -> Option<Installation> {
    if !supported(app) {
        return None;
    }
    if let Some(version) = snapshot
        .packages
        .iter()
        .filter_map(|p| store_version(app, p))
        .max()
    {
        return Some(Installation {
            version: Some(
                version
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join("."),
            ),
        });
    }
    for record in &snapshot.registrations {
        if registered_file(app, record).is_some() {
            return Some(Installation {
                version: numeric_version(&record.version).map(|_| record.version.clone()),
            });
        }
    }
    for root in &snapshot.roots {
        for (folder, executable) in layouts(app) {
            if installed_file(&root.join(folder).join(executable)) {
                return Some(Installation { version: None });
            }
        }
    }
    if matches!(app, "workbuddy" | "workbuddy-ai") {
        if let Some(local) = &snapshot.local {
            for (folder, executable) in layouts(app) {
                if installed_file(&local.join(folder).join(executable)) {
                    return Some(Installation { version: None });
                }
            }
        }
    }
    if app == "claude-desktop" {
        if let Some(local) = &snapshot.local {
            let root = local.join("AnthropicClaude");
            if installed_file(&root.join("claude.exe")) {
                return Some(Installation { version: None });
            }
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().starts_with("app-")
                        && installed_file(&entry.path().join("claude.exe"))
                    {
                        return Some(Installation { version: None });
                    }
                }
            }
        }
    }
    None
}

#[cfg(windows)]
pub(crate) fn snapshot() -> Snapshot {
    use winreg::{enums::*, RegKey};
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let mut snapshot = Snapshot {
        roots: local
            .iter()
            .map(|root| root.join("Programs"))
            .chain(
                ["ProgramFiles", "ProgramFiles(x86)"]
                    .iter()
                    .filter_map(std::env::var_os)
                    .map(PathBuf::from),
            )
            .filter(|path| path.is_absolute())
            .collect(),
        local,
        ..Snapshot::default()
    };
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let root = RegKey::predef(hive);
            if let Ok(uninstall) = root.open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
                KEY_READ | view,
            ) {
                for name in uninstall.enum_keys().flatten() {
                    let Ok(key) = uninstall.open_subkey_with_flags(&name, KEY_READ | view) else {
                        continue;
                    };
                    let read = |field: &str| {
                        key.get_value::<String, _>(field)
                            .map(|raw| super::super::expand_env_chars(&raw))
                            .unwrap_or_default()
                    };
                    snapshot.registrations.push(Registration {
                        name: read("DisplayName"),
                        location: read("InstallLocation"),
                        icon: read("DisplayIcon"),
                        uninstall: read("UninstallString"),
                        version: read("DisplayVersion"),
                    });
                }
            }
            // Electron-builder also registers the chosen folder outside Uninstall.
            if let Ok(key) = root.open_subkey_with_flags(
                r"Software\1bf39983-50d0-5fe0-9ef4-cece76f67c5e",
                KEY_READ | view,
            ) {
                snapshot.registrations.push(Registration {
                    name: "DeepSeek Harness".into(),
                    location: key
                        .get_value::<String, _>("InstallLocation")
                        .map(|raw| super::super::expand_env_chars(&raw))
                        .unwrap_or_default(),
                    ..Registration::default()
                });
            }
        }
    }
    if let Ok(packages) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(
        r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages",
    ) {
        snapshot.packages = packages.enum_keys().flatten().collect();
    }
    snapshot
}
