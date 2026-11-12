//! Shared ChatGPT client identity; model metadata is never imported across accounts.
use std::{
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};

pub(crate) const MIN_VERSION: &str = "0.165.0";
mod identity;
pub(crate) use identity::user_agent;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version([u32; 3]);

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.len() > 80 {
            return None;
        }
        let core = text.split(['-', '+']).next()?;
        let parts = core.split('.').collect::<Vec<_>>();
        if parts.len() != 3
            || !parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            || !text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
        {
            return None;
        }
        Some(Self([
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ]))
    }

    fn text(self) -> String {
        format!("{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

#[derive(Default)]
struct Cache {
    version: Option<Version>,
    checked: Option<Instant>,
}

fn cache() -> &'static RwLock<Cache> {
    static CACHE: OnceLock<RwLock<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// No filesystem access or process execution in request forwarding.
pub(crate) fn current_version() -> String {
    cache()
        .read()
        .ok()
        .and_then(|c| c.version)
        .map(Version::text)
        .unwrap_or_else(|| MIN_VERSION.into())
}

fn observed_version(headers: &http::HeaderMap) -> Option<Version> {
    let ua = headers
        .get(http::header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    if !ua.to_ascii_lowercase().starts_with("codex") {
        return None;
    }
    headers
        .get("version")
        .and_then(|h| h.to_str().ok())
        .and_then(Version::parse)
        .or_else(|| {
            ua.split_once('/')
                .and_then(|(_, tail)| Version::parse(tail.split_whitespace().next()?))
        })
}

pub(crate) fn observe(headers: &http::HeaderMap) {
    let Some(version) = observed_version(headers) else {
        return;
    };
    if let Ok(mut c) = cache().write() {
        let floor = Version::parse(MIN_VERSION).unwrap_or(Version([0, 165, 0]));
        c.version = Some(c.version.unwrap_or(floor).max(version));
    }
}

pub(crate) fn apply_identity(
    headers: &mut http::HeaderMap,
) -> Result<(), http::header::InvalidHeaderValue> {
    let version = current_version();
    headers.insert("version", http::HeaderValue::from_str(&version)?);
    headers.insert(
        http::header::USER_AGENT,
        http::HeaderValue::from_str(&user_agent(&version))?,
    );
    Ok(())
}

/// Passive discovery runs off the forwarding path and never starts a CLI.
pub(crate) async fn resolve_version() -> String {
    identity::prepare().await;
    static PREPARE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = PREPARE.lock().await;
    if cache().read().ok().is_some_and(|c| {
        c.checked
            .is_some_and(|at| at.elapsed() < Duration::from_secs(600))
    }) {
        return current_version();
    }
    let result = tokio::task::spawn_blocking(|| {
        let mut candidates = crate::codex_config::codex_cli_candidates();
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path).filter(|p| p.is_absolute()) {
                candidates.push(dir.join("codex"));
                #[cfg(windows)]
                candidates.push(dir.join("codex.cmd"));
            }
        }
        detect(
            &crate::codex_config::get_codex_config_dir().join("models_cache.json"),
            &candidates,
        )
    })
    .await
    .ok();
    if let Ok(mut c) = cache().write() {
        if let Some(version) = result {
            c.version = Some(c.version.map_or(version, |v| v.max(version)));
        }
        c.checked = Some(Instant::now());
    }
    current_version()
}

fn read_json(path: &Path, limit: u64) -> Option<serde_json::Value> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > limit {
        return None;
    }
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn detect(model_cache: &Path, candidates: &[PathBuf]) -> Version {
    let mut newest = Version::parse(MIN_VERSION).unwrap_or(Version([0, 165, 0]));
    if let Some(version) = read_json(model_cache, 4 * 1024 * 1024).and_then(|v| {
        v.get("client_version")
            .and_then(|v| v.as_str())
            .and_then(Version::parse)
    }) {
        newest = newest.max(version);
    }
    for candidate in candidates {
        if !candidate.is_absolute() {
            continue;
        }
        let Ok(real) = std::fs::canonicalize(candidate) else {
            continue;
        };
        let package_paths = real
            .ancestors()
            .skip(1)
            .take(10)
            .map(|p| p.join("package.json"))
            .chain(
                candidate
                    .parent()
                    .map(|p| p.join("node_modules/@openai/codex/package.json")),
            );
        for package in package_paths {
            let Some(value) = read_json(&package, 64 * 1024) else {
                continue;
            };
            if value["name"] != "@openai/codex" {
                continue;
            }
            if let Some(version) = value["version"].as_str().and_then(Version::parse) {
                newest = newest.max(version);
            }
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_selection_is_numeric_and_rejects_header_injection() {
        assert!(Version::parse("0.165.0") > Version::parse("0.99.99"));
        assert_eq!(Version::parse("0.166.0-alpha.1").unwrap().text(), "0.166.0");
        for text in [
            "1.2",
            "1.2.3.4",
            "1.2.3\r\nAuthorization: secret",
            "999999999999.1.1",
            "x.1.1",
            "codex/0.165.0",
        ] {
            assert!(Version::parse(text).is_none());
        }
    }

    #[test]
    fn discovery_uses_newest_package_or_cache_version_without_importing_models() {
        let dir = tempfile::tempdir().unwrap();
        let model_cache = dir.path().join("models_cache.json");
        let bin = dir.path().join("node_modules/@openai/codex/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let executable = bin.join("codex.js");
        std::fs::write(&executable, "not executable").unwrap();
        std::fs::write(
            bin.parent().unwrap().join("package.json"),
            r#"{"name":"@openai/codex","version":"0.166.0"}"#,
        )
        .unwrap();
        std::fs::write(
            &model_cache,
            r#"{"client_version":"0.164.0","models":[{"slug":"other-account-model"}]}"#,
        )
        .unwrap();
        assert_eq!(
            detect(&model_cache, std::slice::from_ref(&executable)).text(),
            "0.166.0"
        );
        std::fs::write(&model_cache, r#"{"client_version":"0.167.1"}"#).unwrap();
        assert_eq!(detect(&model_cache, &[executable]).text(), "0.167.1");
        std::fs::write(&model_cache, "invalid").unwrap();
        assert_eq!(detect(&model_cache, &[]).text(), MIN_VERSION);
    }

    #[test]
    fn unrelated_packages_and_oversized_metadata_cannot_override_identity() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("codex");
        std::fs::write(&executable, "not executable").unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"unrelated","version":"9.9.9"}"#,
        )
        .unwrap();
        let model_cache = dir.path().join("models_cache.json");
        std::fs::write(&model_cache, " ".repeat(4 * 1024 * 1024 + 1)).unwrap();
        assert_eq!(detect(&model_cache, &[executable]).text(), MIN_VERSION);
    }

    #[test]
    fn only_codex_clients_supply_version_hints_and_outgoing_identity_is_consistent() {
        let mut headers = http::HeaderMap::new();
        headers.insert("version", "9.9.9".parse().unwrap());
        headers.insert(http::header::USER_AGENT, "Mozilla/5.0".parse().unwrap());
        assert!(observed_version(&headers).is_none());
        headers.insert(
            http::header::USER_AGENT,
            "Codex Desktop/0.166.0 (macOS)".parse().unwrap(),
        );
        headers.remove("version");
        assert_eq!(observed_version(&headers).unwrap().text(), "0.166.0");
        headers.insert("version", "0.167.0".parse().unwrap());
        assert_eq!(observed_version(&headers).unwrap().text(), "0.167.0");
        apply_identity(&mut headers).unwrap();
        assert!(headers[http::header::USER_AGENT]
            .to_str()
            .unwrap()
            .starts_with(&format!(
                "codex_cli_rs/{} (",
                headers["version"].to_str().unwrap()
            )));
        assert!(
            Version::parse(headers["version"].to_str().unwrap()) >= Version::parse(MIN_VERSION)
        );
    }
}
