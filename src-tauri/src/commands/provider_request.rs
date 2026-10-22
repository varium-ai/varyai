use serde::{Deserialize, Serialize};
use std::time::Duration;

const API_BASE_URL: &str = match option_env!("VARYAI_PROVIDER_REQUEST_BASE_URL") {
    Some(value) => value,
    None => "https://www.varyai.net",
};
const MAX_RESPONSE_BYTES: usize = 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequest {
    request_id: String,
    name: String,
    website_url: String,
    description: String,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequestError {
    code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_seconds: Option<u32>,
}

impl ProviderRequestError {
    fn new(code: &'static str) -> Self {
        Self {
            code,
            retry_after_seconds: None,
        }
    }
}

#[derive(Serialize)]
struct ServerRequest<'a> {
    request_id: &'a str,
    name: &'a str,
    website_url: &'a str,
    description: &'a str,
    app_version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_model: Option<&'a str>,
}

fn validate_request(mut request: ProviderRequest) -> Result<ProviderRequest, ProviderRequestError> {
    let invalid = || ProviderRequestError::new("invalid_request");
    request.name = request.name.trim().to_owned();
    request.website_url = request.website_url.trim().to_owned();
    request.description = request.description.trim().to_owned();
    let id = uuid::Uuid::parse_str(&request.request_id).map_err(|_| invalid())?;

    if id.get_version_num() != 4
        || id.get_variant() != uuid::Variant::RFC4122
        || !id
            .hyphenated()
            .to_string()
            .eq_ignore_ascii_case(&request.request_id)
    {
        return Err(invalid());
    }
    request.request_id = id.to_string();
    let website = reqwest::Url::parse(&request.website_url).map_err(|_| invalid())?;
    if request.name.is_empty()
        || request.name.chars().count() > 50
        || request.name.chars().any(char::is_control)
        || request.website_url.chars().count() > 300
        || request.website_url.chars().any(char::is_control)
        || request
            .website_url
            .chars()
            .any(|ch| ch.is_whitespace() || ch == '\\')
        || request.description.chars().count() > 1000
        || request
            .description
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        || !matches!(website.scheme(), "http" | "https")
        || website.host_str().is_none()
        || !website.username().is_empty()
        || website.password().is_some()
        || !request
            .website_url
            .split_once("://")
            .is_some_and(|(_, rest)| {
                rest.split(['/', '?', '#'])
                    .next()
                    .is_some_and(|authority| !authority.is_empty() && !authority.contains('@'))
            })
    {
        return Err(invalid());
    }
    Ok(request)
}

fn endpoint(base: &str) -> Result<reqwest::Url, ProviderRequestError> {
    let invalid = || ProviderRequestError::new("temporarily_unavailable");
    let mut url = reqwest::Url::parse(base).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.authority().contains('@')
        || url.query().is_some()
        || url.fragment().is_some()
        || (url.path() != "/" && !url.path().is_empty())
    {
        return Err(invalid());
    }
    url.set_path("/api/v1/provider-requests");
    Ok(url)
}

async fn send_provider_request(
    client: &reqwest::Client,
    url: reqwest::Url,
    payload: &ServerRequest<'_>,
) -> Result<(), ProviderRequestError> {
    let mut response = client
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|_| ProviderRequestError::new("send_failed"))?;
    match response.status().as_u16() {
        200 | 201 => {}
        400 | 415 => return Err(ProviderRequestError::new("invalid_request")),
        409 => return Err(ProviderRequestError::new("request_id_conflict")),
        413 => return Err(ProviderRequestError::new("request_too_large")),
        429 => {
            return Err(ProviderRequestError {
                code: "rate_limited",
                retry_after_seconds: response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u32>().ok()),
            })
        }
        404 | 503 | 500..=599 => return Err(ProviderRequestError::new("temporarily_unavailable")),
        _ => return Err(ProviderRequestError::new("send_failed")),
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(ProviderRequestError::new("send_failed"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ProviderRequestError::new("send_failed"))?
    {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(ProviderRequestError::new("send_failed"));
        }
        body.extend_from_slice(&chunk);
    }
    #[derive(Deserialize)]
    struct Confirmation {
        request_id: String,
        status: String,
    }
    let result: Confirmation =
        serde_json::from_slice(&body).map_err(|_| ProviderRequestError::new("send_failed"))?;
    if result.request_id != payload.request_id || result.status != "pending" {
        return Err(ProviderRequestError::new("send_failed"));
    }
    Ok(())
}

fn normalize_device_model(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 128 || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.to_owned())
}

#[cfg(target_os = "macos")]
async fn read_device_model() -> Option<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.model"])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    normalize_device_model(std::str::from_utf8(&output.stdout).ok()?)
}

#[cfg(target_os = "linux")]
async fn read_device_model() -> Option<String> {
    for path in [
        "/sys/class/dmi/id/product_name",
        "/sys/firmware/devicetree/base/model",
    ] {
        if let Ok(value) = tokio::fs::read_to_string(path).await {
            if let Some(model) = normalize_device_model(value.trim_end_matches('\0')) {
                return Some(model);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
async fn read_device_model() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
        let key = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"HARDWARE\DESCRIPTION\System\BIOS")
            .ok()?;
        let value: String = key.get_value("SystemProductName").ok()?;
        normalize_device_model(&value)
    })
    .await
    .ok()
    .flatten()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
async fn read_device_model() -> Option<String> {
    None
}

async fn device_model() -> Option<&'static str> {
    static MODEL: tokio::sync::OnceCell<Option<String>> = tokio::sync::OnceCell::const_new();
    MODEL.get_or_init(read_device_model).await.as_deref()
}

#[tauri::command]
pub async fn submit_provider_request(request: ProviderRequest) -> Result<(), ProviderRequestError> {
    let request = validate_request(request)?;
    let url = endpoint(API_BASE_URL)?;
    let payload = ServerRequest {
        request_id: &request.request_id,
        name: &request.name,
        website_url: &request.website_url,
        description: &request.description,
        app_version: env!("CARGO_PKG_VERSION"),
        device_model: device_model().await,
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ProviderRequestError::new("send_failed"))?;
    send_provider_request(&client, url, &payload).await
}

#[cfg(test)]
#[path = "provider_request_tests.rs"]
mod tests;
