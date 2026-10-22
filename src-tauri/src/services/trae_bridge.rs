//! TRAE's vendor component API owns account authentication and model storage.
//! Only explicit message text reaches VaryAI; the helper never exports login credentials.
use crate::{error::AppError, provider::Provider};
use serde_json::{json, Value};
#[cfg(unix)]
use std::{
    io::{BufRead, BufReader, Write},
    time::Duration,
};
use std::{path::PathBuf, sync::Mutex};
pub(crate) const ENABLED_KEY: &str = "trae_audit_enabled";
pub(crate) const CONSENT_KEY: &str = "trae_local_connection_consent";
static AUDIT_TOKEN: Mutex<Option<String>> = Mutex::new(None);
fn unavailable() -> AppError {
    AppError::localized(
        "agents.trae_bridge",
        "TRAE 尚未连接，请自行打开 TRAE CN；首次配置后请完全退出并重新打开一次",
        "TRAE is not connected. Open TRAE CN yourself; restart it once after initial setup",
    )
}
fn directory() -> Result<PathBuf, AppError> {
    Ok(crate::config::get_home_dir().join(".cc-switch/trae-bridge"))
}
#[cfg(unix)]
fn request_socket(path: &std::path::Path, command: &Value) -> Result<Value, AppError> {
    use std::os::unix::{fs::PermissionsExt, net::UnixStream};
    let metadata = std::fs::symlink_metadata(path).map_err(|_| unavailable())?;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(unavailable());
    }
    let mut stream = UnixStream::connect(path).map_err(|_| unavailable())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|_| unavailable())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| unavailable())?;
    serde_json::to_writer(&mut stream, command).map_err(|_| unavailable())?;
    stream.write_all(b"\n").map_err(|_| unavailable())?;
    let mut bytes = Vec::new();
    use std::io::Read;
    BufReader::new(stream)
        .take(2 * 1024 * 1024)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| unavailable())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    if value.get("error").is_some() {
        return Err(unavailable());
    }
    value.get("result").cloned().ok_or_else(unavailable)
}
#[cfg(not(unix))]
fn request_socket(_: &std::path::Path, _: &Value) -> Result<Value, AppError> {
    Err(unavailable())
}
fn socket_paths(directory: &std::path::Path, include_legacy: bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            (name.ends_with(".sock")
                && (name.starts_with("bridge-agent-")
                    || (include_legacy && name.starts_with("bridge-"))))
            .then(|| entry.path())
        })
        .collect()
}
fn request(command: &Value, all: bool) -> Result<Value, AppError> {
    let mut result = None;
    for path in socket_paths(
        &directory()?,
        command["op"] == "disable" || !cfg!(target_os = "macos"),
    ) {
        if let Ok(value) = request_socket(&path, command) {
            result = Some(value);
            if !all {
                break;
            }
        }
    }
    result.ok_or_else(unavailable)
}
pub(crate) fn active() -> bool {
    AUDIT_TOKEN.lock().ok().is_some_and(|token| token.is_some())
        && request(&json!({"op":"health"}), false)
            .ok()
            .is_some_and(|v| v["enabled"] == true)
}
fn reconnect_in(directory: &std::path::Path, origin: &str, token: &str) -> bool {
    let mut connected = false;
    let mut protected = true;
    for path in socket_paths(directory, !cfg!(target_os = "macos")) {
        let Ok(health) = request_socket(&path, &json!({"op":"health"})) else {
            continue;
        };
        connected = true;
        if health["enabled"] != true {
            let enabled = request_socket(
                &path,
                &json!({
                    "op":"enable", "url":format!("{origin}/private/trae/audit"), "token":token
                }),
            )
            .is_ok();
            let restored = enabled
                && request_socket(&path, &json!({"op":"health"}))
                    .ok()
                    .is_some_and(|health| health["enabled"] == true);
            protected &= restored;
            if restored {
                log::info!("TRAE bridge reconnected; security audit restored");
            }
        }
    }
    connected && protected
}
/// Observe user-launched TRAE and attach its local helper. Never launch the client.
pub(crate) fn reconnect(origin: &str) -> bool {
    let Ok(directory) = directory() else {
        return false;
    };
    let Ok(argv) = argv_path() else {
        return false;
    };
    if prepare_connection(&directory, &argv).is_err() {
        return false;
    }
    let Ok(token) = audit_token() else {
        return false;
    };
    if ensure_ready().is_err() {
        return false;
    }
    reconnect_in(&directory, origin, &token)
}
fn audit_token() -> Result<String, AppError> {
    let mut token = AUDIT_TOKEN.lock().map_err(|_| unavailable())?;
    Ok(token
        .get_or_insert_with(|| {
            format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )
        })
        .clone())
}
pub(crate) fn authorized(value: &str) -> bool {
    AUDIT_TOKEN
        .lock()
        .ok()
        .and_then(|v| v.clone())
        .is_some_and(|token| {
            value.len() == token.len()
                && value
                    .bytes()
                    .zip(token.bytes())
                    .fold(0u8, |diff, (a, b)| diff | (a ^ b))
                    == 0
        })
}
#[cfg(any(target_os = "macos", test))]
fn install_runtime(directory: &std::path::Path) -> Result<PathBuf, AppError> {
    let runtime = directory.join("runtime");
    std::fs::create_dir_all(&runtime).map_err(|_| unavailable())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [directory, runtime.as_path()] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| unavailable())?;
        }
    }
    for (name, source) in [
        (
            "agent.cjs",
            include_str!("../../resources/trae-bridge/agent.cjs"),
        ),
        (
            "renderer.cjs",
            include_str!("../../resources/trae-bridge/renderer.cjs"),
        ),
    ] {
        let path = runtime.join(name);
        std::fs::write(&path, source).map_err(|_| unavailable())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| unavailable())?;
        }
    }
    Ok(runtime.join("agent.cjs"))
}
#[cfg(target_os = "macos")]
fn installed_executable() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or_else(unavailable)?;
    [
        PathBuf::from("/Applications/TRAE CN.app"),
        home.join("Applications/TRAE CN.app"),
    ]
    .into_iter()
    .map(|p| p.join("Contents/MacOS/Electron"))
    .find(|p| p.is_file())
    .ok_or_else(unavailable)
}
fn prepare_connection(directory: &std::path::Path, argv: &std::path::Path) -> Result<(), AppError> {
    let backup = directory.join("launch-backup.json");
    let original = match std::fs::read_to_string(argv) {
        Ok(source) => Some(source),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(unavailable()),
    };
    let mut value: Value =
        json5::from_str(original.as_deref().unwrap_or("{}")).map_err(|_| unavailable())?;
    let object = value.as_object_mut().ok_or_else(unavailable)?;
    if backup.exists() {
        return if object.get("remote-debugging-port") == Some(&json!("0")) {
            Ok(())
        } else {
            Err(unavailable())
        };
    }
    // Never replace a user's own debugger configuration.
    if object.contains_key("remote-debugging-port") {
        return Err(unavailable());
    }
    object.insert("remote-debugging-port".into(), json!("0"));
    let applied = serde_json::to_string_pretty(&value).map_err(|_| unavailable())?;
    let encoded = serde_json::to_vec(&json!({"original":original,"applied":applied}))
        .map_err(|_| unavailable())?;
    crate::config::atomic_write_private(&backup, &encoded)?;
    crate::config::atomic_write_private(argv, applied.as_bytes())
}
fn restore_connection(directory: &std::path::Path, argv: &std::path::Path) -> Result<(), AppError> {
    let backup = directory.join("launch-backup.json");
    if !backup.exists() {
        return Ok(());
    }
    let saved: Value = serde_json::from_slice(&std::fs::read(&backup).map_err(|_| unavailable())?)
        .map_err(|_| unavailable())?;
    if argv.exists() {
        let current = std::fs::read_to_string(argv).map_err(|_| unavailable())?;
        if saved["applied"].as_str() == Some(current.as_str()) {
            if let Some(original) = saved["original"].as_str() {
                crate::config::atomic_write_private(argv, original.as_bytes())?;
            } else {
                std::fs::remove_file(argv).map_err(|_| unavailable())?;
            }
        } else {
            let mut value: Value = json5::from_str(&current).map_err(|_| unavailable())?;
            let object = value.as_object_mut().ok_or_else(unavailable)?;
            if object.get("remote-debugging-port") == Some(&json!("0")) {
                object.remove("remote-debugging-port");
                let encoded = serde_json::to_vec_pretty(&value).map_err(|_| unavailable())?;
                crate::config::atomic_write_private(argv, &encoded)?;
            }
        }
    }
    std::fs::remove_file(backup).map_err(|_| unavailable())
}
fn argv_path() -> Result<PathBuf, AppError> {
    Ok(dirs::home_dir()
        .ok_or_else(unavailable)?
        .join(".trae-cn/argv.json"))
}
fn ensure_ready() -> Result<(), AppError> {
    if let Ok(health) = request(&json!({"op":"health"}), false) {
        return if health["ready"] == true {
            Ok(())
        } else {
            Err(unavailable())
        };
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        let home = dirs::home_dir().ok_or_else(unavailable)?;
        // Ordinary Dock/Finder launches write this random loopback endpoint. Do not launch TRAE.
        let source = std::fs::read_to_string(
            home.join("Library/Application Support/Trae CN/DevToolsActivePort"),
        )
        .map_err(|_| unavailable())?;
        let mut lines = source.lines();
        let port = lines
            .next()
            .and_then(|line| line.parse::<u16>().ok())
            .filter(|port| *port > 0)
            .ok_or_else(unavailable)?;
        let browser = lines
            .next()
            .filter(|line| {
                line.starts_with("/devtools/browser/")
                    && line[18..]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
            .ok_or_else(unavailable)?;
        let executable = installed_executable()?;
        let dir = directory()?;
        let script = install_runtime(&dir)?;
        let mut child = std::process::Command::new(executable)
            .env("ELECTRON_RUN_AS_NODE", "1")
            .arg(script)
            .arg(format!("ws://127.0.0.1:{port}{browser}"))
            .arg(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|_| unavailable())?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(200));
            if request(&json!({"op":"health"}), false)
                .ok()
                .is_some_and(|v| v["ready"] == true)
            {
                return Ok(());
            }
        }
    }
    Err(unavailable())
}
pub(crate) fn enable(origin: &str) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    let _ = installed_executable()?;
    prepare_connection(&directory()?, &argv_path()?)?;
    // Enabled is an intention. A closed or not-yet-restarted client remains pending.
    if ensure_ready().is_err() {
        return Ok(());
    }
    let token = audit_token()?;
    request(
        &json!({"op":"enable","url":format!("{origin}/private/trae/audit"),"token":token}),
        true,
    )?;
    Ok(())
}
pub(crate) fn disable() -> Result<(), AppError> {
    // Invalidate first; missed/closed windows release their hooks on the heartbeat.
    *AUDIT_TOKEN.lock().map_err(|_| unavailable())? = None;
    let _ = request(&json!({"op":"disable"}), true);
    restore_connection(&directory()?, &argv_path()?)
}
pub(crate) fn profile_models() -> Result<Value, AppError> {
    request(&json!({"op":"list"}), false)
}
pub(crate) fn apply(provider: &Provider) -> Result<(), AppError> {
    super::agent_clients::validate(&provider.settings_config)?;
    ensure_ready()?;
    apply_connected(provider)
}
/// Deliver configuration without preparing, starting or enabling the bridge.
pub(crate) fn apply_connected(provider: &Provider) -> Result<(), AppError> {
    super::agent_clients::validate(&provider.settings_config)?;
    request(
        &json!({"op":"apply","id":provider.id,"name":provider.name,"config":provider.settings_config}),
        false,
    )?;
    Ok(())
}
pub(crate) fn remove(id: &str) -> Result<(), AppError> {
    request(&json!({"op":"remove","id":id}), false)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepares_passive_connection_and_restores_original_launch_config() {
        let dir = tempfile::tempdir().unwrap();
        let argv = dir.path().join("argv.json");
        let original = "{ // keep my preferences\n \"disable-hardware-acceleration\": true\n}";
        std::fs::write(&argv, &original).unwrap();
        prepare_connection(dir.path(), &argv).unwrap();
        prepare_connection(dir.path(), &argv).unwrap();
        let value: Value = json5::from_str(&std::fs::read_to_string(&argv).unwrap()).unwrap();
        assert_eq!(value["remote-debugging-port"], "0");
        assert_eq!(value["disable-hardware-acceleration"], true);
        restore_connection(dir.path(), &argv).unwrap();
        assert_eq!(std::fs::read_to_string(&argv).unwrap(), original);
    }
    #[test]
    fn passive_connection_preserves_user_edits_and_never_overwrites_an_existing_debug_port() {
        let dir = tempfile::tempdir().unwrap();
        let argv = dir.path().join("argv.json");
        prepare_connection(dir.path(), &argv).unwrap();
        let mut value: Value = serde_json::from_slice(&std::fs::read(&argv).unwrap()).unwrap();
        value["force-color-profile"] = json!("srgb");
        std::fs::write(&argv, serde_json::to_vec(&value).unwrap()).unwrap();
        restore_connection(dir.path(), &argv).unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&argv).unwrap()).unwrap();
        assert_eq!(value["force-color-profile"], "srgb");
        assert!(value.get("remote-debugging-port").is_none());
        std::fs::write(&argv, r#"{"remote-debugging-port":"9999"}"#).unwrap();
        assert!(prepare_connection(dir.path(), &argv).is_err());
        assert_eq!(
            std::fs::read_to_string(argv).unwrap(),
            r#"{"remote-debugging-port":"9999"}"#
        );
    }
    #[test]
    fn legacy_ide_socket_does_not_claim_agent_protection() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["bridge-legacy.sock", "bridge-agent-test.sock", "notes.json"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        assert_eq!(
            socket_paths(dir.path(), false),
            vec![dir.path().join("bridge-agent-test.sock")]
        );
        assert_eq!(socket_paths(dir.path(), true).len(), 2);
    }
    #[test]
    fn helper_runtime_is_private_and_contains_no_tests_or_probe() {
        let dir = tempfile::tempdir().unwrap();
        let script = install_runtime(dir.path()).unwrap();
        assert!(script.is_file());
        assert_eq!(
            std::fs::read_dir(script.parent().unwrap()).unwrap().count(),
            2
        );
        let source = std::fs::read_to_string(&script).unwrap();
        assert!(!source.contains("command.expression"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(script).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn reconnects_a_restarted_client_and_verifies_the_hook() {
        use std::os::unix::{fs::PermissionsExt, net::UnixListener};
        let dir = tempfile::tempdir().unwrap();
        assert!(!reconnect_in(
            dir.path(),
            "http://127.0.0.1:1",
            "test-token"
        ));
        let path = dir.path().join("bridge-agent-test.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let server = std::thread::spawn(move || {
            for (index, op) in ["health", "enable", "health", "health"]
                .into_iter()
                .enumerate()
            {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let command: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(command["op"], op);
                let result = if op == "enable" {
                    assert_eq!(command["token"], "test-token");
                    assert_eq!(command["url"], "http://127.0.0.1:1/private/trae/audit");
                    json!(true)
                } else {
                    json!({"ready":true,"enabled":index > 0})
                };
                writeln!(stream, "{}", json!({"result":result})).unwrap();
            }
        });
        assert!(reconnect_in(dir.path(), "http://127.0.0.1:1", "test-token"));
        assert!(reconnect_in(dir.path(), "http://127.0.0.1:1", "test-token"));
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(!reconnect_in(
            dir.path(),
            "http://127.0.0.1:1",
            "test-token"
        ));
    }
    #[cfg(unix)]
    #[test]
    fn reconnect_does_not_report_protection_from_an_enable_acknowledgement_alone() {
        use std::os::unix::{fs::PermissionsExt, net::UnixListener};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bridge-agent-test.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let server = std::thread::spawn(move || {
            for op in ["health", "enable", "health"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let command: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(command["op"], op);
                let result = if op == "enable" {
                    json!(true)
                } else {
                    json!({"ready":true,"enabled":false})
                };
                writeln!(stream, "{}", json!({"result":result})).unwrap();
            }
        });
        assert!(!reconnect_in(
            dir.path(),
            "http://127.0.0.1:1",
            "test-token"
        ));
        server.join().unwrap();
    }
}
