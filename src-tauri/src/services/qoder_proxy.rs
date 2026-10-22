//! Qoder takeover owns only its endpoint assignment and managed provider URLs.
use crate::{database::Database, qoder_config as native};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
pub(crate) const ENABLED_KEY: &str = "qoder_proxy_enabled";
pub(crate) const CERTIFICATE_CLEARED_KEY: &str = "qoder_certificate_manually_cleared";
const CERTIFICATE_KEY: &str = "qoder_saved_certificate";
const ENDPOINT: &str = "QODERCN_SERVER_ENDPOINT";
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Backup {
    pub directory: PathBuf,
    original_env: Option<String>,
    injected_env: String,
    pub custom_origin: String,
    pub upstream: String,
    #[serde(default)]
    pub certificate_dir: Option<PathBuf>,
    #[serde(default)]
    pub certificate_sha1: Option<String>,
    #[serde(default)]
    pub certificate_pem: Option<String>,
}
fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(native::invalid().to_string()),
    }
}
fn assignment(line: &str) -> Option<&str> {
    let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
    let (k, v) = line.split_once('=')?;
    (k.trim() == ENDPOINT).then_some(v.trim())
}
fn endpoint_line(text: &str) -> Result<Option<&str>, String> {
    let mut lines = text
        .split_inclusive('\n')
        .filter(|l| assignment(l).is_some());
    let first = lines.next();
    if lines.next().is_some() {
        return Err(native::invalid().to_string());
    }
    Ok(first)
}
impl Backup {
    pub fn prepare(
        directory: &Path,
        custom_origin: &str,
        official_origin: &str,
    ) -> Result<Self, String> {
        let original_env = read(&directory.join(".env"))?;
        let text = original_env.as_deref().unwrap_or("");
        let original_line = endpoint_line(text)?;
        let upstream = original_line
            .and_then(assignment)
            .map(|v| v.trim_matches(['\'', '"']).to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "https://gateway.qoder.com.cn".into());
        let target = url::Url::parse(&upstream).map_err(|_| native::invalid().to_string())?;
        let local = url::Url::parse(official_origin).map_err(|_| native::invalid().to_string())?;
        if target.scheme() != "https"
            || target.host_str().is_none()
            || !target.username().is_empty()
            || target.password().is_some()
            || target.path() != "/"
            || target.query().is_some()
            || target.fragment().is_some()
            || matches!(target.host_str(), Some("localhost" | "127.0.0.1"))
        {
            return Err(native::invalid().to_string());
        }
        if local.scheme() != "https" || local.host_str() != Some("localhost") || local.path() != "/"
        {
            return Err(native::invalid().to_string());
        }
        let line = format!("{ENDPOINT}={official_origin}\n");
        let injected_env = match original_line {
            Some(old) => text.replacen(old, &line, 1),
            None => format!(
                "{text}{}{line}",
                if text.is_empty() || text.ends_with('\n') {
                    ""
                } else {
                    "\n"
                }
            ),
        };
        Ok(Self {
            directory: directory.into(),
            original_env,
            injected_env,
            custom_origin: custom_origin.into(),
            upstream,
            certificate_dir: None,
            certificate_sha1: None,
            certificate_pem: None,
        })
    }
    pub fn apply(&self) -> Result<(), String> {
        let path = self.directory.join(".env");
        if read(&path)? != self.original_env {
            return Err(native::invalid().to_string());
        }
        crate::config::atomic_write_private(&path, self.injected_env.as_bytes())
            .map_err(|e| e.to_string())
    }
    pub fn restore(&self) -> Result<(), String> {
        let path = self.directory.join(".env");
        let Some(current) = read(&path)? else {
            return Ok(());
        };
        let replacement = if current == self.injected_env {
            self.original_env.clone()
        } else {
            let owned =
                endpoint_line(&self.injected_env)?.ok_or_else(|| native::invalid().to_string())?;
            if endpoint_line(&current)?.and_then(assignment) != assignment(owned) {
                return Ok(());
            }
            let current_line =
                endpoint_line(&current)?.ok_or_else(|| native::invalid().to_string())?;
            Some(current.replacen(
                current_line,
                endpoint_line(self.original_env.as_deref().unwrap_or(""))?.unwrap_or(""),
                1,
            ))
        };
        if read(&path)?.as_deref() != Some(&current) {
            return Err(native::invalid().to_string());
        }
        match replacement {
            Some(text) => crate::config::atomic_write_private(&path, text.as_bytes())
                .map_err(|e| e.to_string()),
            None => std::fs::remove_file(path).map_err(|_| native::invalid().to_string()),
        }
    }
    pub fn project_models(&self, db: &Database, enabled: bool) -> Result<(), String> {
        let path = self.directory.join("settings.json");
        let providers = db.get_all_providers("qoder").map_err(|e| e.to_string())?;
        for p in providers.values().filter(|p| {
            p.category.as_deref() != Some("official")
                && p.meta.as_ref().and_then(|m| m.live_config_managed) != Some(false)
        }) {
            let key = native::native_id(&p.id);
            let doc = native::read_at(&path).map_err(|e| e.to_string())?.0;
            let Some(old) = doc["providers"].get(&key) else {
                continue;
            };
            let local = super::provider::qoder::model_url(&self.custom_origin, &p.id)
                .map_err(|e| e.to_string())?;
            let mut new = old.clone();
            if enabled && old["baseUrl"] == p.settings_config["baseUrl"] {
                new["baseUrl"] = json!(local);
                if old["apiKey"] == p.settings_config["apiKey"] {
                    new["apiKey"] = json!("varyai-local");
                }
            } else if !enabled {
                if old["baseUrl"] == local {
                    new["baseUrl"] = p.settings_config["baseUrl"].clone();
                }
                if old["apiKey"] == "varyai-local" {
                    new["apiKey"] = p.settings_config["apiKey"].clone();
                }
            }
            if new != *old {
                native::change_provider_at(&path, &key, Some(old), Some(&new), || Ok(()))
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

pub(crate) struct Manager {
    db: std::sync::Arc<Database>,
    server: tokio::sync::Mutex<Option<crate::proxy::qoder_tls::Server>>,
    active: std::sync::atomic::AtomicBool,
    valid_until: std::sync::atomic::AtomicI64,
    app_handle: std::sync::OnceLock<tauri::AppHandle>,
}
#[derive(Serialize)]
pub struct CertificateStatus {
    pub expires_at: Option<i64>,
    pub expired: bool,
    pub active: bool,
    pub fingerprint: Option<String>,
}
impl Manager {
    pub fn new(db: std::sync::Arc<Database>) -> Self {
        Self {
            db,
            server: tokio::sync::Mutex::new(None),
            active: std::sync::atomic::AtomicBool::new(false),
            valid_until: std::sync::atomic::AtomicI64::new(0),
            app_handle: std::sync::OnceLock::new(),
        }
    }
    pub fn set_app_handle(&self, app: tauri::AppHandle) {
        let _ = self.app_handle.set(app);
    }
    pub fn is_active(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Acquire)
            && self.valid_until.load(std::sync::atomic::Ordering::Acquire)
                > chrono::Utc::now().timestamp()
    }
    fn saved_certificate(&self) -> Result<Option<super::qoder_certificate::Certificate>, String> {
        self.db
            .get_setting(CERTIFICATE_KEY)
            .map_err(|e| e.to_string())?
            .filter(|s| !s.is_empty())
            .map(|s| {
                serde_json::from_str(&s)
                    .map_err(|_| "Qoder saved certificate is invalid".to_string())
            })
            .transpose()
    }
    fn save_certificate(
        &self,
        certificate: &super::qoder_certificate::Certificate,
    ) -> Result<(), String> {
        self.db
            .set_setting(
                CERTIFICATE_KEY,
                &serde_json::to_string(certificate).map_err(|_| native::invalid().to_string())?,
            )
            .map_err(|e| e.to_string())
    }
    pub fn certificate_status(&self) -> CertificateStatus {
        let saved = self.saved_certificate().ok().flatten();
        let expires_at = saved.as_ref().map(|c| c.expires_at).or_else(|| {
            self.active
                .load(std::sync::atomic::Ordering::Acquire)
                .then(|| self.valid_until.load(std::sync::atomic::Ordering::Acquire))
        });
        CertificateStatus {
            expires_at,
            expired: expires_at.is_some_and(|end| chrono::Utc::now().timestamp() >= end),
            active: self.is_active(),
            fingerprint: saved.map(|c| c.sha1),
        }
    }
    fn ensure_certificate_creation_allowed(&self) -> Result<(), String> {
        if self
            .db
            .get_bool_flag(CERTIFICATE_CLEARED_KEY)
            .map_err(|e| e.to_string())?
        {
            return Err("Qoder certificate was manually cleared. Use Create / Rebuild Certificate to enable protection again.".into());
        }
        Ok(())
    }
    pub fn allow_certificate_creation(&self) -> Result<(), String> {
        self.db
            .set_setting(CERTIFICATE_CLEARED_KEY, "false")
            .map_err(|e| e.to_string())
    }
    pub async fn enable(&self, custom_origin: &str) -> Result<(), String> {
        let mut server = self.server.lock().await;
        self.ensure_certificate_creation_allowed()?;
        if self.is_active() {
            return Ok(());
        }
        if server.is_some()
            && self.valid_until.load(std::sync::atomic::Ordering::Acquire)
                > chrono::Utc::now().timestamp()
        {
            if let Some(saved) = self
                .db
                .get_live_backup("qoder")
                .await
                .map_err(|e| e.to_string())?
            {
                let backup: Backup = serde_json::from_str(&saved.original_config)
                    .map_err(|_| "Qoder recovery backup is invalid".to_string())?;
                if backup.custom_origin == custom_origin {
                    let result = (|| {
                        backup.apply()?;
                        backup.project_models(&self.db, true)?;
                        self.db
                            .set_setting(ENABLED_KEY, "true")
                            .map_err(|e| e.to_string())
                    })();
                    if let Err(error) = result {
                        backup.restore()?;
                        backup.project_models(&self.db, false)?;
                        return Err(error);
                    }
                    self.active
                        .store(true, std::sync::atomic::Ordering::Release);
                    return Ok(());
                }
            }
        }
        self.restore_inner(&mut server).await?;
        crate::proxy::qoder_codec::check()
            .await
            .map_err(|e| e.to_string())?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| "Cannot allocate Qoder TLS port".to_string())?;
        let port = listener
            .local_addr()
            .map_err(|_| "Cannot read Qoder TLS port".to_string())?
            .port();
        let certificate = self.obtain_certificate().await?;
        let prepared = Backup::prepare(
            &native::directory(),
            custom_origin,
            &format!("https://localhost:{port}"),
        );
        let mut backup = match prepared {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        backup.certificate_dir = Some(certificate.directory.clone());
        backup.certificate_sha1 = Some(certificate.sha1.clone());
        backup.certificate_pem = Some(certificate.pem.clone());
        let encoded = serde_json::to_string(&backup).map_err(|_| native::invalid().to_string())?;
        if self.db.save_live_backup("qoder", &encoded).await.is_err() {
            return Err("Cannot save Qoder recovery backup".into());
        }
        let result = async {
            if !super::qoder_certificate::trusted(&certificate).await? {
                super::qoder_certificate::trust(&certificate.directory, self.app_handle.get())
                    .await?;
            }
            *server = Some(
                crate::proxy::qoder_tls::Server::start(
                    listener,
                    &certificate.directory,
                    crate::proxy::qoder::OfficialState {
                        db: self.db.clone(),
                        upstream: backup.upstream.clone(),
                    },
                )
                .await?,
            );
            backup.apply()?;
            backup.project_models(&self.db, true)?;
            self.db
                .set_setting(ENABLED_KEY, "true")
                .map_err(|e| e.to_string())?;
            self.valid_until
                .store(certificate.expires_at, std::sync::atomic::Ordering::Release);
            self.active
                .store(true, std::sync::atomic::Ordering::Release);
            Ok(())
        }
        .await;
        if let Err(error) = result {
            self.restore_inner(&mut server).await?;
            return Err(error);
        }
        Ok(())
    }
    pub async fn restore(&self) -> Result<(), String> {
        let mut server = self.server.lock().await;
        self.restore_inner(&mut server).await
    }
    async fn obtain_certificate(&self) -> Result<super::qoder_certificate::Certificate, String> {
        self.ensure_certificate_creation_allowed()?;
        if let Some(saved) = self.saved_certificate()? {
            let root = crate::config::get_app_config_dir().join("qoder-certificates");
            if saved.directory.parent() != Some(root.as_path()) {
                return Err("Qoder saved certificate directory is invalid".into());
            }
            let directory = saved.directory.clone();
            let inspected =
                tokio::task::spawn_blocking(move || super::qoder_certificate::inspect(&directory))
                    .await
                    .map_err(|_| "Cannot inspect Qoder certificate".to_string())?;
            if let Ok(certificate) = inspected {
                if certificate.sha1 == saved.sha1
                    && certificate.expires_at > chrono::Utc::now().timestamp()
                {
                    return Ok(certificate);
                }
            }
            self.remove_saved_certificate().await?;
        }
        let certificate = super::qoder_certificate::create().await?;
        self.save_certificate(&certificate)?;
        Ok(certificate)
    }
    async fn remove_saved_certificate(&self) -> Result<(), String> {
        if let Some(certificate) = self.saved_certificate()? {
            super::qoder_certificate::remove(
                &certificate.directory,
                &certificate.sha1,
                &certificate.pem,
                self.app_handle.get(),
            )
            .await?;
            self.db
                .set_setting(CERTIFICATE_KEY, "")
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub async fn clear_certificate(&self) -> Result<(), String> {
        let mut server = self.server.lock().await;
        self.db
            .set_setting(CERTIFICATE_CLEARED_KEY, "true")
            .map_err(|e| e.to_string())?;
        self.db
            .set_setting(ENABLED_KEY, "false")
            .map_err(|e| e.to_string())?;
        self.restore_inner(&mut server).await?;
        self.remove_saved_certificate().await
    }
    /// Restore client routing now; keep the certificate until explicitly cleared.
    pub async fn suspend(&self) -> Result<(), String> {
        let _server = self.server.lock().await;
        self.active
            .store(false, std::sync::atomic::Ordering::Release);
        if let Some(saved) = self
            .db
            .get_live_backup("qoder")
            .await
            .map_err(|e| e.to_string())?
        {
            let backup: Backup = serde_json::from_str(&saved.original_config)
                .map_err(|_| "Qoder recovery backup is invalid".to_string())?;
            backup.restore()?;
            backup.project_models(&self.db, false)?;
        }
        Ok(())
    }
    async fn restore_inner(
        &self,
        server: &mut Option<crate::proxy::qoder_tls::Server>,
    ) -> Result<(), String> {
        self.active
            .store(false, std::sync::atomic::Ordering::Release);
        if let Some(saved) = self
            .db
            .get_live_backup("qoder")
            .await
            .map_err(|e| e.to_string())?
        {
            let backup: Backup = serde_json::from_str(&saved.original_config)
                .map_err(|_| "Qoder recovery backup is invalid".to_string())?;
            // Restore network settings before any keychain dialog can delay shutdown.
            let env = backup.restore();
            let models = backup.project_models(&self.db, false);
            if let Some(server) = server.take() {
                server.stop().await;
            }
            env?;
            models?;
            // Migrate the previous session's certificate before removing its route backup.
            if self.saved_certificate()?.is_none() {
                if let Some(directory) = backup.certificate_dir {
                    let certificate = tokio::task::spawn_blocking(move || {
                        super::qoder_certificate::inspect(&directory)
                    })
                    .await
                    .map_err(|_| "Cannot inspect Qoder certificate".to_string())??;
                    self.save_certificate(&certificate)?;
                }
            }
            self.db
                .delete_live_backup("qoder")
                .await
                .map_err(|e| e.to_string())?;
        } else if let Some(server) = server.take() {
            server.stop().await;
        }
        Ok(())
    }
}
#[derive(Serialize)]
pub struct SettingsSnapshot {
    pub path: String,
    pub content: String,
    pub revision: String,
}
pub(crate) fn settings_snapshot() -> Result<SettingsSnapshot, String> {
    use sha2::{Digest, Sha256};
    let path = native::path();
    let (_, bytes) = native::read_at(&path).map_err(|e| e.to_string())?;
    let hash = serde_json::to_vec(&(&path, &bytes)).map_err(|_| native::invalid().to_string())?;
    Ok(SettingsSnapshot {
        path: path.to_string_lossy().into_owned(),
        content: bytes
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| native::invalid().to_string())?
            .unwrap_or_else(|| "{}".into()),
        revision: format!("{:x}", Sha256::digest(hash)),
    })
}
pub(crate) fn save_settings(
    db: &Database,
    revision: &str,
    content: &str,
    active: bool,
) -> Result<(), String> {
    let snapshot = settings_snapshot()?;
    if snapshot.revision != revision {
        return Err(native::invalid().to_string());
    }
    let document: Value =
        serde_json::from_str(content).map_err(|_| native::invalid().to_string())?;
    if !document.is_object() || document.get("providers").is_some_and(|v| !v.is_object()) {
        return Err(native::invalid().to_string());
    }
    if active {
        let current: Value =
            serde_json::from_str(&snapshot.content).map_err(|_| native::invalid().to_string())?;
        for p in db
            .get_all_providers("qoder")
            .map_err(|e| e.to_string())?
            .values()
            .filter(|p| p.category.as_deref() != Some("official"))
        {
            let key = native::native_id(&p.id);
            if let Some(old) = current["providers"].get(&key) {
                for field in ["baseUrl", "apiKey"] {
                    if document["providers"][&key][field] != old[field] {
                        return Err("路由开启时模型地址与凭据由 Vary AI 维护，请先关闭 Qoder 接管 / Disable Qoder routing before editing managed endpoints or credentials".into());
                    }
                }
            }
        }
    }
    if settings_snapshot()?.revision != revision {
        return Err(native::invalid().to_string());
    }
    crate::config::atomic_write_private(&native::path(), content.as_bytes())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_time_certificate_creation_is_allowed() {
        let manager = Manager::new(std::sync::Arc::new(Database::memory().expect("db")));
        assert!(manager.ensure_certificate_creation_allowed().is_ok());
    }
    #[tokio::test]
    async fn manually_cleared_certificate_blocks_implicit_enable_across_managers() {
        let db = std::sync::Arc::new(Database::memory().expect("db"));
        Manager::new(db.clone())
            .clear_certificate()
            .await
            .expect("clear");
        let next = Manager::new(db.clone());
        // Even the active-route shortcut must not override an explicit clear.
        next.active
            .store(true, std::sync::atomic::Ordering::Release);
        next.valid_until.store(
            chrono::Utc::now().timestamp() + 60,
            std::sync::atomic::Ordering::Release,
        );
        assert!(next.enable("http://127.0.0.1:15721").await.is_err());
        assert!(next.saved_certificate().expect("saved").is_none());
        assert!(next.obtain_certificate().await.is_err());
        next.allow_certificate_creation().expect("explicit rebuild");
        assert!(Manager::new(db.clone())
            .ensure_certificate_creation_allowed()
            .is_ok());
        assert_eq!(
            db.get_setting(ENABLED_KEY).expect("enabled").as_deref(),
            Some("false")
        );
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn exit_restores_client_but_keeps_certificate_for_a_new_manager() {
        let dir = tempfile::tempdir().expect("directory");
        let db = std::sync::Arc::new(Database::memory().expect("db"));
        let certificate = super::super::qoder_certificate::create_at(dir.path().join("cert"))
            .expect("certificate");
        let mut backup = Backup::prepare(
            dir.path(),
            "http://127.0.0.1:15721",
            "https://localhost:23456",
        )
        .expect("backup");
        backup.certificate_dir = Some(certificate.directory.clone());
        backup.certificate_sha1 = Some(certificate.sha1.clone());
        backup.certificate_pem = Some(certificate.pem.clone());
        backup.apply().expect("apply");
        db.save_live_backup("qoder", &serde_json::to_string(&backup).expect("json"))
            .await
            .expect("saved");
        Manager::new(db.clone())
            .restore()
            .await
            .expect("restore without keychain changes");
        assert!(!dir.path().join(".env").exists());
        assert!(certificate.directory.join("localhost-key.pem").exists());
        assert!(db.get_live_backup("qoder").await.expect("read").is_none());
        let next = Manager::new(db);
        assert_eq!(
            next.certificate_status().expires_at,
            Some(certificate.expires_at)
        );
        assert!(
            !next.is_active(),
            "saved certificate does not mean routing is active"
        );
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn temporarily_disabled_route_reuses_certificate_listener_and_restores_config() {
        let dir = tempfile::tempdir().expect("directory");
        let db = std::sync::Arc::new(Database::memory().expect("db"));
        let certificate = super::super::qoder_certificate::create_at(dir.path().join("cert"))
            .expect("certificate");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let origin = "http://127.0.0.1:15721";
        let backup = Backup::prepare(
            dir.path(),
            origin,
            &format!("https://localhost:{}", address.port()),
        )
        .expect("backup");
        backup.apply().expect("initial config");
        db.save_live_backup("qoder", &serde_json::to_string(&backup).expect("json"))
            .await
            .expect("saved");
        db.set_setting(ENABLED_KEY, "true").expect("enabled");
        let manager = Manager::new(db.clone());
        *manager.server.lock().await = Some(
            crate::proxy::qoder_tls::Server::start(
                listener,
                &certificate.directory,
                crate::proxy::qoder::OfficialState {
                    db: db.clone(),
                    upstream: "https://example.invalid".into(),
                },
            )
            .await
            .expect("TLS listener"),
        );
        manager
            .valid_until
            .store(certificate.expires_at, std::sync::atomic::Ordering::Release);
        manager
            .active
            .store(true, std::sync::atomic::Ordering::Release);
        db.set_setting(ENABLED_KEY, "false").expect("disabled");
        manager.suspend().await.expect("suspend");
        assert!(!manager.is_active());
        assert!(!dir.path().join(".env").exists());
        assert!(db.get_live_backup("qoder").await.expect("read").is_some());
        let external_env = "QODERCN_SERVER_ENDPOINT=https://user.example\n";
        std::fs::write(dir.path().join(".env"), external_env).expect("external edit");
        assert!(manager.enable(origin).await.is_err());
        assert!(!manager.is_active());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".env")).expect("preserved"),
            external_env
        );
        std::fs::remove_file(dir.path().join(".env")).expect("remove test edit");
        manager
            .enable(origin)
            .await
            .expect("resume without trust operation");
        assert!(manager.is_active());
        assert!(db.get_bool_flag(ENABLED_KEY).expect("flag"));
        assert_eq!(
            manager.certificate_status().expires_at,
            Some(certificate.expires_at)
        );
        assert!(std::fs::read_to_string(dir.path().join(".env"))
            .expect("config")
            .contains(&address.port().to_string()));
        assert!(
            tokio::net::TcpListener::bind(address).await.is_err(),
            "same listener still owns its port"
        );
        manager
            .server
            .lock()
            .await
            .take()
            .expect("server")
            .stop()
            .await;
    }
    #[tokio::test]
    async fn expired_active_certificate_must_enter_recovery_instead_of_reusing() {
        let db = std::sync::Arc::new(Database::memory().expect("db"));
        db.save_live_backup("qoder", "invalid recovery snapshot")
            .await
            .expect("backup");
        let manager = Manager::new(db);
        manager.valid_until.store(
            chrono::Utc::now().timestamp() - 1,
            std::sync::atomic::Ordering::Release,
        );
        manager
            .active
            .store(true, std::sync::atomic::Ordering::Release);
        assert!(manager.enable("http://127.0.0.1:15721").await.is_err());
        assert!(!manager.is_active());
    }
    #[tokio::test]
    async fn repeated_enable_preserves_active_certificate_and_recovery_backup() {
        let db = std::sync::Arc::new(Database::memory().expect("db"));
        db.save_live_backup("qoder", "untouched recovery snapshot")
            .await
            .expect("backup");
        let manager = Manager::new(db.clone());
        let deadline = chrono::Utc::now().timestamp() + 60;
        manager
            .valid_until
            .store(deadline, std::sync::atomic::Ordering::Release);
        manager
            .active
            .store(true, std::sync::atomic::Ordering::Release);
        manager
            .enable("http://127.0.0.1:15721")
            .await
            .expect("reuse");
        assert!(manager.is_active());
        assert_eq!(manager.certificate_status().expires_at, Some(deadline));
        assert_eq!(
            db.get_live_backup("qoder")
                .await
                .expect("read")
                .expect("saved")
                .original_config,
            "untouched recovery snapshot"
        );
    }
    #[test]
    fn expired_certificate_is_reported_without_claiming_protection() {
        let manager = Manager::new(std::sync::Arc::new(Database::memory().expect("db")));
        assert!(manager.certificate_status().expires_at.is_none());
        manager.valid_until.store(
            chrono::Utc::now().timestamp() - 1,
            std::sync::atomic::Ordering::Release,
        );
        manager
            .active
            .store(true, std::sync::atomic::Ordering::Release);
        assert!(manager.certificate_status().expired);
        assert!(!manager.is_active());
        manager.valid_until.store(
            chrono::Utc::now().timestamp() + 60,
            std::sync::atomic::Ordering::Release,
        );
        assert!(!manager.certificate_status().expired);
        assert!(manager.is_active());
        manager
            .active
            .store(false, std::sync::atomic::Ordering::Release);
        assert!(!manager.certificate_status().expired);
    }
    #[test]
    fn qoder_endpoint_backup_restores_only_its_owned_env_assignment() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(".env");
        let original="# native settings\nOTHER=keep\nexport QODERCN_SERVER_ENDPOINT=\"https://gateway.qoder.com.cn\"\n";
        std::fs::write(&path, original).expect("seed");
        let b = Backup::prepare(
            dir.path(),
            "http://127.0.0.1:12345",
            "https://localhost:23456",
        )
        .expect("backup");
        b.apply().expect("apply");
        assert!(std::fs::read_to_string(&path)
            .expect("read")
            .contains("https://localhost:23456"));
        b.restore().expect("restore");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
        b.apply().expect("apply");
        let injected = std::fs::read_to_string(&path).expect("read");
        std::fs::write(&path, format!("{injected}NEW=user-owned\n")).expect("edit");
        b.restore().expect("restore");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            format!("{original}NEW=user-owned\n")
        );
        std::fs::remove_file(&path).expect("remove fixture");
        let b = Backup::prepare(
            dir.path(),
            "http://127.0.0.1:12345",
            "https://localhost:23456",
        )
        .expect("backup absent");
        b.apply().expect("apply");
        b.restore().expect("restore absent");
        assert!(!path.exists());
    }
    #[test]
    fn qoder_restore_preserves_external_endpoint_and_stale_prepare_is_rejected() {
        let dir = tempfile::tempdir().expect("dir");
        let p = dir.path().join(".env");
        let b = Backup::prepare(
            dir.path(),
            "http://127.0.0.1:12345",
            "https://localhost:23456",
        )
        .expect("backup");
        std::fs::write(&p, "NEW=value\n").expect("edit");
        assert!(b.apply().is_err());
        let b = Backup::prepare(
            dir.path(),
            "http://127.0.0.1:12345",
            "https://localhost:23456",
        )
        .expect("backup");
        b.apply().expect("apply");
        std::fs::write(&p, "QODERCN_SERVER_ENDPOINT=https://user.example\n")
            .expect("user endpoint");
        b.restore().expect("preserve");
        assert_eq!(
            std::fs::read_to_string(p).expect("read"),
            "QODERCN_SERVER_ENDPOINT=https://user.example\n"
        );
    }

    #[test]
    fn qoder_model_restore_keeps_unrelated_models_and_external_edits() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("settings.json");
        let db = Database::memory().expect("db");
        let config = json!({"baseUrl":"https://api.example/v1","apiKey":"synthetic-key","protocol":"openai","model":"a","models":[{"model":"a"},{"model":"b"}]});
        let p =
            crate::provider::Provider::with_id("api".into(), "API".into(), config.clone(), None);
        db.save_provider("qoder", &p).expect("saved");
        let key = native::native_id(&p.id);
        let doc = json!({"enabledPlugins":{"keep":true},"providers":{key.clone():config,"user-owned":{"anything":"keep"}}});
        std::fs::write(&path, doc.to_string()).expect("native");
        let b = Backup::prepare(
            dir.path(),
            "http://127.0.0.1:12345",
            "https://localhost:23456",
        )
        .expect("backup");
        b.project_models(&db, true).expect("project");
        let mut projected = native::read_at(&path).expect("read").0;
        assert_eq!(projected["providers"][&key]["apiKey"], "varyai-local");
        projected["theme"] = json!("user-edited");
        projected["providers"][&key]["baseUrl"] = json!("https://user-changed.example/v1");
        std::fs::write(&path, projected.to_string()).expect("user edit");
        b.project_models(&db, false).expect("restore");
        let restored = native::read_at(&path).expect("restored").0;
        assert_eq!(restored["theme"], "user-edited");
        assert_eq!(
            restored["providers"][&key]["baseUrl"],
            "https://user-changed.example/v1"
        );
        assert_eq!(restored["providers"][&key]["apiKey"], "synthetic-key");
        assert_eq!(
            restored["providers"]["user-owned"],
            doc["providers"]["user-owned"]
        );
    }
}
