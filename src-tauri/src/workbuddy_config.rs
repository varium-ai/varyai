use crate::config::{atomic_write, write_json_file_with_contents};
use crate::error::AppError;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
thread_local! {
    static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_test_dir<T>(dir: &Path, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_DIR.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(TEST_DIR.with(|slot| slot.borrow_mut().replace(dir.to_path_buf())));
    operation()
}

pub fn get_workbuddy_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = TEST_DIR.with(|slot| slot.borrow().clone()) {
        return dir;
    }
    if let Some(dir) = crate::settings::get_workbuddy_override_dir() {
        return dir;
    }
    get_workbuddy_default_dir()
}

pub fn get_workbuddy_default_dir() -> PathBuf {
    detect_dir(&crate::config::get_home_dir())
}

fn detect_dir(home: &Path) -> PathBuf {
    let candidates = [".workbuddy-ai", ".workbuddy", ".codebuddy"];
    for name in candidates {
        let dir = home.join(name);
        if dir.join("models.json").exists() {
            return dir;
        }
    }
    for name in candidates {
        let dir = home.join(name);
        if dir.is_dir() {
            return dir;
        }
    }
    home.join(".workbuddy-ai")
}

pub fn get_workbuddy_config_path() -> PathBuf {
    get_workbuddy_dir().join("models.json")
}

fn read_at(path: &Path) -> Result<(Value, Option<Vec<u8>>), AppError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((json!({}), None)),
        Err(error) => return Err(AppError::io(path, error)),
    };
    let mut document: Value =
        serde_json::from_slice(&bytes).map_err(|_| config_error("invalid_json"))?;
    if document.is_array() {
        document = json!({"models": document});
    }
    if !document.is_object()
        || document
            .get("models")
            .is_some_and(|value| !value.is_array())
        || document.get("availableModels").is_some_and(|value| {
            value
                .as_array()
                .map(|items| items.iter().any(|id| !id.is_string()))
                .unwrap_or(true)
        })
    {
        return Err(config_error("invalid_document"));
    }
    let mut ids = HashSet::new();
    if let Some(models) = document.get("models").and_then(Value::as_array) {
        for model in models {
            let id = model
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| config_error("invalid_model"))?;
            if !ids.insert(id) {
                return Err(config_error("duplicate_model"));
            }
        }
    }
    Ok((document, Some(bytes)))
}

fn config_error(code: &str) -> AppError {
    let (zh, en) = match code {
        "invalid_json" => (
            "WorkBuddy 配置不是有效的 JSON，原文件未修改",
            "WorkBuddy configuration is invalid JSON; the original file was not changed",
        ),
        "invalid_document" => (
            "WorkBuddy 配置的根节点、models 或 availableModels 格式无效",
            "Invalid WorkBuddy root, models or availableModels structure",
        ),
        "duplicate_model" => (
            "WorkBuddy 中已存在相同模型 ID，请导入后编辑或使用不同的模型 ID",
            "The WorkBuddy model ID already exists; import and edit it, or use another model ID",
        ),
        "missing_model" => (
            "WorkBuddy 中的模型已被移除，请刷新后重试",
            "The WorkBuddy model was removed; refresh and retry",
        ),
        "changed" => (
            "WorkBuddy 配置被其他程序修改，请刷新后重试",
            "WorkBuddy configuration changed externally; refresh and retry",
        ),
        _ => (
            "WorkBuddy 模型需要名称、模型 ID、API Key 和有效的 Chat Completions 地址",
            "A WorkBuddy model requires a name, model ID, API key and a valid Chat Completions URL",
        ),
    };
    let key = match code {
        "invalid_json" => "workbuddy.invalid_json",
        "invalid_document" => "workbuddy.invalid_document",
        "duplicate_model" => "workbuddy.duplicate_model",
        "missing_model" => "workbuddy.missing_model",
        "changed" => "workbuddy.changed",
        _ => "workbuddy.invalid_model",
    };
    AppError::localized(key, zh, en)
}

pub fn validate_model(model: &Value) -> Result<(), AppError> {
    for key in ["id", "name", "apiKey"] {
        if !model
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Err(config_error("invalid_model"));
        }
    }
    let url = model
        .get("url")
        .and_then(Value::as_str)
        .and_then(|value| reqwest::Url::parse(value).ok())
        .ok_or_else(|| config_error("invalid_model"))?;
    if !matches!(url.scheme(), "http" | "https") || !url.path().ends_with("/chat/completions") {
        return Err(config_error("invalid_model"));
    }
    for key in ["maxInputTokens", "maxOutputTokens"] {
        if model
            .get(key)
            .is_some_and(|value| !value.as_u64().is_some_and(|n| n > 0))
        {
            return Err(config_error("invalid_model"));
        }
    }
    for key in ["supportsToolCall", "supportsImages"] {
        if model.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(config_error("invalid_model"));
        }
    }
    Ok(())
}

