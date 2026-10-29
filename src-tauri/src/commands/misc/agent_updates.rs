use super::*;
#[cfg(any(target_os = "macos", test))]
use base64::Engine;
#[cfg(any(target_os = "macos", test))]
use sha2::{Digest, Sha256, Sha512};

#[derive(Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) struct Release {
    pub version: String,
    pub url: reqwest::Url,
    sha512: Option<String>,
    sha256: Option<String>,
}

fn trusted_url(app: &str, raw: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(raw).ok()?;
    let hosts: &[&str] = match app {
        "claude-desktop" => &["downloads.claude.ai"],
        "workbuddy" => &["download.codebuddy.cn", "download.codebuddy.ai"],
        "qoder" => &["static.qoder.com.cn"],
        "doubaowork" => &["lf9-apk.ugapk.cn", "lf6-apk.ugapk.cn", "lf3-apk.ugapk.cn"],
        "trae" => &["lf-cdn.trae.com.cn"],
        "kimi" => &["code.kimi.com"],
        "zcode" => &["cdn-zcode.z.ai", "cdn.codegeex.cn"],
        _ => return None,
    };
    (url.scheme() == "https"
        && hosts.contains(&url.host_str()?)
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

fn release(app: &str, version: &str, url: &str) -> Option<Release> {
    let version = agent_versions::version_number(version)?;
    let url = trusted_url(app, url)?;
    (url.path().ends_with(".zip") || url.path().ends_with(".dmg")).then_some(Release {
        version,
        url,
        sha512: None,
        sha256: None,
    })
}

async fn response_text(mut response: reqwest::Response) -> Option<String> {
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).ok()
}

async fn get_text(client: &reqwest::Client, url: &str) -> Option<String> {
    response_text(
        client
            .get(url)
            .timeout(LATEST_PROBE_TIMEOUT)
            .send()
            .await
            .ok()?,
    )
    .await
}

fn yaml_release(app: &str, base: &str, text: &str, arch: &str) -> Option<Release> {
    let data: serde_yaml::Value = serde_yaml::from_str(text).ok()?;
    // TRAE's build version differs from its displayed product version.
    let version = data
        .get("appVersion")
        .or_else(|| data.get("version"))?
        .as_str()?;
    let base = reqwest::Url::parse(base).ok()?;
    let file = data.get("files")?.as_sequence()?.iter().find(|file| {
        file.get("url").and_then(|v| v.as_str()).is_some_and(|url| {
            url.ends_with(".zip") && (url.contains(arch) || url.contains("universal"))
        })
    })?;
    let url = base.join(file.get("url")?.as_str()?).ok()?;
    let mut result = release(app, version, url.as_str())?;
    result.sha512 = Some(file.get("sha512")?.as_str()?.to_owned());
    Some(result)
}

fn json_release(app: &str, text: &str) -> Option<Release> {
    let data: serde_json::Value = serde_json::from_str(text).ok()?;
    match app {
        "claude-desktop" => {
            let current = data.get("currentRelease")?.as_str()?;
            let update = data
                .get("releases")?
                .as_array()?
                .iter()
                .find(|entry| entry.get("version").and_then(|v| v.as_str()) == Some(current))?
                .get("updateTo")?;
            release(app, current, update.get("url")?.as_str()?)
        }
        "workbuddy" => {
            let mut result = release(
                app,
                data.get("productVersion")
                    .or_else(|| data.get("version"))?
                    .as_str()?,
                data.get("url")?.as_str()?,
            )?;
            result.sha256 = data
                .get("sha256hash")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .map(str::to_owned);
            Some(result)
        }
        _ => None,
    }
}

fn doubao_release(text: &str) -> Option<Release> {
    let data: serde_json::Value = serde_json::from_str(text).ok()?;
    let url = data.pointer("/data/download_url")?.as_str()?;
    let url = trusted_url("doubaowork", url)?;
    // The Work distribution has a different identity from the standard Doubao app.
    let encoded = url.path().split('/').collect::<Vec<_>>();
    let position = encoded
        .iter()
        .position(|part| *part == "doubao_work_desktop")?;
    let value: u64 = encoded.get(position + 1)?.parse().ok()?;
    let version = format!(
        "{}.{}.{}",
        value / 1_000_000,
        value / 1_000 % 1_000,
        value % 1_000
    );
    release("doubaowork", &version, url.as_str())
}

fn zcode_release(text: &str, arch: &str) -> Option<Release> {
    let pattern = format!(
        r#"https://cdn-zcode\.z\.ai/zcode/electron/releases/(\d+\.\d+\.\d+)/macos-{arch}/ZCode-[^"\s<>]+\.dmg"#
    );
    let regex = Regex::new(&pattern).ok()?;
    let captures = regex.captures(text)?;
    release(
        "zcode",
        captures.get(1)?.as_str(),
        captures.get(0)?.as_str(),
    )
}

pub(super) async fn latest(client: &reqwest::Client, app: &str, arch: &str) -> Option<Release> {
    match app {
        "qoder" | "kimi" => {
            let url = if app == "qoder" {
                "https://static.qoder.com.cn/qoder-app/releases/latest-mac.yml"
            } else {
                "https://code.kimi.com/kimi-code/desktop/latest-mac.yml"
            };
            yaml_release(app, url, &get_text(client, url).await?, arch)
        }
        "claude-desktop" => json_release(
            app,
            &get_text(
                client,
                "https://downloads.claude.ai/releases/darwin/universal/RELEASES.json",
            )
            .await?,
        ),
        "workbuddy" => {
            let url = format!(
                "https://www.workbuddy.cn/v2/update?platform=workbuddy-darwin-{arch}&version=0.0.0"
            );
            json_release(app, &get_text(client, &url).await?)
        }
        "trae" => {
            let url = "https://api.trae.com.cn/icube/api/v1/package/check_update?pid=7409949320595642651&branch=release_desktop_yoma_cn&buildId=0&platform=mac&packageType=stable_cn&appVersion=0.0.0&buildVersion=0.0.0&mid=varyai-update-check&productCode=TRAE";
            let data: serde_json::Value =
                serde_json::from_str(&get_text(client, url).await?).ok()?;
            let base = data
                .pointer("/data/manifest/darwin/urls")?
                .as_array()?
                .iter()
                .find(|entry| entry.get("region").and_then(|v| v.as_str()) == Some("cn"))?
                .get("url")?
                .as_str()?;
            // Validate the server-selected origin before making the second request.
            trusted_url(app, base)?;
            let url = format!("{base}/latest_{arch}");
            yaml_release(app, &url, &get_text(client, &url).await?, arch)
        }
        "doubaowork" => {
            // Anonymous public download metadata used by the publisher's Work landing page.
            let response = client.post("https://99uri.com/ug/pc/landpage_click")
                .timeout(LATEST_PROBE_TIMEOUT)
                .json(&serde_json::json!({"type":1,"os":"mac","content":"{\"surl_token\":\"NwGQs\",\"web_url\":\"https%3A%2F%2Fwww.doubao.com%2Fwork\"}"}))
                .send().await.ok()?;
            doubao_release(&response_text(response).await?)
        }
        "zcode" => zcode_release(
            &get_text(client, "https://zcode.z.ai/en/docs/install").await?,
            arch,
        ),
        _ => None,
    }
}

#[cfg(any(target_os = "macos", test))]
fn verify_digests(sha512: &[u8], sha256: &[u8], release: &Release) -> bool {
    release.sha512.as_ref().is_none_or(|expected| {
        base64::engine::general_purpose::STANDARD.encode(sha512) == *expected
    }) && release.sha256.as_ref().is_none_or(|expected| {
        sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            .eq_ignore_ascii_case(expected)
    })
}

#[cfg(target_os = "macos")]
mod macos;

pub(super) async fn update_desktop_agent(app: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let _guard = TOOL_LIFECYCLE_LOCK
                .try_lock()
                .map_err(|_| "busy".to_owned())?;
            macos::update(&runtime, &app)
        })
        .await
        .map_err(|_| "failed".to_owned())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("unsupported".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publisher_urls_reject_spoofed_hosts_credentials_and_local_feeds() {
        for url in [
            "http://code.kimi.com/a.zip",
            "https://code.kimi.com.evil.test/a.zip",
            "https://user@code.kimi.com/a.zip",
            "http://localhost:8081/a.zip",
            "https://code.kimi.com:8443/a.zip",
        ] {
            assert!(trusted_url("kimi", url).is_none());
        }
        assert!(trusted_url("kimi", "https://code.kimi.com/kimi-code/desktop/a.zip").is_some());
        assert!(trusted_url("unknown", "https://code.kimi.com/a.zip").is_none());
    }

    #[test]
    fn yaml_selects_cpu_architecture_and_trae_product_version() {
        let text = "appVersion: 3.4.9\nversion: 2.3.97430\nfiles:\n - {url: intel-x64.zip, sha512: a}\n - {url: trae-arm64.zip, sha512: b}";
        let release = yaml_release(
            "trae",
            "https://lf-cdn.trae.com.cn/releases/latest",
            text,
            "arm64",
        )
        .unwrap();
        assert_eq!(release.version, "3.4.9");
        assert!(release.url.path().ends_with("trae-arm64.zip"));
        assert!(yaml_release("trae", "https://lf-cdn.trae.com.cn/", text, "riscv").is_none());
        assert!(yaml_release(
            "trae",
            "https://lf-cdn.trae.com.cn/",
            &text
                .replace("sha512: b", "other: b")
                .replace("intel-x64.zip", "intel-arm64.dmg"),
            "arm64"
        )
        .is_none());
    }

    #[test]
    fn doubao_work_is_distinct_from_standard_doubao() {
        let text = r#"{"data":{"download_url":"https://lf9-apk.ugapk.cn/package/pc/doubao_work_desktop/2034004/work.dmg"}}"#;
        assert_eq!(doubao_release(text).unwrap().version, "2.34.4");
        assert!(doubao_release(&text.replace("doubao_work_desktop", "doubao_desktop")).is_none());
    }

    #[test]
    fn checksum_failure_is_not_accepted() {
        let mut info = release("kimi", "1.1.4", "https://code.kimi.com/a.zip").unwrap();
        info.sha512 = Some(
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(b"synthetic artifact")),
        );
        assert!(verify_digests(
            &Sha512::digest(b"synthetic artifact"),
            &Sha256::digest(b"synthetic artifact"),
            &info
        ));
        assert!(!verify_digests(
            &Sha512::digest(b"changed artifact"),
            &Sha256::digest(b"changed artifact"),
            &info
        ));
    }

    #[test]
    fn json_publishers_select_current_release_and_normalize_bundle_versions() {
        let claude = r#"{"currentRelease":"4.29.0","releases":[{"version":"4.28.0"},{"version":"4.29.0","updateTo":{"url":"https://downloads.claude.ai/releases/Claude.zip"}}]}"#;
        assert_eq!(
            json_release("claude-desktop", claude).unwrap().version,
            "4.29.0"
        );
        let buddy = r#"{"productVersion":"5.3.14.36279234","url":"https://download.codebuddy.cn/workbuddy/app.zip"}"#;
        assert_eq!(json_release("workbuddy", buddy).unwrap().version, "5.3.14");
    }
}
