//! WorkBuddy's regional browser grants and vendor catalog.
use super::*;
use http::{header, HeaderMap, HeaderValue, Method, StatusCode};

const VERSION: &str = "5.9.3";
const AUTH: &str = "/v2/plugin/auth";
const MAX_JSON: usize = 4 * 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Identity {
    pub(crate) uid: String,
    domain: String,
    enterprise: String,
    refresh_expires: i64,
}

fn platform(provider: SubscriptionProvider) -> &'static str {
    if provider == SubscriptionProvider::Workbuddy {
        "workbuddy"
    } else {
        "workbuddy-ai"
    }
}

fn origin(provider: SubscriptionProvider) -> &'static str {
    provider.endpoint().trim_end_matches("/v2")
}

pub(crate) fn account_origin(account: &Account) -> String {
    let domain = account.workbuddy.domain.to_ascii_lowercase();
    if !matches!(
        domain.as_str(),
        "tencent.sso.copilot.tencent.com" | "tencent.sso.codebuddy.cn"
    ) && [
        ".sso.copilot.tencent.com",
        ".copilot.qq.com",
        ".sso.codebuddy.cn",
    ]
    .iter()
    .any(|suffix| {
        domain.strip_suffix(suffix).is_some_and(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    }) {
        format!("https://{domain}")
    } else {
        origin(account.provider).into()
    }
}

fn field(value: &Value, key: &str, max: usize) -> Option<String> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
        .map(str::to_owned)
}

fn auth_headers(account: &Account) -> Result<HeaderMap, &'static str> {
    let site = origin(account.provider).trim_start_matches("https://");
    let identity = &account.workbuddy;
    let mut headers = HeaderMap::new();
    for (name, value) in [
        (
            "x-domain",
            if identity.domain.is_empty() {
                site
            } else {
                &identity.domain
            },
        ),
        ("x-user-id", &identity.uid),
        ("x-enterprise-id", &identity.enterprise),
        ("x-tenant-id", &identity.enterprise),
        ("x-product", "SaaS"),
        ("x-ide-type", "WorkBuddy"),
    ] {
        if !value.is_empty() {
            headers.insert(name, HeaderValue::from_str(value).map_err(|_| FAILED)?);
        }
    }
    Ok(headers)
}

pub(crate) fn headers(account: &Account) -> Result<HeaderMap, &'static str> {
    let mut headers = auth_headers(account)?;
    let name = if account.provider == SubscriptionProvider::Workbuddy {
        "WorkBuddy"
    } else {
        "workbuddy-ai"
    };
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_str(&format!("WorkBuddy/{VERSION} {name}/{VERSION} CLI/2.165.0"))
            .map_err(|_| FAILED)?,
    );
    for (name, value) in [
        ("x-requested-with", "XMLHttpRequest"),
        ("x-agent-intent", "craft"),
        ("x-agent-purpose", "conversation"),
        ("x-ide-name", "WorkBuddy"),
        ("x-ide-version", VERSION),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    Ok(headers)
}

struct CallError {
    status: StatusCode,
    code: i64,
}

impl CallError {
    fn unavailable() -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            code: -1,
        }
    }
    fn authentication(self) -> &'static str {
        if self.status == StatusCode::UNAUTHORIZED {
            EXPIRED
        } else {
            FAILED
        }
    }
}

