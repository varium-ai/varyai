//! Cursor subscription credentials and execution through the pinned MIT protocol core.
use super::*;
use futures::StreamExt;
mod worker;

fn checked_auth(value: &Value) -> Result<Value, String> {
    let token = match value["type"].as_str() {
        Some("oauth") => value["access"].as_str(),
        Some("api") => value["key"].as_str(),
        _ => None,
    }
    .filter(|token| {
        !token.is_empty() && token.len() <= 32768 && !token.chars().any(char::is_control)
    })
    .ok_or(EXPIRED)?;
    if value["refresh"] == "cursor-agent" {
        return Err(EXPIRED.into());
    }
    let expires = value["expires"].as_i64().filter(|at| *at > 0).or_else(|| {
        let payload = token.split('.').nth(1)?;
        let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
        claims["exp"].as_i64()?.checked_mul(1000)
    });
    if value["type"] == "oauth"
        && expires.is_some_and(|at| at <= chrono::Utc::now().timestamp_millis())
    {
        return Err(EXPIRED.into());
    }
    Ok(value.clone())
}
pub(super) fn auth(account: &Account) -> Result<Value, String> {
    checked_auth(&serde_json::from_str::<Value>(&account.access).map_err(|_| EXPIRED)?)
}
fn account(value: Value, target: Option<&Account>) -> Result<Account, String> {
    let value = checked_auth(&value)?;
    let login = value["accountId"]
        .as_str()
        .filter(|login| crate::services::routing_upstreams::valid_identity(login))
        .ok_or(FAILED)?
        .to_owned();
    let subject = login.to_ascii_lowercase();
    // Cursor's opaque auth record is backend-only and never used as a bearer header.
    Ok(Account {
        id: target
            .map(|a| a.id.clone())
            .unwrap_or_else(|| format!("cursor-{:x}", Sha256::digest(subject.as_bytes()))),
        provider: SubscriptionProvider::Cursor,
        login,
        subject,
        access: value.to_string(),
        plan: value["plan"]
            .as_str()
            .filter(|s| s.len() <= 128 && !s.chars().any(char::is_control))
            .map(|s| format!("Cursor {s}")),
        project: String::new(),
        claude_home: None,
        reauth_required: false,
        generation: String::new(),
        refresh: String::new(),
        expires: value["expires"].as_i64().unwrap_or(0) / 1000,
        client_id: String::new(),
        id_token: String::new(),
    })
}
struct LoginTicket {
    worker: Arc<worker::Worker>,
    id: String,
}
impl Drop for LoginTicket {
    fn drop(&mut self) {
        let worker = Arc::clone(&self.worker);
        let id = self.id.clone();
        tokio::spawn(async move {
            let _ = worker.json("/login/cancel", &json!({"id":id})).await;
        });
    }
}
pub(super) async fn start(target: Option<Account>) -> Result<Flow, String> {
    let worker = worker::get().await?;
    let pending = worker.json("/login/start", &json!({})).await?;
    let ticket = LoginTicket {
        worker,
        id: pending["id"].as_str().ok_or(FAILED)?.into(),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let mut registry = flows().lock().await;
    registry.retain(|_, flow| flow.state == "waiting");
    if registry.len() >= 8 {
        return Err(FAILED.into());
    }
    let mut flow = Flow {
        id: id.clone(),
        url: Some(pending["url"].as_str().ok_or(FAILED)?.into()),
        state: "waiting".into(),
        account: None,
        error: None,
        task: None,
        callback: None,
    };
    registry.insert(id.clone(), flow.clone());
    let task = tokio::spawn(async move {
        let result = tokio::time::timeout(Duration::from_secs(300), async {
            loop {
                let result = ticket
                    .worker
                    .json("/login/poll", &json!({"id":ticket.id}))
                    .await?;
                match result["state"].as_str() {
                    Some("done") => {
                        let verified = ticket
                            .worker
                            .json("/identity", &json!({"auth":result["auth"]}))
                            .await?;
                        return keep_login(
                            account(verified, target.as_ref())?,
                            target.as_ref(),
                            &id,
                        )
                        .await;
                    }
                    Some("waiting") => tokio::time::sleep(Duration::from_millis(750)).await,
                    _ => return Err(FAILED.into()),
                }
            }
        })
        .await
        .unwrap_or_else(|_| Err(EXPIRED.into()));
        if let Some(flow) = flows().lock().await.get_mut(&id) {
            flow.task = None;
            match result {
                Ok(account) => {
                    flow.state = "done".into();
                    flow.account = Some(account);
                }
                Err(error) => {
                    flow.state = "failed".into();
                    flow.error = Some(error);
                }
            }
        }
    });
    flow.task = Some(task.abort_handle());
    registry.insert(flow.id.clone(), flow.clone());
    Ok(flow)
}
fn imported(value: Value) -> Result<Value, String> {
    if value["type"].is_string() {
        return checked_auth(&value);
    }
    let access = value["accessToken"]
        .as_str()
        .or_else(|| value["access_token"].as_str())
        .ok_or(FAILED)?;
    checked_auth(
        &json!({"type":"oauth","access":access,"refresh":"none","expires":value["expiresAt"].as_i64()}),
    )
}
pub(super) async fn import(path: Option<String>) -> Result<AccountSummary, String> {
    let value = tokio::task::spawn_blocking(move || {
        #[cfg(all(target_os = "macos", not(test)))]
        if path.is_none() {
            if let Ok(output) = std::process::Command::new("security")
                .args([
                    "find-generic-password",
                    "-s",
                    "cursor-access-token",
                    "-a",
                    "cursor-user",
                    "-w",
                ])
                .output()
            {
                if output.status.success() {
                    let token = String::from_utf8(output.stdout).map_err(|_| FAILED)?;
                    return Ok(json!({"type":"oauth","access":token.trim(),"refresh":"none"}));
                }
            }
        }
        let path = path.map(PathBuf::from).unwrap_or_else(|| {
            #[cfg(target_os = "windows")]
            {
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("AppData/Roaming"))
                    .join("Cursor/auth.json")
            }
            #[cfg(target_os = "macos")]
            {
                dirs::home_dir()
                    .unwrap_or_default()
                    .join(".cursor/auth.json")
            }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
                    .join("cursor/auth.json")
            }
        });
        let file = std::fs::File::open(path).map_err(|_| FAILED)?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(file, 1024 * 1024 + 1), &mut bytes)
            .map_err(|_| FAILED)?;
        if bytes.len() > 1024 * 1024 {
            return Err(FAILED);
        }
        serde_json::from_slice::<Value>(&bytes).map_err(|_| FAILED)
    })
    .await
    .map_err(|_| FAILED)??;
    let auth = imported(value)?;
    let verified = worker::get()
        .await?
        .json("/identity", &json!({"auth":auth}))
        .await?;
    keep(account(verified, None)?).await
}
pub(super) async fn forget(account: &Account) {
    if let Some(worker) = worker::running().await {
        if let Ok(auth) = serde_json::from_str::<Value>(&account.access) {
            let _ = worker.json("/forget", &json!({"auth":auth})).await;
        }
    }
}
pub(super) async fn call(account: &Account, path: &str) -> Result<Value, String> {
    let result = worker::get()
        .await?
        .json(path, &json!({"auth":auth(account)?}))
        .await;
    if result.as_ref().is_err_and(|error| error == EXPIRED) {
        observe_rejection(account).await;
    }
    result
}
pub(crate) async fn chat(
    account: &Account,
    chat: Value,
    session: Option<&str>,
) -> Result<PinStream, crate::proxy::ProxyError> {
    use crate::proxy::ProxyError;
    let unavailable =
        || ProxyError::ConfigError("Cursor subscription runtime is unavailable".into());
    let worker = worker::get().await.map_err(|_| unavailable())?;
    let auth = auth(account)
        .map_err(|_| ProxyError::AuthError("Cursor authorization is unavailable".into()))?;
    let response = worker
        .request("/chat", &json!({"auth":auth,"chat":chat,"session":session}))
        .await
        .map_err(|_| unavailable())?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        observe_rejection(account).await;
    }
    if !response.status().is_success() {
        return Err(ProxyError::UpstreamError {
            status: response.status().as_u16(),
            body: None,
        });
    }
    let mut stream = response.bytes_stream();
    Ok(Box::pin(async_stream::stream! {
        let _worker = worker;
        while let Some(chunk) = stream.next().await {
            yield chunk.map_err(|_| std::io::Error::other("Cursor subscription stream interrupted"));
        }
    }))
}
type PinStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send>>;

