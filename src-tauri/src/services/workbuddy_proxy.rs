//! Official WorkBuddy routing. Only the model endpoint is owned by Vary AI;
//! authentication and every other setting remain owned by WorkBuddy.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) const ENABLED_KEY: &str = "workbuddy_official_proxy_enabled";
const ENDPOINT_KEY: &str = "CODEBUDDY_BASE_URL";

#[derive(Serialize)]
pub struct SettingsSnapshot {
    pub path: String,
    pub content: String,
    pub revision: String,
}

pub(crate) fn settings_snapshot(directory: &Path) -> Result<SettingsSnapshot, String> {
    use sha2::{Digest, Sha256};
    let path = directory.join("settings.json");
    let original = read_settings(&path)?;
    let revision = serde_json::to_vec(&(&path, &original)).map_err(|_| invalid_settings())?;
    Ok(SettingsSnapshot {
        path: path.to_string_lossy().into_owned(),
        content: original.unwrap_or_else(|| "{}".into()),
        revision: format!("{:x}", Sha256::digest(revision)),
    })
}

pub(crate) fn save_settings(
    directory: &Path,
    revision: &str,
    content: &str,
    backup: Option<&Backup>,
) -> Result<(), String> {
    let snapshot = settings_snapshot(directory)?;
    if snapshot.revision != revision {
        return Err(changed_settings());
    }
    let settings = parse_settings(Some(content))?;
    if let Some(backup) = backup {
        let injected = parse_settings(Some(&backup.injected))?;
        if backup.directory != directory
            || settings.get("env").and_then(|env| env.get(ENDPOINT_KEY))
                != injected.get("env").and_then(|env| env.get(ENDPOINT_KEY))
        {
            return Err(crate::error::AppError::localized(
                "workbuddy.settings.managed_endpoint",
                "路由开启时模型地址由 Vary AI 维护，请先关闭 WorkBuddy 消息路由再修改此地址",
                "Vary AI manages the model endpoint while routing is enabled; turn off WorkBuddy routing before changing it",
            ).to_string());
        }
    }
    let path = directory.join("settings.json");
    if content == snapshot.content && path.exists() {
        return Ok(());
    }
    if path.exists() {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(directory.join("settings.json.varyai-backup")) {
            Ok(mut file) => file
                .write_all(snapshot.content.as_bytes())
                .map_err(|_| changed_settings())?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(changed_settings()),
        }
    }
    if settings_snapshot(directory)?.revision != revision {
        return Err(changed_settings());
    }
    write_settings(&path, content.as_bytes())
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Backup {
    directory: PathBuf,
    original: Option<String>,
    injected: String,
    pub upstream: String,
}

impl Backup {
    pub fn prepare(directory: &Path, local_base: &str) -> Result<Self, String> {
        let original = read_settings(&directory.join("settings.json"))?;
        let mut settings = parse_settings(original.as_deref())?;
        let upstream = match settings.get("env").and_then(|env| env.get(ENDPOINT_KEY)) {
            Some(Value::String(endpoint)) if !endpoint.trim().is_empty() => endpoint.clone(),
            Some(_) => return Err(invalid_settings()),
            None => {
                let product: Value = std::fs::read(directory.join("cache/acc-product-config-v3.json"))
                    .ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
                    .ok_or_else(|| crate::error::AppError::localized(
                        "workbuddy.proxy.product_missing", "请先启动并登录 WorkBuddy，以读取官方服务配置", "Start and sign in to WorkBuddy first to load its official service configuration"
                    ).to_string())?;
                let endpoint = product
                    .get("endpoint")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid_settings)?;
                format!("{}/v2", endpoint.trim_end_matches('/'))
            }
        };
        let target = url::Url::parse(&upstream).map_err(|_| invalid_settings())?;
        let local = url::Url::parse(local_base).map_err(|_| invalid_settings())?;
        if !matches!(target.scheme(), "http" | "https")
            || target.host_str().is_none()
            || !target.username().is_empty()
            || target.password().is_some()
            || target.origin() == local.origin()
        {
            return Err(invalid_settings());
        }
        if settings.get("env").is_none() {
            settings["env"] = json!({});
        }
        settings["env"][ENDPOINT_KEY] = json!(local_base);
        let injected = serde_json::to_string_pretty(&settings).map_err(|_| invalid_settings())?;
        Ok(Self {
            directory: directory.to_path_buf(),
            original,
            injected,
            upstream,
        })
    }

    pub(crate) fn model_url(&self, provider_id: &str) -> Result<String, String> {
        let settings = parse_settings(Some(&self.injected))?;
        let base = settings["env"][ENDPOINT_KEY]
            .as_str()
            .ok_or_else(invalid_settings)?;
        let mut url = url::Url::parse(base).map_err(|_| invalid_settings())?;
        url.path_segments_mut()
            .map_err(|_| invalid_settings())?
            .clear()
            .extend(["workbuddy", "providers", provider_id, "chat", "completions"]);
        Ok(url.into())
    }

    pub(crate) fn project_models(
        &self,
        db: &crate::database::Database,
        enabled: bool,
    ) -> Result<(), String> {
        let providers = db
            .get_all_providers("workbuddy")
            .map_err(|e| e.to_string())?;
        let mut urls = Vec::new();
        for provider in providers.values().filter(|p| {
            p.category.as_deref() != Some("official")
                && p.meta.as_ref().and_then(|meta| meta.live_config_managed) != Some(false)
        }) {
            for model in crate::workbuddy_config::expanded_models(&provider.settings_config)
                .map_err(|e| e.to_string())?
            {
                if let (Some(id), Some(upstream)) = (model["id"].as_str(), model["url"].as_str()) {
                    urls.push((
                        id.to_string(),
                        upstream.to_string(),
                        self.model_url(&provider.id)?,
                    ));
                }
            }
        }
        crate::workbuddy_config::rewrite_model_urls(&self.directory, |model| {
            let (id, url) = (model["id"].as_str()?, model["url"].as_str()?);
            let (_, upstream, local) = urls.iter().find(|(managed, _, _)| managed == id)?;
            if enabled && url == upstream {
                Some(local.clone())
            } else if !enabled && url == local {
                Some(upstream.clone())
            } else {
                None
            }
        })
        .map_err(|e| e.to_string())
    }

    pub fn apply(&self) -> Result<(), String> {
        let path = self.directory.join("settings.json");
        if read_settings(&path)? != self.original {
            return Err(changed_settings());
        }
        write_settings(&path, self.injected.as_bytes())
    }

    pub fn restore(&self) -> Result<(), String> {
        let path = self.directory.join("settings.json");
        let Some(current) = read_settings(&path)? else {
            return Ok(());
        };
        let replacement = if current == self.injected {
            self.original.clone()
        } else {
            let mut settings = parse_settings(Some(&current))?;
            let injected = parse_settings(Some(&self.injected))?;
            if settings.get("env").and_then(|env| env.get(ENDPOINT_KEY))
                != injected.get("env").and_then(|env| env.get(ENDPOINT_KEY))
            {
                return Ok(()); // The user now owns a different endpoint.
            }
            let original = parse_settings(self.original.as_deref())?;
            let env = settings
                .get_mut("env")
                .and_then(Value::as_object_mut)
                .ok_or_else(invalid_settings)?;
            if let Some(endpoint) = original.get("env").and_then(|env| env.get(ENDPOINT_KEY)) {
                env.insert(ENDPOINT_KEY.into(), endpoint.clone());
            } else {
                env.remove(ENDPOINT_KEY);
            }
            if env.is_empty() && original.get("env").is_none() {
                if let Some(root) = settings.as_object_mut() {
                    root.remove("env");
                }
            }
            Some(serde_json::to_string_pretty(&settings).map_err(|_| invalid_settings())?)
        };
        if read_settings(&path)?.as_deref() != Some(current.as_str()) {
            return Err(changed_settings());
        }
        match replacement {
            Some(contents) => write_settings(&path, contents.as_bytes()),
            None => std::fs::remove_file(&path).map_err(|_| changed_settings()),
        }
    }
}