async fn call(request: reqwest::RequestBuilder) -> Result<Value, CallError> {
    let mut response = request
        .header(header::ACCEPT, "application/json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| CallError::unavailable())?;
    let status = response.status();
    if !status.is_success() {
        return Err(CallError { status, code: -1 });
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| CallError::unavailable())?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_JSON {
            return Err(CallError::unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| CallError::unavailable())?;
    let code = value["code"]
        .as_i64()
        .or_else(|| value["code"].as_str()?.parse().ok())
        .ok_or_else(CallError::unavailable)?;
    if code != 0 {
        return Err(CallError { status, code });
    }
    Ok(value["data"].clone())
}

fn endpoint(base: &str, path: &str) -> String {
    format!("{base}{path}")
}

fn browser_url(provider: SubscriptionProvider, value: &Value) -> Result<url::Url, String> {
    let mut url = url::Url::parse(value["authUrl"].as_str().ok_or(FAILED)?).map_err(|_| FAILED)?;
    let host = if provider == SubscriptionProvider::Workbuddy {
        "www.workbuddy.cn"
    } else {
        "www.workbuddy.ai"
    };
    if url.scheme() != "https"
        || url.host_str() != Some(host)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(FAILED.into());
    }
    url.query_pairs_mut()
        .append_pair("version", VERSION)
        .append_pair("loginSessionId", &uuid::Uuid::new_v4().simple().to_string());
    Ok(url)
}

pub(super) async fn start(
    provider: SubscriptionProvider,
    client: &reqwest::Client,
) -> Result<(String, String), String> {
    let value = call(
        client
            .post(endpoint(origin(provider), &format!("{AUTH}/state")))
            .query(&[("platform", platform(provider))])
            .header(header::USER_AGENT, format!("WorkBuddy/{VERSION}"))
            .header("x-no-authorization", "true")
            .header("x-no-user-id", "true")
            .header("x-no-enterprise-id", "true")
            .header("x-no-department-info", "true")
            .json(&json!({})),
    )
    .await
    .map_err(|_| FAILED)?;
    let state = field(&value, "state", 1024).ok_or(FAILED)?;
    Ok((browser_url(provider, &value)?.into(), state))
}

async fn poll(request: reqwest::RequestBuilder, pending: i64) -> Result<Value, String> {
    loop {
        match call(request.try_clone().ok_or(FAILED)?).await {
            Ok(data) if data.as_object().is_some_and(|object| !object.is_empty()) => {
                return Ok(data)
            }
            Ok(_) => {}
            Err(error)
                if error.code == pending
                    || matches!(
                        error.status,
                        StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
                    ) => {}
            Err(_) => return Err(FAILED.into()),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn expiry(value: &Value, absolute: &str, relative: &str) -> Option<i64> {
    value[absolute]
        .as_i64()
        .filter(|v| *v > 0)
        .map(|v| v / 1000)
        .or_else(|| {
            value[relative]
                .as_i64()
                .filter(|v| *v > 0)?
                .checked_add(chrono::Utc::now().timestamp())
        })
}

pub(super) fn apply_token(account: &mut Account, value: &Value) -> Result<(), String> {
    account.access = field(value, "accessToken", 32768).ok_or(FAILED)?;
    account.expires = expiry(value, "expiresAt", "expiresIn").ok_or(FAILED)?;
    if let Some(refresh) = field(value, "refreshToken", 32768) {
        account.refresh = refresh;
    }
    if let Some(expires) = expiry(value, "refreshExpiresAt", "refreshExpiresIn") {
        account.workbuddy.refresh_expires = expires;
    }
    if let Some(domain) = field(value, "domain", 253) {
        if !domain
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        {
            return Err(FAILED.into());
        }
        if !account.workbuddy.domain.is_empty()
            && !account.workbuddy.domain.eq_ignore_ascii_case(&domain)
        {
            return Err(FAILED.into());
        }
        account.workbuddy.domain = domain;
    }
    account.reauth_required = false;
    Ok(())
}

pub(super) async fn finish(
    provider: SubscriptionProvider,
    state: &str,
    client: &reqwest::Client,
    target: Option<&Account>,
    flow: &str,
) -> Result<AccountSummary, String> {
    let base = origin(provider);
    let request = |path| {
        client
            .get(endpoint(base, path))
            .query(&[("state", state)])
            .header(header::USER_AGENT, format!("WorkBuddy/{VERSION}"))
    };
    let token = poll(request("/v2/plugin/auth/token"), 11217).await?;
    let mut account = Account {
        id: String::new(),
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
        client_id: String::new(),
        subject: String::new(),
        id_token: String::new(),
        workbuddy: Identity::default(),
    };
    apply_token(&mut account, &token)?;
    let profile = poll(
        request("/v2/plugin/login/account")
            .headers(auth_headers(&account)?)
            .bearer_auth(account.token())
            .header("x-no-user-id", "true")
            .header("x-no-enterprise-id", "true"),
        12151,
    )
    .await?;
    let uid = field(&profile, "uid", 256).ok_or(FAILED)?;
    let mut enterprise = field(&profile, "enterpriseId", 256);
    let mut team = field(&profile, "enterpriseName", 128);
    let kind = profile["type"].as_str().unwrap_or_default();
    if enterprise.is_none() && !kind.is_empty() && !kind.eq_ignore_ascii_case("personal") {
        let list = call(
            client
                .get(endpoint(&account_origin(&account), "/v2/plugin/accounts"))
                .headers(headers(&account)?)
                .bearer_auth(account.token()),
        )
        .await
        .map_err(|_| FAILED)?;
        let rows: Vec<_> = list["accounts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["uid"] == uid && field(row, "enterpriseId", 256).is_some())
            .collect();
        let unique = |rows: Vec<&Value>| {
            if rows.len() == 1 {
                rows.first().copied().cloned()
            } else {
                None
            }
        };
        let selected = unique(
            rows.iter()
                .copied()
                .filter(|row| {
                    row["type"]
                        .as_str()
                        .is_some_and(|value| value.eq_ignore_ascii_case(kind))
                })
                .collect(),
        )
        .or_else(|| {
            unique(
                rows.iter()
                    .copied()
                    .filter(|row| row["lastLogin"] == true)
                    .collect(),
            )
        })
        .or_else(|| unique(rows))
        .ok_or(FAILED)?;
        enterprise = field(&selected, "enterpriseId", 256);
        team = field(&selected, "enterpriseName", 128);
    }
    account.login = field(&profile, "nickname", 128)
        .or_else(|| field(&profile, "phoneNumber", 32))
        .unwrap_or_else(|| platform(provider).into());
    account.workbuddy.uid = uid;
    account.workbuddy.enterprise = enterprise.unwrap_or_default();
    auth_headers(&account)?;
    if !account.workbuddy.enterprise.is_empty() {
        let label = team.unwrap_or_else(|| {
            account
                .workbuddy
                .enterprise
                .chars()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        });
        account.login.push_str(&format!(" · {label}"));
    }
    account.subject = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                platform(provider),
                &account.workbuddy.uid,
                &account.workbuddy.enterprise,
            ))
            .map_err(|_| FAILED)?
        )
    );
    account.id = target.map_or_else(|| account.subject.clone(), |old| old.id.clone());
    keep_login(account, target, flow).await
}

pub(super) async fn refresh(account: &Account) -> Result<Value, &'static str> {
    if account.workbuddy.refresh_expires > 0
        && account.workbuddy.refresh_expires <= chrono::Utc::now().timestamp()
    {
        return Err(EXPIRED);
    }
    let client = crate::proxy::http_client::get_without_redirects().map_err(|_| FAILED)?;
    call(
        client
            .post(endpoint(
                &account_origin(account),
                "/v2/plugin/auth/token/refresh",
            ))
            .headers(auth_headers(account)?)
            .header(header::USER_AGENT, format!("WorkBuddy/{VERSION}"))
            .header("x-refresh-token", &account.refresh)
            .header("x-auth-refresh-source", "plugin")
            .json(&json!({})),
    )
    .await
    .map_err(CallError::authentication)
}

