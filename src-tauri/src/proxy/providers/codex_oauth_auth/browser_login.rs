//! Browser authorization shares managed-account commits and cancellation guards with device login.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const LOGIN_FAILED: &str = "codexBrowser.failed";
const CALLBACK_INVALID: &str = "codexBrowser.callbackInvalid";
const REDIRECT: &str = "http://localhost:1455/auth/callback";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserLoginStatus {
    pub id: String,
    pub url: String,
    pub status: String,
    pub callback_blocked: bool,
    pub account: Option<GitHubAccount>,
    pub error: Option<String>,
}
pub(super) struct BrowserLogin {
    response: BrowserLoginStatus,
    state: String,
    verifier: String,
    redirect: String,
    client: reqwest::Client,
    code: Option<String>,
    exchanging: bool,
    expires: i64,
    listener: Option<tokio::task::JoinHandle<()>>,
}

pub(crate) fn login_client(proxy: Option<&str>) -> Result<reqwest::Client, String> {
    if proxy.is_none_or(str::is_empty) {
        return crate::proxy::http_client::get_without_redirects().map_err(|_| LOGIN_FAILED.into());
    }
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(OAUTH_HTTP_TIMEOUT);
    if proxy == Some("direct") {
        builder = builder.no_proxy();
    } else {
        let url =
            url::Url::parse(proxy.ok_or(LOGIN_FAILED)?).map_err(|_| "codexBrowser.proxyInvalid")?;
        if !["http", "https", "socks5", "socks5h"].contains(&url.scheme())
            || url.host_str().is_none()
        {
            return Err("codexBrowser.proxyInvalid".into());
        }
        builder = builder
            .proxy(reqwest::Proxy::all(url.as_str()).map_err(|_| "codexBrowser.proxyInvalid")?);
    }
    builder.build().map_err(|_| LOGIN_FAILED.into())
}

fn authorization_url(redirect: &str, state: &str, verifier: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url =
        url::Url::parse("https://auth.openai.com/oauth/authorize").map_err(|_| LOGIN_FAILED)?;
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", CODEX_CLIENT_ID),
        ("redirect_uri", redirect),
        (
            "scope",
            "openid profile email offline_access api.connectors.read api.connectors.invoke",
        ),
        ("state", state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "codex_cli_rs"),
    ]);
    Ok(url.into())
}
fn callback_code(value: &str, redirect: &str, state: &str) -> Result<String, String> {
    if value.len() > 16384 || value.chars().any(char::is_control) {
        return Err(CALLBACK_INVALID.into());
    }
    let url = url::Url::parse(value).map_err(|_| CALLBACK_INVALID)?;
    let expected = url::Url::parse(redirect).map_err(|_| CALLBACK_INVALID)?;
    if url.scheme() != expected.scheme()
        || url.host_str() != expected.host_str()
        || url.port_or_known_default() != expected.port_or_known_default()
        || url.path() != expected.path()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(CALLBACK_INVALID.into());
    }
    let mut query = HashMap::new();
    for (key, value) in url.query_pairs() {
        if query.insert(key.into_owned(), value.into_owned()).is_some() {
            return Err(CALLBACK_INVALID.into());
        }
    }
    if query.get("state").is_none_or(|value| value != state) || query.contains_key("error") {
        return Err(CALLBACK_INVALID.into());
    }
    query
        .remove("code")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CALLBACK_INVALID.into())
}

