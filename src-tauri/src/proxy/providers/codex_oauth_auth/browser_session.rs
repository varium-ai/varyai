//! Magpie-compatible browser credentials and bounded, safe failure diagnostics.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Session {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    pub access: CachedAccessToken,
    #[serde(skip)]
    pub client: Option<reqwest::Client>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    pub email: String,
    pub workspace: String,
    pub plan: String,
}

impl Identity {
    pub fn same_account(&self, other: &Self) -> bool {
        if !self.workspace.is_empty() && !other.workspace.is_empty() {
            self.email.eq_ignore_ascii_case(&other.email) && self.workspace == other.workspace
        } else {
            self.label().eq_ignore_ascii_case(&other.label())
        }
    }

    pub fn label(&self) -> String {
        match self.plan.as_str() {
            "team" => format!("{} · Team", self.email),
            "business" => format!("{} · Business", self.email),
            "enterprise" => format!("{} · Enterprise", self.email),
            "edu" => format!("{} · Edu", self.email),
            _ => self.email.clone(),
        }
    }
}

pub(super) fn identity(token: &str) -> Option<Identity> {
    let mut parts = token.split('.');
    parts.next()?;
    let payload = parts.next()?;
    parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let email = value.get("email")?.as_str()?.trim();
    if email.is_empty() {
        return None;
    }
    let auth = value.get("https://api.openai.com/auth");
    Some(Identity {
        email: email.into(),
        workspace: auth
            .and_then(|auth| auth.get("chatgpt_account_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .into(),
        plan: auth
            .and_then(|auth| auth.get("chatgpt_plan_type"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .into(),
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Diagnostic {
    pub stage: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
}

pub(super) struct Failure {
    pub key: &'static str,
    pub diagnostic: Diagnostic,
}

impl Failure {
    pub fn new(stage: &'static str, key: &'static str) -> Self {
        Self {
            key,
            diagnostic: Diagnostic {
                stage,
                http_status: None,
                code: None,
            },
        }
    }

    pub fn network(error: &reqwest::Error) -> Self {
        Self::new(
            if error.is_timeout() {
                "token_timeout"
            } else if error.is_connect() {
                "token_connect"
            } else {
                "token_transport"
            },
            if error.is_timeout() {
                "codexBrowser.exchangeTimeout"
            } else {
                "codexBrowser.networkFailed"
            },
        )
    }

    pub fn commit(error: CodexOAuthError) -> Self {
        let (stage, key) = match error {
            CodexOAuthError::DuplicateAccount => ("duplicate_identity", "codexBrowser.duplicate"),
            CodexOAuthError::ExpiredToken => ("login_expired", "codexBrowser.expired"),
            CodexOAuthError::IoError(_) | CodexOAuthError::ParseError(_) => {
                ("account_persistence", "codexBrowser.saveFailed")
            }
            _ => ("account_identity", "codexBrowser.identityMismatch"),
        };
        Self::new(stage, key)
    }
}

fn known_code(value: &serde_json::Value) -> Option<&'static str> {
    let error = value.get("error")?;
    let code = error.as_str().or_else(|| error.get("code")?.as_str());
    match code? {
        "invalid_grant" => Some("invalid_grant"),
        "invalid_request" => Some("invalid_request"),
        "invalid_client" => Some("invalid_client"),
        "invalid_authorization_code" => Some("invalid_authorization_code"),
        "authorization_code_expired" => Some("authorization_code_expired"),
        "access_denied" => Some("access_denied"),
        "unsupported_country_region_territory" => Some("unsupported_country_region_territory"),
        "rate_limit_exceeded" => Some("rate_limit_exceeded"),
        _ => None,
    }
}

pub(super) async fn read_tokens(
    mut response: reqwest::Response,
) -> Result<(OAuthTokenResponse, Identity), Failure> {
    let status = response.status();
    let mut bytes = Vec::new();
    let read = async {
        while let Some(chunk) = response.chunk().await.map_err(|e| Failure::network(&e))? {
            if bytes.len().saturating_add(chunk.len()) > 1024 * 1024 {
                return Err(Failure::new(
                    "token_response_limit",
                    "codexBrowser.invalidTokenResponse",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(())
    }
    .await;
    if status != reqwest::StatusCode::OK {
        return Err(Failure {
            key: "codexBrowser.tokenRejected",
            diagnostic: Diagnostic {
                stage: "token_http",
                http_status: Some(status.as_u16()),
                code: if read.is_ok() {
                    serde_json::from_slice(&bytes)
                        .ok()
                        .as_ref()
                        .and_then(known_code)
                } else {
                    None
                },
            },
        });
    }
    read?;
    let invalid = |stage| Failure::new(stage, "codexBrowser.invalidTokenResponse");
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| invalid("token_json"))?;
    let credential = |key| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| invalid("token_credentials"))
    };
    let id_token = credential("id_token")?;
    let who = identity(&id_token).ok_or_else(|| invalid("token_identity"))?;
    let expires_in = value
        .get("expires_in")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()));
    Ok((
        OAuthTokenResponse {
            access_token: credential("access_token")?,
            refresh_token: Some(credential("refresh_token")?),
            id_token: Some(id_token),
            expires_in: expires_in.filter(|v| *v > 0 && *v <= 365 * 24 * 3600),
        },
        who,
    ))
}