async fn account_request(
    account: &Account,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let client = crate::proxy::http_client::get_without_redirects().map_err(|_| FAILED)?;
    let request = |account: &Account| -> Result<_, String> {
        let request = client
            .request(method.clone(), endpoint(&account_origin(account), path))
            .headers(headers(account)?)
            .bearer_auth(account.token());
        Ok(match &body {
            Some(body) => request.json(body),
            None => request,
        })
    };
    match call(request(account)?).await {
        Err(error) if error.status == StatusCode::UNAUTHORIZED => {
            let renewed = renew_rejected_workbuddy(account).await?;
            call(request(&renewed)?).await.map_err(|_| FAILED.into())
        }
        result => result.map_err(|_| FAILED.into()),
    }
}

pub(super) async fn models(account: &Account) -> Result<Vec<Value>, String> {
    let value = account_request(account, Method::GET, "/v3/config", None).await?;
    catalog(account.provider, &value)
}

fn catalog(provider: SubscriptionProvider, value: &Value) -> Result<Vec<Value>, String> {
    use std::collections::{HashMap, HashSet};
    let rows = value["models"].as_array().ok_or(FAILED)?;
    let by_id: HashMap<_, _> = rows
        .iter()
        .filter_map(|model| Some((model["id"].as_str()?, model)))
        .collect();
    let cli = value["agents"]
        .as_array()
        .and_then(|rows| rows.iter().find(|a| a["name"] == "cli"))
        .and_then(|row| row["models"].as_array())
        .ok_or(FAILED)?;
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for id in cli.iter().filter_map(Value::as_str).chain(
        rows.iter()
            .filter_map(|row| row["id"].as_str())
            .filter(|id| id.starts_with("custom:")),
    ) {
        if id.is_empty()
            || id.len() > 200
            || id.trim() != id
            || id.chars().any(char::is_control)
            || id.starts_with("vary-auto/")
            || id.starts_with("vary-provider/")
            || !seen.insert(id)
        {
            continue;
        }
        let row = by_id.get(id).copied().unwrap_or(&Value::Null);
        let reasoning = &row["reasoning"];
        let mut levels: Vec<_> = reasoning["supportedEfforts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|s| {
                matches!(
                    *s,
                    "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                )
            })
            .collect();
        if levels.is_empty() && row["supportsReasoning"] == true {
            levels.extend(["low", "high", "max"]);
        }
        if provider.workbuddy_messages(id) && !levels.is_empty() && !levels.contains(&"max") {
            levels.push("max");
        }
        if !levels.is_empty()
            && row["onlyReasoning"] != true
            && reasoning["canDisableThinking"] != false
            && !levels.contains(&"none")
        {
            levels.insert(0, "none");
        }
        let images = row["supportsImages"] == true
            && !(provider == SubscriptionProvider::WorkbuddyAi && id.starts_with("glm-"));
        let mut model = json!({"model":id,"id":id,"displayName":field(row,"name",200).unwrap_or_else(|| id.into()),
            "reasoningLevels":levels,"inputModalities":if images { vec!["text","image"] } else { vec!["text"] }, "outputModalities":["text"]});
        for (input, output) in [
            ("maxInputTokens", "contextWindow"),
            ("maxOutputTokens", "outputTokens"),
        ] {
            if let Some(n) = row[input].as_u64().filter(|n| *n > 0) {
                model[output] = json!(n);
            }
        }
        if let Some(level) = reasoning["defaultEffort"]
            .as_str()
            .filter(|level| levels.contains(level))
        {
            model["defaultReasoningLevel"] = json!(level);
        }
        models.push(model);
    }
    Ok(models)
}

