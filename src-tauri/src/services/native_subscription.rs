//! Device-local subscription accounts. Tokens never enter the routing registry.
//! Protocol reference: Magpie provider/{google,chatgpt_api,claude_signin}.go (MIT).
use super::routing_upstreams::SubscriptionProvider;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

const FAILED: &str = "nativeSubscription.failed";
const EXPIRED: &str = "nativeSubscription.expired";
const PROJECT: &str = "nativeSubscription.projectRequired";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    pub id: String,
    pub provider: SubscriptionProvider,
    pub login: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    access: String,
    #[serde(default)]
    refresh: String,
    #[serde(default)]
    expires: i64,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    id_token: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountSummary {
    id: String,
    login: String,
    project: String,
}
impl Account {
    fn summary(&self) -> AccountSummary {
        AccountSummary {
            id: self.id.clone(),
            login: self.login.clone(),
            project: self.project.clone(),
        }
    }
    pub(crate) fn token(&self) -> &str {
        &self.access
    }
    pub(crate) fn claude_dir(&self) -> PathBuf {
        root().join("claude").join(&self.id)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Flow {
    pub id: String,
    pub url: Option<String>,
    pub state: String,
    pub account: Option<AccountSummary>,
    pub error: Option<String>,
    #[serde(skip)]
    task: Option<tokio::task::AbortHandle>,
}

static ACCOUNTS: Mutex<()> = Mutex::const_new(());
type FlowMap = Arc<Mutex<HashMap<String, Flow>>>;
fn flows() -> &'static FlowMap {
    static FLOWS: OnceLock<FlowMap> = OnceLock::new();
    FLOWS.get_or_init(Default::default)
}
fn root() -> PathBuf {
    crate::config::get_device_config_dir().join("subscription-accounts")
}
fn store_path() -> PathBuf {
    root().join("accounts.json")
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn google(provider: SubscriptionProvider) -> bool {
    matches!(
        provider,
        SubscriptionProvider::GeminiCli | SubscriptionProvider::Antigravity
    )
}
fn supported(provider: SubscriptionProvider) -> Result<(), String> {
    matches!(
        provider,
        SubscriptionProvider::Claude
            | SubscriptionProvider::ChatgptApi
            | SubscriptionProvider::GeminiCli
            | SubscriptionProvider::Antigravity
    )
    .then_some(())
    .ok_or(FAILED.into())
}
async fn read() -> Result<Vec<Account>, String> {
    tokio::task::spawn_blocking(|| match std::fs::read(store_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| FAILED.into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(FAILED.into()),
    })
    .await
    .map_err(|_| FAILED)?
}
async fn write(accounts: Vec<Account>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let bytes = serde_json::to_vec(&accounts).map_err(|_| FAILED)?;
        crate::config::atomic_write_private(&store_path(), &bytes).map_err(|_| FAILED.into())
    })
    .await
    .map_err(|_| FAILED)?
}
async fn keep(account: Account) -> Result<AccountSummary, String> {
    let _guard = ACCOUNTS.lock().await;
    let mut accounts = read().await?;
    accounts.retain(|old| old.id != account.id);
    let summary = account.summary();
    accounts.push(account);
    write(accounts).await?;
    Ok(summary)
}
pub(crate) async fn list(provider: SubscriptionProvider) -> Result<Vec<AccountSummary>, String> {
    supported(provider)?;
    Ok(read()
        .await?
        .into_iter()
        .filter(|a| a.provider == provider)
        .map(|a| a.summary())
        .collect())
}
pub(crate) async fn exists(provider: SubscriptionProvider, id: &str) -> bool {
    read().await.is_ok_and(|accounts| {
        accounts
            .iter()
            .any(|a| a.provider == provider && a.id == id)
    })
}
pub(crate) async fn remove(provider: SubscriptionProvider, id: &str) -> Result<(), String> {
    supported(provider)?;
    let _guard = ACCOUNTS.lock().await;
    let mut accounts = read().await?;
    let removed = accounts
        .iter()
        .find(|a| a.id == id && a.provider == provider)
        .cloned();
    accounts.retain(|a| a.id != id || a.provider != provider);
    write(accounts).await?;
    if let Some(account) = removed.filter(|a| a.provider == SubscriptionProvider::Claude) {
        let dir = account.claude_dir();
        tokio::task::spawn_blocking(move || {
            #[cfg(target_os = "macos")]
            {
                let service = format!(
                    "Claude Code-credentials-{}",
                    &format!("{:x}", Sha256::digest(dir.to_string_lossy().as_bytes()))[..8]
                );
                let _ = std::process::Command::new("security")
                    .args(["delete-generic-password", "-s", &service])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            let _ = std::fs::remove_dir_all(dir);
        })
        .await
        .map_err(|_| FAILED)?;
    }
    Ok(())
}
fn client(provider: SubscriptionProvider) -> Result<(String, String), String> {
    if provider == SubscriptionProvider::Antigravity {
        let id = std::env::var("VARYAI_ANTIGRAVITY_CLIENT_ID").ok();
        let secret = std::env::var("VARYAI_ANTIGRAVITY_CLIENT_SECRET").ok();
        match (id, secret) {
            (Some(id), Some(secret)) if !id.trim().is_empty() && !secret.trim().is_empty() => {
                Ok((id, secret))
            }
            _ => Err("nativeSubscription.clientRequired".into()),
        }
    } else {
        Ok((
            super::subscription::GEMINI_OAUTH_CLIENT_ID.into(),
            super::subscription::GEMINI_OAUTH_CLIENT_SECRET.into(),
        ))
    }
}
fn token_endpoint(provider: SubscriptionProvider) -> &'static str {
    if provider == SubscriptionProvider::ChatgptApi {
        "https://auth.openai.com/api/accounts/oauth/token"
    } else {
        "https://oauth2.googleapis.com/token"
    }
}
async fn token_request(
    provider: SubscriptionProvider,
    form: &[(String, String)],
) -> Result<Value, String> {
    let response = crate::proxy::http_client::get()
        .post(token_endpoint(provider))
        .form(form)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| FAILED)?;
    if !response.status().is_success() {
        return Err(EXPIRED.into());
    }
    response.json().await.map_err(|_| FAILED.into())
}
fn apply_token(account: &mut Account, value: &Value) -> Result<(), String> {
    account.access = value["access_token"]
        .as_str()
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .ok_or(EXPIRED)?
        .into();
    if let Some(refresh) = value["refresh_token"].as_str().filter(|s| !s.is_empty()) {
        account.refresh = refresh.into();
    }
    account.expires = chrono::Utc::now().timestamp()
        + value["expires_in"].as_i64().unwrap_or(3600).clamp(1, 86400);
    if let Some(id) = value["id_token"].as_str() {
        account.id_token = id.into();
    }
    Ok(())
}

