//! Bounded provider artwork. Fetches run only after an explicit editor action.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{net::IpAddr, path::Path, time::Duration};

const FAILED: &str = "providerEditor.iconFailed";
pub(crate) const MAX_IMAGE: usize = 128 * 1024;

fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if std::str::from_utf8(bytes).is_ok_and(|text| {
        let text = text.trim_start_matches('\u{feff}').trim_start();
        (text.starts_with("<svg") || text.starts_with("<?xml")) && text.contains("<svg")
    }) {
        Some("image/svg+xml")
    } else {
        None
    }
}

pub(crate) fn encode(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE {
        return None;
    }
    Some(format!(
        "data:{};base64,{}",
        image_type(bytes)?,
        STANDARD.encode(bytes)
    ))
}

pub(crate) fn valid(icon: &str) -> bool {
    if icon.is_empty() || icon.len() > MAX_IMAGE * 4 / 3 + 64 {
        return false;
    }
    if let Some((_, encoded)) = icon.split_once(',') {
        let Ok(bytes) = STANDARD.decode(encoded) else {
            return false;
        };
        return encode(&bytes).is_some_and(|value| value == icon);
    }
    icon.len() <= 80
        && icon
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

pub(crate) fn imported(raw: &str, directory: Option<&Path>) -> Option<String> {
    if let Some(name) = raw.strip_prefix("file:") {
        // Source artwork is confined to Magpie's content-addressed icons folder.
        let (hash, extension) = name.split_once('.')?;
        if hash.len() != 16
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            || !matches!(extension, "png" | "jpg" | "gif" | "webp" | "ico" | "svg")
        {
            return None;
        }
        let directory = directory?.join("icons").canonicalize().ok()?;
        let path = directory.join(name).canonicalize().ok()?;
        if path.parent() != Some(directory.as_path()) {
            return None;
        }
        use std::io::Read;
        let mut bytes = vec![];
        std::fs::File::open(path)
            .ok()?
            .take((MAX_IMAGE + 1) as u64)
            .read_to_end(&mut bytes)
            .ok()?;
        return encode(&bytes);
    }
    valid(raw).then(|| raw.to_owned())
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 240
                || ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1])
                || ip.octets()[0] == 198 && matches!(ip.octets()[1], 18 | 19))
        }
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or_else(
            || ip.segments()[0] & 0xe000 == 0x2000 && ip.segments()[0..2] != [0x2001, 0x0db8],
            |ip| public_ip(IpAddr::V4(ip)),
        ),
    }
}

fn image_url(raw: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(raw).map_err(|_| FAILED)?;
    if raw.len() > 2048
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || raw
            .split('/')
            .nth(2)
            .is_some_and(|authority| authority.contains('@'))
        || url
            .host_str()
            .is_some_and(|host| host.trim_end_matches('.').eq_ignore_ascii_case("localhost"))
    {
        return Err(FAILED.into());
    }
    Ok(url)
}

async fn download(mut url: url::Url) -> Result<Vec<u8>, String> {
    for _ in 0..5 {
        let host = url.host_str().ok_or(FAILED)?.trim_matches(['[', ']']);
        let addresses: Vec<_> =
            tokio::net::lookup_host((host, url.port_or_known_default().ok_or(FAILED)?))
                .await
                .map_err(|_| FAILED)?
                .collect();
        if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
            return Err(FAILED.into());
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| FAILED)?;
        let mut response = client.get(url.clone()).send().await.map_err(|_| FAILED)?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(FAILED)?;
            url = image_url(url.join(location).map_err(|_| FAILED)?.as_str())?;
            continue;
        }
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|size| size > MAX_IMAGE as u64)
        {
            return Err(FAILED.into());
        }
        let mut bytes = vec![];
        while let Some(chunk) = response.chunk().await.map_err(|_| FAILED)? {
            if bytes.len() + chunk.len() > MAX_IMAGE {
                return Err(FAILED.into());
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(bytes);
    }
    Err(FAILED.into())
}

pub(crate) async fn fetch(raw: &str, automatic: bool) -> Result<String, String> {
    let operation = async {
        let url = image_url(raw)?;
        if !automatic {
            return encode(&download(url).await?).ok_or_else(|| FAILED.into());
        }
        let root = url.join("/").map_err(|_| FAILED)?;
        if let Ok(page) = download(root.clone()).await {
            let page = String::from_utf8_lossy(&page);
            let links = regex::Regex::new(r"(?is)<link\b[^>]{0,4096}>").map_err(|_| FAILED)?;
            let attrs = regex::Regex::new(r#"(?is)\b(rel|href)\s*=\s*["']([^"']+)["']"#)
                .map_err(|_| FAILED)?;
            for link in links.find_iter(&page).take(64) {
                let fields: std::collections::HashMap<_, _> = attrs
                    .captures_iter(link.as_str())
                    .map(|c| (c[1].to_ascii_lowercase(), c[2].to_owned()))
                    .collect();
                if !fields.get("rel").is_some_and(|value| {
                    value.split_whitespace().any(|word| {
                        word.eq_ignore_ascii_case("icon")
                            || word.eq_ignore_ascii_case("apple-touch-icon")
                    })
                }) {
                    continue;
                }
                if let Some(href) = fields.get("href") {
                    if let Ok(url) = root.join(&href.replace("&amp;", "&")) {
                        if let Ok(url) = image_url(url.as_str()) {
                            if let Ok(bytes) = download(url).await {
                                if let Some(icon) = encode(&bytes) {
                                    return Ok(icon);
                                }
                            }
                        }
                    }
                }
            }
        }
        encode(&download(root.join("favicon.ico").map_err(|_| FAILED)?).await?)
            .ok_or_else(|| FAILED.into())
    };
    tokio::time::timeout(Duration::from_secs(15), operation)
        .await
        .map_err(|_| FAILED)?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artwork_accepts_bounded_images_and_builtin_names_only() {
        assert!(valid("deepseek-color"));
        assert!(valid(
            &encode(b"\x89PNG\r\n\x1a\nsynthetic").expect("image")
        ));
        for raw in [
            "https://example.test/logo.png",
            "file:../../private",
            "<svg onload='bad'/>",
            "data:text/html;base64,PHNjcmlwdD4=",
        ] {
            assert!(!valid(raw));
        }
        assert!(encode(&vec![0; MAX_IMAGE + 1]).is_none());
    }
    #[test]
    fn artwork_fetch_rejects_private_hosts_and_non_https_urls() {
        for raw in [
            "http://example.test/a.png",
            "https://localhost/a.png",
            "https://user:password@example.test/a.png",
        ] {
            assert!(image_url(raw).is_err());
        }
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
        ] {
            assert!(!public_ip(ip.parse().expect("IP")));
        }
    }
    #[test]
    fn source_artwork_stays_within_the_icons_folder() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("icons"))?;
        let bytes = b"\x89PNG\r\n\x1a\nsynthetic";
        std::fs::write(dir.path().join("icons/0123456789abcdef.png"), bytes)?;
        assert_eq!(
            imported("file:0123456789abcdef.png", Some(dir.path())),
            encode(bytes)
        );
        assert!(imported("file:../../private.png", Some(dir.path())).is_none());
        assert!(imported("https://example.test/image.png", Some(dir.path())).is_none());
        Ok(())
    }
}
