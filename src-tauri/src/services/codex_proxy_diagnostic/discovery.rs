use super::*;
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::process::Stdio;
#[cfg(any(target_os = "macos", all(test, unix)))]
use tokio::{io::AsyncReadExt, process::Command};

pub(super) const KEYS: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];
#[cfg(any(target_os = "macos", windows, test))]
const OUTPUT_LIMIT: usize = 32 * 1024;
#[cfg(any(target_os = "macos", test))]
const MARKER: &[u8] = b"\0VARY_PROXY_ENV_V1\0";
// The extra sh process sees exported values only, including when the login shell
// is fish. No shell expansion of a private value or complete env dump is used.
#[cfg(any(target_os = "macos", all(test, unix)))]
const EXPORTED_SCRIPT: &str = "printf '\\0VARY_PROXY_ENV_V1\\0'; for key in HTTP_PROXY http_proxy HTTPS_PROXY https_proxy ALL_PROXY all_proxy NO_PROXY no_proxy; do printf '%s\\0' \"$key\"; /usr/bin/printenv \"$key\"; printf '\\0'; done";

#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_login(bytes: &[u8]) -> Option<Environment> {
    if bytes.len() > OUTPUT_LIMIT {
        return None;
    }
    let start = bytes
        .windows(MARKER.len())
        .position(|part| part == MARKER)?
        + MARKER.len();
    let mut fields = bytes[start..].split(|b| *b == 0);
    let mut env = Environment::new();
    while let Some(key) = fields.next() {
        if key.is_empty() {
            return Some(env);
        }
        let key = std::str::from_utf8(key).ok()?;
        if !KEYS.contains(&key) || env.contains_key(key) {
            return None;
        }
        let value = fields.next()?;
        if value.len() > 4096 {
            return None;
        }
        let value = std::str::from_utf8(value)
            .ok()?
            .strip_suffix('\n')
            .unwrap_or(std::str::from_utf8(value).ok()?);
        env.insert(key.into(), value.into());
    }
    None
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn merge_login(process: &mut Environment, login: Environment) {
    for (key, value) in login {
        if KEYS.contains(&key.as_str()) && !value.trim().is_empty() {
            process.insert(key, value);
        }
    }
}

/// Output and failures stay private; callers receive only bounded parsed fields.
#[cfg(any(target_os = "macos", all(test, unix)))]
async fn read_tool(mut command: Command) -> Option<Vec<u8>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let operation = async {
        let mut bytes = Vec::new();
        stdout
            .take((OUTPUT_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .ok()?;
        if bytes.len() > OUTPUT_LIMIT {
            return None;
        }
        let status = child.wait().await.ok()?;
        status.success().then_some(bytes)
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), operation)
        .await
        .ok()
        .flatten()
}

#[cfg(target_os = "macos")]
async fn login_environment() -> Option<Environment> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|shell| {
            matches!(
                shell.as_str(),
                "/bin/zsh"
                    | "/bin/bash"
                    | "/bin/sh"
                    | "/usr/local/bin/fish"
                    | "/opt/homebrew/bin/fish"
            )
        })
        .unwrap_or_else(|| "/bin/zsh".into());
    let mut command = Command::new(shell);
    // The program text is fixed; no user-supplied proxy value enters a command.
    let script = format!(
        "exec /bin/sh -c '{}'",
        EXPORTED_SCRIPT.replace('\'', "'\\''")
    );
    command.args(["-l", "-i", "-c", &script]);
    parse_login(&read_tool(command).await?)
}

