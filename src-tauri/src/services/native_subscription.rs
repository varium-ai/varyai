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

#[cfg(test)]
mod google_observation_tests;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    pub id: String,
    pub provider: SubscriptionProvider,
    pub login: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    claude_home: Option<String>,
    #[serde(default)]
    reauth_required: bool,
    #[serde(default)]
    generation: String,
    #[serde(default)]
    plan: Option<String>,
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
    pub id: String,
    pub login: String,
    pub project: String,
    pub reauth_required: bool,
    pub plan: Option<String>,
}
impl Account {
    fn summary(&self) -> AccountSummary {
        AccountSummary {
            id: self.id.clone(),
            login: self.login.clone(),
            project: self.project.clone(),
            reauth_required: self.reauth_required
                || (self.provider != SubscriptionProvider::Claude
                    && self.access.is_empty()
                    && self.refresh.is_empty()),
            plan: if self.provider == SubscriptionProvider::ChatgptApi {
                super::subscription_plan::chatgpt_plan_from_token(&self.id_token)
                    .or_else(|| self.plan.clone())
            } else {
                self.plan.clone()
            },
        }
    }
    pub(crate) fn token(&self) -> &str {
        &self.access
    }
    pub(crate) fn claude_dir(&self) -> PathBuf {
        root()
            .join("claude")
            .join(self.claude_home.as_deref().unwrap_or(&self.id))
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
    #[serde(skip)]
    callback: Option<Arc<Callback>>,
}

struct Callback {
    redirect: String,
    state: String,
    sender: Mutex<Option<tokio::sync::oneshot::Sender<HashMap<String, String>>>>,
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
async fn keep(mut account: Account) -> Result<AccountSummary, String> {
    let _guard = ACCOUNTS.lock().await;
    let mut accounts = read().await?;
    account.generation = random();
    accounts.retain(|old| old.id != account.id);
    let summary = account.summary();
    accounts.push(account);
    write(accounts).await?;
    Ok(summary)
}
fn reauth_matches(target: &Account, account: &Account) -> bool {
    target.provider == account.provider
        && if account.provider == SubscriptionProvider::Claude {
            target.login.eq_ignore_ascii_case(&account.login)
        } else {
            !target.subject.is_empty()
                && target.subject == account.subject
                && (account.provider == SubscriptionProvider::ChatgptApi
                    || target.client_id == account.client_id)
        }
}
async fn keep_login(
    mut account: Account,
    target: Option<&Account>,
    flow_id: &str,
) -> Result<AccountSummary, String> {
    // Linearize cancellation with credential publication, including filesystem persistence.
    let registry = flows().lock().await;
    if !registry
        .get(flow_id)
        .is_some_and(|flow| flow.state == "waiting")
    {
        return Err(EXPIRED.into());
    }
    let Some(target) = target else {
        return keep(account).await;
    };
    if !reauth_matches(target, &account) {
        return Err("nativeSubscription.identityMismatch".into());
    }
    let _guard = ACCOUNTS.lock().await;
    let mut accounts = read().await?;
    let current = accounts
        .iter_mut()
        .find(|row| row.id == target.id && row.provider == target.provider)
        .ok_or(EXPIRED)?;
    if current.access != target.access
        || current.refresh != target.refresh
        || current.claude_home != target.claude_home
    {
        return Err(EXPIRED.into());
    }
    account.generation = random();
    let summary = account.summary();
    *current = account;
    write(accounts).await?;
    Ok(summary)
}

pub(crate) async fn list(provider: SubscriptionProvider) -> Result<Vec<AccountSummary>, String> {
    supported(provider)?;
    let mut summaries = Vec::new();
    for mut account in read()
        .await?
        .into_iter()
        .filter(|account| account.provider == provider)
    {
        if provider == SubscriptionProvider::Claude {
            account.plan = claude_file_plan(account.claude_dir())
                .await
                .or(account.plan);
        }
        summaries.push(account.summary());
    }
    Ok(summaries)
}
async fn claude_file_plan(dir: PathBuf) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        let value: Value =
            serde_json::from_slice(&std::fs::read(dir.join(".credentials.json")).ok()?).ok()?;
        super::subscription_plan::claude_plan(&value)
    })
    .await
    .ok()
    .flatten()
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
// Public installed-app OAuth parameters shipped by Antigravity and Magpie.
// These identify the desktop client; users provide authorization in the browser.
const ANTIGRAVITY_OAUTH_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const ANTIGRAVITY_OAUTH_CLIENT_SECRET: &str = "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";