/// Expand a saved provider into native models, including legacy single-model providers.
pub(crate) fn configured_models(config: &Value) -> Result<Vec<Value>, AppError> {
    let models = expanded_models(config)?;
    let mut ids = HashSet::new();
    for model in &models {
        validate_model(model)?;
        if !ids.insert(model["id"].as_str()) {
            return Err(config_error("duplicate_model"));
        }
    }
    Ok(models)
}

/// Endpoint recovery also accepts older records that lack optional display fields.
pub(crate) fn expanded_models(config: &Value) -> Result<Vec<Value>, AppError> {
    let mut shared = config.clone();
    let entries = shared
        .as_object_mut()
        .ok_or_else(|| config_error("invalid_model"))?
        .remove("models");
    let models = match entries {
        None => vec![shared],
        Some(Value::Array(entries)) if !entries.is_empty() => entries
            .into_iter()
            .map(|entry| {
                let mut model = shared.clone();
                let fields = entry
                    .as_object()
                    .ok_or_else(|| config_error("invalid_model"))?;
                if !fields
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.trim().is_empty())
                {
                    return Err(config_error("invalid_model"));
                }
                if let Some(target) = model.as_object_mut() {
                    target.extend(fields.clone());
                }
                Ok(model)
            })
            .collect::<Result<Vec<_>, AppError>>()?,
        _ => return Err(config_error("invalid_model")),
    };
    Ok(models)
}

pub fn read_models() -> Result<Vec<Value>, AppError> {
    let (document, _) = read_at(&get_workbuddy_config_path())?;
    Ok(document
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub fn visible_model_ids() -> Result<Vec<String>, AppError> {
    let (document, _) = read_at(&get_workbuddy_config_path())?;
    let filter = document
        .get("availableModels")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty());
    Ok(document
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model["id"].as_str()?;
            if filter.is_some_and(|items| !items.iter().any(|item| item.as_str() == Some(id))) {
                return None;
            }
            Some(id.to_string())
        })
        .collect())
}

static MODEL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) fn change_models<T>(
    old_ids: &[String],
    models: &[Value],
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = MODEL_LOCK.get_or_init(|| Mutex::new(())).lock()?;
    edit_models_at(&get_workbuddy_config_path(), old_ids, models, commit)
}

/// Rewrite only managed endpoint URLs, preserving native fields and visibility.
pub(crate) fn rewrite_model_urls(
    directory: &Path,
    replacement: impl Fn(&Value) -> Option<String>,
) -> Result<(), AppError> {
    let _guard = MODEL_LOCK.get_or_init(|| Mutex::new(())).lock()?;
    let path = directory.join("models.json");
    let (mut document, original) = read_at(&path)?;
    let mut changed = false;
    if let Some(models) = document.get_mut("models").and_then(Value::as_array_mut) {
        for model in models {
            if let Some(url) = replacement(model) {
                if model["url"].as_str() != Some(&url) {
                    model["url"] = json!(url);
                    changed = true;
                }
            }
        }
    }
    if !changed {
        return Ok(());
    }
    if std::fs::read(&path).ok() != original {
        return Err(config_error("changed"));
    }
    if let Some(bytes) = &original {
        let backup = directory.join("models.json.varyai-backup");
        if !backup.exists() {
            atomic_write(&backup, bytes)?;
            secure_file(&backup)?;
        }
    }
    let array_root = original
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .is_some_and(|root| root.is_array());
    write_json_file_with_contents(
        &path,
        if array_root {
            &document["models"]
        } else {
            &document
        },
    )?;
    secure_file(&path)
}

#[cfg(test)]
fn edit_at<T>(
    path: &Path,
    old_id: Option<&str>,
    model: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    edit_models_at(
        path,
        &old_id.into_iter().map(str::to_string).collect::<Vec<_>>(),
        &model.into_iter().cloned().collect::<Vec<_>>(),
        commit,
    )
}

