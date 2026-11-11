//! Only Cursor Private Inference reads these variables; regular Cursor is cloud-backed.
use super::{CONFLICT, FAILED};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const VARIABLES: [&str; 2] = ["CURSOR_LOCAL_AGENT_BASE_URL", "CURSOR_LOCAL_AGENT_API_KEY"];

pub(crate) fn cli_path() -> PathBuf {
    let isolated = std::env::var("CC_SWITCH_TEST_HOME").is_ok_and(|home| !home.trim().is_empty());
    let home = crate::config::get_home_dir();
    let directory = std::env::var_os("CURSOR_CONFIG_DIR")
        .filter(|value| !isolated && !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            #[cfg(target_os = "linux")]
            if let Some(path) =
                std::env::var_os("XDG_CONFIG_HOME").filter(|value| !isolated && !value.is_empty())
            {
                return PathBuf::from(path).join("cursor");
            }
            home.join(".cursor")
        });
    directory.join("cli-config.json")
}

pub(crate) fn set_native_model(model: &str) -> Result<(), String> {
    native_model_edit(&cli_path(), model)?.write()
}

fn native_model_edit(path: &Path, model: &str) -> Result<super::FileEdit, String> {
    // As in Magpie, only Cursor's own Auto choice is offered without a native catalog.
    if model != "auto" {
        return Err(FAILED.into());
    }
    super::FileEdit::prepare(path.to_path_buf(), |value| {
        if value.is_object() && value.as_object().is_some_and(|object| object.is_empty()) {
            *value = json!({"version":1,"editor":{"vimMode":false},"permissions":{"allow":[],"deny":[]}});
        }
        for key in ["modelId", "displayModelId", "displayName"] {
            super::put(value, &["model".into(), key.into()], Some(json!(model)))?;
        }
        value["hasChangedDefaultModel"] = json!(true);
        Ok(())
    })
}

pub(crate) fn model_entry(model: &super::Model, mut entry: Value) -> Value {
    entry["api_types"] = json!(["responses"]);
    let mut capabilities = json!({
        "supports_tool_use":true, "supports_streaming":true,
        "output_modalities":["text"], "supports_vision":model.images,
        "supports_reasoning":model.reasoning || model.reasoning_levels.iter().any(|level| level != "none")
    });
    if let Some(window) = model.context_window {
        capabilities["context_length"] = json!(window);
    }
    if let Some(output) = super::output_limit(model) {
        capabilities["max_output_tokens"] = json!(output);
    }
    if !model.reasoning_levels.is_empty() {
        capabilities["reasoning_effort"] = json!(model.reasoning_levels);
    }
    entry["capabilities"] = capabilities;
    entry
}

impl super::Manager {
    #[cfg(any(target_os = "macos", test))]
    fn resume_cursor_after_boot(&self, boot_ms: i64) -> Result<(), String> {
        let Some(mut receipt) = self.load("cursor")? else {
            return Ok(());
        };
        if receipt.suspended
            || receipt
                .applied_at_ms
                .is_none_or(|applied| applied >= boot_ms)
        {
            return Ok(());
        }
        for file in &receipt.files {
            if !matches!(file.format, super::Format::CursorEnvironment) {
                return Err(FAILED.into());
            }
            let live = file.format.read(&file.path)?;
            let current = file.format.parse(live.as_deref())?;
            // A saved external value, including a partially removed pair, belongs to the user.
            if current.as_object().is_none_or(|value| !value.is_empty()) {
                return Ok(());
            }
            let mut resumed = file.clone();
            resumed.original = live;
            resumed.write()?;
        }
        receipt.applied_at_ms = Some(chrono::Utc::now().timestamp_millis());
        crate::config::atomic_write_private(
            &self.path("cursor")?,
            &serde_json::to_vec(&receipt).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED.into())
    }
}

pub(crate) fn resume_after_reboot(manager: &super::Manager) -> Result<(), String> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let output = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "kern.boottime"])
            .output()
            .map_err(|_| FAILED)?;
        if !output.status.success() {
            return Err(FAILED.into());
        }
        let text = String::from_utf8(output.stdout).map_err(|_| FAILED)?;
        let pattern = regex::Regex::new(r"\bsec\s*=\s*(\d+)").map_err(|_| FAILED)?;
        let boot_ms = pattern
            .captures(&text)
            .and_then(|captures| captures.get(1))
            .and_then(|value| value.as_str().parse::<i64>().ok())
            .and_then(|value| value.checked_mul(1000))
            .ok_or(FAILED)?;
        manager.resume_cursor_after_boot(boot_ms)
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        let _ = manager;
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn read_environment(path: &Path) -> Result<Option<String>, String> {
    super::read(path).map(|value| Some(value.unwrap_or_else(|| "{}".into())))
}

