//! One-day, name-constrained local trust retained until explicitly cleared or replaced.
use crate::config::atomic_write_private;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;
fn failure() -> String {
    "Qoder local certificate setup or removal failed; approve the macOS authentication dialog and retry".into()
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Certificate {
    pub directory: PathBuf,
    pub sha1: String,
    pub pem: String,
    pub expires_at: i64,
}
pub(crate) fn inspect(directory: &Path) -> Result<Certificate, String> {
    let pem = std::fs::read_to_string(directory.join("ca.pem")).map_err(|_| failure())?;
    let fingerprint = String::from_utf8(openssl(
        directory,
        &["x509", "-in", "ca.pem", "-noout", "-sha1", "-fingerprint"],
    )?)
    .map_err(|_| failure())?;
    let sha1 = fingerprint
        .split_once('=')
        .map(|(_, s)| s.trim().replace(':', ""))
        .ok_or_else(failure)?;
    let end = String::from_utf8(openssl(
        directory,
        &["x509", "-in", "localhost.pem", "-noout", "-enddate"],
    )?)
    .map_err(|_| failure())?;
    let date = end
        .trim()
        .strip_prefix("notAfter=")
        .and_then(|s| s.strip_suffix(" GMT"))
        .ok_or_else(failure)?;
    let expires_at = chrono::NaiveDateTime::parse_from_str(date, "%b %e %H:%M:%S %Y")
        .map_err(|_| failure())?
        .and_utc()
        .timestamp();
    if sha1.len() != 40
        || !sha1.chars().all(|c| c.is_ascii_hexdigit())
        || !directory.join("localhost-key.pem").is_file()
    {
        return Err(failure());
    }
    Ok(Certificate {
        directory: directory.into(),
        sha1,
        pem,
        expires_at,
    })
}
pub(crate) async fn trusted(certificate: &Certificate) -> Result<bool, String> {
    use std::ffi::OsStr;
    let cert = certificate.directory.join("localhost.pem");
    let keychain = keychain();
    Ok(security(&[
        OsStr::new("verify-cert"),
        OsStr::new("-c"),
        cert.as_os_str(),
        OsStr::new("-p"),
        OsStr::new("ssl"),
        OsStr::new("-L"),
        OsStr::new("-n"),
        OsStr::new("localhost"),
        OsStr::new("-k"),
        keychain.as_os_str(),
    ])
    .await?
    .status
    .success())
}
fn lifetime_seconds(test_value: Option<&str>, development: bool) -> i64 {
    if development && test_value == Some("60") {
        60
    } else {
        86_400
    }
}
fn openssl(directory: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = std::process::Command::new("/usr/bin/openssl")
        .current_dir(directory)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| failure())?;
    if !output.status.success() {
        return Err(failure());
    }
    Ok(output.stdout)
}
pub(crate) async fn create() -> Result<Certificate, String> {
    if !cfg!(target_os = "macos") {
        return Err("Qoder official routing currently requires macOS".into());
    }
    tokio::task::spawn_blocking(|| {
        create_at_with_lifetime(
            crate::config::get_app_config_dir()
                .join("qoder-certificates")
                .join(uuid::Uuid::new_v4().to_string()),
            lifetime_seconds(
                std::env::var("VARYAI_QODER_CERT_TEST_SECONDS")
                    .ok()
                    .as_deref(),
                cfg!(debug_assertions),
            ),
        )
    })
    .await
    .map_err(|_| failure())?
}
#[cfg(test)]
pub(crate) fn create_at(directory: PathBuf) -> Result<Certificate, String> {
    create_at_with_lifetime(directory, 86_400)
}
fn create_at_with_lifetime(directory: PathBuf, lifetime: i64) -> Result<Certificate, String> {
    std::fs::create_dir_all(&directory).map_err(|_| failure())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| failure())?;
    }
    let result = (|| {
        let now = chrono::Utc::now();
        let expires_at = now.timestamp() + lifetime;
        let start = (now - chrono::Duration::seconds(5))
            .format("%y%m%d%H%M%SZ")
            .to_string();
        let end = chrono::DateTime::from_timestamp(expires_at, 0)
            .ok_or_else(failure)?
            .format("%y%m%d%H%M%SZ")
            .to_string();
        let ca="[req]\ndistinguished_name=dn\nx509_extensions=ca\nprompt=no\n[dn]\nCN=VaryAI Qoder Local CA\n[ca]\nbasicConstraints=critical,CA:true,pathlen:0\nkeyUsage=critical,keyCertSign,cRLSign\nnameConstraints=critical,permitted;DNS:localhost,permitted;IP:127.0.0.1/255.255.255.255\n";
        atomic_write_private(&directory.join("ca.cnf"), ca.as_bytes()).map_err(|_| failure())?;
        openssl(
            &directory,
            &[
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-sha256",
                "-days",
                "1",
                "-keyout",
                "ca-key.pem",
                "-out",
                "ca.pem",
                "-config",
                "ca.cnf",
            ],
        )?;
        // `x509 -days` cannot express a minute. Use explicit notAfter for both certificates.
        atomic_write_private(&directory.join("index"), b"").map_err(|_| failure())?;
        atomic_write_private(&directory.join("serial"), b"1000\n").map_err(|_| failure())?;
        let signing = "[ca]\ndefault_ca=local\n[local]\ndatabase=index\nserial=serial\nnew_certs_dir=.\ncertificate=ca.pem\nprivate_key=ca-key.pem\ndefault_md=sha256\npolicy=subject\nunique_subject=no\n[subject]\ncommonName=supplied\n[ca_ext]\nbasicConstraints=critical,CA:true,pathlen:0\nkeyUsage=critical,keyCertSign,cRLSign\nnameConstraints=critical,permitted;DNS:localhost,permitted;IP:127.0.0.1/255.255.255.255\n[leaf_ext]\nbasicConstraints=critical,CA:false\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost\n";
        atomic_write_private(&directory.join("signing.cnf"), signing.as_bytes())
            .map_err(|_| failure())?;
        openssl(
            &directory,
            &[
                "req",
                "-new",
                "-key",
                "ca-key.pem",
                "-out",
                "ca.csr",
                "-subj",
                "/CN=VaryAI Qoder Local CA",
            ],
        )?;
        openssl(
            &directory,
            &[
                "ca",
                "-batch",
                "-selfsign",
                "-config",
                "signing.cnf",
                "-in",
                "ca.csr",
                "-out",
                "ca-new.pem",
                "-notext",
                "-extensions",
                "ca_ext",
                "-startdate",
                &start,
                "-enddate",
                &end,
            ],
        )?;
        std::fs::rename(directory.join("ca-new.pem"), directory.join("ca.pem"))
            .map_err(|_| failure())?;
        openssl(
            &directory,
            &[
                "req",
                "-new",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                "localhost-key.pem",
                "-out",
                "localhost.csr",
                "-subj",
                "/CN=localhost",
            ],
        )?;
        openssl(
            &directory,
            &[
                "ca",
                "-batch",
                "-config",
                "signing.cnf",
                "-in",
                "localhost.csr",
                "-out",
                "localhost.pem",
                "-notext",
                "-extensions",
                "leaf_ext",
                "-startdate",
                &start,
                "-enddate",
                &end,
            ],
        )?;
        for name in ["ca-key.pem", "localhost-key.pem"] {
            let path = directory.join(name);
            let bytes = std::fs::read(&path).map_err(|_| failure())?;
            atomic_write_private(&path, &bytes).map_err(|_| failure())?;
        }
        let fingerprint = String::from_utf8(openssl(
            &directory,
            &["x509", "-in", "ca.pem", "-noout", "-sha1", "-fingerprint"],
        )?)
        .map_err(|_| failure())?;
        let sha1 = fingerprint
            .split_once('=')
            .map(|(_, s)| s.trim().replace(':', ""))
            .filter(|s| s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(failure)?;
        let pem = std::fs::read_to_string(directory.join("ca.pem")).map_err(|_| failure())?;
        // Signing is finished; the CA private key is never needed by the runtime.
        std::fs::remove_file(directory.join("ca-key.pem")).map_err(|_| failure())?;
        Ok(Certificate {
            directory: directory.clone(),
            sha1,
            pem,
            expires_at,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&directory);
    }
    result
}
async fn security(args: &[&std::ffi::OsStr]) -> Result<std::process::Output, String> {
    let child = Command::new("/usr/bin/security")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| failure())?;
    tokio::time::timeout(
        std::time::Duration::from_secs(180),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| failure())?
    .map_err(|_| failure())
}
fn keychain() -> PathBuf {
    crate::config::get_home_dir().join("Library/Keychains/login.keychain-db")
}
async fn explain_auth(app: Option<&tauri::AppHandle>, operation: &str) -> Result<(), String> {
    let Some(app) = app else {
        return Ok(());
    };
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let language = crate::settings::get_settings()
        .language
        .unwrap_or_else(|| "en".into());
    let source = match language.as_str() {
        "zh" => include_str!("../../../src/i18n/locales/zh.json"),
        "zh-TW" => include_str!("../../../src/i18n/locales/zh-TW.json"),
        "ja" => include_str!("../../../src/i18n/locales/ja.json"),
        _ => include_str!("../../../src/i18n/locales/en.json"),
    };
    let texts: serde_json::Value = serde_json::from_str(source).map_err(|_| failure())?;
    let text = |key: &str| {
        texts["qoder"]["certificate"][key]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(text(operation))
        .title(text("authTitle"))
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCustom(text("authContinue")))
        .show(move |_| {
            let _ = tx.send(());
        });
    rx.await.map_err(|_| failure())
}
pub(crate) async fn trust(directory: &Path, app: Option<&tauri::AppHandle>) -> Result<(), String> {
    explain_auth(app, "authTrust").await?;
    use std::ffi::OsStr;
    let cert = directory.join("ca.pem");
    let keychain = keychain();
    let result = security(&[
        OsStr::new("add-trusted-cert"),
        OsStr::new("-r"),
        OsStr::new("trustRoot"),
        OsStr::new("-p"),
        OsStr::new("ssl"),
        OsStr::new("-k"),
        keychain.as_os_str(),
        cert.as_os_str(),
    ])
    .await?;
    if result.status.success() {
        Ok(())
    } else {
        Err(failure())
    }
}
pub(crate) async fn remove(
    directory: &Path,
    sha1: &str,
    pem: &str,
    app: Option<&tauri::AppHandle>,
) -> Result<(), String> {
    use std::ffi::OsStr;
    let root = crate::config::get_app_config_dir().join("qoder-certificates");
    if directory.parent() != Some(root.as_path())
        || sha1.len() != 40
        || !sha1.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(failure());
    }
    let cert = directory.join("ca.pem");
    if !cert.exists() {
        atomic_write_private(&cert, pem.as_bytes()).map_err(|_| failure())?;
    }
    explain_auth(app, "authRemove").await?;
    let result = security(&[OsStr::new("remove-trusted-cert"), cert.as_os_str()]).await?;
    if !result.status.success()
        && !String::from_utf8_lossy(&result.stderr).contains("could not be found")
        && !String::from_utf8_lossy(&result.stderr).contains("No Trust Settings")
    {
        return Err(failure());
    }
    let keychain = keychain();
    let result = security(&[
        OsStr::new("delete-certificate"),
        OsStr::new("-Z"),
        OsStr::new(sha1),
        keychain.as_os_str(),
    ])
    .await?;
    if !result.status.success()
        && !String::from_utf8_lossy(&result.stderr).contains("could not be found")
    {
        return Err(failure());
    }
    std::fs::remove_dir_all(directory).map_err(|_| failure())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn one_minute_mode_is_development_only() {
        assert_eq!(lifetime_seconds(Some("60"), true), 60);
        assert_eq!(lifetime_seconds(Some("60"), false), 86_400);
        assert_eq!(lifetime_seconds(None, true), 86_400);
        assert_eq!(lifetime_seconds(Some("invalid"), true), 86_400);
    }
    #[test]
    fn qoder_test_certificates_really_expire_after_one_minute() {
        let temp = tempfile::tempdir().expect("dir");
        let before = chrono::Utc::now().timestamp();
        let certificate = create_at_with_lifetime(temp.path().join("cert"), 60).expect("cert");
        assert!(
            (before + 60..=chrono::Utc::now().timestamp() + 60).contains(&certificate.expires_at)
        );
        for name in ["ca.pem", "localhost.pem"] {
            let output = String::from_utf8(
                openssl(
                    &certificate.directory,
                    &["x509", "-in", name, "-noout", "-enddate"],
                )
                .expect("date"),
            )
            .expect("utf8");
            let date = chrono::NaiveDateTime::parse_from_str(
                output.trim().trim_start_matches("notAfter="),
                "%b %e %H:%M:%S %Y GMT",
            )
            .expect("parsed")
            .and_utc()
            .timestamp();
            assert_eq!(date, certificate.expires_at);
        }
    }
    #[tokio::test]
    async fn qoder_certificate_and_tls_listener_validate_without_installing_system_trust() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let temp = tempfile::tempdir().expect("dir");
        let certificate = create_at(temp.path().join("cert")).expect("certificate");
        assert!(!certificate.directory.join("ca-key.pem").exists());
        let description = String::from_utf8(
            openssl(
                &certificate.directory,
                &["x509", "-in", "ca.pem", "-text", "-noout"],
            )
            .expect("x509"),
        )
        .expect("text");
        assert!(description.contains("Name Constraints: critical"));
        assert!(description.contains("DNS:localhost"));
        assert!(description.contains("127.0.0.1"));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("upstream");
        let address = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/health",
            axum::routing::get(|| async { "official-upstream-ok" }),
        );
        let upstream = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("upstream");
        });
        let db = std::sync::Arc::new(crate::database::Database::memory().expect("db"));
        db.set_setting(crate::services::qoder_proxy::ENABLED_KEY, "true")
            .expect("enabled");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("tls");
        let local = listener.local_addr().expect("addr");
        let server = crate::proxy::qoder_tls::Server::start(
            listener,
            &certificate.directory,
            crate::proxy::qoder::OfficialState {
                db,
                upstream: format!("http://{address}"),
            },
        )
        .await
        .expect("tls server");
        let client = reqwest::Client::builder()
            .no_proxy()
            .add_root_certificate(
                reqwest::Certificate::from_pem(certificate.pem.as_bytes()).expect("ca"),
            )
            .resolve("localhost", local)
            .build()
            .expect("client");
        let response = client
            .get(format!("https://localhost:{}/health", local.port()))
            .send()
            .await
            .expect("verified local TLS");
        assert_eq!(response.text().await.expect("body"), "official-upstream-ok");
        server.stop().await;
        let _listener = tokio::net::TcpListener::bind(local)
            .await
            .expect("port released");
        upstream.abort();
    }
}