impl CodexOAuthManager {
    pub(crate) async fn start_browser_login(
        self: &Arc<Self>,
        target: Option<String>,
        proxy: Option<String>,
    ) -> Result<BrowserLoginStatus, String> {
        self.start_browser_login_on(target, proxy, 1455).await
    }
    async fn start_browser_login_on(
        self: &Arc<Self>,
        target: Option<String>,
        proxy: Option<String>,
        port: u16,
    ) -> Result<BrowserLoginStatus, String> {
        let client = login_client(proxy.as_deref())?;
        let epoch = self.login_epoch.load(Ordering::Acquire);
        let generation = if let Some(id) = target.as_deref() {
            if !self.accounts.read().await.contains_key(id) {
                return Err(LOGIN_FAILED.into());
            }
            let next = self
                .next_target_login_generation
                .fetch_add(1, Ordering::AcqRel)
                .wrapping_add(1);
            self.target_login_generations
                .write()
                .await
                .entry(id.into())
                .and_modify(|current| *current = (*current).max(next))
                .or_insert(next);
            Some(next)
        } else {
            None
        };
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .ok();
        let redirect = if port == 0 {
            format!(
                "http://localhost:{}/auth/callback",
                listener
                    .as_ref()
                    .ok_or(LOGIN_FAILED)?
                    .local_addr()
                    .map_err(|_| LOGIN_FAILED)?
                    .port()
            )
        } else {
            REDIRECT.into()
        };
        let id = Uuid::new_v4().to_string();
        let state = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let response = BrowserLoginStatus {
            id: id.clone(),
            url: authorization_url(&redirect, &state, &verifier)?,
            status: "waiting".into(),
            callback_blocked: listener.is_none(),
            account: None,
            error: None,
        };
        let expires = chrono::Utc::now().timestamp_millis() + 300_000;
        self.register_pending_device_code(
            id.clone(),
            String::new(),
            expires,
            epoch,
            target,
            generation,
        )
        .await
        .map_err(|_| LOGIN_FAILED)?;
        {
            let mut flows = self.browser_logins.lock().await;
            let now = chrono::Utc::now().timestamp_millis();
            flows.retain(|_, flow| {
                if flow.expires <= now || flow.response.status != "waiting" {
                    if let Some(task) = &flow.listener {
                        task.abort();
                    }
                    false
                } else {
                    true
                }
            });
            if flows.len() >= 8 {
                self.cancel_device_flow(&id).await;
                return Err(LOGIN_FAILED.into());
            }
            flows.insert(
                id.clone(),
                BrowserLogin {
                    response: response.clone(),
                    state,
                    verifier,
                    redirect: redirect.clone(),
                    client,
                    code: None,
                    exchanging: false,
                    expires,
                    listener: None,
                },
            );
        }
        if let Some(listener) = listener {
            let manager = Arc::clone(self);
            let flow_id = id.clone();
            let task = tokio::spawn(async move {
                let serve = async {
                    loop {
                        let Ok((mut socket, peer)) = listener.accept().await else {
                            return;
                        };
                        if !peer.ip().is_loopback() {
                            continue;
                        }
                        let mut bytes = Vec::new();
                        loop {
                            let mut chunk = [0u8; 1024];
                            let Ok(Ok(n)) = tokio::time::timeout(
                                Duration::from_secs(3),
                                socket.read(&mut chunk),
                            )
                            .await
                            else {
                                break;
                            };
                            if n == 0 {
                                break;
                            }
                            bytes.extend_from_slice(&chunk[..n]);
                            if bytes.len() > 16384
                                || bytes.windows(4).any(|part| part == b"\r\n\r\n")
                            {
                                break;
                            }
                        }
                        let target = std::str::from_utf8(&bytes)
                            .ok()
                            .and_then(|text| text.lines().next())
                            .and_then(|line| line.strip_prefix("GET "))
                            .and_then(|line| line.split_whitespace().next());
                        let valid = if let Some(target) =
                            target.filter(|target| target.starts_with("/auth/callback?"))
                        {
                            let value = format!(
                                "{}{}",
                                redirect.trim_end_matches("/auth/callback"),
                                target
                            );
                            manager
                                .submit_browser_callback(&flow_id, &value)
                                .await
                                .is_ok()
                        } else {
                            false
                        };
                        let reply = if valid {
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nVary AI received the sign-in. Return to Vary AI."
                        } else {
                            "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid sign-in callback."
                        };
                        let _ = socket.write_all(reply.as_bytes()).await;
                        if valid {
                            return;
                        }
                    }
                };
                let _ = tokio::time::timeout(Duration::from_secs(300), serve).await;
            });
            if let Some(flow) = self.browser_logins.lock().await.get_mut(&id) {
                flow.listener = Some(task);
            }
        }
        Ok(response)
    }
    pub(crate) async fn submit_browser_callback(
        &self,
        id: &str,
        callback: &str,
    ) -> Result<(), String> {
        let mut flows = self.browser_logins.lock().await;
        let flow = flows.get_mut(id).ok_or(CALLBACK_INVALID)?;
        if flow.expires <= chrono::Utc::now().timestamp_millis()
            || flow.exchanging
            || flow.code.is_some()
            || flow.response.status != "waiting"
        {
            return Err(CALLBACK_INVALID.into());
        }
        flow.code = Some(callback_code(callback, &flow.redirect, &flow.state)?);
        Ok(())
    }
    pub(crate) async fn cancel_browser_login(&self, id: &str) -> bool {
        let cancelled = self.cancel_device_flow(id).await;
        let removed = self.browser_logins.lock().await.remove(id);
        if let Some(flow) = removed {
            if let Some(task) = flow.listener {
                task.abort();
                let _ = task.await;
            }
        }
        cancelled
    }
    pub(crate) async fn poll_browser_login<BeforeCommit, CommitFuture, CommitGuard>(
        &self,
        id: &str,
        before_commit: BeforeCommit,
    ) -> Result<BrowserLoginStatus, String>
    where
        BeforeCommit: FnOnce() -> CommitFuture,
        CommitFuture: std::future::Future<Output = CommitGuard>,
    {
        self.poll_browser_login_at(id, before_commit, OAUTH_TOKEN_URL)
            .await
    }
    async fn poll_browser_login_at<BeforeCommit, CommitFuture, CommitGuard>(
        &self,
        id: &str,
        before_commit: BeforeCommit,
        token_url: &str,
    ) -> Result<BrowserLoginStatus, String>
    where
        BeforeCommit: FnOnce() -> CommitFuture,
        CommitFuture: std::future::Future<Output = CommitGuard>,
    {
        let (code, verifier, redirect, client) = {
            let mut flows = self.browser_logins.lock().await;
            let flow = flows.get_mut(id).ok_or(LOGIN_FAILED)?;
            if flow.expires <= chrono::Utc::now().timestamp_millis() {
                flow.response.status = "failed".into();
                flow.response.error = Some("codexBrowser.expired".into());
            }
            if flow.response.status != "waiting" || flow.exchanging || flow.code.is_none() {
                return Ok(flow.response.clone());
            }
            flow.exchanging = true;
            (
                flow.code.take().ok_or(LOGIN_FAILED)?,
                flow.verifier.clone(),
                flow.redirect.clone(),
                flow.client.clone(),
            )
        };
        let result = async {
            let pending = self
                .pending_device_codes
                .read()
                .await
                .get(id)
                .cloned()
                .ok_or(LOGIN_FAILED)?;
            let response = client
                .post(token_url)
                .header("User-Agent", CODEX_USER_AGENT)
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", code.as_str()),
                    ("redirect_uri", redirect.as_str()),
                    ("client_id", CODEX_CLIENT_ID),
                    ("code_verifier", verifier.as_str()),
                ])
                .send()
                .await
                .map_err(|_| LOGIN_FAILED)?;
            if !response.status().is_success() {
                return Err(LOGIN_FAILED);
            }
            let tokens: OAuthTokenResponse = response.json().await.map_err(|_| LOGIN_FAILED)?;
            let (workspace, email) = extract_account_metadata_from_tokens(&tokens);
            let workspace = workspace.ok_or(LOGIN_FAILED)?;
            let refresh = tokens
                .refresh_token
                .filter(|value| !value.trim().is_empty())
                .ok_or(LOGIN_FAILED)?;
            let id_token = tokens
                .id_token
                .filter(|value| {
                    crate::codex_config::extract_codex_id_token_subject(value).is_some()
                })
                .ok_or(LOGIN_FAILED)?;
            let _guard = before_commit().await;
            self.add_account_internal(
                workspace,
                refresh,
                email,
                Some(id_token),
                Some(CachedAccessToken {
                    token: tokens.access_token,
                    expires_at_ms: compute_expires_at_ms(tokens.expires_in),
                    obtained_at_ms: chrono::Utc::now().timestamp_millis(),
                }),
                AccountLoginContext {
                    target_account_id: pending.target_account_id.as_deref(),
                    pending_device_code: Some(id),
                    target_generation: pending.target_generation,
                },
            )
            .await
            .map_err(|error| match error {
                CodexOAuthError::DuplicateAccount => "codexBrowser.duplicate",
                _ => LOGIN_FAILED,
            })
        }
        .await;
        if result.is_err() {
            self.cancel_device_flow(id).await;
        }
        let mut flows = self.browser_logins.lock().await;
        let flow = flows.get_mut(id).ok_or(LOGIN_FAILED)?;
        match result {
            Ok(account) => {
                flow.response.status = "done".into();
                flow.response.account = Some(account);
            }
            Err(error) => {
                flow.response.status = "failed".into();
                flow.response.error = Some(error.into());
            }
        }
        flow.exchanging = false;
        if let Some(task) = flow.listener.take() {
            task.abort();
        }
        // Authorization codes and PKCE verifiers stay backend-only and leave memory after use.
        flow.verifier.clear();
        Ok(flow.response.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn browser_login_validates_callback_and_cancels_without_persisting(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(temp.path().to_owned()));
        let flow = manager
            .start_browser_login_on(None, Some("direct".into()), 0)
            .await?;
        let url = url::Url::parse(&flow.url)?;
        assert!(url.host_str() == Some("auth.openai.com"));
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert!(query
            .get("code_challenge_method")
            .is_some_and(|value| value == "S256"));
        let redirect = query.get("redirect_uri").ok_or("redirect")?;
        let state = query.get("state").ok_or("state")?;
        for bad in [
            format!("https://example.test/auth/callback?state={state}&code=synthetic"),
            format!("{redirect}?state=wrong&code=synthetic"),
            format!("{redirect}?state={state}&state={state}&code=synthetic"),
        ] {
            assert!(manager
                .submit_browser_callback(&flow.id, &bad)
                .await
                .is_err());
        }
        manager
            .submit_browser_callback(
                &flow.id,
                &format!("{redirect}?state={state}&code=synthetic"),
            )
            .await?;
        assert!(manager
            .submit_browser_callback(&flow.id, &format!("{redirect}?state={state}&code=other"))
            .await
            .is_err());
        assert!(manager.cancel_browser_login(&flow.id).await);
        assert!(manager
            .poll_browser_login(&flow.id, || async {})
            .await
            .is_err());
        assert!(manager.list_accounts().await.is_empty());
        Ok(())
    }
}