fn antigravity_client(
    id: Option<String>,
    secret: Option<String>,
) -> Result<(String, String), String> {
    match (id, secret) {
        (None, None) => Ok((
            ANTIGRAVITY_OAUTH_CLIENT_ID.into(),
            ANTIGRAVITY_OAUTH_CLIENT_SECRET.into(),
        )),
        (Some(id), Some(secret)) if !id.trim().is_empty() && !secret.trim().is_empty() => {
            Ok((id, secret))
        }
        _ => Err("nativeSubscription.clientRequired".into()),
    }
}
fn client(provider: SubscriptionProvider) -> Result<(String, String), String> {
    if provider == SubscriptionProvider::Antigravity {
        antigravity_client(
            std::env::var("VARYAI_ANTIGRAVITY_CLIENT_ID").ok(),
            std::env::var("VARYAI_ANTIGRAVITY_CLIENT_SECRET").ok(),
        )
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
    token_request_with_client(provider, form, &crate::proxy::http_client::get()).await
}
async fn token_request_with_client(
    provider: SubscriptionProvider,
    form: &[(String, String)],
    client: &reqwest::Client,
) -> Result<Value, String> {
    let response = client
        .post(token_endpoint(provider))
        .form(form)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| FAILED)?;
    if !response.status().is_success() {
        let status = response.status();
        let value: Value = response.json().await.unwrap_or(Value::Null);
        return Err(token_failure(status, &value).into());
    }
    response.json().await.map_err(|_| FAILED.into())
}
fn token_failure(status: reqwest::StatusCode, value: &Value) -> &'static str {
    if status == reqwest::StatusCode::UNAUTHORIZED
        || (status == reqwest::StatusCode::BAD_REQUEST
            && matches!(
                value["error"].as_str(),
                Some("invalid_grant" | "invalid_token")
            ))
    {
        EXPIRED
    } else {
        FAILED
    }
}
fn google_plan(value: &Value) -> Option<String> {
    super::subscription_plan::google_plan(value)
}
fn apply_token(account: &mut Account, value: &Value) -> Result<(), String> {
    account.reauth_required = false;
    account.access = value["access_token"]
        .as_str()
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .ok_or(FAILED)?
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
    // Only rotations for the same binding share a network wait. Store mutations
    // remain short and compare the original credentials before publishing.
    let coordinator = refresh_coordinator(provider, id).await;
    let _refresh = coordinator.lock().await;
    let account = read()
        .await?
        .into_iter()
        .find(|a| a.provider == provider && a.id == id)
        .ok_or(EXPIRED)?;
    if account.reauth_required {
        return Err(EXPIRED.into());
    }
    if provider == SubscriptionProvider::Claude
        || account.expires > chrono::Utc::now().timestamp() + 180
    {
        return Ok(account);
    }
    if account.refresh.is_empty() {
        observe_rejection(&account).await;
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
    let token = match token_request(provider, &form).await {
        Ok(token) => token,
        Err(error) => {
            if error == EXPIRED {
                observe_rejection(&account).await;
            }
            return Err(error);
        }
    };
    if provider == SubscriptionProvider::ChatgptApi
        && token["scope"].as_str().is_some_and(|scope| {
            !scope
                .split_whitespace()
                .any(|s| s == "chatgpt.tokens.use.direct")
        })
    {
        observe_rejection(&account).await;
        return Err(EXPIRED.into());
    }
    let _guard = ACCOUNTS.lock().await;
    let mut accounts = read().await?;
    let current = accounts
        .iter_mut()
        .find(|a| a.provider == provider && a.id == id)
        .ok_or(EXPIRED)?;
    if !same_credentials(current, &account) {
        return Err(FAILED.into());
    }
    apply_token(current, &token)?;
    let result = current.clone();
    write(accounts).await?;
    Ok(result)
}

fn same_credentials(current: &Account, snapshot: &Account) -> bool {
    current.generation == snapshot.generation
        && current.access == snapshot.access
        && current.refresh == snapshot.refresh
        && current.expires == snapshot.expires
        && current.claude_home == snapshot.claude_home
        && current.reauth_required == snapshot.reauth_required
}

async fn refresh_coordinator(provider: SubscriptionProvider, id: &str) -> Arc<Mutex<()>> {
    type Coordinators = Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>;
    static LOCKS: OnceLock<Coordinators> = OnceLock::new();
    let key = format!(
        "{}:{id}",
        serde_json::to_string(&provider).unwrap_or_default()
    );
    let mut locks = LOCKS.get_or_init(Default::default).lock().await;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&key).and_then(std::sync::Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

pub(crate) async fn observe_rejection(account: &Account) {
    let _guard = ACCOUNTS.lock().await;
    let Ok(mut accounts) = read().await else {
        return;
    };
    let Some(current) = accounts
        .iter_mut()
        .find(|row| row.id == account.id && row.provider == account.provider)
    else {
        return;
    };
    if !same_credentials(current, account) {
        return;
    }
    current.reauth_required = true;
    if write(accounts).await.is_err() {
        log::warn!("Could not persist subscription authorization status");
    }
}
async fn observe_plan(account: &Account, plan: Option<String>) {
    let Some(plan) = plan else {
        return;
    };
    let _guard = ACCOUNTS.lock().await;
    let Ok(mut accounts) = read().await else {
        return;
    };
    let Some(current) = accounts.iter_mut().find(|row| {
        row.id == account.id && row.provider == account.provider && row.access == account.access
    }) else {
        return;
    };
    current.plan = Some(plan);
    let _ = write(accounts).await;
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

pub(crate) async fn start(
    provider: SubscriptionProvider,
    target_id: Option<String>,
    proxy: Option<String>,
) -> Result<Flow, String> {
    let login_client = crate::proxy::providers::codex_oauth_auth::sign_in_client(proxy.as_deref())
        .map_err(|error| {
            if error == "codexBrowser.proxyInvalid" {
                "nativeSubscription.proxyInvalid"
            } else {
                FAILED
            }
        })?;
    let target = if let Some(id) = target_id {
        Some(
            read()
                .await?
                .into_iter()
                .find(|row| row.id == id && row.provider == provider)
                .ok_or(EXPIRED)?,
        )
    } else {
        None
    };
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
        callback: None,
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
    if let Some(flow) = flows().lock().await.get_mut(&id) {
        flow.callback = pending
            .as_ref()
            .map(|pending| Arc::clone(&pending.callback));
    }
    let flow_id = id.clone();
    let task = tokio::spawn(async move {
        let result = tokio::time::timeout(Duration::from_secs(300), async {
            match pending {
                Some(pending) => {
                    finish_oauth(provider, pending, target.as_ref(), &flow_id, &login_client).await
                }
                None => claude_login(&flow_id, target.as_ref(), proxy.as_deref()).await,
            }
        })
        .await
        .unwrap_or_else(|_| Err(EXPIRED.into()));
        if let Some(flow) = flows().lock().await.get_mut(&flow_id) {
            flow.task = None;
            flow.callback = None;
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
pub(crate) async fn submit_callback(id: &str, value: &str) -> Result<(), String> {
    const INVALID: &str = "nativeSubscription.callbackInvalid";
    if value.len() > 16384 || value.chars().any(char::is_control) {
        return Err(INVALID.into());
    }
    let callback = flows()
        .lock()
        .await
        .get(id)
        .filter(|flow| flow.state == "waiting")
        .and_then(|flow| flow.callback.clone())
        .ok_or(INVALID)?;
    let url = url::Url::parse(value).map_err(|_| INVALID)?;
    let expected = url::Url::parse(&callback.redirect).map_err(|_| INVALID)?;
    if url.origin() != expected.origin()
        || url.path() != expected.path()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(INVALID.into());
    }
    let target = format!("{}?{}", url.path(), url.query().ok_or(INVALID)?);
    let query = valid_callback(&target, expected.path(), &callback.state).ok_or(INVALID)?;
    if query.contains_key("error") || query.get("code").is_none_or(String::is_empty) {
        return Err(INVALID.into());
    }
    let sender = callback.sender.lock().await.take().ok_or(INVALID)?;
    sender.send(query).map_err(|_| INVALID.into())
}
struct Pending {
    listener: tokio::net::TcpListener,
    url: String,
    path: &'static str,
    state: String,
    nonce: String,
    verifier: String,
    redirect: String,
    callback: Arc<Callback>,
    manual: tokio::sync::oneshot::Receiver<HashMap<String, String>>,
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
    let (sender, manual) = tokio::sync::oneshot::channel();
    let callback = Arc::new(Callback {
        redirect: redirect.clone(),
        state: state.clone(),
        sender: Mutex::new(Some(sender)),
    });
    Ok(Pending {
        listener,
        url: url.into(),
        path,
        state,
        nonce,
        verifier,
        redirect,
        callback,
        manual,
    })
}
async fn finish_oauth(
    provider: SubscriptionProvider,
    mut pending: Pending,
    target: Option<&Account>,
    flow_id: &str,
    login_client: &reqwest::Client,
) -> Result<AccountSummary, String> {
    let query = tokio::select! {
        manual = &mut pending.manual => manual.map_err(|_| FAILED)?,
        automatic = receive_callback(&pending.listener, pending.path, &pending.state) => automatic?,
    };
    pending.callback.sender.lock().await.take();
    finish_oauth_query(provider, pending, query, target, flow_id, login_client).await
}
async fn receive_callback(
    listener: &tokio::net::TcpListener,
    path: &str,
    state: &str,
) -> Result<HashMap<String, String>, String> {
    loop {
        let (mut socket, peer) = listener.accept().await.map_err(|_| FAILED)?;
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
            .and_then(|s| valid_callback(s, path, state));
        let message = if query.is_some() {
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nVary AI received the sign-in. Return to Vary AI to check the result."
        } else {
            "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid sign-in callback."
        };
        let _ = socket.write_all(message.as_bytes()).await;
        if let Some(query) = query {
            return Ok(query);
        }
    }
}
async fn finish_oauth_query(
    provider: SubscriptionProvider,
    pending: Pending,
    query: HashMap<String, String>,
    target: Option<&Account>,
    flow_id: &str,
    login_client: &reqwest::Client,
) -> Result<AccountSummary, String> {
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
    let token = token_request_with_client(provider, &form, login_client).await?;
    let mut account = Account {
        id: uuid::Uuid::new_v4().to_string(),
        provider,
        login: String::new(),
        project: String::new(),
        claude_home: None,
        reauth_required: false,
        generation: String::new(),
        plan: None,
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
        let claims = validate_openai_id(
            &account.id_token,
            &account.client_id,
            &pending.nonce,
            login_client,
        )
        .await?;
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
        let response = login_client
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
    if let Some(target) = target {
        if !reauth_matches(target, &account) {
            return Err("nativeSubscription.identityMismatch".into());
        }
        account.id = target.id.clone();
        account.project = target.project.clone();
        account.plan = target.plan.clone();
    }
    if openai {
        account.plan = super::subscription_plan::chatgpt_plan_from_token(&account.id_token);
    }
    keep_login(account, target, flow_id).await
}

async fn validate_openai_id(
    token: &str,
    client_id: &str,
    nonce: &str,
    login_client: &reqwest::Client,
) -> Result<Value, String> {
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
    let discovery: Value = login_client
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
    let jwks: Value = login_client
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
    claude_profile(account).await.map(|(login, _)| login)
}
async fn claude_profile(account: &Account) -> Result<(String, Option<String>), String> {
    let output = claude_command()
        .env("CLAUDE_CONFIG_DIR", account.claude_dir())
        .args(["auth", "status", "--json"])
        .output()
        .await
        .map_err(|_| "nativeSubscription.claudeMissing")?;
    let status: Value = serde_json::from_slice(&output.stdout).map_err(|_| FAILED)?;
    if !output.status.success() && status["loggedIn"] != false {
        return Err(FAILED.into());
    }
    if status["loggedIn"] != true
        || status["authMethod"]
            .as_str()
            .is_some_and(|s| s != "oauth_token" && s != "claude.ai")
    {
        return Err(EXPIRED.into());
    }
    let plan = status["subscriptionType"]
        .as_str()
        .and_then(|plan| super::subscription_plan::vendor_plan("Claude", plan));
    Ok((
        status["email"].as_str().unwrap_or("Claude account").into(),
        plan,
    ))
}
async fn claude_login(
    id: &str,
    target: Option<&Account>,
    proxy: Option<&str>,
) -> Result<AccountSummary, String> {
    let mut account = Account {
        id: id.into(),
        provider: SubscriptionProvider::Claude,
        login: String::new(),
        project: String::new(),
        claude_home: None,
        reauth_required: false,
        generation: String::new(),
        plan: None,
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
    let mut command = claude_command();
    if let Some(proxy) = proxy.filter(|proxy| !proxy.is_empty()) {
        for name in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
        ] {
            command.env_remove(name);
        }
        if proxy == "direct" {
            command.env("NO_PROXY", "*").env("no_proxy", "*");
        } else {
            command
                .env("HTTP_PROXY", proxy)
                .env("HTTPS_PROXY", proxy)
                .env("ALL_PROXY", proxy);
        }
    }
    let output = command
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
    let (login, plan) = claude_profile(&account).await?;
    account.login = login;
    account.plan = plan.or(claude_file_plan(account.claude_dir()).await);
    if let Some(target) = target {
        if !target.login.eq_ignore_ascii_case(&account.login) {
            return Err("nativeSubscription.identityMismatch".into());
        }
        account.claude_home = Some(account.id.clone());
        account.id = target.id.clone();
        account.plan = account.plan.or_else(|| target.plan.clone());
    }
    keep_login(account, target, id).await
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
        claude_home: None,
        reauth_required: false,
        generation: String::new(),
        plan: None,
        access: String::new(),
        refresh: String::new(),
        expires: 0,
        client_id: client(provider)?.0,
        subject: String::new(),
        id_token: String::new(),
    };
    if provider == SubscriptionProvider::Claude {
        account.plan = super::subscription_plan::claude_plan(&value);
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
    observe_plan(account, google_plan(&response)).await;
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
    let mut request = crate::proxy::http_client::get()
        .post(format!("{base}/v1internal:{method}"))
        .bearer_auth(&account.access)
        .json(body)
        .timeout(Duration::from_secs(25));
    request = request.header("User-Agent", google_user_agent(account.provider));
    if account.provider == SubscriptionProvider::Antigravity {
        request = request.header("X-Goog-Api-Client", "gl-node/22.21.1");
    }
    let response = request.send().await.map_err(|_| FAILED)?;
    if !response.status().is_success() {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            observe_rejection(account).await;
            return Err(EXPIRED.into());
        }
        return Err(FAILED.into());
    }
    response.json().await.map_err(|_| FAILED.into())
}

fn google_user_agent(provider: SubscriptionProvider) -> String {
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        value => value,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        value => value,
    };
    match provider {
        SubscriptionProvider::Antigravity => {
            format!("antigravity/hub/2.9.1 {platform}/{arch}")
        }
        SubscriptionProvider::GeminiCli => {
            format!("GeminiCLI/0.61.0/gemini-2.5-pro ({platform}; {arch}; terminal)")
        }
        _ => "vary-ai".into(),
    }
}

async fn antigravity_models(account: &Account, project: &str) -> Result<Value, String> {
    let mut tiers = Vec::new();
    if let Ok(response) = codeassist(
        account,
        "https://cloudcode-pa.googleapis.com",
        "loadCodeAssist",
        &json!({"metadata":{"ideType":"ANTIGRAVITY"}}),
    )
    .await
    {
        for tier in [
            response["paidTier"]["id"].as_str(),
            response["currentTier"]["id"].as_str(),
        ]
        .into_iter()
        .flatten()
        {
            if !tier.is_empty() && tier != "free-tier" && !tiers.iter().any(|item| item == tier) {
                tiers.push(tier.to_owned());
            }
        }
    }
    for tier in tiers {
        let result = codeassist(
            account,
            account.provider.endpoint(),
            "fetchAvailableModels",
            &json!({"project":project,"entitlement":{"userTier":tier}}),
        )
        .await;
        if result.is_ok() {
            return result;
        }
    }
    codeassist(
        account,
        account.provider.endpoint(),
        "fetchAvailableModels",
        &json!({"project":project}),
    )
    .await
}
async fn observe_google_plan(account: &Account) {
    if account.plan.is_none() && !account.project.is_empty() {
        if let Ok(info) = codeassist(account, "https://cloudcode-pa.googleapis.com", "loadCodeAssist",
            &json!({"cloudaicompanionProject":account.project,"metadata":{"ideType":if account.provider == SubscriptionProvider::Antigravity {"ANTIGRAVITY"} else {"IDE_UNSPECIFIED"}}})).await {
            observe_plan(account, google_plan(&info)).await;
        }
    }
}

fn subscription_model(row: &Value) -> Option<Value> {
    let id = row["slug"]
        .as_str()
        .or_else(|| row["id"].as_str())?
        .to_owned();
    let mut model = serde_json::to_value(super::model_fetch::FetchedModel::from_entry(
        id.clone(),
        None,
        row,
    ))
    .ok()?;
    model["model"] = json!(id);
    model["displayName"] = json!(row["display_name"]
        .as_str()
        .or_else(|| row["name"].as_str())
        .unwrap_or(&id));
    Some(model)
}

pub(crate) async fn models(provider: SubscriptionProvider, id: &str) -> Result<Vec<Value>, String> {
    let account = resolve(provider, id).await?;
    if provider == SubscriptionProvider::Claude {
        if let Err(error) = claude_identity(&account).await {
            if error == EXPIRED {
                observe_rejection(&account).await;
            }
            return Err(error);
        }
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
            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                observe_rejection(&account).await;
                return Err(EXPIRED.into());
            }
            return Err(FAILED.into());
        }
        let body: Value = response.json().await.map_err(|_| FAILED)?;
        let models = body["models"]
            .as_array()
            .or_else(|| body["data"].as_array())
            .ok_or(FAILED)?;
        return Ok(models
            .iter()
            .filter(|row| row["visibility"].as_str().is_none_or(|v| v == "list"))
            .filter_map(subscription_model)
            .collect());
    }
    observe_google_plan(&account).await;

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
    let body = if provider == SubscriptionProvider::Antigravity {
        antigravity_models(&account, &project).await?
    } else {
        codeassist(
            &account,
            provider.endpoint(),
            "fetchAvailableModels",
            &json!({"project":project}),
        )
        .await?
    };
    Ok(body["models"].as_object().ok_or(FAILED)?.iter().filter(|(_, row)| row["disabled"] != true).map(|(id, row)| json!({"model":id,"displayName":row["displayName"].as_str().unwrap_or(id),"contextWindow":row["maxTokens"],"maxOutputTokens":row["maxOutputTokens"]})).collect())
}

fn quota_tier(name: &str, value: &Value) -> Option<super::subscription::QuotaTier> {
    let remaining = value["remainingFraction"]
        .as_f64()
        .filter(|v| v.is_finite())?
        .clamp(0.0, 1.0);
    Some(super::subscription::QuotaTier {
        name: name.into(),
        utilization: (1.0 - remaining) * 100.0,
        resets_at: value["resetTime"].as_str().map(str::to_owned),
        used_value_usd: None,
        max_value_usd: None,
    })
}

fn quota_group_tier(name: &str, value: &Value) -> Option<super::subscription::QuotaTier> {
    if value["disabled"] == true {
        return None;
    }
    let remaining = value["remainingFraction"]
        .as_f64()
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    Some(super::subscription::QuotaTier {
        name: name.into(),
        utilization: (1.0 - remaining) * 100.0,
        resets_at: value["resetTime"].as_str().map(str::to_owned),
        used_value_usd: None,
        max_value_usd: None,
    })
}
fn parse_google_quota(
    provider: SubscriptionProvider,
    value: &Value,
) -> super::subscription::SubscriptionQuota {
    let pools: Vec<_> = value["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| {
            group["buckets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|bucket| bucket["disabled"] != true)
                .filter_map(move |bucket| {
                    let name = format!(
                        "{} · {}",
                        group["displayName"].as_str().unwrap_or("Quota"),
                        bucket["displayName"]
                            .as_str()
                            .or_else(|| bucket["window"].as_str())
                            .unwrap_or("")
                    );
                    quota_group_tier(&name, bucket)
                })
        })
        .collect();
    let tiers = if !pools.is_empty() {
        pools
    } else if provider == SubscriptionProvider::GeminiCli {
        value["buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| quota_tier(row["modelId"].as_str()?, row))
            .collect()
    } else {
        value["models"]
            .as_object()
            .into_iter()
            .flat_map(|models| models.iter())
            .filter(|(_, row)| row["disabled"] != true)
            .filter_map(|(id, row)| {
                quota_tier(row["displayName"].as_str().unwrap_or(id), &row["quotaInfo"])
            })
            .collect()
    };
    super::subscription::SubscriptionQuota {
        tool: "subscription".into(),
        credential_status: super::subscription::CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        codex: None,
        error: None,
        queried_at: Some(chrono::Utc::now().timestamp_millis()),
    }
}
/// Query only the explicitly selected managed account; never fall back to a native login.
pub(crate) async fn quota(
    provider: SubscriptionProvider,
    id: &str,
) -> Result<super::subscription::SubscriptionQuota, String> {
    use super::subscription::{CredentialStatus, SubscriptionQuota};
    let account = match resolve(provider, id).await {
        Ok(account) => account,
        Err(error) if error == EXPIRED => {
            return Ok(SubscriptionQuota::error(
                "subscription",
                CredentialStatus::Expired,
                EXPIRED.into(),
            ))
        }
        Err(_) => return Err(FAILED.into()),
    };
    let result = async {
        if google(provider) {
            observe_google_plan(&account).await;
            let project = google_project(&account).await?;
            let method = if provider == SubscriptionProvider::GeminiCli {
                "retrieveUserQuota"
            } else {
                "fetchAvailableModels"
            };
            let value = if provider == SubscriptionProvider::Antigravity {
                antigravity_models(&account, &project).await?
            } else {
                codeassist(
                    &account,
                    provider.endpoint(),
                    method,
                    &json!({"project":project}),
                )
                .await?
            };
            if provider == SubscriptionProvider::Antigravity {
                if let Ok(summary) = codeassist(
                    &account,
                    provider.endpoint(),
                    "retrieveUserQuotaSummary",
                    &json!({"project":project}),
                )
                .await
                {
                    let pools = parse_google_quota(provider, &summary);
                    if !pools.tiers.is_empty() {
                        return Ok(pools);
                    }
                }
            }
            let mut quota = parse_google_quota(provider, &value);
            if provider == SubscriptionProvider::GeminiCli {
                let flags = gemini_flags(&account, &project).await;
                for tier in &mut quota.tiers {
                    tier.name = gemini_shown_model(&flags, &tier.name).into();
                }
            }
            return Ok(quota);
        }
        if provider == SubscriptionProvider::Claude {
            if let Some(quota) = claude_cli_usage(&account).await {
                return Ok(quota);
            }
            let dir = account.claude_dir();
            let value = tokio::task::spawn_blocking(move || {
                #[cfg(target_os = "macos")]
                {
                    let service = format!(
                        "Claude Code-credentials-{}",
                        &format!("{:x}", Sha256::digest(dir.to_string_lossy().as_bytes()))[..8]
                    );
                    if let Ok(output) = std::process::Command::new("security")
                        .args(["find-generic-password", "-s", &service, "-w"])
                        .stderr(std::process::Stdio::null())
                        .output()
                    {
                        if output.status.success() {
                            if let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) {
                                return Some(value);
                            }
                        }
                    }
                }
                std::fs::read(dir.join(".credentials.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            })
            .await
            .map_err(|_| FAILED)?
            .ok_or(FAILED)?;
            observe_plan(&account, super::subscription_plan::claude_plan(&value)).await;
            let token = value
                .pointer("/claudeAiOauth/accessToken")
                .and_then(Value::as_str)
                .ok_or(FAILED)?;
            let response = crate::proxy::http_client::get()
                .get("https://api.anthropic.com/api/oauth/usage")
                .bearer_auth(token)
                .header("anthropic-beta", "oauth-2025-04-20")
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|_| FAILED)?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                // A direct usage read cannot prove that the CLI's refresh grant
                // was revoked. Let the CLI renew its own isolated credentials.
                if !claude_usage_rejection_is_terminal(&value) {
                    return Err(FAILED.into());
                }
                return Err(EXPIRED.into());
            }
            if !response.status().is_success() {
                return Err(FAILED.into());
            }
            let body = response.json::<Value>().await.map_err(|_| FAILED)?;
            return Ok(super::subscription::parse_claude_quota(&body));
        }
        // Sign in with ChatGPT does not expose a separate allowance endpoint.
        Ok(parse_google_quota(provider, &Value::Null))
    }
    .await;
    match result {
        Err(error) if error == EXPIRED => {
            observe_rejection(&account).await;
            Ok(SubscriptionQuota::error(
                "subscription",
                CredentialStatus::Expired,
                EXPIRED.into(),
            ))
        }
        other => other,
    }
}

#[derive(Default)]
struct ExperimentSnapshot {
    flags: Value,
    checked: Option<std::time::Instant>,
}
async fn experiment_cache(account: &Account, project: &str) -> Arc<Mutex<ExperimentSnapshot>> {
    type Cache = Mutex<HashMap<String, Arc<Mutex<ExperimentSnapshot>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = format!(
        "{}:{}:{}:{}:{}",
        account.id, account.generation, account.subject, account.client_id, project
    );
    let mut cache = CACHE.get_or_init(Default::default).lock().await;
    cache.entry(key).or_default().clone()
}
fn parse_claude_cli_usage(value: &Value) -> Option<super::subscription::SubscriptionQuota> {
    if value["is_error"] == true {
        return None;
    }
    let text = value["result"].as_str()?;
    let pattern =
        regex::Regex::new(r"^Current (session|week(?: \(([^)]+)\))?):\s*([0-9.]+)% used").ok()?;
    let tiers: Vec<_> = text
        .lines()
        .filter_map(|line| {
            let captures = pattern.captures(line.trim())?;
            let used: f64 = captures.get(3)?.as_str().parse().ok()?;
            if !used.is_finite() || !(0.0..=100.0).contains(&used) {
                return None;
            }
            let name = if captures.get(1)?.as_str() == "session" {
                "five_hour".into()
            } else {
                match captures.get(2).map(|m| m.as_str()) {
                    None | Some("all models") => "seven_day".into(),
                    Some(name) => {
                        format!("seven_day_{}", name.to_ascii_lowercase().replace(' ', "_"))
                    }
                }
            };
            Some(super::subscription::QuotaTier {
                name,
                utilization: used,
                resets_at: None,
                used_value_usd: None,
                max_value_usd: None,
            })
        })
        .collect();
    if tiers.is_empty() {
        return None;
    }
    let mut quota = parse_google_quota(SubscriptionProvider::Claude, &Value::Null);
    quota.tiers = tiers;
    Some(quota)
}
async fn claude_cli_usage(account: &Account) -> Option<super::subscription::SubscriptionQuota> {
    let temporary = tempfile::tempdir().ok()?;
    let mut command = claude_command();
    command
        .env("CLAUDE_CONFIG_DIR", account.claude_dir())
        .current_dir(temporary.path())
        .args([
            "-p",
            "/usage",
            "--output-format",
            "json",
            "--no-session-persistence",
            "--setting-sources",
            "",
            "--tools",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
        ])
        .stderr(std::process::Stdio::null());
    let output = tokio::time::timeout(Duration::from_secs(25), command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = serde_json::from_slice::<Value>(&output.stdout).ok()?;
    parse_claude_cli_usage(&value)
}
fn claude_usage_rejection_is_terminal(value: &Value) -> bool {
    let oauth = value
        .get("claudeAiOauth")
        .or_else(|| value.get("claude.ai_oauth"));
    oauth
        .and_then(|entry| entry["refreshToken"].as_str())
        .is_none_or(str::is_empty)
}
async fn gemini_flags(account: &Account, project: &str) -> Value {
    let cache = experiment_cache(account, project).await;
    let coordinator = refresh_coordinator(
        account.provider,
        &format!("experiments:{}:{project}", account.id),
    )
    .await;
    let _refresh = coordinator.lock().await;
    let snapshot = cache.lock().await;
    if snapshot
        .checked
        .is_some_and(|at| at.elapsed() < Duration::from_secs(3600))
    {
        return snapshot.flags.clone();
    }
    drop(snapshot);
    // Experiment observations never change authentication state or replace a
    // known rollout with all flags off after an auxiliary service failure.
    let response = crate::proxy::http_client::get()
        .post(format!("{}/v1internal:listExperiments", account.provider.endpoint()))
        .bearer_auth(&account.access)
        .json(&json!({"project":project,"metadata":{"ideName":"IDE_UNSPECIFIED","pluginType":"GEMINI","platform":"PLATFORM_UNSPECIFIED","duetProject":project}}))
        .timeout(Duration::from_secs(5)).send().await;
    if let Ok(response) = response {
        if response.status().is_success() {
            if let Ok(flags) = response.json::<Value>().await {
                if flags["flags"].is_array() {
                    cache.lock().await.flags = flags;
                }
            }
        }
    }
    let mut snapshot = cache.lock().await;
    snapshot.checked = Some(std::time::Instant::now());
    snapshot.flags.clone()
}
fn gemini_shown_model<'a>(flags: &Value, model: &'a str) -> &'a str {
    match model {
        "gemini-3-flash" | "gemini-3.5-flash" | "gemini-3.8-flash" => {
            if flag(flags, 45842815) {
                "gemini-3.8-flash"
            } else {
                "gemini-3.5-flash"
            }
        }
        "gemini-3.1-flash-lite" | "gemini-3.5-flash-lite" => {
            if flag(flags, 45827489) {
                "gemini-3.5-flash-lite"
            } else {
                "gemini-3.1-flash-lite"
            }
        }
        _ => model,
    }
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
    ids.into_iter()
        .map(|id| {
            let mut row = json!({"model":id,"inputModalities":["text","image"],"reasoning":true});
            // Stable Gemini 2.5 limits; rollout aliases expose only known facts.
            if id.starts_with("gemini-2.5-") {
                row["contextWindow"] = json!(1_048_576);
                row["maxOutputTokens"] = json!(65_536);
            }
            row
        })
        .collect()
}
pub(crate) async fn gemini_wire_model(account: &Account, project: &str, model: &str) -> String {
    let cache = experiment_cache(account, project).await;
    let flags = cache.lock().await.flags.clone();
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
    #[test]
    fn chatgpt_api_summary_recovers_previously_unrecognized_token_plans(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let claims = json!({"https://api.openai.com/auth":{"chatgpt_plan_type":"prolite"}});
        let token = format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?)
        );
        let account: Account = serde_json::from_value(
            json!({"id":"fixture", "provider":"chatgpt_api", "login":"fixture@example.test", "access":"synthetic-access", "idToken":token}),
        )?;
        let summary = account.summary();
        assert_eq!(summary.plan.as_deref(), Some("ChatGPT Prolite"));
        let public = serde_json::to_string(&summary)?;
        assert!(!public.contains("synthetic-access") && !public.contains("idToken"));
        Ok(())
    }
    #[tokio::test]
    async fn claude_local_plan_reads_metadata_without_exposing_credential_fields(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join(".credentials.json"),
            json!({"claudeAiOauth":{"accessToken":"synthetic-access","subscriptionType":"max_5x"}})
                .to_string(),
        )?;
        assert_eq!(
            claude_file_plan(dir.path().to_owned()).await.as_deref(),
            Some("Claude Max_5x")
        );
        Ok(())
    }
    #[test]
    fn google_plan_uses_upstream_tier_names_and_paid_tier_precedence() {
        for (value, expected) in [
            (
                json!({"paidTier":{"id":"future-paid","name":"Google AI Premium"},"currentTier":{"id":"free-tier","name":"Free"}}),
                "Google AI Premium",
            ),
            (
                json!({"currentTier":{"id":"future-tier","name":"Google AI Education"}}),
                "Google AI Education",
            ),
            (
                json!({"paidTier":{"id":"g1-ultra-tier"}}),
                "Google AI Ultra",
            ),
        ] {
            assert_eq!(google_plan(&value).as_deref(), Some(expected));
        }
    }
    #[tokio::test]
    async fn pasted_google_callback_is_bound_to_the_active_attempt_and_accepted_once(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for provider in [
            SubscriptionProvider::GeminiCli,
            SubscriptionProvider::Antigravity,
        ] {
            let mut pending = prepare_oauth(provider).await?;
            let id = random();
            flows().lock().await.insert(
                id.clone(),
                Flow {
                    id: id.clone(),
                    url: Some(pending.url.clone()),
                    state: "waiting".into(),
                    account: None,
                    error: None,
                    task: None,
                    callback: Some(Arc::clone(&pending.callback)),
                },
            );
            for invalid in [
                format!(
                    "https://example.test/{}?state={}&code=synthetic",
                    pending.path, pending.state
                ),
                format!("{}?state=wrong&code=synthetic", pending.redirect),
                format!(
                    "{}?state={}&state={}&code=synthetic",
                    pending.redirect, pending.state, pending.state
                ),
            ] {
                assert!(submit_callback(&id, &invalid).await.is_err());
            }
            let callback = format!(
                "{}?state={}&code=synthetic",
                pending.redirect, pending.state
            );
            submit_callback(&id, &callback).await?;
            let query = (&mut pending.manual).await?;
            assert!(query.get("code").is_some_and(|value| value == "synthetic"));
            assert!(submit_callback(&id, &callback).await.is_err());
            cancel(&id).await;
            assert!(submit_callback(&id, &callback).await.is_err());
        }
        Ok(())
    }
    #[tokio::test]
    async fn antigravity_login_uses_installed_app_defaults_and_opens_a_google_flow(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (id, secret) = antigravity_client(None, None)?;
        assert!(id.ends_with(".apps.googleusercontent.com"));
        assert!(!secret.is_empty());
        assert!(antigravity_client(Some("synthetic-client".into()), None).is_err());
        let override_pair = antigravity_client(
            Some("synthetic-client".into()),
            Some("synthetic-secret".into()),
        )?;
        assert!(override_pair.0 == "synthetic-client" && override_pair.1 == "synthetic-secret");
        let pending = prepare_oauth(SubscriptionProvider::Antigravity).await?;
        let url = url::Url::parse(&pending.url)?;
        assert!(url.host_str() == Some("accounts.google.com"));
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert!(query
            .get("redirect_uri")
            .is_some_and(|value| value.starts_with("http://localhost:")
                && value.ends_with("/oauth-callback")));
        assert!(query
            .get("scope")
            .is_some_and(|value| value.contains("/auth/cclog")
                && value.contains("/auth/experimentsandconfigs")));
        assert!(query
            .get("code_challenge_method")
            .is_some_and(|value| value == "S256"));
        Ok(())
    }

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
    fn refresh_errors_distinguish_revocation_from_transient_and_permission_failures() {
        assert_eq!(
            token_failure(
                reqwest::StatusCode::BAD_REQUEST,
                &json!({"error":"invalid_grant"})
            ),
            EXPIRED
        );
        for status in [
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            reqwest::StatusCode::FORBIDDEN,
        ] {
            assert_eq!(token_failure(status, &json!({})), FAILED);
        }
        assert_eq!(
            google_plan(&json!({"paidTier":{"id":"g1-pro-tier"}})).as_deref(),
            Some("Google AI Pro")
        );
    }

    #[test]
    fn reauthentication_preserves_identity_and_status_without_exposing_credentials(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut account = Account {
            id: "fixture".into(),
            provider: SubscriptionProvider::GeminiCli,
            login: "fixture@example.test".into(),
            project: "project-fixture".into(),
            claude_home: None,
            reauth_required: true,
            generation: String::new(),
            plan: Some("Google AI Pro".into()),
            access: "synthetic-access".into(),
            refresh: "synthetic-refresh".into(),
            expires: 0,
            client_id: "fixture-client".into(),
            subject: "fixture-subject".into(),
            id_token: String::new(),
        };
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("accounts.json");
        std::fs::write(&path, serde_json::to_vec(&account)?)?;
        let restored: Account = serde_json::from_slice(&std::fs::read(path)?)?;
        assert!(restored.summary().reauth_required);
        let mut next = restored.clone();
        next.subject = "different-user".into();
        assert!(!reauth_matches(&restored, &next));
        next.subject = restored.subject.clone();
        assert!(reauth_matches(&restored, &next));
        apply_token(
            &mut account,
            &json!({"access_token":"new-access","expires_in":3600}),
        )?;
        assert!(!account.summary().reauth_required);
        let public = serde_json::to_string(&account.summary())?;
        assert!(!public.contains("synthetic-access") && !public.contains("synthetic-refresh"));
        Ok(())
    }

    #[test]
    fn google_quota_never_invents_remaining_allowance_for_missing_or_invalid_fields() {
        let quota = parse_google_quota(
            SubscriptionProvider::GeminiCli,
            &json!({"buckets":[
                {"modelId":"fixture", "remainingFraction":0.25, "resetTime":"2026-11-06T00:00:00Z"},
                {"modelId":"missing"}, {"modelId":"bad","remainingFraction":"not-a-number"}
            ]}),
        );
        assert_eq!(quota.tiers.len(), 1);
        assert_eq!(quota.tiers[0].utilization, 75.0);
        let antigravity = parse_google_quota(
            SubscriptionProvider::Antigravity,
            &json!({"models":{
                "one":{"displayName":"Fixture model","quotaInfo":{"remainingFraction":0.6}},
                "two":{"disabled":true,"quotaInfo":{"remainingFraction":1.0}}
            }}),
        );
        assert_eq!(antigravity.tiers.len(), 1);
        assert_eq!(antigravity.tiers[0].utilization, 40.0);
        let pools = parse_google_quota(
            SubscriptionProvider::Antigravity,
            &json!({"groups":[{"displayName":"Gemini models", "buckets":[{"window":"weekly","remainingFraction":0.2}, {"window":"five_hour"}, {"disabled":true,"remainingFraction":1.0}]}]}),
        );
        assert_eq!(pools.tiers.len(), 2);
        assert_eq!(pools.tiers[0].utilization, 80.0);
        assert_eq!(pools.tiers[1].utilization, 100.0);
        assert!(pools.tiers[0].name.contains("weekly"));
    }

    #[test]
    fn summaries_never_serialize_tokens_or_config_paths() {
        let account = Account {
            id: "fixture".into(),
            provider: SubscriptionProvider::Claude,
            login: "Fixture".into(),
            project: String::new(),
            claude_home: None,
            reauth_required: false,
            generation: String::new(),
            plan: Some("Claude Pro".into()),
            access: "synthetic-secret".into(),
            refresh: "synthetic-refresh".into(),
            expires: 0,
            client_id: String::new(),
            subject: String::new(),
            id_token: String::new(),
        };
        let public = serde_json::to_value(account.summary()).expect("summary");
        assert_eq!(public["plan"], "Claude Pro");
        assert_eq!(public["reauthRequired"], false);
        assert!(public.get("access").is_none());
        assert!(public.get("refresh").is_none());
    }
}

#[cfg(test)]
mod subscription_parity_tests {
    use super::*;
    #[test]
    fn gemini_catalog_has_context_images_and_reasoning() {
        for model in gemini_models(&Value::Null, true) {
            if model["model"]
                .as_str()
                .is_some_and(|id| id.starts_with("gemini-2.5-"))
            {
                assert!(model["contextWindow"].as_u64().is_some_and(|n| n > 0));
            }
            assert!(model["inputModalities"]
                .as_array()
                .is_some_and(|v| v.contains(&json!("image"))));
            assert!(model["reasoning"].is_boolean());
        }
    }
}

#[cfg(test)]
mod renewal_isolation_tests {
    use super::*;
    #[tokio::test]
    async fn rotations_are_single_flight_only_for_the_same_account() {
        let first = refresh_coordinator(SubscriptionProvider::GeminiCli, "fixture-one").await;
        let same = refresh_coordinator(SubscriptionProvider::GeminiCli, "fixture-one").await;
        let second = refresh_coordinator(SubscriptionProvider::GeminiCli, "fixture-two").await;
        assert!(Arc::ptr_eq(&first, &same));
        let _held = first.lock().await;
        assert!(same.try_lock().is_err());
        assert!(second.try_lock().is_ok());
        assert!(ACCOUNTS.try_lock().is_ok());
    }
    #[test]
    fn direct_claude_usage_rejection_is_not_terminal_for_renewable_credentials() {
        assert!(!claude_usage_rejection_is_terminal(
            &json!({"claudeAiOauth":{"accessToken":"synthetic","refreshToken":"synthetic-refresh"}})
        ));
        assert!(claude_usage_rejection_is_terminal(
            &json!({"claudeAiOauth":{"accessToken":"synthetic"}})
        ));
    }
    #[test]
    fn siwc_discovery_preserves_capabilities() {
        let model = subscription_model(&json!({"slug":"fixture","input_modalities":["text","image"],"context_window":100000,"max_context_window":200000,"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]})).expect("public model");
        assert_eq!(model["max_context_window"], 200000);
        assert_eq!(model["inputModalities"], json!(["text", "image"]));
        assert_eq!(model["reasoningLevels"], json!(["low", "high"]));
    }
    #[tokio::test]
    async fn inference_uses_cached_rollout_without_an_experiment_request() {
        let account: Account = serde_json::from_value(
            json!({"id":"fixture-flags","provider":"gemini_cli","login":"fixture"}),
        )
        .expect("fixture");
        let cache = experiment_cache(&account, "fixture-project").await;
        cache.lock().await.flags = json!({"flags":[{"flagId":45842815,"boolValue":true}]});
        assert_eq!(
            gemini_wire_model(&account, "fixture-project", "gemini-3.5-flash").await,
            "gemini-3.8-flash"
        );
        assert_eq!(
            gemini_shown_model(&cache.lock().await.flags, "gemini-3-flash"),
            "gemini-3.8-flash"
        );
        assert!(!Arc::ptr_eq(
            &cache,
            &experiment_cache(&account, "other-project").await
        ));
    }
}

#[cfg(test)]
mod claude_usage_tests {
    use super::*;
    #[test]
    fn cli_usage_observations_are_typed_and_do_not_invent_missing_windows() {
        let quota = parse_claude_cli_usage(&json!({"result":"Current session: 13% used\nCurrent week (all models): 4% used\nCurrent week (Opus): 0% used"})).expect("CLI usage");
        assert_eq!(quota.tiers.len(), 3);
        assert_eq!(quota.tiers[0].utilization, 13.0);
        assert_eq!(quota.tiers[1].name, "seven_day");
        assert!(parse_claude_cli_usage(&json!({"result":"Loading usage data…"})).is_none());
    }
}