fn edit_models_at<T>(
    path: &Path,
    old_ids: &[String],
    replacements: &[Value],
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut new_ids = HashSet::new();
    for model in replacements {
        validate_model(model)?;
        if !new_ids.insert(model["id"].as_str()) {
            return Err(config_error("duplicate_model"));
        }
    }
    let (mut document, original) = read_at(path)?;
    if document.get("models").is_none() {
        document["models"] = json!([]);
    }
    let models = document
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| config_error("invalid_document"))?;
    let is_old = |model: &Value| {
        old_ids
            .iter()
            .any(|id| model["id"].as_str() == Some(id.as_str()))
    };
    let position = models.iter().position(is_old).unwrap_or(models.len());
    if old_ids.iter().any(|id| {
        !models
            .iter()
            .any(|model| model["id"].as_str() == Some(id.as_str()))
    }) {
        return Err(config_error("missing_model"));
    }
    if models
        .iter()
        .any(|model| !is_old(model) && new_ids.contains(&model["id"].as_str()))
    {
        return Err(config_error("duplicate_model"));
    }
    if old_ids.is_empty() && replacements.is_empty() {
        return commit();
    }
    models.retain(|model| !is_old(model));
    let position = position.min(models.len());
    models.splice(position..position, replacements.iter().cloned());
    if let Some(filter) = document
        .get_mut("availableModels")
        .and_then(Value::as_array_mut)
    {
        // An absent or empty list means unrestricted; never turn it into a restriction.
        if !filter.is_empty() {
            filter.retain(|id| !old_ids.iter().any(|old| id.as_str() == Some(old.as_str())));
            for id in replacements.iter().filter_map(|model| model["id"].as_str()) {
                if !filter.iter().any(|value| value.as_str() == Some(id)) {
                    filter.push(json!(id));
                }
            }
        }
    }
    let current = std::fs::read(path).ok();
    if current != original {
        return Err(config_error("changed"));
    }
    if let Some(bytes) = &original {
        let backup = path.with_file_name("models.json.varyai-backup");
        if !backup.exists() {
            atomic_write(&backup, bytes)?;
            secure_file(&backup)?;
        }
    }
    let array_root = original
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .is_some_and(|root| root.is_array());
    let output = if array_root {
        &document["models"]
    } else {
        &document
    };
    let written = write_json_file_with_contents(path, output)?;
    let outcome = secure_file(path).and_then(|_| commit());
    match outcome {
        Ok(result) => Ok(result),
        Err(error) => {
            if std::fs::read(path).ok().as_deref() != Some(written.as_slice()) {
                return Err(config_error("changed"));
            }
            if let Some(bytes) = original {
                atomic_write(path, &bytes)?;
            } else {
                std::fs::remove_file(path).map_err(|error| AppError::io(path, error))?;
            }
            Err(error)
        }
    }
}

