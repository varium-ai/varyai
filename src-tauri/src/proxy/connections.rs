//! Authenticated local consumers execute the same immutable global model catalog.
use super::{intent, server::ProxyState};
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::RwLock};

#[derive(Clone)]
pub(crate) struct Connection {
    token: String,
    model_map: BTreeMap<String, String>,
    visible_models: Option<Vec<String>>,
}
#[derive(Clone, Copy)]
pub(crate) struct Agent(pub &'static str);
#[derive(Default)]
pub(crate) struct Connections(RwLock<BTreeMap<String, Connection>>);
impl Connections {
    pub fn active(&self, app: &str) -> bool {
        self.0
            .read()
            .is_ok_and(|connections| connections.contains_key(app))
    }
    pub fn install(
        &self,
        receipt: &crate::services::agent_connections::Receipt,
    ) -> Result<(), String> {
        let mut model_map = receipt.model_map.clone();
        if receipt.app == "claude-desktop" {
            // Publish valid native names, but retain the old aliases for existing conversations.
            for alias in receipt.model_map.values() {
                model_map.insert(
                    crate::claude_desktop_config::shared_reasoning_model_name(alias),
                    alias.clone(),
                );
                model_map.insert(
                    crate::claude_desktop_config::shared_model_name(alias),
                    alias.clone(),
                );
                model_map.insert(
                    crate::claude_desktop_config::legacy_shared_model_name(alias),
                    alias.clone(),
                );
            }
        }
        self.0
            .write()
            .map_err(|_| "agentConnections.failed")?
            .insert(
                receipt.app.clone(),
                Connection {
                    token: receipt.token.clone(),
                    model_map,
                    visible_models: receipt.visible_models.clone(),
                },
            );
        Ok(())
    }
    pub fn revoke(&self, app: &str) {
        if let Ok(mut connections) = self.0.write() {
            connections.remove(app);
        }
    }
    fn get(&self, app: &str, supplied: &str) -> Option<Connection> {
        let connections = self.0.read().ok()?;
        let connection = connections.get(app)?;
        // Fixed-length credentials; compare every byte before deciding.
        let difference = supplied
            .bytes()
            .zip(connection.token.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b));
        (supplied.len() == connection.token.len() && difference == 0).then(|| connection.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_connections::Receipt;

    #[test]
    fn desktop_alias_upgrade_preserves_existing_conversation_names() -> Result<(), String> {
        let alias = "synthetic-provider/first";
        let current = crate::claude_desktop_config::shared_model_name(alias);
        let reasoning = crate::claude_desktop_config::shared_reasoning_model_name(alias);
        use sha2::{Digest, Sha256};
        let legacy = format!("claude-vary-{:x}", Sha256::digest(alias.as_bytes()));
        assert_ne!(legacy, current);
        let connections = Connections::default();
        for wire in [&current, &legacy, &reasoning] {
            connections.install(&Receipt {
                app: "claude-desktop".into(),
                token: concat!("synthetic", "-local-token").into(),
                model: alias.into(),
                model_map: BTreeMap::from([(wire.clone(), alias.into())]),
                visible_models: Some(vec![]),
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            })?;
            let connection = connections
                .get("claude-desktop", concat!("synthetic", "-local-token"))
                .ok_or("missing fixture connection")?;
            assert_eq!(
                connection.model_map.get(&reasoning).map(String::as_str),
                Some(alias)
            );
            assert_eq!(
                connection.model_map.get(&current).map(String::as_str),
                Some(alias)
            );
            assert_eq!(
                connection.model_map.get(&legacy).map(String::as_str),
                Some(alias)
            );
        }
        assert!(connections
            .get("claude-desktop", "wrong-local-token")
            .is_none());
        Ok(())
    }
}

fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(json!({"error":{"code":code,"message":code}}))).into_response()
}
pub(crate) async fn handle(State(state): State<ProxyState>, request: Request) -> Response {
    if request
        .extensions()
        .get::<std::net::SocketAddr>()
        .is_some_and(|peer| !peer.ip().is_loopback())
        || request.headers().contains_key("origin")
        || request.headers().contains_key("sec-fetch-site")
    {
        return error(StatusCode::FORBIDDEN, "routing_local_clients_only");
    }
    let Some((app, endpoint)) = request
        .uri()
        .path()
        .strip_prefix("/connections/")
        .and_then(|path| path.split_once('/'))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(app) = crate::services::agent_connections::APPS
        .iter()
        .copied()
        .chain(std::iter::once("trae"))
        .find(|id| *id == app && *id != "codex")
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let supplied = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .or_else(|| {
            request
                .headers()
                .get("x-api-key")
                .and_then(|h| h.to_str().ok())
        })
        .or_else(|| {
            request
                .headers()
                .get("x-goog-api-key")
                .and_then(|h| h.to_str().ok())
        })
        .unwrap_or("");
    let Some(connection) = state.intent_runtime.connections.get(app, supplied) else {
        return error(StatusCode::UNAUTHORIZED, "agent_connection_unavailable");
    };
    let endpoint = format!("/{}", endpoint.trim_start_matches('/'));
    let gemini = endpoint
        .strip_prefix("/v1beta/models/")
        .and_then(|path| path.rsplit_once(':'))
        .filter(|(model, operation)| {
            !model.is_empty()
                && matches!(
                    *operation,
                    "generateContent" | "streamGenerateContent" | "countTokens"
                )
        });
    if !super::protocol_bridge::supported_query(&endpoint, request.uri().query()) {
        return error(StatusCode::BAD_REQUEST, "routing_query_unsupported");
    }
    if request.method() == http::Method::GET
        && matches!(endpoint.as_str(), "/v1/models" | "/v1beta/models")
    {
        if !state
            .intent_runtime
            .global_current(state.db.routing_generation())
        {
            return intent::catalog_stale();
        }
        let menu = match state.intent_runtime.global_snapshot() {
            Ok(Some(menu)) => menu,
            _ => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "routing_catalog_unavailable",
                )
            }
        };
        let mut catalog = crate::services::agent_connections::models(&menu)
            .into_iter()
            .filter(|model| {
                // Visibility is a picker preference, not an authorization boundary for existing chats.
                app != "claude-desktop"
                    || connection
                        .visible_models
                        .as_ref()
                        .is_none_or(|visible| visible.contains(&model.id))
            })
            .collect::<Vec<_>>();
        if app == "claude-desktop" {
            if let Some(visible) = &connection.visible_models {
                catalog.sort_by_key(|model| {
                    visible
                        .iter()
                        .position(|id| id == &model.id)
                        .unwrap_or(usize::MAX)
                });
            }
        }
        if endpoint == "/v1beta/models" {
            let models = catalog.into_iter().map(|model| {
                let wire = connection.model_map.iter().find(|(_, alias)| **alias == model.id).map(|(wire, _)| wire.as_str()).unwrap_or(&model.id);
                let mut row = json!({"name":format!("models/{wire}"),"displayName":model.name,"supportedGenerationMethods":["generateContent","streamGenerateContent","countTokens"]});
                if let Some(window) = model.context_window { row["inputTokenLimit"] = json!(window); }
                row
            }).collect::<Vec<_>>();
            return Json(json!({"models":models})).into_response();
        }
        let models = catalog
            .into_iter()
            .filter_map(|model| {
                let mut entry = menu.groups.get(&model.id)?.model_entry(&model.id);
                if app == "claude-desktop" {
                    let wire = crate::claude_desktop_config::shared_model_wire(&model);
                    if connection.model_map.get(&wire) != Some(&model.id) {
                        return None;
                    }
                    entry["id"] = json!(wire);
                    if let Some(effort) = crate::claude_desktop_config::desktop_max_effort(&model) {
                        entry["max_effort_level"] = json!(effort);
                    }
                    if let Some(window) = model.context_window {
                        entry["max_input_tokens"] = json!(window);
                        entry["supports_1m"] = json!(window >= 1_000_000);
                    }
                }
                entry["display_name"] = json!(model.name);
                Some(entry)
            })
            .collect::<Vec<_>>();
        return Json(json!({"object":"list","data":models})).into_response();
    }
    if request.method() != http::Method::POST
        || (gemini.is_none()
            && !matches!(
                endpoint.as_str(),
                "/v1/messages"
                    | "/v1/messages/count_tokens"
                    | "/v1/chat/completions"
                    | "/v1/responses"
            ))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let (mut parts, body) = request.into_parts();
    let bytes = match to_bytes(body, 8 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return error(StatusCode::PAYLOAD_TOO_LARGE, "routing_body_limit"),
    };
    let mut body: Value = match super::pi::json::parse(&bytes) {
        Ok(body) if body.is_object() => body,
        _ => return error(StatusCode::BAD_REQUEST, "routing_invalid_json"),
    };
    let Some(model) = gemini
        .map(|(model, _)| model)
        .or_else(|| body["model"].as_str())
    else {
        return error(StatusCode::BAD_REQUEST, "routing_model_required");
    };
    let alias = connection
        .model_map
        .get(model)
        .or_else(|| {
            (app == "claude-desktop")
                .then(|| crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model))
                .and_then(|plain| connection.model_map.get(plain))
        })
        .map(String::as_str)
        .unwrap_or(model)
        .to_owned();
    if gemini.is_some_and(|(_, op)| op == "countTokens") {
        if !state
            .intent_runtime
            .global_current(state.db.routing_generation())
        {
            return intent::catalog_stale();
        }
        if !state
            .intent_runtime
            .global_snapshot()
            .ok()
            .flatten()
            .is_some_and(|menu| menu.groups.contains_key(&alias))
        {
            return error(StatusCode::NOT_FOUND, "routing_model_unavailable");
        }
    }
    let mut rewritten = gemini.is_none() && alias != model;
    if rewritten {
        body["model"] = json!(alias);
    }
    if app == "trae" && endpoint == "/v1/responses" {
        rewritten |= super::native_protocol::normalize_trae_responses(&mut body);
    }
    let db = state.db.clone();
    let policy_model = alias.clone();
    let grant = match tokio::task::spawn_blocking(move || {
        db.authorize_gateway_agent(app, &policy_model, chrono::Utc::now().timestamp())
    })
    .await
    {
        Ok(Ok(grant)) => grant,
        Ok(Err(error)) => return super::gateway::policy_error(error),
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "gateway_policy_unavailable",
            )
        }
    };
    if gemini.is_some_and(|(_, op)| op == "countTokens") {
        let payload = body.get("generateContentRequest").unwrap_or(&body);
        let projection = match super::group_forward::canonical_request(
            super::protocol_bridge::Protocol::Gemini,
            payload,
            &endpoint,
        ) {
            Ok(value) => value,
            Err(_) => return error(StatusCode::BAD_REQUEST, "routing_invalid_json"),
        };
        let Some(tokens) = intent::length::estimate(&projection, payload) else {
            return error(
                StatusCode::BAD_REQUEST,
                "routing_token_estimate_unavailable",
            );
        };
        return Json(json!({"totalTokens":tokens})).into_response();
    }
    if let Some(grant) = &grant {
        parts.extensions.insert(grant.upstream_model.clone());
    }
    for name in [
        "authorization",
        "x-api-key",
        "x-goog-api-key",
        "cookie",
        "proxy-authorization",
        "chatgpt-account-id",
    ] {
        parts.headers.remove(name);
    }
    parts.headers.remove(http::header::CONTENT_LENGTH);
    parts.extensions.insert(Agent(app));
    let target_endpoint = parts
        .uri
        .query()
        .filter(|query| !query.is_empty())
        .map_or_else(|| endpoint.clone(), |query| format!("{endpoint}?{query}"));
    parts.uri = match target_endpoint.parse::<Uri>() {
        Ok(uri) => uri,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let bytes = if rewritten {
        match serde_json::to_vec(&body) {
            Ok(bytes) => bytes.into(),
            Err(_) => return error(StatusCode::BAD_REQUEST, "routing_invalid_json"),
        }
    } else {
        bytes
    };
    let response = intent::handle_global(
        state.clone(),
        Request::from_parts(parts, Body::from(bytes)),
        format!("agent:{app}"),
        &alias,
    )
    .await;
    match grant {
        Some(grant) if endpoint != "/v1/messages/count_tokens" => {
            super::gateway::meter_response(response, state.db, grant, endpoint != "/v1/messages")
        }
        _ => response,
    }
}
