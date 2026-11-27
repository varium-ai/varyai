//! Local agent policies reuse existing routing and client authentication.
use super::*;
use crate::{
    app_config::AppType,
    proxy::{
        intent::{Menu, PublishedMenu, Session},
        server::ProxyState,
    },
    services::intent_routing::menu::direct_id,
};
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

#[derive(Clone, Debug)]
pub(crate) struct GroupMemberPolicy {
    group_id: String,
    published_group: bool,
    catalog_generation: Option<u64>,
    models: std::collections::HashSet<String>,
}

fn direct_alias(provider_id: &str, model_id: &str) -> String {
    format!("vary-provider/{}", direct_id(provider_id, model_id))
}

impl GroupMemberPolicy {
    pub(crate) fn allows(&self, provider_id: &str, model_id: &str) -> bool {
        self.models.contains(&direct_alias(provider_id, model_id))
    }

    pub(crate) fn permits_session(&self, session: &Session) -> bool {
        self.published_group == session.group.enabled
            && self.group_id == session.group.id
            && session
                .menu_targets()
                .any(|target| self.allows(&target.provider_id, &target.model))
    }

    pub(crate) fn current(&self, generation: u64) -> bool {
        self.catalog_generation
            .is_none_or(|used| used == generation)
    }

    fn allow_members(&mut self, session: &Session) {
        self.models.extend(
            session
                .menu_targets()
                .map(|target| direct_alias(&target.provider_id, &target.model)),
        );
    }

    pub(crate) fn expanded_models(
        policy: &KeyPolicy,
        catalog: Option<&Menu>,
    ) -> std::collections::HashSet<String> {
        let mut models: std::collections::HashSet<String> = policy.models.iter().cloned().collect();
        if let Some(catalog) = catalog {
            for group in policy.models.iter().filter_map(|id| {
                catalog
                    .groups
                    .get(id)
                    .filter(|group| group.group.enabled)
                    .map(Arc::as_ref)
            }) {
                models.extend(
                    group
                        .menu_targets()
                        .map(|target| direct_alias(&target.provider_id, &target.model)),
                );
            }
        }
        models
    }

    pub(crate) fn for_session(
        policy: &KeyPolicy,
        session: &Session,
        requested: &str,
        exact: bool,
        temporary_route: bool,
        catalog: Option<&PublishedMenu>,
        routing_generation: u64,
    ) -> Result<Self, GatewayError> {
        let mut allowed = Self {
            group_id: session.group.id.clone(),
            published_group: session.group.enabled,
            catalog_generation: catalog.and_then(|menu| menu.generation),
            models: Self::expanded_models(policy, catalog.map(|menu| menu.menu.as_ref())),
        };
        if !allowed.current(routing_generation) {
            return Err(GatewayError::Storage);
        }
        // Temporary execution can come from an unsaved draft. Only the
        // published catalog or explicit model permissions may grant its targets.
        if exact && session.group.enabled && !temporary_route {
            let published_group = catalog
                .and_then(|menu| menu.groups.get(requested))
                .filter(|group| group.group.enabled)
                .map(Arc::as_ref);
            allowed.allow_members(published_group.unwrap_or(session));
        }
        if !allowed.permits_session(session) {
            return Err(GatewayError::Forbidden);
        }
        Ok(allowed)
    }

    pub(crate) fn permits_published_group(
        policy: &KeyPolicy,
        alias: &str,
        session: &Session,
        catalog: &Menu,
    ) -> bool {
        if !session.group.enabled {
            return false;
        }
        if policy.all_models || policy.models.iter().any(|model| model == alias) {
            return true;
        }
        let models = Self::expanded_models(policy, Some(catalog));
        session
            .menu_targets()
            .any(|target| models.contains(&direct_alias(&target.provider_id, &target.model)))
    }
}

