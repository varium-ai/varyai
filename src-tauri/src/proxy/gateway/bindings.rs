//! Local agent policies reuse existing routing and client authentication.
use super::*;
use crate::{app_config::AppType, proxy::server::ProxyState};
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::collections::BTreeSet;

pub(super) fn path_model(path: &str) -> Option<String> {
    let encoded = path.split("/models/").nth(1)?.rsplit_once(':')?.0;
    url::form_urlencoded::parse(format!("id={}", encoded.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, id)| id.into_owned())
}

pub fn supports_binding(app: &str) -> bool {
    matches!(
        app,
        "claude"
            | "claude-desktop"
            | "codex"
            | "gemini"
            | "grokbuild"
            | "pi"
            | "workbuddy"
            | "qoder"
            | "doubaowork"
            | "trae"
            | "kimi"
            | "qwen"
            | "zcode"
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUsage {
    pub key_id: String,
    pub window: i64,
    pub agent: String,
    pub model: String,
    pub tokens: u64,
    pub requests: u64,
    pub unmetered: u64,
    pub last_used: i64,
}

impl Database {
    pub fn bind_gateway_agent(&self, app: &str, id: Option<&str>) -> Result<(), AppError> {
        if !supports_binding(app) {
            return Err(AppError::Config("Agent has no managed model route".into()));
        }
        let conn = crate::database::lock_conn!(self.extension_connection());
        if let Some(id) = id {
            let active: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM gateway_keys WHERE id=?1 AND revoked=0)",
                [id],
                |r| r.get(0),
            )?;
            if !active {
                return Err(AppError::Config("Gateway key unavailable".into()));
            }
            conn.execute("INSERT INTO gateway_agent_bindings(app,key_id) VALUES(?1,?2) ON CONFLICT(app) DO UPDATE SET key_id=excluded.key_id", rusqlite::params![app,id])?;
        } else {
            conn.execute("DELETE FROM gateway_agent_bindings WHERE app=?1", [app])?;
        }
        Ok(())
    }

    pub fn gateway_agent_key(&self, app: &str) -> Result<Option<String>, AppError> {
        let conn = crate::database::lock_conn!(self.extension_connection());
        Ok(conn
            .query_row(
                "SELECT key_id FROM gateway_agent_bindings WHERE app=?1",
                [app],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn authorize_gateway_agent(
        &self,
        app: &str,
        model: &str,
        now: i64,
    ) -> Result<Option<Grant>, GatewayError> {
        let conn = self
            .extension_connection()
            .lock()
            .map_err(|_| GatewayError::Storage)?;
        let id: Option<String> = conn
            .query_row(
                "SELECT key_id FROM gateway_agent_bindings WHERE app=?1",
                [app],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| GatewayError::Storage)?;
        let Some(id) = id else {
            return Ok(None);
        };
        let row: Option<(String, bool)> = conn
            .query_row(
                "SELECT policy,revoked FROM gateway_keys WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|_| GatewayError::Storage)?;
        let (raw, revoked) = row.ok_or(GatewayError::Unauthorized)?;
        if revoked {
            return Err(GatewayError::Unauthorized);
        }
        let policy: KeyPolicy = serde_json::from_str(&raw).map_err(|_| GatewayError::Storage)?;
        if model.is_empty()
            || model.len() > 200
            || (!policy.all_models && !policy.models.iter().any(|m| m == model))
        {
            return Err(GatewayError::Forbidden);
        }
        let (window, reset) = policy.period.window(now);
        let used: u64 = conn
            .query_row(
                "SELECT tokens FROM gateway_usage WHERE key_id=?1 AND window=?2",
                rusqlite::params![id, window],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| GatewayError::Storage)?
            .unwrap_or(0);
        if policy.token_limit.is_some_and(|limit| used >= limit) {
            return Err(GatewayError::Exhausted(reset));
        }
        Ok(Some(Grant {
            id,
            window,
            agent: app.into(),
            model: model.into(),
        }))
    }

    pub fn record_gateway_usage(&self, grant: &Grant, tokens: u64) -> Result<(), AppError> {
        self.record_gateway_observation(grant, Some(tokens))
    }

    pub fn record_gateway_observation(
        &self,
        grant: &Grant,
        tokens: Option<u64>,
    ) -> Result<(), AppError> {
        let mut conn = crate::database::lock_conn!(self.extension_connection());
        let tx = conn.transaction()?;
        let count = tokens.unwrap_or(0).min(i64::MAX as u64);
        tx.execute("INSERT INTO gateway_usage(key_id,window,tokens) VALUES(?1,?2,?3) ON CONFLICT(key_id,window) DO UPDATE SET tokens=MIN(9223372036854775807,tokens+excluded.tokens)", rusqlite::params![grant.id,grant.window,count])?;
        tx.execute("INSERT INTO gateway_usage_details(key_id,window,agent,model,tokens,requests,unmetered,last_used) VALUES(?1,?2,?3,?4,?5,1,?6,?7) ON CONFLICT(key_id,window,agent,model) DO UPDATE SET tokens=MIN(9223372036854775807,tokens+excluded.tokens),requests=requests+1,unmetered=unmetered+excluded.unmetered,last_used=excluded.last_used", rusqlite::params![grant.id,grant.window,grant.agent,grant.model,count,u8::from(tokens.is_none()),Utc::now().timestamp()])?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_gateway_usage(&self, id: Option<&str>) -> Result<Vec<GatewayUsage>, AppError> {
        let conn = crate::database::lock_conn!(self.extension_connection());
        let mut stmt=conn.prepare("SELECT key_id,window,agent,model,tokens,requests,unmetered,last_used FROM gateway_usage_details WHERE (?1 IS NULL OR key_id=?1) ORDER BY last_used DESC LIMIT 200")?;
        let rows = stmt
            .query_map([id], |r| {
                Ok(GatewayUsage {
                    key_id: r.get(0)?,
                    window: r.get(1)?,
                    agent: r.get(2)?,
                    model: r.get(3)?,
                    tokens: r.get(4)?,
                    requests: r.get(5)?,
                    unmetered: r.get(6)?,
                    last_used: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

// Resolve the agent from its route, never from a caller-controlled header.
fn route_agent(path: &str) -> Option<&str> {
    if path.starts_with("/images/")
        || path.starts_with("/v1/images/")
        || path.starts_with("/v1/v1/images/")
        || matches!(
            path,
            "/alpha/search" | "/v1/alpha/search" | "/v1/v1/alpha/search"
        )
    {
        return Some("codex");
    }
    if let Some(rest) = path.strip_prefix("/agents/") {
        let (app, suffix) = rest.split_once('/')?;
        return (supports_binding(app) && suffix.starts_with("providers/")).then_some(app);
    }
    for app in [
        "claude-desktop",
        "claude",
        "codex",
        "gemini",
        "grokbuild",
        "pi",
        "workbuddy",
        "qoder",
    ] {
        if path
            .strip_prefix('/')
            .and_then(|p| p.strip_prefix(app))
            .is_some_and(|p| p.starts_with('/'))
        {
            return Some(app);
        }
    }
    if path.starts_with("/v1beta/") {
        return Some("gemini");
    }
    if path == "/v1/messages" {
        return Some("claude");
    }
    if matches!(
        path,
        "/responses"
            | "/v1/responses"
            | "/v1/v1/responses"
            | "/responses/compact"
            | "/v1/responses/compact"
            | "/v1/v1/responses/compact"
            | "/chat/completions"
            | "/v1/chat/completions"
            | "/v1/v1/chat/completions"
    ) {
        return Some("codex");
    }
    None
}

pub async fn enforce_agent(
    State(state): State<ProxyState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    let Some(app) = route_agent(&path).map(str::to_owned) else {
        return next.run(request).await;
    };
    let inference = path.ends_with("/messages")
        || path.ends_with("/responses")
        || path.ends_with("/responses/compact")
        || path.ends_with("/chat/completions")
        || path.ends_with(":generateContent")
        || path.ends_with(":streamGenerateContent");
    let unsupported = path.contains("/images/") || path.ends_with("/alpha/search");
    if request.method() != http::Method::POST || (!inference && !unsupported) {
        return next.run(request).await;
    }
    let db = state.db.clone();
    let lookup = app.clone();
    match tokio::task::spawn_blocking(move || db.gateway_agent_key(&lookup)).await {
        Ok(Ok(None)) => return next.run(request).await,
        Ok(Ok(Some(_))) => {}
        _ => return super::ingress::error(GatewayError::Storage),
    }
    if unsupported {
        return super::ingress::error(GatewayError::UnsupportedOperation);
    }
    let (parts, body) = request.into_parts();
    let bytes = match to_bytes(body, 200 * 1024 * 1024).await {
        Ok(v) => v,
        Err(_) => return super::ingress::error(GatewayError::Forbidden),
    };
    let mut headers = parts.headers.clone();
    let decoded =
        match crate::proxy::handlers::decode_codex_request_body(&mut headers, bytes.clone()) {
            Ok(v) => v,
            Err(_) => return super::ingress::error(GatewayError::Forbidden),
        };
    let model = path_model(&path)
        .or_else(|| {
            serde_json::from_slice::<serde_json::Value>(&decoded)
                .ok()
                .and_then(|v| v["model"].as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let db = state.db.clone();
    let lookup = app.clone();
    let grant = match tokio::task::spawn_blocking(move || {
        db.authorize_gateway_agent(&lookup, &model, Utc::now().timestamp())
    })
    .await
    {
        Ok(Ok(grant)) => grant,
        Ok(Err(err)) => return super::ingress::error(err),
        Err(_) => return super::ingress::error(GatewayError::Storage),
    };
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    match grant {
        Some(grant) => {
            super::ingress::meter(response, state.db, grant, !path.ends_with("/messages"))
        }
        None => response,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAgent {
    pub id: String,
    pub local_routing: bool,
}
#[derive(Serialize)]
pub struct GatewayModel {
    pub agent: String,
    pub provider: String,
    pub model: String,
}
#[derive(Serialize)]
pub struct GatewayCatalog {
    pub agents: Vec<GatewayAgent>,
    pub models: Vec<GatewayModel>,
}

// Read only recognized model fields, never arbitrary configuration strings.
fn model_ids(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    if let Some(array) = value.as_array() {
        for row in array {
            if let Some(s) = row.as_str() {
                out.insert(s.into());
            } else {
                model_ids(row, out);
            }
        }
        return;
    }
    let Some(obj) = value.as_object() else {
        return;
    };
    for field in [
        "model",
        "id",
        "modelId",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_SMALL_FAST_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "GEMINI_MODEL",
    ] {
        if let Some(s) = obj.get(field).and_then(serde_json::Value::as_str) {
            if !s.trim().is_empty() && s.len() <= 200 {
                out.insert(s.into());
            }
        }
    }
    for field in [
        "env",
        "models",
        "modelPicker",
        "options",
        "modelCatalog",
        "settings",
    ] {
        if let Some(v) = obj.get(field) {
            model_ids(v, out);
        }
    }
    if let Some(s) = obj.get("config").and_then(serde_json::Value::as_str) {
        if let Ok(v) = s.parse::<toml::Value>() {
            if let Some(m) = v.get("model").and_then(toml::Value::as_str) {
                out.insert(m.into());
            }
        }
    }
}

pub fn catalog(db: &Database) -> Result<GatewayCatalog, AppError> {
    let mut agents = Vec::new();
    let mut models = Vec::new();
    for app in AppType::all() {
        let id = app.as_str();
        agents.push(GatewayAgent {
            id: id.into(),
            local_routing: supports_binding(id),
        });
        for provider in db.get_all_providers(id)?.values() {
            let mut ids = BTreeSet::new();
            model_ids(&provider.settings_config, &mut ids);
            if let Some(meta) = &provider.meta {
                ids.extend(meta.claude_desktop_model_routes.keys().cloned());
            }
            models.extend(ids.into_iter().map(|model| GatewayModel {
                agent: id.into(),
                provider: provider.name.clone(),
                model,
            }));
        }
    }
    Ok(GatewayCatalog { agents, models })
}
