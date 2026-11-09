//! Browser authorization shares managed-account commits and cancellation guards with device login.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const LOGIN_FAILED: &str = "codexBrowser.failed";
const CALLBACK_INVALID: &str = "codexBrowser.callbackInvalid";
#[cfg(test)]
const REDIRECT: &str = "http://localhost:1455/auth/callback";
const BROWSER_LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
#[cfg(test)]
const MAX_TOKEN_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserLoginStatus {
    pub id: String,
    pub url: String,
    pub status: String,
    pub callback_blocked: bool,
    pub account: Option<GitHubAccount>,
    pub error: Option<String>,
    pub diagnostic: Option<browser_session::Diagnostic>,
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
    worker: Option<tokio::task::JoinHandle<()>>,
    ready: Arc<tokio::sync::Notify>,
    changed: tokio::sync::watch::Sender<()>,
    proxy: Option<String>,
    proxy_selected: bool,
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
    let loopback_alias = match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip == std::net::Ipv4Addr::LOCALHOST,
        Some(url::Host::Ipv6(ip)) => ip == std::net::Ipv6Addr::LOCALHOST,
        _ => false,
    };
    if url.scheme() != expected.scheme()
        || !loopback_alias
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
    if query.get("state").is_none_or(|value| value != state) {
        return Err(CALLBACK_INVALID.into());
    }
    if query.contains_key("error") {
        return Ok(String::new());
    }
    query
        .remove("code")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CALLBACK_INVALID.into())
}