pub(crate) async fn resolve(provider: SubscriptionProvider, id: &str) -> Result<Account, String> {
    supported(provider)?;
    // Serialize refresh-token rotation with account edits and imports.
    let _guard = ACCOUNTS.lock().await;
    let mut accounts = read().await?;
    let account = accounts
        .iter_mut()
        .find(|a| a.provider == provider && a.id == id)
        .ok_or(EXPIRED)?;
    if provider != SubscriptionProvider::Claude
        && account.expires <= chrono::Utc::now().timestamp() + 180
    {
        if account.refresh.is_empty() {
            return Err(EXPIRED.into());
        }
        let mut form = vec![
            ("grant_type".into(), "refresh_token".into()),
            ("refresh_token".into(), account.refresh.clone()),
            ("client_id".into(), account.client_id.clone()),
        ];
        if google(provider) {
            form.push(("client_secret".into(), client(provider)?.1));
        } else {
            form.push(("resource".into(), provider.endpoint().into()));
        }
        let token = token_request(provider, &form).await?;
        if provider == SubscriptionProvider::ChatgptApi {
            if let Some(scope) = token["scope"].as_str() {
                if !scope
                    .split_whitespace()
                    .any(|s| s == "chatgpt.tokens.use.direct")
                {
                    return Err(EXPIRED.into());
                }
            }
        }
        apply_token(account, &token)?;
        let result = account.clone();
        write(accounts).await?;
        return Ok(result);
    }
    Ok(account.clone())
}

pub(crate) async fn project(
    provider: SubscriptionProvider,
    id: &str,
    project: String,
) -> Result<(), String> {
    if !google(provider) || project.len() > 200 || project.chars().any(char::is_control) {
        return Err(FAILED.into());
    }
    let _guard = ACCOUNTS.lock().await;
    let mut accounts = read().await?;
    let account = accounts
        .iter_mut()
        .find(|a| a.provider == provider && a.id == id)
        .ok_or(EXPIRED)?;
    account.project = project.trim().into();
    write(accounts).await
}

async fn host_id() -> Result<String, String> {
    let _guard = ACCOUNTS.lock().await;
    tokio::task::spawn_blocking(|| {
        let path = root().join("host-id");
        match std::fs::read_to_string(&path) {
            Ok(id) if id.starts_with("urn:uuid:") && id.len() < 100 => Ok(id),
            Ok(_) => Err(FAILED.into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
                crate::config::atomic_write_private(&path, id.as_bytes()).map_err(|_| FAILED)?;
                Ok(id)
            }
            Err(_) => Err(FAILED.into()),
        }
    })
    .await
    .map_err(|_| FAILED)?
}

