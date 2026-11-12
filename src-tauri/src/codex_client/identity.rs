//! Codex's platform identity, prepared outside request forwarding.
use std::sync::OnceLock;

static SYSTEM: OnceLock<String> = OnceLock::new();

fn platform() -> (&'static str, &'static str) {
    match std::env::consts::OS {
        "macos" => ("Mac OS", "Apple_Terminal/455"),
        "windows" => ("Windows", "WindowsTerminal"),
        _ => ("Linux", "xterm-256color"),
    }
}

pub(super) async fn prepare() {
    static PREPARE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = PREPARE.lock().await;
    if SYSTEM.get().is_some() {
        return;
    }
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = tokio::process::Command::new("/usr/bin/sw_vers");
        command.arg("-productVersion");
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = tokio::process::Command::new("cmd");
        command.args(["/c", "ver"]).creation_flags(0x08000000);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut command = tokio::process::Command::new("uname");
        command.arg("-r");
        command
    };
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        command.kill_on_drop(true).output(),
    )
    .await;
    let version = output
        .ok()
        .and_then(Result::ok)
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| numeric_version(&text));
    let name = platform().0;
    let _ = SYSTEM.set(version.map_or_else(|| name.into(), |v| format!("{name} {v}")));
}

fn numeric_version(text: &str) -> Option<String> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let version: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts: Vec<_> = version.trim_end_matches('.').split('.').take(3).collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    while parts.len() < 3 {
        parts.push("0");
    }
    Some(parts.join("."))
}

/// Reads cached platform information only; never starts a process in forwarding.
pub(crate) fn user_agent(version: &str) -> String {
    let (fallback, terminal) = platform();
    let os = SYSTEM.get().map(String::as_str).unwrap_or(fallback);
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => other,
    };
    format!("codex_cli_rs/{version} ({os}; {arch}) {terminal}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_versions_follow_codex_on_macos_windows_and_linux() {
        for (text, expected) in [
            ("15.3", Some("15.3.0")),
            (
                "Microsoft Windows [Version 10.0.26100.4652]",
                Some("10.0.26100"),
            ),
            ("6.8.0-generic", Some("6.8.0")),
            ("unknown", None),
        ] {
            assert_eq!(numeric_version(text).as_deref(), expected);
        }
        let ua = user_agent("0.165.0");
        assert!(ua.starts_with("codex_cli_rs/0.165.0 ("));
        assert!(ua.contains("; ") && ua.contains(") "));
        assert!(http::HeaderValue::from_str(&ua).is_ok());
    }
}