fn read_settings(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(invalid_settings()),
    }
}

fn parse_settings(contents: Option<&str>) -> Result<Value, String> {
    let settings: Value =
        serde_json::from_str(contents.unwrap_or("{}")).map_err(|_| invalid_settings())?;
    if !settings.is_object() || settings.get("env").is_some_and(|env| !env.is_object()) {
        return Err(invalid_settings());
    }
    Ok(settings)
}

fn write_settings(path: &Path, contents: &[u8]) -> Result<(), String> {
    crate::config::atomic_write(path, contents).map_err(|_| changed_settings())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| changed_settings())?;
    }
    Ok(())
}

fn invalid_settings() -> String {
    crate::error::AppError::localized("workbuddy.proxy.invalid", "WorkBuddy 配置或上游地址无效，原配置未修改", "Invalid WorkBuddy configuration or upstream endpoint; the original configuration was not changed").to_string()
}

fn changed_settings() -> String {
    crate::error::AppError::localized(
        "workbuddy.proxy.changed",
        "WorkBuddy 配置读写失败或已被修改，请刷新后重试",
        "WorkBuddy configuration could not be written or changed externally; refresh and retry",
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(settings: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temporary WorkBuddy directory");
        std::fs::create_dir(dir.path().join("cache")).expect("cache");
        std::fs::write(
            dir.path().join("cache/acc-product-config-v3.json"),
            r#"{"endpoint":"https://www.workbuddy.cn"}"#,
        )
        .expect("product configuration");
        if let Some(settings) = settings {
            std::fs::write(dir.path().join("settings.json"), settings).expect("settings");
        }
        dir
    }

    #[test]
    fn custom_model_projection_preserves_array_root_and_externally_changed_url() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"env":{"CODEBUDDY_BASE_URL":"https://official.example/v2"}}"#,
        )
        .expect("settings");
        let model = json!({"id":"custom","url":"https://api.example/v1/chat/completions","apiKey":"synthetic-key"});
        let db = crate::database::Database::memory().expect("db");
        let provider = crate::provider::Provider::with_id(
            "custom / api".into(),
            "API".into(),
            model.clone(),
            None,
        );
        db.save_provider("workbuddy", &provider).expect("provider");
        let backup =
            Backup::prepare(dir.path(), "http://127.0.0.1:19999/workbuddy/v1").expect("backup");
        let path = dir.path().join("models.json");
        std::fs::write(&path, json!([model]).to_string()).expect("models");
        backup.project_models(&db, true).expect("project");
        let mut native: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("JSON");
        assert!(native.is_array());
        assert!(native[0]["url"]
            .as_str()
            .expect("url")
            .contains("custom%20%2F%20api"));
        native[0]["url"] = json!("https://user-changed.example/chat/completions");
        std::fs::write(&path, native.to_string()).expect("external edit");
        backup.project_models(&db, false).expect("restore");
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).expect("read")).expect("JSON"),
            native
        );
    }

    #[test]
    fn official_routing_preserves_login_and_restores_exact_settings() {
        let original = "{\n  \"env\": {\"OTHER\": \"keep\"}, \"sandbox\": true\n}";
        let dir = fixture(Some(original));
        let auth = dir.path().join("credentials.json");
        std::fs::write(&auth, "login-owned-by-workbuddy").expect("login");
        let backup =
            Backup::prepare(dir.path(), "http://127.0.0.1:12345/workbuddy/v1").expect("prepare");
        assert_eq!(backup.upstream, "https://www.workbuddy.cn/v2");
        backup.apply().expect("takeover");
        let settings: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("settings.json")).expect("read"))
                .expect("JSON");
        assert_eq!(
            settings["env"][ENDPOINT_KEY],
            "http://127.0.0.1:12345/workbuddy/v1"
        );
        assert_eq!(settings["env"]["OTHER"], "keep");
        assert_eq!(settings["sandbox"], true);
        assert_eq!(
            std::fs::read_to_string(auth).expect("login"),
            "login-owned-by-workbuddy"
        );
        backup.restore().expect("restore");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("settings.json")).expect("read"),
            original
        );
    }

    #[test]
    fn restores_previous_endpoint_without_losing_settings_changed_by_workbuddy() {
        let dir = fixture(Some(
            r#"{"env":{"CODEBUDDY_BASE_URL":"https://gateway.example/v3","OTHER":"keep"}}"#,
        ));
        let backup =
            Backup::prepare(dir.path(), "http://127.0.0.1:12345/workbuddy/v1").expect("prepare");
        assert_eq!(backup.upstream, "https://gateway.example/v3");
        backup.apply().expect("apply");
        let mut settings: Value = serde_json::from_str(&backup.injected).expect("injected JSON");
        settings["appearance"] = json!("dark");
        std::fs::write(dir.path().join("settings.json"), settings.to_string())
            .expect("external setting");
        backup.restore().expect("restore");
        let restored: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("settings.json")).expect("read"))
                .expect("JSON");
        assert_eq!(restored["appearance"], "dark");
        assert_eq!(restored["env"][ENDPOINT_KEY], "https://gateway.example/v3");
    }

    #[test]
    fn preserves_an_endpoint_changed_externally_and_uses_original_directory() {
        let dir = fixture(Some("{}"));
        let backup =
            Backup::prepare(dir.path(), "http://127.0.0.1:12345/workbuddy/v1").expect("prepare");
        backup.apply().expect("apply");
        let external = r#"{"env":{"CODEBUDDY_BASE_URL":"https://new.example/v1"}}"#;
        std::fs::write(dir.path().join("settings.json"), external).expect("external edit");
        backup.restore().expect("restore");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("settings.json")).expect("read"),
            external
        );
    }

    #[test]
    fn missing_settings_are_removed_on_restore_and_bad_env_is_not_modified() {
        let dir = fixture(None);
        let backup =
            Backup::prepare(dir.path(), "http://127.0.0.1:12345/workbuddy/v1").expect("prepare");
        backup.apply().expect("apply");
        backup.restore().expect("restore");
        assert!(!dir.path().join("settings.json").exists());
        for invalid in ["[]", r#"{"env":42}"#, "not json"] {
            std::fs::write(dir.path().join("settings.json"), invalid).expect("invalid settings");
            assert!(Backup::prepare(dir.path(), "http://127.0.0.1:12345/workbuddy/v1").is_err());
            assert_eq!(
                std::fs::read_to_string(dir.path().join("settings.json")).expect("read"),
                invalid
            );
        }
    }

    #[test]
    fn refuses_to_overwrite_settings_changed_after_prepare() {
        let dir = fixture(Some("{}"));
        let backup =
            Backup::prepare(dir.path(), "http://127.0.0.1:12345/workbuddy/v1").expect("prepare");
        std::fs::write(dir.path().join("settings.json"), r#"{"new":true}"#).expect("external edit");
        assert!(backup.apply().is_err());
    }

    #[test]
    fn settings_editor_writes_the_real_file_and_preserves_edits_on_route_restore() {
        let dir = fixture(Some(r#"{"env":{"OTHER":"keep"},"theme":"light"}"#));
        let backup =
            Backup::prepare(dir.path(), "http://127.0.0.1:12345/workbuddy/v1").expect("prepare");
        backup.apply().expect("apply");
        let snapshot = settings_snapshot(dir.path()).expect("snapshot");
        assert_eq!(
            snapshot.path,
            dir.path().join("settings.json").to_string_lossy()
        );
        let mut edited: Value = serde_json::from_str(&snapshot.content).expect("JSON");
        edited["theme"] = json!("dark");
        save_settings(
            dir.path(),
            &snapshot.revision,
            &edited.to_string(),
            Some(&backup),
        )
        .expect("save real file");
        let live = std::fs::read_to_string(dir.path().join("settings.json")).expect("live");
        assert_eq!(serde_json::from_str::<Value>(&live).expect("JSON"), edited);
        assert!(save_settings(dir.path(), &snapshot.revision, "{}", Some(&backup)).is_err());
        backup.restore().expect("restore");
        let restored: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).expect("restored"),
        )
        .expect("JSON");
        assert_eq!(restored["theme"], "dark");
        assert_eq!(restored["env"], json!({"OTHER":"keep"}));
    }

    #[test]
    fn settings_editor_rejects_invalid_json_stale_files_and_changes_to_the_managed_endpoint() {
        let dir = fixture(Some("{}"));
        let backup =
            Backup::prepare(dir.path(), "http://127.0.0.1:12345/workbuddy/v1").expect("prepare");
        backup.apply().expect("apply");
        let snapshot = settings_snapshot(dir.path()).expect("snapshot");
        for content in ["{", "[]", r#"{"env":42}"#, "{}"] {
            assert!(save_settings(dir.path(), &snapshot.revision, content, Some(&backup)).is_err());
            assert_eq!(
                std::fs::read_to_string(dir.path().join("settings.json")).expect("live"),
                snapshot.content
            );
        }
        std::fs::write(dir.path().join("settings.json"), r#"{"changed":true}"#)
            .expect("external edit");
        assert!(save_settings(
            dir.path(),
            &snapshot.revision,
            &snapshot.content,
            Some(&backup)
        )
        .is_err());
        let other = fixture(Some("{}"));
        assert!(save_settings(other.path(), &snapshot.revision, "{}", None).is_err());
    }

    #[test]
    fn settings_editor_creates_missing_settings_and_can_edit_endpoints_without_takeover() {
        let dir = fixture(None);
        let snapshot = settings_snapshot(dir.path()).expect("missing snapshot");
        let content =
            r#"{"env":{"CODEBUDDY_BASE_URL":"https://example.com/v1"},"other":{"keep":true}}"#;
        save_settings(dir.path(), &snapshot.revision, content, None).expect("save");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("settings.json")).expect("read"),
            content
        );
    }
}