fn valid_callback(target: &str, path: &str, state: &str) -> Option<HashMap<String, String>> {
    if !target.starts_with('/') || target.starts_with("//") {
        return None;
    }
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != path {
        return None;
    }
    let mut query = HashMap::new();
    for (key, value) in url.query_pairs() {
        if query.insert(key.into_owned(), value.into_owned()).is_some() {
            return None;
        }
    }
    (query.get("state").is_some_and(|s| s == state)).then_some(query)
}

pub(crate) async fn start(provider: SubscriptionProvider) -> Result<Flow, String> {
    supported(provider)?;
    let id = uuid::Uuid::new_v4().to_string();
    let mut registry = flows().lock().await;
    registry.retain(|_, flow| flow.state == "waiting");
    if registry.len() >= 8 {
        return Err(FAILED.into());
    }
    let flow = Flow {
        id: id.clone(),
        url: None,
        state: "waiting".into(),
        account: None,
        error: None,
        task: None,
    };
    registry.insert(id.clone(), flow);
    drop(registry);
    let prepared = if provider == SubscriptionProvider::Claude {
        Ok((None, None))
    } else {
        prepare_oauth(provider)
            .await
            .map(|pending| (Some(pending.url.clone()), Some(pending)))
    };
    let (url, pending) = match prepared {
        Ok(value) => value,
        Err(error) => {
            flows().lock().await.remove(&id);
            return Err(error);
        }
    };
    let flow_id = id.clone();
    let task = tokio::spawn(async move {
        let result = tokio::time::timeout(Duration::from_secs(300), async {
            match pending {
                Some(pending) => finish_oauth(provider, pending).await,
                None => claude_login(&flow_id).await,
            }
        })
        .await
        .unwrap_or_else(|_| Err(EXPIRED.into()));
        if let Some(flow) = flows().lock().await.get_mut(&flow_id) {
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
    let mut registry = flows().lock().await;
    let flow = registry.get_mut(&id).ok_or(FAILED)?;
    flow.url = url;
    if flow.state == "waiting" {
        flow.task = Some(task.abort_handle());
    }
    Ok(flow.clone())
}
pub(crate) async fn poll(id: &str) -> Result<Flow, String> {
    flows().lock().await.get(id).cloned().ok_or(EXPIRED.into())
}
pub(crate) async fn cancel(id: &str) {
    if let Some(flow) = flows().lock().await.remove(id) {
        if let Some(task) = flow.task {
            task.abort();
        }
    }
}
struct Pending {
    listener: tokio::net::TcpListener,
    url: String,
    path: &'static str,
    state: String,
    nonce: String,
    verifier: String,
    redirect: String,
}
async fn prepare_oauth(provider: SubscriptionProvider) -> Result<Pending, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| FAILED)?;
    let path = match provider {
        SubscriptionProvider::ChatgptApi => "/auth/callback",
        SubscriptionProvider::Antigravity => "/oauth-callback",
        _ => "/oauth2callback",
    };
    let host = if provider == SubscriptionProvider::Antigravity {
        "localhost"
    } else {
        "127.0.0.1"
    };
    let redirect = format!(
        "http://{host}:{}{path}",
        listener.local_addr().map_err(|_| FAILED)?.port()
    );
    let state = random();
    let nonce = random();
    let verifier = random();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let openai = provider == SubscriptionProvider::ChatgptApi;
    let mut url = url::Url::parse(if openai {
        "https://auth.openai.com/api/accounts/authorize"
    } else {
        "https://accounts.google.com/o/oauth2/v2/auth"
    })
    .map_err(|_| FAILED)?;
    let host_id = if openai { Some(host_id().await?) } else { None };
    let oauth_client = if openai {
        "dynamic_agent_client".to_owned()
    } else {
        client(provider)?.0
    };
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", &oauth_client)
            .append_pair("redirect_uri", &redirect)
            .append_pair("response_type", "code")
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        if openai {
            q.append_pair("agent_name_hint", "Vary AI")
                .append_pair("ext_agent_host_id", host_id.as_deref().unwrap_or_default())
                .append_pair("nonce", &nonce)
                .append_pair("resource", provider.endpoint())
                .append_pair(
                    "scope",
                    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
                );
        } else {
            let mut scopes = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile".to_string();
            if provider == SubscriptionProvider::Antigravity {
                scopes.push_str(" https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs");
            }
            q.append_pair("scope", &scopes)
                .append_pair("access_type", "offline")
                .append_pair("prompt", "consent");
        }
    }
    Ok(Pending {
        listener,
        url: url.into(),
        path,
        state,
        nonce,
        verifier,
        redirect,
    })
}
async fn finish_oauth(
    provider: SubscriptionProvider,
    pending: Pending,
) -> Result<AccountSummary, String> {
    let query = loop {
        let (mut socket, peer) = pending.listener.accept().await.map_err(|_| FAILED)?;
        if !peer.ip().is_loopback() {
            continue;
        }
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0u8; 1024];
            let n = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut buffer))
                .await
                .map_err(|_| FAILED)?
                .map_err(|_| FAILED)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if bytes.len() > 16384 || bytes.windows(4).any(|s| s == b"\r\n\r\n") {
                break;
            }
        }
        let first = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|s| s.lines().next());
        let query = first
            .and_then(|s| s.strip_prefix("GET "))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| valid_callback(s, pending.path, &pending.state));
        let message = if query.is_some() {
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nVary AI received the sign-in. Return to Vary AI to check the result."
        } else {
            "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid sign-in callback."
        };
        let _ = socket.write_all(message.as_bytes()).await;
        if let Some(query) = query {
            break query;
        }
    };
    if query.contains_key("error") {
        return Err(EXPIRED.into());
    }
    let code = query.get("code").filter(|s| !s.is_empty()).ok_or(FAILED)?;
    let openai = provider == SubscriptionProvider::ChatgptApi;
    let client_id = if openai {
        query
            .get("client_id")
            .filter(|s| !s.is_empty() && s.as_str() != "dynamic_agent_client")
            .ok_or(FAILED)?
            .clone()
    } else {
        client(provider)?.0
    };
    let mut form = vec![
        ("grant_type".into(), "authorization_code".into()),
        ("client_id".into(), client_id.clone()),
        ("code".into(), code.clone()),
        ("redirect_uri".into(), pending.redirect),
        ("code_verifier".into(), pending.verifier),
    ];
    if openai {
        form.push(("resource".into(), provider.endpoint().into()));
    } else {
        form.push(("client_secret".into(), client(provider)?.1));
    }
    let token = token_request(provider, &form).await?;
    let mut account = Account {
        id: uuid::Uuid::new_v4().to_string(),
        provider,
        login: String::new(),
        project: String::new(),
        access: String::new(),
        refresh: String::new(),
        expires: 0,
        client_id,
        subject: String::new(),
        id_token: String::new(),
    };
    apply_token(&mut account, &token)?;
    if account.refresh.is_empty() {
        return Err(FAILED.into());
    }
    if openai {
        if !token["scope"].as_str().is_some_and(|s| {
            s.split_whitespace()
                .any(|s| s == "chatgpt.tokens.use.direct")
        }) {
            return Err(EXPIRED.into());
        }
        let claims =
            validate_openai_id(&account.id_token, &account.client_id, &pending.nonce).await?;
        account.subject = claims["sub"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(FAILED)?
            .into();
        account.login = claims["email"].as_str().unwrap_or(&account.subject).into();
        account.id = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}", account.client_id, account.subject))
        );
    } else {
        let response = crate::proxy::http_client::get()
            .get("https://www.googleapis.com/oauth2/v2/userinfo?alt=json")
            .bearer_auth(&account.access)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| FAILED)?;
        if !response.status().is_success() {
            return Err(FAILED.into());
        }
        let user: Value = response.json().await.map_err(|_| FAILED)?;
        account.subject = user["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(FAILED)?
            .into();
        account.login = user["email"].as_str().unwrap_or(&account.subject).into();
        account.id = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}", account.client_id, account.subject))
        );
    }
    keep(account).await
}