pub(crate) async fn agent_policy_catalog(
    state: &ProxyState,
) -> Result<Option<PublishedMenu>, GatewayError> {
    if !state.intent_runtime.ensure_global_current(&state.db).await {
        return Err(GatewayError::Storage);
    }
    state
        .intent_runtime
        .global_publication()
        .map_err(|_| GatewayError::Storage)
}

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
            | "workbuddy-ai"
            | "opencode"
            | "qoder"
            | "hermes"
            | "doubaowork"
            | "trae"
            | "trae-intl"
            | "kimi"
            | "qwen"
            | "zcode"
            | "deepseek-harness"
            | "t3code"
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUsage {
    pub key_id: String,
    pub window: i64,
    pub agent: String,
    pub model: String,
    pub upstream_model: Option<String>,
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
        self.authorize_gateway_agent_model(app, model, None, None, None, false, now)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn authorize_gateway_agent_model(
        &self,
        app: &str,
        model: &str,
        alias: Option<&str>,
        route: Option<&Session>,
        catalog: Option<&PublishedMenu>,
        temporary_route: bool,
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
        if model.is_empty() || model.len() > 200 {
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
        drop(conn);
        let requested = alias.unwrap_or(model);
        let exact = policy.models.iter().any(|m| m == model || m == requested);
        let managed = route.filter(|s| {
            temporary_route || s.group.enabled || requested.starts_with("vary-provider/")
        });
        let group_members = if policy.all_models {
            None
        } else if let Some(session) = managed {
            Some(GroupMemberPolicy::for_session(
                &policy,
                session,
                requested,
                exact,
                temporary_route,
                catalog,
                self.routing_generation(),
            )?)
        } else if exact {
            None
        } else {
            return Err(GatewayError::Forbidden);
        };
        if policy.token_limit.is_some_and(|limit| used >= limit) {
            return Err(GatewayError::Exhausted(reset));
        }
        Ok(Some(Grant {
            id,
            window,
            agent: app.into(),
            model: requested.into(),
            upstream_model: GatewayUpstreamModel::default(),
            group_members,
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
        // A completed in-flight response must not recreate a permanently deleted key's history.
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM gateway_keys WHERE id=?1)",
            [&grant.id],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(());
        }
        let count = tokens.unwrap_or(0).min(i64::MAX as u64);
        tx.execute("INSERT INTO gateway_usage(key_id,window,tokens) VALUES(?1,?2,?3) ON CONFLICT(key_id,window) DO UPDATE SET tokens=MIN(9223372036854775807,tokens+excluded.tokens)", rusqlite::params![grant.id,grant.window,count])?;
        tx.execute("INSERT INTO gateway_usage_models(key_id,window,agent,model,upstream_model,tokens,requests,unmetered,last_used) VALUES(?1,?2,?3,?4,?5,?6,1,?7,?8) ON CONFLICT(key_id,window,agent,model,upstream_model) DO UPDATE SET tokens=MIN(9223372036854775807,tokens+excluded.tokens),requests=requests+1,unmetered=unmetered+excluded.unmetered,last_used=excluded.last_used", rusqlite::params![grant.id,grant.window,grant.agent,grant.model,grant.upstream_model.get().unwrap_or(""),count,u8::from(tokens.is_none()),Utc::now().timestamp()])?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_gateway_usage(&self, id: Option<&str>) -> Result<Vec<GatewayUsage>, AppError> {
        let conn = crate::database::lock_conn!(self.extension_connection());
        let mut stmt=conn.prepare("SELECT key_id,window,agent,model,SUM(tokens),SUM(requests),SUM(unmetered),MAX(last_used),NULLIF(upstream_model,'') FROM (SELECT key_id,window,agent,model,tokens,requests,unmetered,last_used,'' AS upstream_model FROM gateway_usage_details UNION ALL SELECT key_id,window,agent,model,tokens,requests,unmetered,last_used,upstream_model FROM gateway_usage_models) WHERE (?1 IS NULL OR key_id=?1) GROUP BY key_id,window,agent,model,upstream_model ORDER BY MAX(last_used) DESC LIMIT 200")?;
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
                    upstream_model: r.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

// Resolve the agent from its route, never from a caller-controlled header.
fn route_agent(path: &str) -> Option<&str> {
    if matches!(
        path,
        "/intent/v1/responses"
            | "/intent/v1/responses/compact"
            | "/backend-api/codex/responses"
            | "/backend-api/codex/responses/compact"
    ) {
        return Some("codex");
    }
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
    if let Some(path) = path.strip_prefix("/upstream/") {
        let (app, suffix) = path.split_once('/')?;
        return (matches!(app, "hermes" | "trae" | "trae-intl")
            && suffix.starts_with("providers/"))
        .then_some(app);
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

#[derive(Clone)]
struct AuthorizedCodexModel(String);

pub(crate) fn codex_model_changed(extensions: &http::Extensions, body: &serde_json::Value) -> bool {
    extensions
        .get::<AuthorizedCodexModel>()
        .is_some_and(|expected| body["model"].as_str() != Some(expected.0.as_str()))
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
        return super::ingress::policy_error_for(
            &app,
            "<unsupported>",
            GatewayError::UnsupportedOperation,
        );
    }
    let (mut parts, body) = request.into_parts();
    let bytes = match to_bytes(body, 200 * 1024 * 1024).await {
        Ok(v) => v,
        Err(_) => {
            return super::ingress::policy_error_for(
                &app,
                "<body_too_large>",
                GatewayError::Forbidden,
            )
        }
    };
    let mut headers = parts.headers.clone();
    let decoded =
        match crate::proxy::handlers::decode_codex_request_body(&mut headers, bytes.clone()) {
            Ok(v) => v,
            Err(_) => {
                return super::ingress::policy_error_for(
                    &app,
                    "<invalid_encoding>",
                    GatewayError::Forbidden,
                )
            }
        };
    let model = path_model(&path)
        .or_else(|| {
            serde_json::from_slice::<serde_json::Value>(&decoded)
                .ok()
                .and_then(|v| v["model"].as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let is_intent_path = matches!(
        path.as_str(),
        "/intent/v1/responses" | "/intent/v1/responses/compact"
    );
    let is_native_codex = matches!(
        path.as_str(),
        "/backend-api/codex/responses" | "/backend-api/codex/responses/compact"
    );
    if is_intent_path || is_native_codex {
        let generation = state.db.routing_generation();
        let menu_stale = match state.intent_runtime.menu_publication() {
            Ok(menu) => menu.is_some_and(|m| !m.current(generation)),
            Err(_) => return super::ingress::policy_error_for(&app, &model, GatewayError::Storage),
        };
        if (menu_stale || state.intent_runtime.native.is_stale(generation))
            && !state.intent_runtime.ensure_menu_current(&state.db).await
        {
            return crate::proxy::intent::catalog_stale();
        }
    }
    let token = is_intent_path
        .then(|| {
            parts
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        })
        .flatten();
    let intent_menu = if is_intent_path {
        match state.intent_runtime.menu_publication() {
            Ok(menu) => menu.filter(|menu| token == Some(menu.token.as_str())),
            Err(_) => return super::ingress::policy_error_for(&app, &model, GatewayError::Storage),
        }
    } else {
        None
    };
    let temp_session = if is_intent_path && intent_menu.is_none() {
        token
            .and_then(|token| state.intent_runtime.temporary_session(token))
            .filter(|session| {
                model == session.group.alias() || model == format!("vary-auto/{}", session.group.id)
            })
    } else {
        None
    };
    let managed_codex = intent_menu.is_some() || is_native_codex;
    let (alias, expected, group, temporary_route) = if managed_codex {
        let (native_group, resolved) = match state.intent_runtime.native.policy_model(&model) {
            Ok(policy) => policy,
            Err(_) => return super::ingress::policy_error_for(&app, &model, GatewayError::Storage),
        };
        let (policy_alias, group) = if let Some(menu) = intent_menu.as_ref() {
            let side_alias = crate::proxy::intent::is_side_request(&parts.headers)
                .then(|| {
                    state.intent_runtime.side_session_alias(
                        "local-codex",
                        crate::proxy::intent::named_conversation(&parts.headers),
                    )
                })
                .flatten()
                .filter(|alias| menu.groups.contains_key(alias));
            let policy_alias = side_alias.unwrap_or_else(|| resolved.clone());
            let group = menu.groups.get(&policy_alias).cloned();
            (policy_alias, group)
        } else {
            (resolved.clone(), native_group)
        };
        (
            group.as_ref().map(|_| policy_alias),
            Some(AuthorizedCodexModel(resolved)),
            group,
            false,
        )
    } else if let Some(session) = temp_session {
        (Some(session.group.alias()), None, Some(session), true)
    } else {
        (None, None, None, false)
    };
    let catalog = if group.is_some() {
        match agent_policy_catalog(&state).await {
            Ok(catalog) => catalog,
            Err(err) => {
                return super::ingress::policy_error_for(
                    &app,
                    alias.as_deref().unwrap_or(&model),
                    err,
                );
            }
        }
    } else {
        None
    };
    let db = state.db.clone();
    let lookup = app.clone();
    let logged_model = alias.clone().unwrap_or_else(|| model.clone());
    let policy_model = match (&alias, &expected) {
        // When the request follows its conversation's route, only that route's
        // or its actual members' permissions can authorize the operation.
        (Some(alias), Some(expected)) if alias != &expected.0 => alias.clone(),
        _ => model,
    };
    let grant = match tokio::task::spawn_blocking(move || {
        db.authorize_gateway_agent_model(
            &lookup,
            &policy_model,
            alias.as_deref(),
            group.as_deref(),
            catalog.as_ref(),
            temporary_route,
            Utc::now().timestamp(),
        )
    })
    .await
    {
        Ok(Ok(grant)) => grant,
        Ok(Err(err)) => return super::ingress::policy_error_for(&app, &logged_model, err),
        Err(_) => {
            return super::ingress::policy_error_for(&app, &logged_model, GatewayError::Storage);
        }
    };
    if let Some(grant) = &grant {
        parts.extensions.insert(grant.upstream_model.clone());
        if let Some(group_members) = &grant.group_members {
            parts.extensions.insert(group_members.clone());
        }
        if let Some(expected) = expected {
            parts.extensions.insert(expected);
        }
    }
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
    pub model: String,
    pub name: String,
}
#[derive(Serialize)]
pub struct GatewayCatalog {
    pub agents: Vec<GatewayAgent>,
    pub models: Vec<GatewayModel>,
}

pub(crate) fn catalog(menu: Option<&Menu>) -> GatewayCatalog {
    let agents = AppType::all()
        .map(|app| GatewayAgent {
            id: app.as_str().into(),
            local_routing: supports_binding(app.as_str()),
        })
        .collect();
    let mut entries = menu
        .into_iter()
        .flat_map(|menu| menu.models.values().map(move |model| (menu, model)))
        .collect::<Vec<_>>();
    entries.sort_by_key(|(_, model)| (model.order, &model.label, &model.alias));
    let models = entries
        .into_iter()
        .filter_map(|(menu, model)| {
            let session = menu.groups.get(&model.alias)?;
            let name = if model.alias.starts_with("vary-provider/") {
                &model.label
            } else {
                &session.group.name
            };
            Some(GatewayModel {
                model: model.alias.clone(),
                name: name.clone(),
            })
        })
        .collect();
    GatewayCatalog { agents, models }
}

#[cfg(test)]
mod global_catalog_tests {
    use super::*;
    use crate::{
        provider::Provider,
        proxy::providers::codex_oauth_auth::CodexOAuthManager,
        services::intent_routing::{menu, RoutingGroup},
    };
    use serde_json::json;

    #[tokio::test]
    async fn policy_picker_lists_shared_provider_routes_and_groups_once() {
        let home = tempfile::tempdir().unwrap();
        let manager = Arc::new(CodexOAuthManager::new(home.path().to_owned()));
        let provider = |id: &str, name: &str| {
            Provider::with_id(
                id.into(),
                name.into(),
                json!({"auth":{"OPENAI_API_KEY":"synthetic-source"},"apiFormat":"openai_responses","config":"model='exact'\nmodel_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.invalid/v1'\nwire_api='responses'"}),
                None,
            )
        };
        let group: RoutingGroup = serde_json::from_value(json!({
            "id":"global-picker","name":"Synthetic group","revision":0,"client":"global","enabled":true,
            "members":[{"id":"a","providerId":"fixture-a","modelId":"exact"}],"defaultMemberId":"a","fallbackMemberIds":[],"intentRecognitionEnabled":false,"classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
        })).unwrap();
        let mut stand_in = group.clone();
        stand_in.id = "historical".into();
        let menu = menu::prepare_with_stand_ins(
            manager,
            vec![
                provider("fixture-a", "Supplier A"),
                provider("fixture-b", "Supplier B"),
            ],
            vec![group],
            None,
            vec![stand_in],
        )
        .await
        .unwrap();
        assert!(menu.groups.contains_key("vary-auto/historical"));
        let catalog = catalog(Some(&menu));
        assert!(!serde_json::to_string(&catalog)
            .unwrap()
            .contains("synthetic-source"));
        let models = catalog.models;
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].model, "vary-auto/global-picker");
        assert_eq!(models[0].name, "Synthetic group");
        for (id, name) in [("fixture-a", "Supplier A"), ("fixture-b", "Supplier B")] {
            let alias = format!("vary-provider/{}", menu::direct_id(id, "exact"));
            assert!(models
                .iter()
                .any(|row| row.model == alias && row.name == format!("exact · {name}")));
        }
    }
}