async fn listen_callback(port: u16) -> Option<tokio::net::TcpListener> {
    for attempt in 0..10 {
        match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await {
            Ok(listener) => return Some(listener),
            Err(_) if port == 0 => return None,
            Err(_) if attempt == 0 => {
                // Match Codex/Magpie: an abandoned login server accepts /cancel.
                if let Ok(client) = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(Duration::from_secs(2))
                    .build()
                {
                    let _ = client
                        .get(format!("http://127.0.0.1:{port}/cancel"))
                        .send()
                        .await;
                }
            }
            Err(_) => {}
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    None
}

impl CodexOAuthManager {
    pub(crate) async fn start_browser_login<BeforeCommit, CommitFuture, CommitGuard>(
        self: &Arc<Self>,
        target: Option<String>,
        proxy: Option<String>,
        before_commit: BeforeCommit,
    ) -> Result<BrowserLoginStatus, String>
    where
        BeforeCommit: FnOnce() -> CommitFuture + Send + 'static,
        CommitFuture: std::future::Future<Output = CommitGuard> + Send,
        CommitGuard: Send,
    {
        crate::codex_client::resolve_version().await;
        let ids: Vec<_> = self.browser_logins.lock().await.keys().cloned().collect();
        for id in ids {
            self.cancel_browser_login(&id).await;
        }
        let response = self.start_browser_login_on(target, proxy, 1455).await?;
        self.start_browser_worker(&response.id, OAUTH_TOKEN_URL.into(), before_commit)
            .await?;
        Ok(response)
    }
    async fn start_browser_login_on(
        self: &Arc<Self>,
        target: Option<String>,
        proxy: Option<String>,
        port: u16,
    ) -> Result<BrowserLoginStatus, String> {
        let proxy_selected = proxy.is_some();
        let proxy = if let Some(proxy) = proxy {
            Some(proxy.trim().to_owned()).filter(|proxy| !proxy.is_empty())
        } else if let Some(target) = target.as_deref() {
            self.account_proxy(target).await?
        } else {
            None
        };
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
        let listener = listen_callback(port).await;
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
            format!("http://localhost:{port}/auth/callback")
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
            diagnostic: None,
        };
        let expires =
            chrono::Utc::now().timestamp_millis() + BROWSER_LOGIN_TIMEOUT.as_millis() as i64;
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
                    if let Some(task) = &flow.worker {
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
                    worker: None,
                    ready: Arc::new(tokio::sync::Notify::new()),
                    changed: tokio::sync::watch::channel(()).0,
                    proxy,
                    proxy_selected,
                },
            );
        }
        if let Some(listener) = listener {
            let manager = Arc::clone(self);
            let flow_id = id.clone();
            let mut changed = self
                .browser_logins
                .lock()
                .await
                .get(&id)
                .ok_or(LOGIN_FAILED)?
                .changed
                .subscribe();
            let task = tokio::spawn(async move {
                let serve = async {
                    loop {
                        let accepted = tokio::select! {
                            accepted = listener.accept() => accepted,
                            _ = changed.changed() => return,
                        };
                        let Ok((mut socket, peer)) = accepted else {
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
                        if target == Some("/cancel") {
                            let _ = manager
                                .fail_browser_login(&flow_id, "codexBrowser.expired")
                                .await;
                            let _ = socket
                                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                                .await;
                            return;
                        }
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
                            let status = manager.wait_browser_login(&flow_id).await;
                            if status.as_ref().is_ok_and(|status| status.status == "done") {
                                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nYou're signed in. Your account was added to Vary AI. You can close this tab.".to_owned()
                            } else {
                                let detail = status
                                    .ok()
                                    .map(|s| failure_message(&s))
                                    .unwrap_or_else(|| "Return to Vary AI to retry.".into());
                                format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nSign-in didn't finish. {detail}")
                            }
                        } else {
                            "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid sign-in callback.".into()
                        };
                        let _ = socket.write_all(reply.as_bytes()).await;
                        if valid {
                            return;
                        }
                    }
                };
                let _ =
                    tokio::time::timeout(BROWSER_LOGIN_TIMEOUT + OAUTH_HTTP_TIMEOUT, serve).await;
            });
            if let Some(flow) = self.browser_logins.lock().await.get_mut(&id) {
                flow.listener = Some(task);
            }
        }
        Ok(response)
    }

    async fn start_browser_worker<BeforeCommit, CommitFuture, CommitGuard>(
        self: &Arc<Self>,
        id: &str,
        endpoint: String,
        before_commit: BeforeCommit,
    ) -> Result<(), String>
    where
        BeforeCommit: FnOnce() -> CommitFuture + Send + 'static,
        CommitFuture: std::future::Future<Output = CommitGuard> + Send,
        CommitGuard: Send,
    {
        let mut flows = self.browser_logins.lock().await;
        let flow = flows.get_mut(id).ok_or(LOGIN_FAILED)?;
        let ready = flow.ready.clone();
        let remaining = Duration::from_millis(
            flow.expires
                .saturating_sub(chrono::Utc::now().timestamp_millis())
                .max(0) as u64,
        );
        let manager = Arc::clone(self);
        let id = id.to_owned();
        flow.worker = Some(tokio::spawn(async move {
            if tokio::time::timeout(remaining, ready.notified())
                .await
                .is_err()
            {
                let _ = manager
                    .fail_browser_login(&id, "codexBrowser.expired")
                    .await;
                return;
            }
            let _ = manager
                .poll_browser_login_at(&id, before_commit, &endpoint)
                .await;
        }));
        Ok(())
    }

    async fn fail_browser_login(&self, id: &str, error: &str) -> Result<(), String> {
        let cancelled = self.cancel_device_flow(id).await;
        let mut flows = self.browser_logins.lock().await;
        let flow = flows.get_mut(id).ok_or(LOGIN_FAILED)?;
        if flow.response.status == "waiting" && (cancelled || !flow.exchanging) {
            flow.response.status = "failed".into();
            flow.response.error = Some(error.into());
            flow.verifier.clear();
            flow.code = None;
            flow.ready.notify_one();
            flow.changed.send_replace(());
        }
        Ok(())
    }

    pub(crate) async fn browser_login_status(
        &self,
        id: &str,
    ) -> Result<BrowserLoginStatus, String> {
        let flows = self.browser_logins.lock().await;
        Ok(flows.get(id).ok_or(LOGIN_FAILED)?.response.clone())
    }

    async fn wait_browser_login(&self, id: &str) -> Result<BrowserLoginStatus, String> {
        let mut changed = self
            .browser_logins
            .lock()
            .await
            .get(id)
            .ok_or(LOGIN_FAILED)?
            .changed
            .subscribe();
        let wait = async {
            loop {
                let status = self.browser_login_status(id).await?;
                if status.status != "waiting" {
                    return Ok(status);
                }
                changed
                    .changed()
                    .await
                    .map_err(|_| LOGIN_FAILED.to_string())?;
            }
        };
        tokio::time::timeout(OAUTH_HTTP_TIMEOUT + Duration::from_secs(30), wait)
            .await
            .map_err(|_| LOGIN_FAILED.to_string())?
    }
    pub(crate) async fn finish_browser_callback(
        &self,
        id: &str,
        callback: &str,
    ) -> Result<(), String> {
        self.submit_browser_callback(id, callback).await?;
        let status = self.wait_browser_login(id).await?;
        if status.status == "done" {
            Ok(())
        } else {
            Err(status.error.unwrap_or_else(|| LOGIN_FAILED.into()))
        }
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
        let code = callback_code(callback, &flow.redirect, &flow.state)?;
        if code.is_empty() {
            flow.response.status = "failed".into();
            flow.response.error = Some(LOGIN_FAILED.into());
            flow.verifier.clear();
            flow.ready.notify_one();
            flow.changed.send_replace(());
            drop(flows);
            self.cancel_device_flow(id).await;
            return Ok(());
        }
        flow.code = Some(code);
        flow.ready.notify_one();
        log::info!("[CodexOAuth] Browser sign-in callback received");
        Ok(())
    }
    pub(crate) async fn cancel_browser_login(&self, id: &str) -> bool {
        let cancelled = self.cancel_device_flow(id).await;
        let removed = self.browser_logins.lock().await.remove(id);
        if let Some(flow) = removed {
            if let Some(task) = flow.worker {
                task.abort();
                let _ = task.await;
            }
            if let Some(task) = flow.listener {
                task.abort();
                let _ = task.await;
            }
        }
        cancelled
    }
    #[cfg(test)]
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
        let (code, verifier, redirect, client, proxy, proxy_selected) = {
            let mut flows = self.browser_logins.lock().await;
            let flow = flows.get_mut(id).ok_or(LOGIN_FAILED)?;
            if flow.expires <= chrono::Utc::now().timestamp_millis() {
                flow.response.status = "failed".into();
                flow.response.error = Some("codexBrowser.expired".into());
                flow.changed.send_replace(());
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
                flow.proxy.clone(),
                flow.proxy_selected,
            )
        };
        log::info!("[CodexOAuth] Browser sign-in token exchange started");
        let result = async {
            let pending = self
                .pending_device_codes
                .read()
                .await
                .get(id)
                .cloned()
                .ok_or_else(|| {
                    browser_session::Failure::new("login_expired", "codexBrowser.expired")
                })?;
            let response = client
                .post(token_url)
                .timeout(OAUTH_HTTP_TIMEOUT)
                .header("originator", CODEX_OAUTH_ORIGINATOR)
                .header(
                    "User-Agent",
                    crate::codex_client::user_agent(&crate::codex_client::current_version()),
                )
                .header("Accept", "application/json")
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", code.as_str()),
                    ("redirect_uri", redirect.as_str()),
                    ("client_id", CODEX_CLIENT_ID),
                    ("code_verifier", verifier.as_str()),
                ])
                .send()
                .await
                .map_err(|error| browser_session::Failure::network(&error))?;
            let (tokens, who) = browser_session::read_tokens(response).await?;
            let refresh = tokens.refresh_token.ok_or_else(|| {
                browser_session::Failure::new(
                    "token_credentials",
                    "codexBrowser.invalidTokenResponse",
                )
            })?;
            let cached = CachedAccessToken {
                token: tokens.access_token,
                expires_at_ms: compute_expires_at_ms(tokens.expires_in),
                obtained_at_ms: chrono::Utc::now().timestamp_millis(),
            };
            let _guard = before_commit().await;
            self.add_account_internal(
                who.workspace,
                refresh,
                Some(who.email),
                tokens.id_token,
                Some(cached.clone()),
                AccountLoginContext {
                    target_account_id: pending.target_account_id.as_deref(),
                    pending_device_code: Some(id),
                    target_generation: pending.target_generation,
                    browser_session: Some(browser_session::Session {
                        proxy,
                        access: cached,
                        client: Some(client),
                    }),
                    browser_proxy_selected: proxy_selected,
                },
            )
            .await
            .map_err(browser_session::Failure::commit)
        }
        .await;
        if result.is_err() {
            self.cancel_device_flow(id).await;
        }
        let mut flows = self.browser_logins.lock().await;
        let flow = flows.get_mut(id).ok_or(LOGIN_FAILED)?;
        match result {
            Ok(account) => {
                log::info!("[CodexOAuth] Browser sign-in account saved");
                flow.response.status = "done".into();
                flow.response.account = Some(account);
            }
            Err(error) => {
                log::warn!(
                    "[CodexOAuth] Browser sign-in failed (stage: {}, HTTP: {}, code: {})",
                    error.diagnostic.stage,
                    error
                        .diagnostic
                        .http_status
                        .map(|s| s.to_string())
                        .as_deref()
                        .unwrap_or("none"),
                    error.diagnostic.code.unwrap_or("none"),
                );
                flow.response.status = "failed".into();
                flow.response.error = Some(error.key.into());
                flow.response.diagnostic = Some(error.diagnostic);
            }
        }
        flow.exchanging = false;
        flow.changed.send_replace(());
        // Authorization codes and PKCE verifiers stay backend-only and leave memory after use.
        flow.verifier.clear();
        Ok(flow.response.clone())
    }
}