async fn validate_openai_id(token: &str, client_id: &str, nonce: &str) -> Result<Value, String> {
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(FAILED.into());
    }
    let header: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).map_err(|_| FAILED)?)
            .map_err(|_| FAILED)?;
    if header["alg"] != "RS256" {
        return Err(FAILED.into());
    }
    let discovery: Value = crate::proxy::http_client::get()
        .get("https://auth.openai.com/.well-known/openid-configuration")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| FAILED)?
        .error_for_status()
        .map_err(|_| FAILED)?
        .json()
        .await
        .map_err(|_| FAILED)?;
    let uri = url::Url::parse(discovery["jwks_uri"].as_str().ok_or(FAILED)?).map_err(|_| FAILED)?;
    if uri.scheme() != "https" || uri.host_str() != Some("auth.openai.com") {
        return Err(FAILED.into());
    }
    let jwks: Value = crate::proxy::http_client::get()
        .get(uri)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| FAILED)?
        .error_for_status()
        .map_err(|_| FAILED)?
        .json()
        .await
        .map_err(|_| FAILED)?;
    let key = jwks["keys"]
        .as_array()
        .ok_or(FAILED)?
        .iter()
        .find(|k| k["kid"] == header["kid"] && k["kty"] == "RSA")
        .ok_or(FAILED)?;
    let n = URL_SAFE_NO_PAD
        .decode(key["n"].as_str().ok_or(FAILED)?)
        .map_err(|_| FAILED)?;
    let e = URL_SAFE_NO_PAD
        .decode(key["e"].as_str().ok_or(FAILED)?)
        .map_err(|_| FAILED)?;
    let signature = URL_SAFE_NO_PAD.decode(parts[2]).map_err(|_| FAILED)?;
    ring::signature::RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &signature,
        )
        .map_err(|_| FAILED)?;
    let claims: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).map_err(|_| FAILED)?)
            .map_err(|_| FAILED)?;
    check_claims(&claims, client_id, nonce)?;
    Ok(claims)
}
fn check_claims(claims: &Value, client_id: &str, nonce: &str) -> Result<(), String> {
    let audience = claims["aud"].as_str() == Some(client_id)
        || claims["aud"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(client_id)));
    if claims["iss"] != "https://auth.openai.com"
        || claims["nonce"] != nonce
        || !audience
        || claims["exp"]
            .as_i64()
            .is_none_or(|n| n <= chrono::Utc::now().timestamp())
    {
        return Err(FAILED.into());
    }
    Ok(())
}