#[cfg(not(test))]
pub(super) fn read_environment(_path: &Path) -> Result<Option<String>, String> {
    let mut value = json!({});
    for name in VARIABLES {
        if let Some(current) = platform::get(name)? {
            value[name] = json!(current);
        }
    }
    serde_json::to_string(&value)
        .map(Some)
        .map_err(|_| FAILED.into())
}

pub(super) fn replace_environment(
    path: &Path,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<(), String> {
    let parse = |text: Option<&str>| -> Result<Value, String> {
        serde_json::from_str(text.unwrap_or("{}")).map_err(|_| FAILED.into())
    };
    let previous = parse(before)?;
    let next = parse(after)?;
    if !next.is_object()
        || next.as_object().is_some_and(|values| {
            values
                .keys()
                .any(|name| !VARIABLES.contains(&name.as_str()))
        })
        || VARIABLES
            .iter()
            .any(|name| next.get(name).is_some_and(|value| !value.is_string()))
    {
        return Err(FAILED.into());
    }
    if parse(read_environment(path)?.as_deref())? != previous {
        return Err(CONFLICT.into());
    }
    #[cfg(test)]
    return super::replace_checked(path, super::read(path)?.as_deref(), after);
    #[cfg(not(test))]
    {
        for (index, name) in VARIABLES.iter().enumerate() {
            if previous.get(name) == next.get(name) {
                continue;
            }
            if platform::set(name, next.get(name).and_then(Value::as_str)).is_err() {
                // A partial failure restores only variables still equal to this write.
                for name in VARIABLES[..=index].iter().rev() {
                    if platform::get(name).ok().flatten().as_deref()
                        == next.get(name).and_then(Value::as_str)
                    {
                        let _ = platform::set(name, previous.get(name).and_then(Value::as_str));
                    }
                }
                return Err(FAILED.into());
            }
        }
        Ok(())
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod platform {
    use super::FAILED;
    pub fn get(name: &str) -> Result<Option<String>, String> {
        let output = std::process::Command::new("/bin/launchctl")
            .args(["getenv", name])
            .output()
            .map_err(|_| FAILED)?;
        if !output.status.success() {
            return Err(FAILED.into());
        }
        let value = String::from_utf8(output.stdout).map_err(|_| FAILED)?;
        let value = value.strip_suffix('\n').unwrap_or(&value);
        Ok((!value.is_empty()).then(|| value.to_owned()))
    }
    pub fn set(name: &str, value: Option<&str>) -> Result<(), String> {
        let mut command = std::process::Command::new("/bin/launchctl");
        if let Some(value) = value {
            command.args(["setenv", name, value]);
        } else {
            command.args(["unsetenv", name]);
        }
        command
            .output()
            .map_err(|_| FAILED)?
            .status
            .success()
            .then_some(())
            .ok_or_else(|| FAILED.into())
    }
}

#[cfg(all(windows, not(test)))]
mod platform {
    use super::FAILED;
    use winreg::{
        enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE},
        RegKey,
    };
    pub fn get(name: &str) -> Result<Option<String>, String> {
        let key = match RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Environment", KEY_READ)
        {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(FAILED.into()),
        };
        match key.get_value(name) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(FAILED.into()),
        }
    }
    pub fn set(name: &str, value: Option<&str>) -> Result<(), String> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey_with_flags("Environment", KEY_SET_VALUE)
            .map_err(|_| FAILED)?;
        match value {
            Some(value) => key.set_value(name, &value).map_err(|_| FAILED)?,
            None => match key.delete_value(name) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(FAILED.into()),
            },
        }
        let message = "Environment\0".encode_utf16().collect::<Vec<_>>();
        // Notify Explorer so newly launched desktop apps inherit the updated user environment.
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                message.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                1000,
                std::ptr::null_mut(),
            );
        }
        Ok(())
    }
}