fn billing_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
}

fn quota_dates(
    provider: SubscriptionProvider,
    enterprise: bool,
    value: &Value,
    now: i64,
) -> (Option<String>, Option<String>) {
    let date = |value: &Value| {
        let utc = if let Some(n) = value.as_i64() {
            if n >= 1_000_000_000_000 {
                chrono::DateTime::from_timestamp_millis(n)
            } else {
                chrono::DateTime::from_timestamp(n, 0)
            }
        } else {
            let text = value.as_str()?.trim();
            chrono::DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|date| date.to_utc())
                .or_else(|| {
                    if provider != SubscriptionProvider::Workbuddy {
                        return None;
                    }
                    let date =
                        chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").ok()?;
                    let offset = chrono::FixedOffset::east_opt(8 * 3600)?;
                    date.and_local_timezone(offset)
                        .single()
                        .map(|date| date.to_utc())
                })
        }?;
        (utc.timestamp() > now).then_some(utc)
    };
    let format = |date: chrono::DateTime<chrono::Utc>| date.to_rfc3339();
    if enterprise {
        return (date(&value["cycleResetTime"]).map(format), None);
    }
    let packages = ["CreditGroups", "Packages"]
        .into_iter()
        .filter_map(|key| value[key].as_array())
        .flatten();
    let reset = packages
        .clone()
        .filter_map(|row| date(&row["CycleEndTime"]))
        .min()
        .map(format);
    let expiry = packages
        .filter(|row| {
            billing_number(&row["CycleCapacityRemainPrecise"])
                .or_else(|| billing_number(&row["CycleRemainCapacity"]))
                .or_else(|| {
                    Some(
                        billing_number(&row["CycleTotalCapacity"])?
                            - billing_number(&row["CycleUsedCapacity"])?,
                    )
                })
                .is_some_and(|remaining| remaining > 0.0)
        })
        .filter_map(|row| date(&row["DeductionEndTime"]))
        .min()
        .map(format);
    (reset, expiry)
}

pub(super) async fn quota(account: &Account) -> Result<QuotaObservation, String> {
    let mut quota = parse_google_quota(account.provider, &Value::Null);
    let enterprise = !account.workbuddy.enterprise.is_empty();
    let path = if enterprise {
        "/billing/meter/get-enterprise-user-usage"
    } else {
        "/billing/meter/get-user-resource-summary"
    };
    let value = account_request(account, Method::POST, path, Some(json!({}))).await?;
    let (reset_at, credits_expires_at) = quota_dates(
        account.provider,
        enterprise,
        &value,
        chrono::Utc::now().timestamp(),
    );
    let amounts = if enterprise {
        billing_number(&value["credit"]).zip(billing_number(&value["limitNum"]))
    } else {
        ["CreditGroups", "Packages"].into_iter().find_map(|key| {
            value[key]
                .as_array()?
                .iter()
                .filter_map(|row| {
                    Some((
                        billing_number(&row["CycleUsedCapacity"])?,
                        billing_number(&row["CycleTotalCapacity"])?,
                    ))
                })
                .reduce(|(used, total), (u, t)| (used + u, total + t))
        })
    };
    if let Some((used, total)) =
        amounts.filter(|(used, total)| used.is_finite() && total.is_finite() && *total > 0.0)
    {
        quota.tiers.push(crate::services::subscription::QuotaTier {
            name: "credits".into(),
            utilization: (used / total * 100.0).clamp(0.0, 100.0),
            resets_at: reset_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }
    Ok(QuotaObservation {
        quota,
        credits_expires_at,
    })
}

#[cfg(test)]
mod tests;