pub(crate) fn claude_command() -> tokio::process::Command {
    let home = crate::config::get_home_dir();
    let candidates = [
        home.join(".local/bin/claude"),
        home.join(".npm-global/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    let binary = candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("claude"));
    let mut command = tokio::process::Command::new(binary);
    command.kill_on_drop(true);
    for key in [
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDECODE",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_SSE_PORT",
        "CLAUDE_CODE_EXTRA_BODY",
    ] {
        command.env_remove(key);
    }
    command
        .env("ENABLE_CLAUDEAI_MCP_SERVERS", "0")
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1");
    command
}
async fn claude_identity(account: &Account) -> Result<String, String> {
    let output = claude_command()
        .env("CLAUDE_CONFIG_DIR", account.claude_dir())
        .args(["auth", "status", "--json"])
        .output()
        .await
        .map_err(|_| "nativeSubscription.claudeMissing")?;
    if !output.status.success() {
        return Err(EXPIRED.into());
    }
    let status: Value = serde_json::from_slice(&output.stdout).map_err(|_| FAILED)?;
    if status["loggedIn"] != true
        || status["authMethod"]
            .as_str()
            .is_some_and(|s| s != "oauth_token" && s != "claude.ai")
    {
        return Err(EXPIRED.into());
    }
    Ok(status["email"].as_str().unwrap_or("Claude account").into())
}
async fn claude_login(id: &str) -> Result<AccountSummary, String> {
    let mut account = Account {
        id: id.into(),
        provider: SubscriptionProvider::Claude,
        login: String::new(),
        project: String::new(),
        access: String::new(),
        refresh: String::new(),
        expires: 0,
        client_id: String::new(),
        subject: String::new(),
        id_token: String::new(),
    };
    let dir = account.claude_dir();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(dir))
        .await
        .map_err(|_| FAILED)?
        .map_err(|_| FAILED)?;
    let output = claude_command()
        .env("CLAUDE_CONFIG_DIR", account.claude_dir())
        .args(["auth", "login", "--claudeai"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|_| "nativeSubscription.claudeMissing")?;
    if !output.success() {
        return Err(EXPIRED.into());
    }
    account.login = claude_identity(&account).await?;
    keep(account).await
}

pub(crate) async fn import(
    provider: SubscriptionProvider,
    path: Option<String>,
) -> Result<AccountSummary, String> {
    if !matches!(
        provider,
        SubscriptionProvider::Claude
            | SubscriptionProvider::GeminiCli
            | SubscriptionProvider::Antigravity
    ) {
        return Err(FAILED.into());
    }
    let native = path.is_none();
    let value = tokio::task::spawn_blocking(move || {
        if let Some(path) = path {
            let bytes = std::fs::read(path).map_err(|_| FAILED)?;
            if bytes.len() > 1024 * 1024 {
                return Err(FAILED);
            }
            serde_json::from_slice::<Value>(&bytes).map_err(|_| FAILED)
        } else if provider == SubscriptionProvider::GeminiCli {
            let bytes =
                std::fs::read(crate::gemini_config::get_gemini_dir().join("oauth_creds.json"))
                    .map_err(|_| FAILED)?;
            serde_json::from_slice(&bytes).map_err(|_| FAILED)
        } else if provider == SubscriptionProvider::Claude {
            let bytes =
                std::fs::read(crate::config::get_claude_config_dir().join(".credentials.json"));
            if let Ok(bytes) = bytes {
                return serde_json::from_slice(&bytes).map_err(|_| FAILED);
            }
            #[cfg(target_os = "macos")]
            {
                let output = std::process::Command::new("security")
                    .args([
                        "find-generic-password",
                        "-s",
                        "Claude Code-credentials",
                        "-w",
                    ])
                    .output()
                    .map_err(|_| FAILED)?;
                if output.status.success() {
                    return serde_json::from_slice(&output.stdout).map_err(|_| FAILED);
                }
            }
            Err(FAILED)
        } else {
            Err(FAILED)
        }
    })
    .await
    .map_err(|_| FAILED)??;
    let mut account = Account {
        id: uuid::Uuid::new_v4().to_string(),
        provider,
        login: String::new(),
        project: String::new(),
        access: String::new(),
        refresh: String::new(),
        expires: 0,
        client_id: client(provider)?.0,
        subject: String::new(),
        id_token: String::new(),
    };
    if provider == SubscriptionProvider::Claude {
        if value
            .pointer("/claudeAiOauth/accessToken")
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(FAILED.into());
        }
        let path = account.claude_dir().join(".credentials.json");
        let bytes = serde_json::to_vec(&value).map_err(|_| FAILED)?;
        tokio::task::spawn_blocking(move || crate::config::atomic_write_private(&path, &bytes))
            .await
            .map_err(|_| FAILED)?
            .map_err(|_| FAILED)?;
        let path = account.claude_dir().join(".claude.json");
        let state = if native {
            tokio::task::spawn_blocking(|| {
                std::fs::read(crate::config::get_home_dir().join(".claude.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        // Only account identity and model entitlements accompany imported tokens.
        let mut profile = json!({"hasCompletedOnboarding":true});
        if let Some(state) = state {
            for key in [
                "oauthAccount",
                "additionalModelOptionsCache",
                "modelAccessCache",
                "additionalModelOptionsAnsweredAt",
            ] {
                if let Some(value) = state.get(key) {
                    profile[key] = value.clone();
                }
            }
        }
        tokio::task::spawn_blocking(move || {
            crate::config::atomic_write_private(&path, profile.to_string().as_bytes())
        })
        .await
        .map_err(|_| FAILED)?
        .map_err(|_| FAILED)?;
        account.login = claude_identity(&account).await?;
    } else {
        let value = value.get("token").unwrap_or(&value);
        account.refresh = value["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(FAILED)?
            .into();
        account.project = value["project"].as_str().unwrap_or("").into();
        let token = token_request(
            provider,
            &[
                ("grant_type".into(), "refresh_token".into()),
                ("refresh_token".into(), account.refresh.clone()),
                ("client_id".into(), account.client_id.clone()),
                ("client_secret".into(), client(provider)?.1),
            ],
        )
        .await?;
        apply_token(&mut account, &token)?;
        let user: Value = crate::proxy::http_client::get()
            .get("https://www.googleapis.com/oauth2/v2/userinfo?alt=json")
            .bearer_auth(&account.access)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| FAILED)?
            .error_for_status()
            .map_err(|_| FAILED)?
            .json()
            .await
            .map_err(|_| FAILED)?;
        account.subject = user["id"].as_str().ok_or(FAILED)?.into();
        account.login = user["email"].as_str().unwrap_or(&account.subject).into();
        account.id = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}", account.client_id, account.subject))
        );
        if native {
            let project = tokio::task::spawn_blocking(|| {
                std::fs::read_to_string(crate::gemini_config::get_gemini_dir().join(".env"))
                    .ok()
                    .and_then(|text| {
                        text.lines().find_map(|line| {
                            line.strip_prefix("GOOGLE_CLOUD_PROJECT=")
                                .or_else(|| line.strip_prefix("GOOGLE_CLOUD_PROJECT_ID="))
                                .map(|s| s.trim().trim_matches(['\'', '"']).to_owned())
                        })
                    })
            })
            .await
            .ok()
            .flatten();
            if let Some(project) = project {
                account.project = project;
            }
        }
    }
    keep(account).await
}

pub(crate) async fn google_project(account: &Account) -> Result<String, String> {
    if !account.project.is_empty() {
        return Ok(account.project.clone());
    }
    let metadata = if account.provider == SubscriptionProvider::Antigravity {
        json!({"ideType":"ANTIGRAVITY"})
    } else {
        json!({"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"})
    };
    let response = codeassist(
        account,
        "https://cloudcode-pa.googleapis.com",
        "loadCodeAssist",
        &json!({"metadata":metadata}),
    )
    .await?;
    if let Some(project) = project_value(&response["cloudaicompanionProject"]) {
        return Ok(project);
    }
    let tier = response["allowedTiers"]
        .as_array()
        .and_then(|tiers| tiers.iter().find(|tier| tier["isDefault"] == true));
    if tier.is_some_and(|tier| tier["userDefinedCloudaicompanionProject"] == true) {
        return Err(PROJECT.into());
    }
    let tier = tier
        .and_then(|tier| tier["id"].as_str())
        .unwrap_or("free-tier");
    for _ in 0..5 {
        let response = codeassist(
            account,
            account.provider.endpoint(),
            "onboardUser",
            &json!({"tierId":tier,"metadata":metadata}),
        )
        .await?;
        if let Some(project) = project_value(&response["response"]["cloudaicompanionProject"]) {
            return Ok(project);
        }
        if response["done"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Err(PROJECT.into())
}
fn project_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .or_else(|| value["id"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
async fn codeassist(
    account: &Account,
    base: &str,
    method: &str,
    body: &Value,
) -> Result<Value, String> {
    let response = crate::proxy::http_client::get()
        .post(format!("{base}/v1internal:{method}"))
        .bearer_auth(&account.access)
        .json(body)
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .map_err(|_| FAILED)?;
    if !response.status().is_success() {
        return Err(FAILED.into());
    }
    response.json().await.map_err(|_| FAILED.into())
}
pub(crate) async fn models(provider: SubscriptionProvider, id: &str) -> Result<Vec<Value>, String> {
    let account = resolve(provider, id).await?;
    if provider == SubscriptionProvider::Claude {
        claude_identity(&account).await?;
        let path = account.claude_dir().join(".claude.json");
        let state = tokio::task::spawn_blocking(move || {
            std::fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(Value::Null);
        let mut models: Vec<Value> = [
            ("sonnet", "Claude Sonnet"),
            ("opus", "Claude Opus"),
            ("haiku", "Claude Haiku"),
        ]
        .into_iter()
        .map(|(id, name)| json!({"model":id,"displayName":name}))
        .collect();
        for option in state["additionalModelOptionsCache"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if option["disabled"] == true {
                continue;
            }
            if let Some(id) = option["value"]
                .as_str()
                .filter(|id| id.starts_with("claude-") && !id.contains([' ', '/']))
            {
                let id = id.split('[').next().unwrap_or(id);
                if !models.iter().any(|row| row["model"] == id) {
                    models.push(
                        json!({"model":id,"displayName":option["label"].as_str().unwrap_or(id)}),
                    );
                }
            }
        }
        for option in state["modelAccessCache"].as_array().into_iter().flatten() {
            if let Some(id) = option["apiName"]
                .as_str()
                .filter(|id| id.starts_with("claude-"))
            {
                if option["entitled"] == false {
                    models.retain(|row| row["model"] != id);
                } else if option["entitled"] == true && !models.iter().any(|row| row["model"] == id)
                {
                    models.push(json!({"model":id}));
                }
            }
        }
        return Ok(models);
    }
    if provider == SubscriptionProvider::ChatgptApi {
        let response = crate::proxy::http_client::get()
            .get(format!("{}/models", provider.endpoint()))
            .bearer_auth(&account.access)
            .timeout(Duration::from_secs(25))
            .send()
            .await
            .map_err(|_| FAILED)?;
        if !response.status().is_success() {
            return Err(FAILED.into());
        }
        let body: Value = response.json().await.map_err(|_| FAILED)?;
        let models = body["models"]
            .as_array()
            .or_else(|| body["data"].as_array())
            .ok_or(FAILED)?;
        return Ok(models
            .iter()
            .filter(|row| {
                row["visibility"]
                    .as_str()
                    .is_none_or(|visibility| visibility == "list")
            })
            .filter_map(|row| {
                let id = row["slug"].as_str().or_else(|| row["id"].as_str())?;
                Some(json!({"model":id,"displayName":row["display_name"].as_str().unwrap_or(id)}))
            })
            .collect());
    }
    let project = google_project(&account).await?;
    if provider == SubscriptionProvider::GeminiCli {
        let quota = codeassist(
            &account,
            provider.endpoint(),
            "retrieveUserQuota",
            &json!({"project":project}),
        )
        .await?;
        let preview = quota["buckets"]
            .as_array()
            .ok_or(FAILED)?
            .iter()
            .any(|bucket| {
                bucket["modelId"]
                    .as_str()
                    .is_some_and(|s| s.contains("-preview"))
            });
        let flags = gemini_flags(&account, &project).await;
        return Ok(gemini_models(&flags, preview));
    }
    let body = codeassist(
        &account,
        provider.endpoint(),
        "fetchAvailableModels",
        &json!({"project":project}),
    )
    .await?;
    Ok(body["models"].as_object().ok_or(FAILED)?.iter().filter(|(_, row)| row["disabled"] != true).map(|(id, row)| json!({"model":id,"displayName":row["displayName"].as_str().unwrap_or(id),"contextWindow":row["maxTokens"],"maxOutputTokens":row["maxOutputTokens"]})).collect())
}

async fn gemini_flags(account: &Account, project: &str) -> Value {
    codeassist(account, account.provider.endpoint(), "listExperiments", &json!({"project":project,"metadata":{"ideName":"IDE_UNSPECIFIED","pluginType":"GEMINI","platform":"PLATFORM_UNSPECIFIED","duetProject":project}})).await.unwrap_or(Value::Null)
}
fn flag(flags: &Value, id: i64) -> bool {
    flags["flags"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|flag| flag["flagId"].as_i64() == Some(id) && flag["boolValue"] == true)
}
fn gemini_models(flags: &Value, preview: bool) -> Vec<Value> {
    let pro = flag(flags, 45760185);
    let no_pro = flag(flags, 45768879);
    let flash = flag(flags, 45842815);
    let lite = flag(flags, 45827489);
    let mut ids = Vec::new();
    if preview {
        if !no_pro {
            ids.push(if pro {
                "gemini-3.1-pro-preview"
            } else {
                "gemini-3-pro-preview"
            });
        }
        ids.push("gemini-3-flash-preview");
    }
    ids.push(if flash {
        "gemini-3.8-flash"
    } else {
        "gemini-3.5-flash"
    });
    ids.push(if lite {
        "gemini-3.5-flash-lite"
    } else {
        "gemini-3.1-flash-lite"
    });
    if !no_pro {
        ids.push("gemini-2.5-pro");
    }
    if !flash {
        ids.push("gemini-2.5-flash");
    }
    ids.push("gemini-2.5-flash-lite");
    ids.into_iter().map(|id| json!({"model":id})).collect()
}
pub(crate) async fn gemini_wire_model(account: &Account, project: &str, model: &str) -> String {
    let flags = gemini_flags(account, project).await;
    let flash = flag(&flags, 45842815);
    let lite = flag(&flags, 45827489);
    match model {
        "gemini-3.5-flash" | "gemini-3-flash" if flash => "gemini-3.8-flash",
        "gemini-3.5-flash" | "gemini-3.8-flash" if !flash => "gemini-3-flash",
        "gemini-3.1-flash-lite" if lite => "gemini-3.5-flash-lite",
        "gemini-3.5-flash-lite" if !lite => "gemini-3.1-flash-lite",
        _ => model,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn google_login_binds_loopback_state_and_pkce_to_each_attempt() {
        let pending = prepare_oauth(SubscriptionProvider::GeminiCli)
            .await
            .expect("loopback OAuth fixture");
        let url = url::Url::parse(&pending.url).expect("authorization URL");
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query.get("state"), Some(&pending.state));
        assert_eq!(
            query.get("code_challenge"),
            Some(&URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.as_bytes())))
        );
        assert_eq!(
            query.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(query.get("redirect_uri"), Some(&pending.redirect));
        assert!(pending
            .listener
            .local_addr()
            .expect("listener address")
            .ip()
            .is_loopback());
        assert!(
            pending
                .listener
                .local_addr()
                .expect("listener address")
                .port()
                != 15722
        );
    }
    #[test]
    fn gemini_catalog_follows_account_experiments_instead_of_quota_bucket_names() {
        let flags = json!({"flags":[{"flagId":45760185,"boolValue":true},{"flagId":45842815,"boolValue":true},{"flagId":45768879,"boolValue":true}]});
        let models = gemini_models(&flags, true);
        assert!(models.iter().any(|row| row["model"] == "gemini-3.8-flash"));
        assert!(!models
            .iter()
            .any(|row| row["model"].as_str().is_some_and(|id| id.contains("pro"))));
        assert!(!models.iter().any(|row| row["model"] == "gemini-3.5-flash"));
    }
    #[test]
    fn callbacks_require_exact_path_state_and_unambiguous_parameters() {
        assert!(valid_callback(
            "/auth/callback?state=fixture&code=synthetic",
            "/auth/callback",
            "fixture"
        )
        .is_some());
        for target in [
            "/other?state=fixture",
            "/auth/callback?state=wrong",
            "/auth/callback?state=fixture&state=other",
            "//evil/auth/callback?state=fixture",
        ] {
            assert!(valid_callback(target, "/auth/callback", "fixture").is_none());
        }
    }
    #[test]
    fn id_claims_are_bound_to_issuer_client_nonce_and_expiry() {
        let good = json!({"iss":"https://auth.openai.com","aud":"fixture-client","nonce":"fixture-nonce","exp":chrono::Utc::now().timestamp()+300});
        assert!(check_claims(&good, "fixture-client", "fixture-nonce").is_ok());
        for (key, value) in [
            ("iss", json!("https://example.com")),
            ("aud", json!("other")),
            ("nonce", json!("other")),
            ("exp", json!(0)),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(check_claims(&bad, "fixture-client", "fixture-nonce").is_err());
        }
    }
    #[test]
    fn summaries_never_serialize_tokens_or_config_paths() {
        let account = Account {
            id: "fixture".into(),
            provider: SubscriptionProvider::Claude,
            login: "Fixture".into(),
            project: String::new(),
            access: "synthetic-secret".into(),
            refresh: "synthetic-refresh".into(),
            expires: 0,
            client_id: String::new(),
            subject: String::new(),
            id_token: String::new(),
        };
        let public = serde_json::to_value(account.summary()).expect("summary");
        assert_eq!(public.as_object().expect("object").len(), 3);
        assert!(public.get("access").is_none());
        assert!(public.get("refresh").is_none());
    }
}
