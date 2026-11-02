use crate::{config, database::Database, error::AppError};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Manager;

pub(crate) fn resolve_target(
    configured: Option<&Path>,
    default: &Path,
) -> Result<PathBuf, AppError> {
    let target = configured
        .filter(|path| {
            let native = ["varyai.db", "varyai-ext.db", "storage.json"]
                .iter()
                .any(|name| path.join(name).exists());
            let legacy = ["cc-switch.db", "vary-ai.db", "config.json"]
                .iter()
                .any(|name| path.join(name).exists())
                || path.components().any(|part| {
                    part.as_os_str()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(".cc-switch")
                });
            native || !legacy
        })
        .unwrap_or(default);
    config::validate_independent_data_dir(target)?;
    Ok(target.to_path_buf())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorageSetupStatus {
    target: PathBuf,
    needs_setup: bool,
}

fn setup_status(app: &tauri::AppHandle) -> Result<StorageSetupStatus, AppError> {
    crate::app_store::validate_paths_store(app)?;
    let configured = crate::app_store::read_override_from_store(app);
    let target = resolve_target(configured.as_deref(), &config::get_device_config_dir())?;
    let needs_setup = configured.as_deref() == Some(target.as_path()) && !target.exists();
    Ok(StorageSetupStatus {
        target,
        needs_setup,
    })
}

pub(crate) fn gate_startup(app: &tauri::AppHandle) -> Result<bool, AppError> {
    let resolved = setup_status(app);
    if let Ok(status) = &resolved {
        if !status.needs_setup {
            let configured = crate::app_store::read_override_from_store(app);
            if configured
                .as_deref()
                .is_some_and(|path| path != status.target)
            {
                // Historical pointers never authorize modifying another application's data.
                crate::app_store::set_app_config_dir_to_store(app, None)?;
            }
            return Ok(true);
        }
    }
    let (path, error) = match resolved {
        Ok(status) => (status.target, "storage.setupRequired".to_owned()),
        Err(AppError::Message(key)) if key.starts_with("storage.") => {
            (config::get_device_config_dir(), key)
        }
        Err(_) => (
            config::get_device_config_dir(),
            "storage.invalidRecovery".to_owned(),
        ),
    };
    crate::init_status::set_init_error(crate::init_status::InitErrorPayload {
        path: path.to_string_lossy().into_owned(),
        error,
        kind: Some("storage_setup".into()),
        db_version: None,
        supported_version: None,
    });
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }
    Ok(false)
}

#[tauri::command]
pub(crate) async fn get_storage_setup(app: tauri::AppHandle) -> Result<StorageSetupStatus, String> {
    setup_status(&app).map_err(setup_error)
}

fn setup_error(error: AppError) -> String {
    match error {
        AppError::Message(key) if key.starts_with("storage.") => key,
        _ => "storage.setupReadFailed".into(),
    }
}

#[tauri::command]
pub(crate) async fn complete_storage_setup(app: tauri::AppHandle) -> Result<bool, String> {
    if app.try_state::<crate::AppState>().is_some() {
        return Err("storage.restartRequired".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        static SETUP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = SETUP_LOCK
            .lock()
            .map_err(|_| "storage.writeFailed".to_owned())?;
        let status = setup_status(&app).map_err(setup_error)?;
        if !status.needs_setup {
            return Err("storage.destinationChanged".into());
        }
        drop(
            Database::init_native_at(&status.target.join("varyai.db"))
                .map_err(|_| "storage.writeFailed".to_owned())?,
        );
        crate::app_store::set_app_config_dir_to_store(&app, Some(&status.target.to_string_lossy()))
            .map_err(|_| "storage.writeFailed".to_owned())?;
        Ok(true)
    })
    .await
    .map_err(|_| "storage.writeFailed".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_errors_preserve_validation_codes_and_redact_private_details() {
        assert_eq!(
            setup_error(AppError::Message("storage.invalidTarget".into())),
            "storage.invalidTarget"
        );
        assert_eq!(
            setup_error(AppError::Config("synthetic private detail".into())),
            "storage.setupReadFailed"
        );
    }

    struct TestHome {
        _directory: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl TestHome {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", directory.path());
            Self {
                _directory: directory,
                previous,
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn default_native_storage_preserves_legacy_filenames_without_migration() {
        let _home = TestHome::new();
        let root = config::get_device_config_dir();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("cc-switch.db"),
            b"synthetic legacy main untouched",
        )
        .unwrap();
        std::fs::write(
            root.join("vary-ai.db"),
            b"synthetic legacy extension untouched",
        )
        .unwrap();
        assert_eq!(resolve_target(None, &root).unwrap(), root);
        drop(Database::init_native_at(&root.join("varyai.db")).unwrap());
        assert!(root.join("varyai.db").is_file());
        assert!(root.join("varyai-ext.db").is_file());
        assert!(root.join("storage.json").is_file());
        assert_eq!(
            std::fs::read(root.join("cc-switch.db")).unwrap(),
            b"synthetic legacy main untouched"
        );
        assert_eq!(
            std::fs::read(root.join("vary-ai.db")).unwrap(),
            b"synthetic legacy extension untouched"
        );
        drop(Database::init_native_at(&root.join("varyai.db")).unwrap());
    }

    #[test]
    #[serial_test::serial]
    fn custom_cc_switch_source_and_nested_destinations_are_rejected() {
        let _home = TestHome::new();
        let source = config::get_home_dir().join("custom-source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("cc-switch.db"), b"synthetic source untouched").unwrap();
        assert!(config::validate_independent_data_dir(&source).is_err());
        assert!(config::validate_independent_data_dir(&source.join("nested")).is_err());
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn default_storage_alias_to_a_cc_switch_source_is_rejected() {
        let _home = TestHome::new();
        let source = config::get_home_dir().join("custom-source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("cc-switch.db"), b"synthetic source untouched").unwrap();
        let root = config::get_device_config_dir();
        std::os::unix::fs::symlink(&source, &root).unwrap();
        assert!(resolve_target(None, &root).is_err());
        assert!(!source.join("varyai.db").exists());
    }
}