fn secure_file(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| AppError::io(path, error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model(id: &str) -> Value {
        json!({"id":id,"name":id,"url":"https://example.com/v1/chat/completions","apiKey":"test-key","supportsToolCall":true})
    }

    #[test]
    fn workbuddy_model_batch_validates_and_rolls_back_as_one_change() {
        let dir = tempfile::tempdir().expect("dir");
        with_test_dir(dir.path(), || {
            let path = get_workbuddy_config_path();
            let original = json!({"models":[model("old"),model("keep")],"availableModels":["old","keep"],"clientField":true}).to_string();
            std::fs::write(&path, &original).expect("seed");
            let old = vec!["old".to_string()];
            assert!(change_models(&old, &[model("new"), model("new")], || Ok(())).is_err());
            assert!(change_models(&old, &[model("keep")], || Ok(())).is_err());
            assert_eq!(std::fs::read_to_string(&path).expect("unchanged"), original);
            let failed: Result<(), AppError> =
                change_models(&old, &[model("a"), model("b")], || {
                    Err(AppError::Config("synthetic DB failure".into()))
                });
            assert!(failed.is_err());
            assert_eq!(std::fs::read_to_string(&path).expect("rollback"), original);
            change_models(&old, &[model("a"), model("b")], || Ok(())).expect("batch");
            let written: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("JSON");
            assert_eq!(written["availableModels"], json!(["keep", "a", "b"]));
            assert_eq!(written["clientField"], true);
            for entries in [
                json!([]),
                json!([{}]),
                json!([{"id":"a"},{"id":"a"}]),
                json!([{"id":""}]),
            ] {
                let mut config = model("a");
                config["models"] = entries;
                assert!(configured_models(&config).is_err());
            }
        });
    }

    #[test]
    fn workbuddy_detects_existing_config_before_empty_new_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join(".workbuddy-ai")).expect("mkdir");
        let legacy = dir.path().join(".codebuddy");
        std::fs::create_dir(&legacy).expect("mkdir");
        std::fs::write(legacy.join("models.json"), "{}").expect("seed");
        assert_eq!(detect_dir(dir.path()), legacy);
    }

    #[test]
    fn workbuddy_native_array_round_trip_preserves_other_models() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("models.json");
        let original = serde_json::to_vec(&json!([model("existing")])).expect("JSON");
        std::fs::write(&path, &original).expect("seed");
        edit_at(&path, None, Some(&model("new")), || Ok(())).expect("add array model");
        let written: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("JSON");
        assert!(written.is_array());
        assert_eq!(written[0], model("existing"));
        assert_eq!(written[1], model("new"));
        edit_at(&path, Some("new"), None, || Ok(())).expect("remove array model");
        let restored: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("JSON");
        assert_eq!(restored, json!([model("existing")]));
        assert_eq!(
            std::fs::read(path.with_file_name("models.json.varyai-backup")).expect("backup"),
            original
        );
    }

    #[test]
    fn workbuddy_rejects_corrupt_documents_without_echoing_credentials() {
        for contents in [
            "{\"apiKey\":\"private-test-key\",",
            "42",
            "{\"models\":{}}",
            "{\"availableModels\":42}",
        ] {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = dir.path().join("models.json");
            std::fs::write(&path, contents).expect("seed");
            let error =
                edit_at(&path, None, Some(&model("new")), || Ok(())).expect_err("invalid document");
            assert!(!error.to_string().contains("private-test-key"));
            assert_eq!(std::fs::read_to_string(&path).expect("read"), contents);
        }
    }

    #[test]
    fn workbuddy_preserves_external_edits_when_rollback_would_overwrite_them() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("models.json");
        std::fs::write(&path, "{}").expect("seed");
        let result: Result<(), AppError> = edit_at(&path, None, Some(&model("new")), || {
            std::fs::write(&path, "{\"external\":true}").expect("external edit");
            Err(AppError::Config("database unavailable".into()))
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "{\"external\":true}"
        );
    }

    #[test]
    fn workbuddy_add_preserves_native_models_and_restrictive_filter() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("models.json");
        let original = json!({"models":[model("existing")],"availableModels":["existing","built-in"],"customSetting":{"keep":true}});
        std::fs::write(&path, original.to_string()).expect("seed");
        edit_at(&path, None, Some(&model("new")), || Ok(())).expect("add");
        let actual: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
        assert_eq!(actual["models"][0], original["models"][0]);
        assert_eq!(actual["customSetting"], original["customSetting"]);
        assert_eq!(
            actual["availableModels"],
            json!(["existing", "built-in", "new"])
        );
    }

    #[test]
    fn workbuddy_rename_and_remove_only_touch_target_model() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("models.json");
        std::fs::write(
            &path,
            json!({"models":[model("old"),model("keep")],"availableModels":["old","keep"]})
                .to_string(),
        )
        .expect("seed");
        edit_at(&path, Some("old"), Some(&model("renamed")), || Ok(())).expect("rename");
        edit_at(&path, Some("renamed"), None, || Ok(())).expect("remove");
        let actual: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
        assert_eq!(actual["models"], json!([model("keep")]));
        assert_eq!(actual["availableModels"], json!(["keep"]));
    }

    #[test]
    fn workbuddy_rejects_duplicate_ids_without_rewriting_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("models.json");
        let original = json!({"models":[model("same")]}).to_string();
        std::fs::write(&path, &original).expect("seed");
        assert!(edit_at(&path, None, Some(&model("same")), || Ok(())).is_err());
        assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
    }

    #[test]
    fn workbuddy_rolls_back_file_when_database_commit_fails() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("models.json");
        let original = "{\n  \"models\": [], \"keep\": true\n}";
        std::fs::write(&path, original).expect("seed");
        let result: Result<(), AppError> = edit_at(&path, None, Some(&model("new")), || {
            Err(AppError::Config("database unavailable".into()))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
    }

    #[test]
    fn workbuddy_does_not_create_a_filter_for_unrestricted_lists() {
        for original in [json!({}), json!({"models":[],"availableModels":[]})] {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = dir.path().join("models.json");
            std::fs::write(&path, original.to_string()).expect("seed");
            edit_at(&path, None, Some(&model("new")), || Ok(())).expect("add");
            let actual: Value =
                serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
            assert_eq!(
                actual.get("availableModels"),
                original.get("availableModels")
            );
        }
    }
}