pub(super) fn quota(
    value: &Value,
) -> Result<super::super::subscription::SubscriptionQuota, String> {
    use super::super::subscription::{CredentialStatus, QuotaTier, SubscriptionQuota};
    if value.get("error").is_some() {
        return Err(FAILED.into());
    }
    let windows = value["windows"].as_array().ok_or(FAILED)?;
    let tiers = windows
        .iter()
        .filter_map(|w| {
            let utilization = w["used"].as_f64().filter(|v| v.is_finite() && *v >= 0.0)?;
            let name = w["name"].as_str()?.to_owned();
            Some(QuotaTier {
                display: w["display"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
                    .map(str::to_owned),
                name,
                utilization: utilization.clamp(0.0, 100.0),
                resets_at: w["resetsAt"].as_str().map(str::to_owned),
                used_value_usd: w["amount"].as_f64().filter(|v| v.is_finite() && *v >= 0.0),
                max_value_usd: w["limit"].as_f64().filter(|v| v.is_finite() && *v > 0.0),
            })
        })
        .collect();
    Ok(SubscriptionQuota {
        tool: "subscription".into(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        codex: None,
        error: None,
        queried_at: Some(chrono::Utc::now().timestamp_millis()),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_snapshots_and_identity_controls_reauthentication() {
        assert!(
            imported(json!({"type":"oauth","access":"synthetic","refresh":"cursor-agent"}))
                .is_err()
        );
        let first = account(
            json!({"type":"oauth","access":"synthetic-a","accountId":"a@example.invalid"}),
            None,
        )
        .expect("fixture");
        let second = account(
            json!({"type":"api","key":"synthetic-b","accountId":"b@example.invalid"}),
            Some(&first),
        )
        .expect("fixture");
        assert!(!reauth_matches(&first, &second));
        assert!(first.summary().login.contains("example.invalid"));
        assert!(serde_json::to_string(&first.summary())
            .expect("summary")
            .find("synthetic-a")
            .is_none());
    }
    #[test]
    fn missing_usage_is_not_zero_allowance() {
        assert!(quota(&json!({"error":"synthetic","windows":[]})).is_err());
        assert!(quota(&json!({})).is_err());
        let q = quota(&json!({"windows":[{"name":"Weekly","used":25,"amount":5,"limit":20},{"name":"Unknown"}]})).expect("quota");
        assert_eq!(q.tiers.len(), 1);
        assert_eq!(q.tiers[0].utilization, 25.0);
    }
}