#[cfg(test)]
mod exchange_tests {
    use super::*;
    fn tokens() -> serde_json::Value {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::json!({"sub":"fixture-user","email":"fixture@example.test","https://api.openai.com/auth":{"chatgpt_account_id":"fixture-workspace"}}).to_string());
        serde_json::json!({"access_token":"synthetic-browser-access","refresh_token":"synthetic-browser-refresh","id_token":format!("{header}.{payload}."),"expires_in":3600})
    }
    #[tokio::test]
    async fn automatic_callback_exchanges_pkce_once_and_survives_reload(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(temp.path().to_owned()));
        let flow = manager
            .start_browser_login_on(None, Some("direct".into()), 0)
            .await?;
        let url = url::Url::parse(&flow.url)?;
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        let redirect = query.get("redirect_uri").ok_or("redirect")?;
        let state = query.get("state").ok_or("state")?;
        let verifier = manager
            .browser_logins
            .lock()
            .await
            .get(&flow.id)
            .ok_or("flow")?
            .verifier
            .clone();
        let callback = url::Url::parse(redirect)?;
        let mut socket = tokio::net::TcpStream::connect((
            std::net::Ipv4Addr::LOCALHOST,
            callback.port().ok_or("port")?,
        ))
        .await?;
        socket.write_all(format!("GET /auth/callback?code=synthetic-code&state={state} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes()).await?;
        let mut reply = Vec::new();
        socket.read_to_end(&mut reply).await?;
        assert!(reply.starts_with(b"HTTP/1.1 200"));
        assert!(manager
            .submit_browser_callback(&flow.id, &format!("{redirect}?code=another&state={state}"))
            .await
            .is_err());
        let expected_redirect = redirect.clone();
        let app = axum::Router::new().route(
            "/token",
            axum::routing::post(
                move |axum::Form(form): axum::Form<HashMap<String, String>>| {
                    let verifier = verifier.clone();
                    let redirect = expected_redirect.clone();
                    async move {
                        assert!(form.get("code_verifier") == Some(&verifier));
                        assert!(form.get("redirect_uri") == Some(&redirect));
                        assert!(form
                            .get("code")
                            .is_some_and(|code| code == "synthetic-code"));
                        axum::Json(tokens())
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}/token", listener.local_addr()?);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let completed = manager
            .poll_browser_login_at(&flow.id, || async {}, &endpoint)
            .await?;
        server.abort();
        assert!(completed.status == "done" && manager.list_accounts().await.len() == 1);
        let again = manager
            .poll_browser_login_at(&flow.id, || async {}, &endpoint)
            .await?;
        assert!(again.status == "done" && manager.list_accounts().await.len() == 1);
        let public = serde_json::to_string(&completed)?;
        assert!(
            !public.contains("synthetic-browser-access")
                && !public.contains("synthetic-browser-refresh")
        );
        let reloaded = CodexOAuthManager::new(temp.path().to_owned());
        assert!(reloaded.list_accounts().await.len() == 1);
        Ok(())
    }
    #[tokio::test]
    async fn cancellation_during_exchange_prevents_account_commit(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(temp.path().to_owned()));
        let flow = manager
            .start_browser_login_on(None, Some("direct".into()), 0)
            .await?;
        let query: HashMap<_, _> = url::Url::parse(&flow.url)?
            .query_pairs()
            .into_owned()
            .collect();
        let redirect = query.get("redirect_uri").ok_or("redirect")?;
        let state = query.get("state").ok_or("state")?;
        manager
            .submit_browser_callback(
                &flow.id,
                &format!("{redirect}?code=synthetic-code&state={state}"),
            )
            .await?;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let seen = entered.clone();
        let proceed = release.clone();
        let app = axum::Router::new().route(
            "/token",
            axum::routing::post(move || {
                let seen = seen.clone();
                let proceed = proceed.clone();
                async move {
                    seen.notify_one();
                    proceed.notified().await;
                    axum::Json(tokens())
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}/token", listener.local_addr()?);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let pending_manager = manager.clone();
        let id = flow.id.clone();
        let pending = tokio::spawn(async move {
            pending_manager
                .poll_browser_login_at(&id, || async {}, &endpoint)
                .await
        });
        tokio::time::timeout(Duration::from_secs(3), entered.notified()).await?;
        assert!(manager.cancel_browser_login(&flow.id).await);
        release.notify_one();
        assert!(pending.await?.is_err());
        server.abort();
        assert!(manager.list_accounts().await.is_empty());
        assert!(!temp.path().join("codex_oauth_auth.json").exists());
        Ok(())
    }
}
