//! Native Qoder CN providers. Authentication remains owned by Qoder.

use crate::{config::atomic_write_private, error::AppError};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[cfg(test)]
thread_local! { static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(crate) fn with_test_dir<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_DIR.with(|s| *s.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(TEST_DIR.with(|s| s.borrow_mut().replace(dir.into())));
    f()
}
pub(crate) fn directory() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = TEST_DIR.with(|s| s.borrow().clone()) {
        return dir;
    }
    crate::config::get_home_dir().join(".qoder-cn")
}
pub(crate) fn path() -> PathBuf {
    directory().join("settings.json")
}
pub(crate) fn invalid() -> AppError {
    AppError::localized(
        "qoder.invalid_config",
        "Qoder 配置无效或已被其他程序修改，请刷新后重试",
        "Qoder configuration is invalid or changed externally; refresh and retry",
    )
}
pub(crate) fn read_at(path: &Path) -> Result<(Value, Option<Vec<u8>>), AppError> {
    let bytes = match std::fs::read(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((json!({}), None)),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if !value.is_object() || value.get("providers").is_some_and(|p| !p.is_object()) {
        return Err(invalid());
    }
    Ok((value, Some(bytes)))
}
pub(crate) fn validate_provider(config: &Value) -> Result<(), AppError> {
    let nonempty = |v: Option<&Value>| {
        v.and_then(Value::as_str)
            .is_some_and(|v| !v.trim().is_empty())
    };
    if !config.is_object() || !nonempty(config.get("apiKey")) || !nonempty(config.get("model")) {
        return Err(invalid());
    }
    let endpoint = config["baseUrl"]
        .as_str()
        .and_then(|s| url::Url::parse(s).ok())
        .ok_or_else(invalid)?;
    if !matches!(endpoint.scheme(), "http" | "https")
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(invalid());
    }
    if config
        .get("authType")
        .is_some_and(|v| !matches!(v.as_str(), Some("bearer" | "api-key")))
    {
        return Err(invalid());
    }
    if config.get("type").is_some_and(|v| {
        !matches!(
            v.as_str(),
            Some("openai-compatible" | "alibaba" | "aliyun-bailian")
        )
    }) {
        return Err(invalid());
    }
    if !matches!(
        config["protocol"].as_str(),
        Some("openai" | "openai-responses" | "anthropic")
    ) {
        return Err(invalid());
    }
    if http::HeaderValue::from_str(config["apiKey"].as_str().ok_or_else(invalid)?).is_err() {
        return Err(invalid());
    }
    let models = config["models"]
        .as_array()
        .filter(|m| !m.is_empty())
        .ok_or_else(invalid)?;
    let mut ids = std::collections::HashSet::new();
    for model in models {
        let id = model["model"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(invalid)?;
        if !ids.insert(id) {
            return Err(invalid());
        }
        for key in ["contextWindow", "maxOutputTokens"] {
            if model
                .get(key)
                .is_some_and(|v| !v.as_u64().is_some_and(|n| n > 0))
            {
                return Err(invalid());
            }
        }
    }
    if !ids.contains(config["model"].as_str().ok_or_else(invalid)?) {
        return Err(invalid());
    }
    Ok(())
}
pub(crate) fn native_id(id: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("varyai-{:x}", Sha256::digest(id.as_bytes()))
}
pub(crate) fn providers() -> Result<serde_json::Map<String, Value>, AppError> {
    Ok(read_at(&path())?.0["providers"]
        .as_object()
        .cloned()
        .unwrap_or_default())
}
static CONFIG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
pub(crate) fn change_provider<T>(
    id: &str,
    expected: Option<&Value>,
    replacement: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    change_provider_at(&path(), id, expected, replacement, commit)
}
pub(crate) fn change_provider_at<T>(
    path: &Path,
    id: &str,
    expected: Option<&Value>,
    replacement: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    change_provider_at_with_replacement(path, id, expected, replacement, false, commit)
}
pub(crate) fn replace_provider<T>(
    id: &str,
    expected: Option<&Value>,
    replacement: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    change_provider_at_with_replacement(&path(), id, expected, replacement, true, commit)
}
fn change_provider_at_with_replacement<T>(
    path: &Path,
    id: &str,
    expected: Option<&Value>,
    replacement: Option<&Value>,
    replace_managed: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = CONFIG_LOCK.get_or_init(|| Mutex::new(())).lock()?;
    if let Some(config) = replacement {
        validate_provider(config)?;
    }
    let (mut document, original) = read_at(path)?;
    if document.get("providers").and_then(|p| p.get(id)) != expected {
        return Err(invalid());
    }
    if document.get("providers").is_none() {
        document["providers"] = json!({});
    }
    let providers = document["providers"].as_object_mut().ok_or_else(invalid)?;
    if replace_managed {
        providers.retain(|key, _| {
            !key.strip_prefix("varyai-").is_some_and(|hash| {
                hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        });
    }
    match replacement {
        Some(value) => {
            providers.insert(id.into(), value.clone());
        }
        None => {
            providers.remove(id);
        }
    }
    if std::fs::read(path).ok() != original {
        return Err(invalid());
    }
    if let Some(bytes) = &original {
        let backup = path.with_file_name("settings.json.varyai-backup");
        if !backup.exists() {
            atomic_write_private(&backup, bytes)?;
        }
    }
    let written = serde_json::to_vec_pretty(&document).map_err(|_| invalid())?;
    atomic_write_private(path, &written)?;
    match commit() {
        Ok(v) => Ok(v),
        Err(e) => {
            if std::fs::read(path).ok().as_deref() != Some(written.as_slice()) {
                return Err(invalid());
            }
            match original {
                Some(bytes) => atomic_write_private(path, &bytes)?,
                None => std::fs::remove_file(path).map_err(|e| AppError::io(path, e))?,
            };
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider() -> serde_json::Value {
        json!({"baseUrl":"https://api.example/v1","apiKey":"synthetic-key","type":"openai-compatible","protocol":"openai","authType":"bearer","model":"model-a","models":[{"model":"model-a","displayName":"A"},{"model":"model-b","displayName":"B"}]})
    }

    #[test]
    fn qoder_provider_changes_preserve_other_settings_and_roll_back_failed_database_write() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("settings.json");
        let original =
            json!({"enabledPlugins":{"keep":true},"providers":{"native-owned":{"keep":true}}})
                .to_string();
        std::fs::write(&path, &original).expect("seed");
        let config = provider();
        change_provider_at(&path, "varyai-test", None, Some(&config), || Ok(())).expect("add");
        let native = read_at(&path).expect("read").0;
        assert_eq!(native["providers"]["varyai-test"], config);
        assert_eq!(native["providers"]["native-owned"], json!({"keep":true}));
        assert_eq!(native["enabledPlugins"], json!({"keep":true}));
        let bytes = std::fs::read(&path).expect("read");
        assert!(
            change_provider_at(&path, "varyai-test", Some(&config), None, || Err::<(), _>(
                crate::error::AppError::Config("synthetic failure".into())
            ))
            .is_err()
        );
        assert_eq!(std::fs::read(&path).expect("read"), bytes);
        let changed = json!({"baseUrl":"https://user.example/v1"});
        let mut external = native.clone();
        external["providers"]["varyai-test"] = changed;
        std::fs::write(&path, external.to_string()).expect("external edit");
        assert!(change_provider_at(&path, "varyai-test", Some(&config), None, || Ok(())).is_err());
        assert_eq!(read_at(&path).expect("read").0, external);
    }

    #[test]
    fn qoder_models_validate_per_provider_and_support_all_native_protocols() {
        for protocol in ["openai", "openai-responses", "anthropic"] {
            let mut config = provider();
            config["protocol"] = json!(protocol);
            validate_provider(&config).expect("supported protocol");
        }
        for bad in [
            json!({}),
            json!({"baseUrl":"file:///tmp/test"}),
            json!({"protocol":"unsupported"}),
            json!({"models":[{"model":"same"},{"model":"same"}]}),
        ] {
            let mut config = provider();
            config
                .as_object_mut()
                .expect("config")
                .extend(bad.as_object().expect("fixture").clone());
            if bad.as_object().expect("fixture").is_empty() {
                config["apiKey"] = json!("");
            }
            assert!(validate_provider(&config).is_err());
        }
        validate_provider(&provider()).expect("independent providers may share model IDs");
    }
}
