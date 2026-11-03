//! Independent browser grants. Native refresh tokens are never copied or spent.
use super::*;

const AUTH_ERROR: &str = "workbuddy.checkin.authError";
const ACCOUNT_ERROR: &str = "workbuddy.checkin.accountUnavailable";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManagedAccount {
    uid: String,
    access_token: String,
    refresh_token: String,
    domain: String,
    expires_at: i64,
    #[serde(default)]
    refresh_expires_at: i64,
    enterprise_id: String,
    label: String,
}

impl ManagedAccount {
    pub(super) fn account(&self) -> Account {
        Account {
            uid: self.uid.clone(),
            access: self.access_token.clone(),
            domain: self.domain.clone(),
            enterprise_id: self.enterprise_id.clone(),
            label: self.label.clone(),
            client_version: None,
            device_token: None,
            native: false,
        }
    }

    fn update_tokens(&mut self, value: &Value) -> Result<(), String> {
        let access = value
            .get("accessToken")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or(AUTH_ERROR)?;
        self.access_token = access.into();
        for (name, target) in [
            ("refreshToken", &mut self.refresh_token),
            ("domain", &mut self.domain),
        ] {
            if let Some(value) = value
                .get(name)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
            {
                *target = value.into();
            }
        }
        let now = Utc::now().timestamp_millis();
        self.expires_at = expiry(value, "expiresAt", "expiresIn", now);
        let refresh_expiry = expiry(value, "refreshExpiresAt", "refreshExpiresIn", now);
        if refresh_expiry > 0 {
            self.refresh_expires_at = refresh_expiry;
        }
        Ok(())
    }
}