#[cfg(windows)]
fn windows_system() -> Option<SystemProxy> {
    use winreg::{
        enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE},
        RegKey,
    };
    fn missing(error: &std::io::Error) -> bool {
        error.kind() == std::io::ErrorKind::NotFound
    }
    let key = match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        KEY_QUERY_VALUE,
    ) {
        Ok(key) => key,
        Err(error) if missing(&error) => return Some(SystemProxy::default()),
        Err(_) => return None,
    };
    match key.get_value::<u32, _>("ProxyEnable") {
        Ok(0) => return Some(SystemProxy::default()),
        Ok(_) => {}
        Err(error) if missing(&error) => return Some(SystemProxy::default()),
        Err(_) => return None,
    }
    let server: String = match key.get_value("ProxyServer") {
        Ok(server) => server,
        Err(error) if missing(&error) => return Some(SystemProxy::default()),
        Err(_) => return None,
    };
    let overrides: String = match key.get_value("ProxyOverride") {
        Ok(value) => value,
        Err(error) if missing(&error) => String::new(),
        Err(_) => return None,
    };
    if server.len() + overrides.len() > OUTPUT_LIMIT {
        return None;
    }
    Some(resolver::parse_windows(&server, &overrides))
}

pub(super) fn host_discovery_allowed() -> bool {
    !cfg!(test) && std::env::var_os("CC_SWITCH_TEST_HOME").is_none()
}

fn process_environment(read: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<Environment> {
    let mut env = Environment::new();
    for key in KEYS {
        if let Some(value) = read(key) {
            let value = value.to_str()?;
            if value.len() > 4096 {
                return None;
            }
            env.insert(key.to_string(), value.to_string());
        }
    }
    Some(env)
}

pub(super) async fn discover() -> Option<(Platform, Environment, SystemProxy)> {
    if !host_discovery_allowed() {
        return None;
    }
    let platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Other
    };
    let env = process_environment(|key| std::env::var_os(key))?;
    #[cfg(target_os = "macos")]
    {
        let mut env = env;
        merge_login(&mut env, login_environment().await?);
        let mut command = Command::new("/usr/sbin/scutil");
        command.arg("--proxy");
        let bytes = read_tool(command).await?;
        Some((
            platform,
            env,
            resolver::parse_mac(std::str::from_utf8(&bytes).ok()?),
        ))
    }
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(move || windows_system().map(|system| (platform, env, system)))
            .await
            .ok()
            .flatten()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        // No native system resolver is available on this platform.
        Some((platform, env, SystemProxy::default()))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn codex_proxy_diagnostic_subprocess_reads_only_exported_values(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let home = tempfile::tempdir()?;
        let mut command = Command::new("/bin/sh");
        // This fixture never loads the user's shell profile.
        let script=format!("HTTP_PROXY=http://127.0.0.1:7890; NO_PROXY=127.0.0.1; export NO_PROXY; /bin/sh -c '{}'",EXPORTED_SCRIPT.replace('\'',"'\\''"));
        command
            .env_clear()
            .env("HOME", home.path())
            .args(["-c", &script]);
        let bytes = read_tool(command)
            .await
            .ok_or("synthetic subprocess failed")?;
        let env = parse_login(&bytes).ok_or("synthetic parse failed")?;
        assert_eq!(env.get("HTTP_PROXY").map(String::as_str), Some(""));
        assert_eq!(env.get("NO_PROXY").map(String::as_str), Some("127.0.0.1"));
        assert!(!host_discovery_allowed());
        assert!(discover().await.is_none());
        Ok(())
    }

    #[test]
    fn codex_proxy_diagnostic_environment_read_failure_is_unknown() {
        use std::os::unix::ffi::OsStringExt;
        assert!(process_environment(
            |key| (key == "NO_PROXY").then(|| std::ffi::OsString::from("x".repeat(4097)))
        )
        .is_none());
        assert!(process_environment(
            |key| (key == "NO_PROXY").then(|| std::ffi::OsString::from_vec(vec![0xff]))
        )
        .is_none());
    }

    #[tokio::test]
    async fn codex_proxy_diagnostic_subprocess_bounds_output() {
        let mut command = Command::new("/bin/sh");
        command.env_clear().args(["-c","while :; do printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; done"]);
        assert!(read_tool(command).await.is_none());
    }
}