#[cfg(all(not(any(target_os = "macos", windows)), not(test)))]
mod platform {
    pub fn get(_name: &str) -> Result<Option<String>, String> {
        Err(super::FAILED.into())
    }
    pub fn set(_name: &str, _value: Option<&str>) -> Result<(), String> {
        Err(super::FAILED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_connections::{FileEdit, Format};
    #[test]
    fn cursor_cli_auto_preserves_native_settings_and_rejects_shared_models() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("cli-config.json");
        let original = json!({"version":1,"model":{"modelId":"native-example","parameter":"keep"},"permissions":{"allow":["Read(**)"]},"authentication":{"token":"synthetic-private"}});
        std::fs::write(&path, original.to_string()).map_err(|_| FAILED)?;
        assert!(native_model_edit(&path, "vary-auto/shared").is_err());
        assert_eq!(
            super::super::read(&path)?.as_deref(),
            Some(original.to_string().as_str())
        );
        native_model_edit(&path, "auto")?.write()?;
        let changed: Value =
            serde_json::from_str(&super::super::read(&path)?.ok_or(FAILED)?).map_err(|_| FAILED)?;
        assert_eq!(changed["model"]["modelId"], "auto");
        assert_eq!(changed["model"]["parameter"], "keep");
        assert_eq!(changed["authentication"], original["authentication"]);
        assert_eq!(changed["permissions"], original["permissions"]);
        Ok(())
    }
    #[test]
    fn cursor_catalog_describes_the_gateway_protocol_and_capabilities() {
        let model = super::super::Model {
            id: "vary-auto/example".into(),
            name: "Example".into(),
            group: "routing".into(),
            context_window: Some(200_000),
            compact_at: None,
            output_tokens: Some(300_000),
            cost: None,
            reasoning_levels: vec!["low".into(), "high".into()],
            reasoning: true,
            default_reasoning_level: Some("low".into()),
            images: true,
            icon: None,
        };
        let row = model_entry(&model, json!({"id":model.id,"owned_by":"varyai"}));
        assert_eq!(row["api_types"], json!(["responses"]));
        assert_eq!(row["capabilities"]["context_length"], 200_000);
        assert_eq!(row["capabilities"]["max_output_tokens"], 200_000);
        assert_eq!(
            row["capabilities"]["reasoning_effort"],
            json!(["low", "high"])
        );
        for capability in [
            "supports_tool_use",
            "supports_streaming",
            "supports_vision",
            "supports_reasoning",
        ] {
            assert_eq!(row["capabilities"][capability], true);
        }
        assert_eq!(row["owned_by"], "varyai");
    }

    #[test]
    fn cursor_resumes_only_after_reboot_and_preserves_external_environment() -> Result<(), String> {
        use crate::services::agent_connections::{Manager, Receipt};
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("environment.json");
        let manager = Manager {
            root: dir.path().join("receipts"),
        };
        let file = FileEdit::prepare_format(path.clone(), Format::CursorEnvironment, |value| {
            value[VARIABLES[0]] = json!("http://127.0.0.1:1/connections/cursor/v1");
            value[VARIABLES[1]] = json!("synthetic-local");
            Ok(())
        })?;
        manager.install(Receipt {
            app: "cursor".into(),
            token: "synthetic-local".into(),
            model: "example".into(),
            model_map: Default::default(),
            visible_models: None,
            files: vec![file],
            suspended: false,
            applied_at_ms: Some(100),
        })?;
        std::fs::write(
            &path,
            r#"{"CURSOR_LOCAL_AGENT_API_KEY":"synthetic-external"}"#,
        )
        .map_err(|_| FAILED)?;
        manager.resume_cursor_after_boot(200)?;
        assert!(!manager.status("cursor")?.configured);
        std::fs::write(&path, "{}").map_err(|_| FAILED)?;
        manager.resume_cursor_after_boot(50)?;
        assert!(!manager.status("cursor")?.configured);
        manager.resume_cursor_after_boot(200)?;
        assert!(manager.status("cursor")?.configured);
        std::fs::write(&path, "{}").map_err(|_| FAILED)?;
        manager.resume_cursor_after_boot(200)?;
        assert!(!manager.status("cursor")?.configured);
        Ok(())
    }
    #[test]
    fn cursor_environment_is_owned_restorable_and_conflict_checked() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("environment.json");
        let original =
            json!({VARIABLES[0]:"https://synthetic.example/v1",VARIABLES[1]:"synthetic-previous"});
        std::fs::write(&path, original.to_string()).map_err(|_| FAILED)?;
        let file = FileEdit::prepare_format(path.clone(), Format::CursorEnvironment, |value| {
            value[VARIABLES[0]] = json!("http://127.0.0.1:1/connections/cursor/v1");
            value[VARIABLES[1]] = json!("synthetic-local");
            Ok(())
        })?;
        file.write()?;
        assert!(file.intact()?);
        std::fs::write(
            &path,
            r#"{"CURSOR_LOCAL_AGENT_BASE_URL":"https://external.example/v1"}"#,
        )
        .map_err(|_| FAILED)?;
        assert!(!file.intact()?);
        assert_eq!(file.restore(), Err(CONFLICT.into()));
        std::fs::write(&path, file.installed.to_string()).map_err(|_| FAILED)?;
        file.restore()?;
        assert_eq!(
            serde_json::from_str::<Value>(&read_environment(&path)?.ok_or(FAILED)?)
                .map_err(|_| FAILED)?,
            original
        );
        Ok(())
    }
}