fn expiry(value: &Value, absolute: &str, relative: &str, now: i64) -> i64 {
    value
        .get(absolute)
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .or_else(|| {
            value
                .get(relative)
                .and_then(Value::as_i64)
                .filter(|n| *n > 0)
                .and_then(|n| n.checked_mul(1000)?.checked_add(now))
        })
        .unwrap_or(0)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountEntry {
    pub(crate) id: String,
    label: String,
    source: &'static str,
    status: Status,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountList {
    pub(crate) accounts: Vec<AccountEntry>,
    native_status: Status,
}

#[derive(Clone)]
pub(super) struct PendingLogin {
    id: String,
    state: String,
    deadline: i64,
    grant: Option<ManagedAccount>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginStart {
    login_id: String,
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginStatus {
    complete: bool,
    account_id: Option<String>,
}

impl Manager {
    fn accounts_path(&self) -> PathBuf {
        self.path.with_file_name("workbuddy-checkin-accounts.json")
    }

    async fn read_managed(&self) -> Result<BTreeMap<String, ManagedAccount>, String> {
        let path = self.accounts_path();
        tokio::task::spawn_blocking(move || {
            let bytes = match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
                Err(_) => return Err(STORAGE_ERROR.into()),
            };
            let accounts: BTreeMap<String, ManagedAccount> =
                serde_json::from_slice(&bytes).map_err(|_| STORAGE_ERROR.to_owned())?;
            if accounts.iter().any(|(id, a)| {
                a.uid.is_empty() || a.access_token.is_empty() || id != &a.account().key()
            }) {
                return Err(STORAGE_ERROR.into());
            }
            Ok(accounts)
        })
        .await
        .map_err(|_| STORAGE_ERROR.to_owned())?
    }

    pub(super) async fn write_managed(
        &self,
        accounts: &BTreeMap<String, ManagedAccount>,
    ) -> Result<(), String> {
        let path = self.accounts_path();
        let bytes = serde_json::to_vec(accounts).map_err(|_| STORAGE_ERROR.to_owned())?;
        tokio::task::spawn_blocking(move || {
            crate::config::atomic_write_private(&path, &bytes).map_err(|_| STORAGE_ERROR.to_owned())
        })
        .await
        .map_err(|_| STORAGE_ERROR.to_owned())?
    }

    pub(crate) async fn accounts(&self) -> Result<AccountList, String> {
        let _guard = self.lock.lock().await;
        let store = self.read_store().await?;
        let managed = self.read_managed().await?;
        let native = self.account().await;
        let now = Utc::now().timestamp();
        let mut accounts = Vec::new();
        let mut append = |account: Account, source| {
            let id = account.key();
            let label = if account.label.trim().is_empty() {
                format!("WorkBuddy · {}", &id[..6])
            } else {
                account.label.clone()
            };
            let status = Self::snapshot(&store, &Ok(account), now);
            accounts.push(AccountEntry {
                id,
                label,
                source,
                status,
            });
        };
        if let Ok(ref account) = native {
            if !managed.contains_key(&account.key()) {
                append(
                    Account {
                        uid: account.uid.clone(),
                        access: account.access.clone(),
                        domain: account.domain.clone(),
                        enterprise_id: account.enterprise_id.clone(),
                        label: account.label.clone(),
                        native: true,
                        client_version: None,
                        device_token: None,
                    },
                    "native",
                );
            }
        }
        for account in managed.values() {
            append(account.account(), "browser");
        }
        Ok(AccountList {
            accounts,
            native_status: Self::snapshot(&store, &native, now),
        })
    }

    async fn selected_account(&self, id: &str, refresh: bool) -> Result<Account, String> {
        let mut managed = self.read_managed().await?;
        if let Some(account) = managed.get_mut(id) {
            let now = Utc::now().timestamp_millis();
            if refresh && account.expires_at > 0 && account.expires_at <= now + 60_000 {
                if account.refresh_token.is_empty()
                    || (account.refresh_expires_at > 0 && account.refresh_expires_at <= now)
                {
                    return Err(AUTH_ERROR.into());
                }
                let value = self
                    .auth_call(
                        reqwest::Method::POST,
                        "/v2/plugin/auth/token/refresh",
                        Some(&account.account()),
                        Some(&account.refresh_token),
                    )
                    .await
                    .map_err(|_| AUTH_ERROR.to_owned())?;
                account.update_tokens(&value)?;
                // Persist rotated grants before any billing request can spend them again.
                self.write_managed(&managed).await?;
            }
            return managed
                .get(id)
                .map(ManagedAccount::account)
                .ok_or_else(|| ACCOUNT_ERROR.into());
        }
        self.account()
            .await
            .ok()
            .filter(|account| account.key() == id)
            .ok_or_else(|| ACCOUNT_ERROR.into())
    }

    pub(crate) async fn set_auto_for(&self, id: &str, enabled: bool) -> Result<Status, String> {
        let _guard = self.lock.lock().await;
        let account = self.selected_account(id, false).await?;
        let mut store = self.read_store().await?;
        self.bind_legacy_auto(&mut store).await;
        store.account_auto.insert(id.into(), enabled);
        self.write_store(&store).await?;
        Ok(Self::snapshot(&store, &Ok(account), Utc::now().timestamp()))
    }

    async fn bind_legacy_auto(&self, store: &mut Store) {
        if store.auto_enabled {
            if let Ok(account) = self.account().await {
                store.account_auto.entry(account.key()).or_insert(true);
                store.auto_enabled = false;
            }
        }
    }

    async fn claim_selected(
        &self,
        store: &mut Store,
        id: &str,
        manual: bool,
    ) -> Result<Status, String> {
        let account = self.selected_account(id, false).await?;
        let now = Utc::now().timestamp();
        if (!manual && !store.account_auto.get(id).copied().unwrap_or(false))
            || store
                .records
                .get(id)
                .is_some_and(|r| !r.should_run(now, manual))
        {
            return Ok(Self::snapshot(store, &Ok(account), now));
        }
        let record = match self.selected_account(id, true).await {
            Ok(account) => self.claim(&account).await,
            Err(error) if error == AUTH_ERROR => Record {
                day: beijing_day(now),
                checked_at: now,
                outcome: Outcome::SignInRequired,
                credit: None,
                streak: None,
                credit_delayed: false,
            },
            Err(error) => return Err(error),
        };
        store.records.insert(id.into(), record);
        self.write_store(store).await?;
        Ok(Self::snapshot(store, &Ok(account), Utc::now().timestamp()))
    }

    pub(crate) async fn check_in_for(&self, id: &str) -> Result<Status, String> {
        let _guard = self.lock.lock().await;
        let mut store = self.read_store().await?;
        self.claim_selected(&mut store, id, true).await
    }

    pub(crate) async fn run_auto(&self) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        let mut store = self.read_store().await?;
        let legacy = store.auto_enabled;
        self.bind_legacy_auto(&mut store).await;
        if legacy && !store.auto_enabled {
            self.write_store(&store).await?;
        }
        let ids: Vec<_> = store
            .account_auto
            .iter()
            .filter(|(_, enabled)| **enabled)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            // An unavailable native account never substitutes the new active account.
            let _ = self.claim_selected(&mut store, &id, false).await;
        }
        Ok(())
    }

    pub(crate) async fn remove_account(&self, id: &str) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        let mut managed = self.read_managed().await?;
        if managed.remove(id).is_none() {
            return Err(ACCOUNT_ERROR.into());
        }
        let mut store = self.read_store().await?;
        store.account_auto.remove(id);
        self.write_store(&store).await?;
        self.write_managed(&managed).await
    }

    pub(crate) async fn start_login(&self) -> Result<LoginStart, String> {
        let _guard = self.lock.lock().await;
        let value = self
            .auth_call(
                reqwest::Method::POST,
                "/v2/plugin/auth/state?platform=workbuddy",
                None,
                None,
            )
            .await
            .map_err(|_| AUTH_ERROR.to_owned())?;
        let state = value
            .get("state")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(AUTH_ERROR)?;
        let mut url = value
            .get("authUrl")
            .and_then(Value::as_str)
            .and_then(|s| url::Url::parse(s).ok())
            .filter(|u| {
                u.scheme() == "https"
                    && u.host_str().is_some()
                    && u.username().is_empty()
                    && u.password().is_none()
            })
            .ok_or(AUTH_ERROR)?;
        let id = uuid::Uuid::new_v4().to_string();
        url.query_pairs_mut()
            .append_pair("version", "2.0.0")
            .append_pair("loginSessionId", &id);
        *self.pending_login.lock().await = Some(PendingLogin {
            id: id.clone(),
            state: state.into(),
            deadline: Utc::now().timestamp() + 300,
            grant: None,
        });
        Ok(LoginStart {
            login_id: id,
            url: url.into(),
        })
    }

    pub(crate) async fn cancel_login(&self, id: &str) {
        // Cancellation must invalidate network work without waiting for the vendor.
        let mut pending = self.pending_login.lock().await;
        if pending.as_ref().is_some_and(|p| p.id == id) {
            *pending = None;
        }
    }

    pub(crate) async fn poll_login(&self, id: &str) -> Result<LoginStatus, String> {
        let _guard = self.lock.lock().await;
        let mut login = self
            .pending_login
            .lock()
            .await
            .as_ref()
            .filter(|p| p.id == id && p.deadline > Utc::now().timestamp())
            .cloned()
            .ok_or(AUTH_ERROR)?;
        let state: String = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("state", &login.state)
            .finish();
        let waiting = || LoginStatus {
            complete: false,
            account_id: None,
        };
        if login.grant.is_none() {
            let token = match self
                .auth_call(
                    reqwest::Method::GET,
                    &format!("/v2/plugin/auth/token?{state}"),
                    None,
                    None,
                )
                .await
            {
                Err(CallError::Code(11217)) => return Ok(waiting()),
                result => result.map_err(|_| AUTH_ERROR.to_owned())?,
            };
            let mut grant = ManagedAccount {
                uid: String::new(),
                access_token: String::new(),
                refresh_token: String::new(),
                domain: "copilot.tencent.com".into(),
                expires_at: 0,
                refresh_expires_at: 0,
                enterprise_id: String::new(),
                label: String::new(),
            };
            grant.update_tokens(&token)?;
            login.grant = Some(grant);
            let mut pending = self.pending_login.lock().await;
            let active = pending.as_mut().filter(|p| p.id == id).ok_or(AUTH_ERROR)?;
            active.grant = login.grant.clone();
        }
        let grant = login.grant.as_mut().ok_or(AUTH_ERROR)?;
        let profile = match self
            .auth_call(
                reqwest::Method::GET,
                &format!("/v2/plugin/login/account?{state}"),
                Some(&grant.account()),
                None,
            )
            .await
        {
            Err(CallError::Code(12151)) => return Ok(waiting()),
            result => result.map_err(|_| AUTH_ERROR.to_owned())?,
        };
        grant.uid = profile
            .get("uid")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .ok_or(AUTH_ERROR)?
            .into();
        grant.enterprise_id = profile
            .get("enterpriseId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into();
        let account_type = profile
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        let mut enterprise_name = profile
            .get("enterpriseName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if grant.enterprise_id.is_empty() && !account_type.is_empty() && account_type != "personal"
        {
            let list = self
                .auth_call(
                    reqwest::Method::GET,
                    "/v2/plugin/accounts",
                    Some(&grant.account()),
                    None,
                )
                .await
                .map_err(|_| AUTH_ERROR.to_owned())?;
            let candidates: Vec<_> = list
                .get("accounts")
                .and_then(Value::as_array)
                .ok_or(AUTH_ERROR)?
                .iter()
                .filter(|account| {
                    account.get("uid").and_then(Value::as_str) == Some(&grant.uid)
                        && account
                            .get("enterpriseId")
                            .and_then(Value::as_str)
                            .is_some_and(|id| !id.trim().is_empty())
                })
                .collect();
            let same_type: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|a| {
                    a.get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .eq_ignore_ascii_case(&account_type)
                })
                .collect();
            let last_login: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|a| a.get("lastLogin").and_then(Value::as_bool) == Some(true))
                .collect();
            let selected = if same_type.len() == 1 {
                same_type.first().copied()
            } else if last_login.len() == 1 {
                last_login.first().copied()
            } else if candidates.len() == 1 {
                candidates.first().copied()
            } else {
                None
            }
            .ok_or(AUTH_ERROR)?;
            grant.enterprise_id = selected
                .get("enterpriseId")
                .and_then(Value::as_str)
                .ok_or(AUTH_ERROR)?
                .trim()
                .into();
            enterprise_name = selected
                .get("enterpriseName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into();
        }
        grant.label = profile
            .get("nickname")
            .and_then(Value::as_str)
            .unwrap_or("WorkBuddy")
            .into();
        if !grant.enterprise_id.is_empty() {
            grant.label.push_str(" · ");
            grant.label.push_str(if enterprise_name.is_empty() {
                "Enterprise"
            } else {
                &enterprise_name
            });
        }
        let account_id = grant.account().key();
        let mut pending = self.pending_login.lock().await;
        if !pending
            .as_ref()
            .is_some_and(|p| p.id == id && p.deadline > Utc::now().timestamp())
        {
            return Err(AUTH_ERROR.into());
        }
        let mut managed = self.read_managed().await?;
        managed.insert(account_id.clone(), grant.clone());
        self.write_managed(&managed).await?;
        *pending = None;
        Ok(LoginStatus {
            complete: true,
            account_id: Some(account_id),
        })
    }

    async fn auth_call(
        &self,
        method: reqwest::Method,
        path: &str,
        account: Option<&Account>,
        refresh: Option<&str>,
    ) -> Result<Value, CallError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| CallError::Invalid)?;
        let mut request = client
            .request(method.clone(), format!("{}{path}", self.endpoint))
            .header(reqwest::header::USER_AGENT, "WorkBuddy/5.9.3")
            .header(reqwest::header::ACCEPT, "application/json")
            .header("X-No-User-Id", "true")
            .header("X-No-Department-Info", "true");
        if let Some(account) = account {
            request = request.header("X-Domain", &account.domain);
            if refresh.is_none() {
                let mut auth =
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", account.access))
                        .map_err(|_| CallError::Invalid)?;
                auth.set_sensitive(true);
                request = request.header(reqwest::header::AUTHORIZATION, auth);
            }
        }
        if account.is_none() || refresh.is_some() {
            request = request.header("X-No-Authorization", "true");
        }
        if let Some(account) = account.filter(|a| refresh.is_some() && !a.enterprise_id.is_empty())
        {
            request = request
                .header("X-Enterprise-Id", &account.enterprise_id)
                .header("X-Tenant-Id", &account.enterprise_id);
        } else {
            request = request.header("X-No-Enterprise-Id", "true");
        }
        if let Some(refresh) = refresh {
            let mut header =
                reqwest::header::HeaderValue::from_str(refresh).map_err(|_| CallError::Invalid)?;
            header.set_sensitive(true);
            request = request
                .header("X-Refresh-Token", header)
                .header("X-Auth-Refresh-Source", "plugin");
        }
        if method == reqwest::Method::POST {
            request = request.json(&serde_json::json!({}));
        }
        let mut response = request.send().await.map_err(|_| CallError::Invalid)?;
        if response.status().is_redirection() {
            return Err(CallError::Invalid);
        }
        let success = response.status().is_success();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| CallError::Invalid)? {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err(CallError::Invalid);
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| CallError::Invalid)?;
        let code = value
            .get("code")
            .and_then(|c| c.as_i64().or_else(|| c.as_str()?.parse().ok()))
            .ok_or(CallError::Invalid)?;
        if code != 0 {
            return Err(CallError::Code(code));
        }
        if !success {
            return Err(CallError::Invalid);
        }
        value
            .get("data")
            .filter(|v| v.is_object())
            .cloned()
            .ok_or(CallError::Invalid)
    }
}