fn failure_message(status: &BrowserLoginStatus) -> String {
    let message = match status.error.as_deref() {
        Some("codexBrowser.tokenRejected") => "ChatGPT refused the sign-in request.",
        Some("codexBrowser.networkFailed") => "Could not reach ChatGPT. Check the selected proxy.",
        Some("codexBrowser.exchangeTimeout") => "The token request timed out after 30 seconds.",
        Some("codexBrowser.invalidTokenResponse") => {
            "ChatGPT returned incomplete or unreadable credentials."
        }
        Some("codexBrowser.saveFailed") => "The account could not be saved. Check disk space.",
        Some("codexBrowser.identityMismatch") => {
            "The signed-in account does not match the account being updated."
        }
        Some("codexBrowser.expired") => "This sign-in expired.",
        _ => "Return to Vary AI to retry.",
    };
    match &status.diagnostic {
        Some(diagnostic) => format!(
            "{message} Stage: {}. HTTP: {}. Code: {}.",
            diagnostic.stage,
            diagnostic
                .http_status
                .map(|s| s.to_string())
                .as_deref()
                .unwrap_or("none"),
            diagnostic.code.unwrap_or("none")
        ),
        None => message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn callback_releases_stale_listener_and_preserves_unrelated_listener(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = occupied.local_addr()?.port();
        let holder = tokio::spawn(async move {
            let (mut socket, _) = occupied.accept().await?;
            let mut bytes = [0; 2048];
            let n = socket.read(&mut bytes).await?;
            let valid = bytes[..n].starts_with(b"GET /cancel HTTP/1.1\r\n");
            drop(occupied);
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await?;
            Ok::<_, std::io::Error>(valid)
        });
        let listener = listen_callback(port)
            .await
            .ok_or("stale callback was not released")?;
        assert!(tokio::time::timeout(Duration::from_secs(3), holder).await???);
        drop(listener);
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = occupied.local_addr()?.port();
        let holder = tokio::spawn(async move {
            let (mut socket, _) = occupied.accept().await?;
            let mut bytes = [0; 2048];
            socket.read(&mut bytes).await?;
            socket
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await?;
            drop(socket);
            std::future::pending::<()>().await;
            drop(occupied);
            Ok::<_, std::io::Error>(())
        });
        let blocked = listen_callback(port).await.is_none();
        let still_owned = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .is_err();
        holder.abort();
        assert!(blocked && still_owned);
        Ok(())
    }

    #[test]
    fn callback_accepts_loopback_aliases_but_rejects_foreign_hosts() {
        for host in ["localhost", "127.0.0.1", "[::1]"] {
            assert_eq!(
                callback_code(
                    &format!("http://{host}:1455/auth/callback?state=fixture&code=synthetic"),
                    REDIRECT,
                    "fixture"
                ),
                Ok("synthetic".into())
            );
        }
        for host in ["localhost.evil.test", "127.0.0.2", "user@localhost"] {
            assert!(callback_code(
                &format!("http://{host}:1455/auth/callback?state=fixture&code=synthetic"),
                REDIRECT,
                "fixture"
            )
            .is_err());
        }
    }

    #[tokio::test]
    async fn authorization_denial_is_terminal_and_redacted(
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
        manager
            .submit_browser_callback(
                &flow.id,
                &format!(
                    "{}?state={}&error=access_denied&error_description=synthetic-private-detail",
                    query.get("redirect_uri").ok_or("redirect")?,
                    query.get("state").ok_or("state")?
                ),
            )
            .await?;
        let status = manager.browser_login_status(&flow.id).await?;
        assert_eq!(status.status, "failed");
        assert_eq!(status.error.as_deref(), Some(LOGIN_FAILED));
        assert!(!serde_json::to_string(&status)?.contains("synthetic-private-detail"));
        assert!(manager.list_accounts().await.is_empty());
        manager.cancel_browser_login(&flow.id).await;
        Ok(())
    }
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
    #[tokio::test]
    async fn magpie_login_accepts_email_identity_and_updates_the_existing_account(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for workspace in [None, Some("fixture-workspace")] {
            let temp = tempfile::tempdir()?;
            let manager = Arc::new(CodexOAuthManager::new(temp.path().to_owned()));
            let claims = serde_json::json!({
                "email": "fixture@example.test",
                "https://api.openai.com/auth": {"chatgpt_account_id": workspace}
            });
            // Magpie reads the JWT payload without imposing a header or subject requirement.
            let token = format!("opaque.{}=.", URL_SAFE_NO_PAD.encode(claims.to_string()));
            let response = serde_json::json!({
                "access_token": "synthetic-access",
                "refresh_token": "synthetic-refresh",
                "id_token": token,
                "expires_in": "ignored-by-magpie"
            });
            let app = axum::Router::new().route(
                "/token",
                axum::routing::post(move || {
                    let response = response.clone();
                    async move { axum::Json(response) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let endpoint = format!("http://{}/token", listener.local_addr()?);
            let server = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            let mut first_id = None;
            for _ in 0..2 {
                let flow = manager
                    .start_browser_login_on(None, Some("direct".into()), 0)
                    .await?;
                let query: HashMap<_, _> = url::Url::parse(&flow.url)?
                    .query_pairs()
                    .into_owned()
                    .collect();
                let callback = format!(
                    "{}?state={}&code=synthetic",
                    query.get("redirect_uri").ok_or("redirect")?,
                    query.get("state").ok_or("state")?
                );
                manager.submit_browser_callback(&flow.id, &callback).await?;
                let completed = manager
                    .poll_browser_login_at(&flow.id, || async {}, &endpoint)
                    .await?;
                assert_eq!(completed.status, "done");
                let account = completed.account.ok_or("account")?;
                assert!(!account.reauth_required);
                assert_eq!(
                    manager.account_proxy(&account.id).await?.as_deref(),
                    Some("direct")
                );
                if let Some(id) = first_id.as_ref() {
                    assert_eq!(&account.id, id);
                } else {
                    first_id = Some(account.id.clone());
                }
                assert_eq!(manager.list_accounts().await.len(), 1);
                assert!(manager
                    .get_valid_token_for_account(&account.id)
                    .await
                    .is_ok());
                manager.cancel_browser_login(&flow.id).await;
            }
            server.abort();
            let reloaded = CodexOAuthManager::new(temp.path().to_owned());
            let id = first_id.ok_or("id")?;
            assert_eq!(reloaded.list_accounts().await.len(), 1);
            assert!(!reloaded.list_accounts().await[0].reauth_required);
            assert!(reloaded.get_valid_token_for_account(&id).await.is_ok());
            assert_eq!(
                reloaded.account_proxy(&id).await?.as_deref(),
                Some("direct")
            );
            let (token, updated) = {
                let accounts = reloaded.accounts.read().await;
                let account = accounts.get(&id).ok_or("account")?;
                (
                    account.id_token.clone(),
                    account.token_updated_at_ms.saturating_add(1000),
                )
            };
            reloaded
                .adopt_account_refresh_token(
                    &id,
                    "synthetic-new-generation".into(),
                    token,
                    Some(updated),
                )
                .await?;
            let after_adoption = CodexOAuthManager::new(temp.path().to_owned());
            assert!(!after_adoption.access_tokens.read().await.contains_key(&id));
        }
        Ok(())
    }

    #[tokio::test]
    async fn account_proxy_survives_restart_is_inherited_and_can_be_reset(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(temp.path().to_owned()));
        let app = axum::Router::new().fallback(move || async { axum::Json(tokens()) });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let proxy = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let flow = manager
            .start_browser_login_on(None, Some(proxy.clone()), 0)
            .await?;
        let query: HashMap<_, _> = url::Url::parse(&flow.url)?
            .query_pairs()
            .into_owned()
            .collect();
        manager
            .submit_browser_callback(
                &flow.id,
                &format!(
                    "{}?state={}&code=synthetic",
                    query.get("redirect_uri").ok_or("redirect")?,
                    query.get("state").ok_or("state")?
                ),
            )
            .await?;
        let completed = manager
            .poll_browser_login_at(&flow.id, || async {}, "http://oauth.invalid/token")
            .await?;
        assert_eq!(completed.status, "done");
        let id = completed.account.ok_or("account")?.id;
        manager.cancel_browser_login(&flow.id).await;
        let manager = Arc::new(CodexOAuthManager::new(temp.path().to_owned()));
        assert_eq!(manager.account_proxy(&id).await?, Some(proxy.clone()));
        // The reserved invalid hostname can only be reached through this synthetic proxy.
        let result = manager
            .account_forwarding_client(&id)
            .await?
            .ok_or("account proxy override")?
            .get("http://account.invalid/usage")
            .send()
            .await?;
        assert!(result.status().is_success());
        let inherited = manager
            .start_browser_login_on(Some(id.clone()), None, 0)
            .await?;
        assert_eq!(
            manager.browser_logins.lock().await[&inherited.id]
                .proxy
                .as_deref(),
            Some(proxy.as_str())
        );
        manager.cancel_browser_login(&inherited.id).await;
        let reset = manager
            .start_browser_login_on(Some(id.clone()), Some(String::new()), 0)
            .await?;
        assert!(manager.browser_logins.lock().await[&reset.id]
            .proxy
            .is_none());
        assert!(manager.browser_logins.lock().await[&reset.id].proxy_selected);
        manager
            .browser_logins
            .lock()
            .await
            .get_mut(&reset.id)
            .ok_or("flow")?
            .client = reqwest::Client::builder().no_proxy().build()?;
        let query: HashMap<_, _> = url::Url::parse(&reset.url)?
            .query_pairs()
            .into_owned()
            .collect();
        manager
            .submit_browser_callback(
                &reset.id,
                &format!(
                    "{}?state={}&code=synthetic",
                    query.get("redirect_uri").ok_or("redirect")?,
                    query.get("state").ok_or("state")?
                ),
            )
            .await?;
        assert_eq!(
            manager
                .poll_browser_login_at(&reset.id, || async {}, &format!("{proxy}/token"))
                .await?
                .status,
            "done"
        );
        assert_eq!(manager.account_proxy(&id).await?, None);
        assert!(manager.account_forwarding_client(&id).await?.is_none());
        assert_eq!(
            CodexOAuthManager::new(temp.path().to_owned())
                .account_proxy(&id)
                .await?,
            None
        );
        manager.cancel_browser_login(&reset.id).await;
        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn browser_login_distinguishes_workspace_seats_and_guards_reauthentication_generations(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(temp.path().to_owned()));
        let mut first_id = None;
        for (workspace, target, expected) in [
            ("seat-one", false, "done"),
            ("seat-two", false, "done"),
            ("seat-two", true, "failed"),
            ("seat-one", false, "failed"),
        ] {
            let flow = manager
                .start_browser_login_on(
                    if target { first_id.clone() } else { None },
                    Some("direct".into()),
                    0,
                )
                .await?;
            let query: HashMap<_, _> = url::Url::parse(&flow.url)?
                .query_pairs()
                .into_owned()
                .collect();
            let newer = if expected == "failed" && !target {
                Some(
                    manager
                        .start_browser_login_on(first_id.clone(), Some("direct".into()), 0)
                        .await?,
                )
            } else {
                None
            };
            let mut response = tokens();
            let claims = serde_json::json!({"email":"fixture@example.test","https://api.openai.com/auth":{"chatgpt_account_id":workspace,"chatgpt_plan_type":"team"}});
            response["id_token"] =
                format!("opaque.{}.", URL_SAFE_NO_PAD.encode(claims.to_string())).into();
            let app = axum::Router::new().route(
                "/token",
                axum::routing::post(move || {
                    let response = response.clone();
                    async move { axum::Json(response) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let endpoint = format!("http://{}/token", listener.local_addr()?);
            let server = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            manager
                .start_browser_worker(&flow.id, endpoint, || async {})
                .await?;
            let reply = reqwest::Client::builder()
                .no_proxy()
                .build()?
                .get(format!(
                    "{}?state={}&code=synthetic",
                    query.get("redirect_uri").ok_or("redirect")?,
                    query.get("state").ok_or("state")?
                ))
                .timeout(Duration::from_secs(5))
                .send()
                .await?
                .text()
                .await?;
            let status = manager.browser_login_status(&flow.id).await?;
            server.abort();
            assert_eq!(status.status, expected);
            assert_eq!(reply.contains("You're signed in"), expected == "done");
            if first_id.is_none() {
                first_id = status.account.map(|a| a.id);
            }
            if target {
                assert_eq!(
                    status.error.as_deref(),
                    Some("codexBrowser.identityMismatch")
                );
            } else if newer.is_some() {
                assert_eq!(status.error.as_deref(), Some("codexBrowser.expired"));
            }
            manager.cancel_browser_login(&flow.id).await;
            if let Some(newer) = newer {
                manager.cancel_browser_login(&newer.id).await;
            }
        }
        assert_eq!(manager.list_accounts().await.len(), 2);
        assert_eq!(
            manager
                .chatgpt_account_id_for_account(&first_id.ok_or("account")?)
                .await?,
            "seat-one"
        );
        Ok(())
    }

    #[tokio::test]
    async fn token_refusal_preserves_only_safe_status_and_known_error_codes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for error in [
            serde_json::json!({"error":"invalid_grant","error_description":"synthetic-private-text"}),
            serde_json::json!({"error":{"code":"synthetic-private-text","message":"synthetic-private-text"}}),
        ] {
            let temp = tempfile::tempdir()?;
            let manager = Arc::new(CodexOAuthManager::new(temp.path().to_owned()));
            let flow = manager
                .start_browser_login_on(None, Some("direct".into()), 0)
                .await?;
            let query: HashMap<_, _> = url::Url::parse(&flow.url)?
                .query_pairs()
                .into_owned()
                .collect();
            let app = axum::Router::new().route(
                "/token",
                axum::routing::post(move || {
                    let error = error.clone();
                    async move { (axum::http::StatusCode::UNAUTHORIZED, axum::Json(error)) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let endpoint = format!("http://{}/token", listener.local_addr()?);
            let server = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            manager
                .start_browser_worker(&flow.id, endpoint, || async {})
                .await?;
            let reply = reqwest::Client::builder()
                .no_proxy()
                .build()?
                .get(format!(
                    "{}?state={}&code=synthetic",
                    query.get("redirect_uri").ok_or("redirect")?,
                    query.get("state").ok_or("state")?
                ))
                .timeout(Duration::from_secs(5))
                .send()
                .await?
                .text()
                .await?;
            let status = manager.browser_login_status(&flow.id).await?;
            server.abort();
            assert_eq!(status.error.as_deref(), Some("codexBrowser.tokenRejected"));
            let public = serde_json::to_value(&status)?;
            assert_eq!(
                public.pointer("/diagnostic/httpStatus"),
                Some(&serde_json::json!(401))
            );
            assert!(!public.to_string().contains("synthetic-private-text"));
            assert!(reply.contains("HTTP: 401"));
            assert!(!reply.contains("synthetic-private-text"));
            assert!(manager.list_accounts().await.is_empty());
            manager.cancel_browser_login(&flow.id).await;
        }
        Ok(())
    }
    #[tokio::test]
    async fn manual_callback_completes_without_renderer_polling_and_rejects_bad_token_bodies(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for mode in [
            "valid",
            "empty-access",
            "empty-refresh",
            "missing-id",
            "missing-email",
            "oversized",
        ] {
            let temp = tempfile::tempdir()?;
            let manager = Arc::new(CodexOAuthManager::new(temp.path().to_owned()));
            let flow = manager
                .start_browser_login_on(None, Some("direct".into()), 0)
                .await?;
            let query: HashMap<_, _> = url::Url::parse(&flow.url)?
                .query_pairs()
                .into_owned()
                .collect();
            let mut response = tokens();
            if mode == "empty-access" {
                response["access_token"] = "".into();
            }
            if mode == "empty-refresh" {
                response["refresh_token"] = "".into();
            }
            if mode == "missing-id" {
                response["id_token"] = serde_json::Value::Null;
            }
            if mode == "missing-email" {
                response["id_token"] = format!(
                    "opaque.{}.",
                    URL_SAFE_NO_PAD.encode(br#"{"sub":"fixture"}"#)
                )
                .into();
            }
            if mode == "oversized" {
                response["padding"] = "x".repeat(MAX_TOKEN_RESPONSE_BYTES).into();
            }
            let app = axum::Router::new().route(
                "/token",
                axum::routing::post(move || {
                    let response = response.clone();
                    async move { axum::Json(response) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let endpoint = format!("http://{}/token", listener.local_addr()?);
            let server = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            manager
                .start_browser_worker(&flow.id, endpoint, || async {})
                .await?;
            let callback = format!(
                "{}?state={}&code=synthetic",
                query.get("redirect_uri").ok_or("redirect")?,
                query.get("state").ok_or("state")?
            )
            .replace("localhost", "127.0.0.1");
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                manager.finish_browser_callback(&flow.id, &callback),
            )
            .await?;
            assert_eq!(result.is_ok(), mode == "valid");
            let status = manager.browser_login_status(&flow.id).await?;
            server.abort();
            assert_eq!(
                status.status,
                if mode == "valid" { "done" } else { "failed" }
            );
            assert_eq!(
                manager.list_accounts().await.len(),
                usize::from(mode == "valid")
            );
            assert!(manager
                .submit_browser_callback(&flow.id, &callback)
                .await
                .is_err());
            manager.cancel_browser_login(&flow.id).await;
        }
        Ok(())
    }

    #[tokio::test]
    async fn stalled_token_body_has_a_terminal_oauth_deadline(
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
        manager
            .submit_browser_callback(
                &flow.id,
                &format!(
                    "{}?state={}&code=synthetic",
                    query.get("redirect_uri").ok_or("redirect")?,
                    query.get("state").ok_or("state")?
                ),
            )
            .await?;
        manager
            .browser_logins
            .lock()
            .await
            .get_mut(&flow.id)
            .ok_or("flow")?
            .client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(600))
            .build()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}/token", listener.local_addr()?);
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let mut bytes = [0; 4096];
            socket.read(&mut bytes).await?;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{").await?;
            std::future::pending::<()>().await;
            Ok::<_, std::io::Error>(())
        });
        let result = tokio::time::timeout(
            OAUTH_HTTP_TIMEOUT + Duration::from_secs(2),
            manager.poll_browser_login_at(&flow.id, || async {}, &endpoint),
        )
        .await;
        server.abort();
        let result = result??;
        assert_eq!(result.status, "failed");
        assert_eq!(
            result.error.as_deref(),
            Some("codexBrowser.exchangeTimeout")
        );
        assert!(manager.list_accounts().await.is_empty());
        manager.cancel_browser_login(&flow.id).await;
        Ok(())
    }

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
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if manager
                    .browser_logins
                    .lock()
                    .await
                    .get(&flow.id)
                    .is_some_and(|flow| flow.code.is_some())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await?;
        assert!(manager
            .submit_browser_callback(&flow.id, &format!("{redirect}?code=another&state={state}"))
            .await
            .is_err());
        let expected_redirect = redirect.clone();
        let app = axum::Router::new().route(
            "/token",
            axum::routing::post(
                move |headers: axum::http::HeaderMap,
                      axum::Form(form): axum::Form<HashMap<String, String>>| {
                    let verifier = verifier.clone();
                    let redirect = expected_redirect.clone();
                    async move {
                        assert_eq!(
                            headers.get("originator").and_then(|v| v.to_str().ok()),
                            Some("codex_cli_rs")
                        );
                        assert_eq!(
                            headers.get("accept").and_then(|v| v.to_str().ok()),
                            Some("application/json")
                        );
                        assert!(headers
                            .get("user-agent")
                            .and_then(|v| v.to_str().ok())
                            .is_some_and(|v| v.starts_with("codex_cli_rs/")
                                && v.contains(" (")
                                && v.contains("; ")
                                && v.contains(") ")));
                        assert!(!headers.contains_key("version"));
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
        manager
            .start_browser_worker(&flow.id, endpoint.clone(), || async {})
            .await?;
        tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut reply)).await??;
        assert!(reply.starts_with(b"HTTP/1.1 200"));
        assert!(String::from_utf8_lossy(&reply).contains("You're signed in"));
        let completed = manager.browser_login_status(&flow.id).await?;
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
