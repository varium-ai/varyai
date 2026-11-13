//! Redacted Codex editor observations; opening never renews credentials or probes a vendor.
use crate::{
    services::{
        codex_provider::{self, Settings},
        routing_upstreams::RoutingUpstream,
    },
    store::AppState,
};
use serde::Serialize;
use serde_json::Value;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Editor {
    #[serde(flatten)]
    upstream: RoutingUpstream,
    codex: Option<CodexEditor>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexEditor {
    settings: Settings,
    accounts: Vec<Account>,
    models: Vec<Value>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    id: String,
    login: String,
    plan: Option<String>,
    native: bool,
    ready: bool,
    aliases: Vec<String>,
}

fn merge_native_account(
    mut rows: Vec<Account>,
    matching: Option<&str>,
    settings: &mut Settings,
) -> Vec<Account> {
    let Some(managed) = matching else {
        return rows;
    };
    if !rows.iter().any(|row| row.id == "native") || !rows.iter().any(|row| row.id == managed) {
        return rows;
    }
    // Preserve the first selected credential source; deduplication is not a login switch.
    let representative = settings
        .accounts
        .iter()
        .find(|id| id.as_str() == "native" || id.as_str() == managed)
        .map(String::as_str)
        .unwrap_or(managed)
        .to_owned();
    let alias = if representative == "native" {
        managed
    } else {
        "native"
    };
    rows.retain(|row| row.id != alias);
    if let Some(row) = rows.iter_mut().find(|row| row.id == representative) {
        row.aliases.push(alias.into());
    }
    settings.accounts.retain(|id| id != alias);
    rows
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelTest {
    ok: bool,
    elapsed_ms: u64,
}

fn new_codex_draft() -> RoutingUpstream {
    RoutingUpstream {
        notes: None,
        icon: None,
        transport: None,
        schema_version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        name: "ChatGPT / Codex".into(),
        revision: 0,
        source: crate::services::routing_upstreams::UpstreamSource::Codex,
        has_api_key: false,
        enabled: true,
        preset_id: None,
        region: None,
        codex_settings: Some(Settings::default()),
    }
}

#[tauri::command]
pub(crate) async fn create_codex_provider(state: State<'_, AppState>) -> Result<Editor, String> {
    let upstream = new_codex_draft();
    let provider = crate::services::routing_upstreams::resolve_provider(&upstream)?;
    observe_editor(state.inner(), upstream, provider, Settings::default()).await
}

fn discovery_provider(
    db: &crate::database::Database,
    id: String,
    revision: u32,
) -> Result<crate::provider::Provider, String> {
    if revision == 0 {
        let mut draft = new_codex_draft();
        draft.id = id;
        crate::services::routing_upstreams::resolve_provider(&draft)
    } else {
        db.routing_discovery_provider(&id, revision)
    }
}

#[tauri::command]
pub(crate) async fn test_codex_provider_model(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
    account: String,
    model: String,
) -> Result<ModelTest, String> {
    if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err("routingUpstreams.invalid".into());
    }
    let db = state.db.clone();
    let mut provider = tokio::task::spawn_blocking(move || discovery_provider(&db, id, revision))
        .await
        .map_err(|_| "routingUpstreams.failed")??;
    if !codex_provider::is_codex(&provider) {
        return Err("routingUpstreams.invalid".into());
    }
    codex_provider::bind(&mut provider, &account);
    let (token, workspace) = crate::services::intent_routing::catalog::execution_credentials(
        &provider,
        &state.codex_oauth_manager,
    )
    .await
    .ok_or("routingUpstreams.sourceUnavailable")?;
    let client = if account == "native" {
        crate::proxy::http_client::get_without_redirects()
    } else {
        state.codex_oauth_manager.account_client(&account).await
    }
    .map_err(|_| "routingUpstreams.failed")?;
    let version = crate::codex_client::resolve_version().await;
    let request = client
        .post("https://chatgpt.com/backend-api/codex/responses")
        .bearer_auth(token)
        .header(
            "originator",
            crate::proxy::providers::codex_oauth_auth::CODEX_OAUTH_ORIGINATOR,
        )
        .header("version", &version)
        .header("User-Agent", crate::codex_client::user_agent(&version));
    let request = if workspace.is_empty() {
        request
    } else {
        request.header("chatgpt-account-id", workspace)
    };
    let start = std::time::Instant::now();
    let ok = crate::services::codex_provider::probe_model(request, &model).await;
    Ok(ModelTest {
        ok,
        elapsed_ms: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
    })
}

pub(crate) async fn editor(state: &AppState, id: String, revision: u32) -> Result<Editor, String> {
    let db = state.db.clone();
    let (upstream, provider) = tokio::task::spawn_blocking(move || {
        Ok::<_, String>((
            db.routing_upstream_editor(&id, revision)?,
            db.routing_discovery_provider(&id, revision)?,
        ))
    })
    .await
    .map_err(|_| "routingUpstreams.failed")??;
    if !codex_provider::is_codex(&provider)
        || !matches!(
            upstream.source,
            crate::services::routing_upstreams::UpstreamSource::Codex
        )
    {
        return Ok(Editor {
            upstream,
            codex: None,
        });
    }
    let mut settings = Settings::from_provider(&provider);
    if provider
        .settings_config
        .get(codex_provider::POLICY)
        .is_none()
        && provider.is_codex_oauth()
        && provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("codex_oauth"))
            .is_none()
    {
        settings.accounts = state
            .codex_oauth_manager
            .default_account_id()
            .await
            .into_iter()
            .collect();
    }
    observe_editor(state, upstream, provider, settings).await
}

async fn observe_editor(
    state: &AppState,
    upstream: RoutingUpstream,
    provider: crate::provider::Provider,
    mut settings: Settings,
) -> Result<Editor, String> {
    let native =
        tokio::task::spawn_blocking(crate::services::intent_routing::catalog::native_editor_info)
            .await
            .map_err(|_| "routingUpstreams.failed")?;
    let mut accounts = vec![Account {
        id: "native".into(),
        login: native
            .as_ref()
            .map(|info| info.login.clone())
            .unwrap_or_else(|| "Codex".into()),
        plan: native.as_ref().and_then(|info| info.plan.clone()),
        native: true,
        ready: native.is_some(),
        aliases: vec![],
    }];
    for account in state.codex_oauth_manager.list_accounts().await {
        let plan = state.codex_oauth_manager.editor_plan(&account.id).await;
        accounts.push(Account {
            id: account.id.clone(),
            login: account.login,
            plan,
            native: false,
            ready: !account.reauth_required,
            aliases: vec![],
        });
    }
    let matching =
        if let Some((user, workspace)) = native.as_ref().and_then(|info| info.identity.as_ref()) {
            state
                .codex_oauth_manager
                .account_for_editor_identity(user, workspace)
                .await
        } else {
            None
        };
    let accounts = merge_native_account(accounts, matching.as_deref(), &mut settings);
    // Local cached catalog only. A refresh is a separate explicit action.
    let mut unfiltered = provider.clone();
    unfiltered
        .settings_config
        .as_object_mut()
        .map(|o| o.remove(codex_provider::POLICY));
    let (prepared, _) = crate::services::intent_routing::catalog::prepare_provider(
        state.db.clone(),
        state.codex_oauth_manager.clone(),
        unfiltered,
        false,
    )
    .await?;
    let models = prepared
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(Editor {
        upstream,
        codex: Some(CodexEditor {
            settings,
            accounts,
            models,
        }),
    })
}

#[tauri::command]
pub(crate) async fn import_codex_provider_account(
    state: State<'_, AppState>,
    path: String,
) -> Result<crate::proxy::providers::copilot_auth::GitHubAccount, String> {
    let value = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|_| "routingUpstreams.invalid")?;
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "routingUpstreams.invalid")?;
        if bytes.len() > 1024 * 1024 {
            return Err("routingUpstreams.invalid");
        }
        serde_json::from_slice(&bytes).map_err(|_| "routingUpstreams.invalid")
    })
    .await
    .map_err(|_| "routingUpstreams.failed")??;
    state
        .codex_oauth_manager
        .import_editor_account(value)
        .await
        .map_err(|e| match e {
            crate::proxy::providers::codex_oauth_auth::CodexOAuthError::DuplicateAccount => {
                "codex_oauth_duplicate_account"
            }
            _ => "routingUpstreams.invalid",
        })
        .map_err(str::to_owned)
}

fn save_codex_record(
    db: &crate::database::Database,
    mut upstream: RoutingUpstream,
    settings: Settings,
) -> Result<RoutingUpstream, String> {
    if !matches!(
        upstream.source,
        crate::services::routing_upstreams::UpstreamSource::Codex
    ) || !settings.valid()
    {
        return Err("routingUpstreams.invalid".into());
    }
    if upstream.revision != 0 {
        let provider = db.routing_discovery_provider(&upstream.id, upstream.revision)?;
        if !codex_provider::is_codex(&provider) {
            return Err("routingUpstreams.invalid".into());
        }
    }
    upstream.codex_settings = Some(settings);
    db.save_routing_upstream(upstream)
}

#[tauri::command]
pub(crate) async fn save_codex_provider(
    state: State<'_, AppState>,
    upstream: RoutingUpstream,
    settings: Settings,
) -> Result<RoutingUpstream, String> {
    if !settings.valid() {
        return Err("routingUpstreams.invalid".into());
    }
    let available = state.codex_oauth_manager.list_accounts().await;
    if settings
        .accounts
        .iter()
        .any(|id| id != "native" && !available.iter().any(|a| &a.id == id && !a.reauth_required))
    {
        return Err("routingUpstreams.sourceUnavailable".into());
    }
    let db = state.db.clone();
    let saved = tokio::task::spawn_blocking(move || save_codex_record(&db, upstream, settings))
        .await
        .map_err(|_| "routingUpstreams.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(saved)
}

#[tauri::command]
pub(crate) async fn codex_provider_models(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
    account: String,
) -> Result<Vec<Value>, String> {
    let db = state.db.clone();
    let mut provider = tokio::task::spawn_blocking(move || discovery_provider(&db, id, revision))
        .await
        .map_err(|_| "routingUpstreams.failed")??;
    if !codex_provider::is_codex(&provider) {
        return Err("routingUpstreams.invalid".into());
    }
    codex_provider::bind(&mut provider, &account);
    provider
        .settings_config
        .as_object_mut()
        .map(|o| o.remove(codex_provider::POLICY));
    let (provider, status) = crate::services::intent_routing::catalog::prepare_provider(
        state.db.clone(),
        state.codex_oauth_manager.clone(),
        provider,
        true,
    )
    .await?;
    if !status.is_some_and(|s| s.status == "updated") {
        return Err("routingUpstreams.sourceUnavailable".into());
    }
    Ok(provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub(crate) fn quota_auth_error(
    error: crate::proxy::providers::codex_oauth_auth::CodexOAuthError,
) -> Result<crate::services::subscription::SubscriptionQuota, String> {
    use crate::proxy::providers::codex_oauth_auth::CodexOAuthError;
    use crate::services::subscription::{CredentialStatus, SubscriptionQuota};
    let status = match error {
        CodexOAuthError::RefreshTokenInvalid | CodexOAuthError::ExpiredToken => {
            CredentialStatus::Expired
        }
        CodexOAuthError::AccountNotFound(_) => CredentialStatus::NotFound,
        CodexOAuthError::ParseError(_) => CredentialStatus::ParseError,
        _ => return Err("routingUpstreams.failed".into()),
    };
    let mut quota = SubscriptionQuota::error("codex_oauth", status, String::new());
    quota.error = None;
    quota.credential_message = None;
    Ok(quota)
}

#[tauri::command]
pub(crate) async fn codex_provider_quota(
    state: State<'_, AppState>,
    account: String,
) -> Result<CodexQuotaObservation, String> {
    use crate::services::subscription::{query_codex_quota_with_client, SubscriptionQuota};
    let identity = state.quota_panel.account_identity(&account).await;
    let result = if account == "native" {
        crate::services::subscription::get_subscription_quota("codex").await
    } else {
        let manager = &state.codex_oauth_manager;
        let token = match manager.get_valid_token_for_account(&account).await {
            Ok(token) => token,
            Err(error) => {
                return quota_auth_error(error).map(|quota| CodexQuotaObservation {
                    quota,
                    account_identity: None,
                })
            }
        };
        let workspace = match manager.chatgpt_account_id_for_account(&account).await {
            Ok(workspace) => workspace,
            Err(error) => {
                return quota_auth_error(error).map(|quota| CodexQuotaObservation {
                    quota,
                    account_identity: None,
                })
            }
        };
        let client = manager.account_client(&account).await?;
        let result = query_codex_quota_with_client(
            &client,
            &token,
            Some(&workspace),
            "codex_oauth",
            "Account authorization unavailable",
        )
        .await;
        if result.as_ref().is_ok_and(|quota| {
            matches!(
                quota.credential_status,
                crate::services::subscription::CredentialStatus::Expired
            )
        }) {
            manager.observe_rejected_access(&account, &token).await;
        }
        result
    };
    let mut quota: SubscriptionQuota = result.map_err(|_| "routingUpstreams.failed")?;
    // Vendor text and authentication errors never enter this editor.
    quota.error = None;
    quota.credential_message = None;
    let account_identity = state
        .quota_panel
        .observe(&account, identity.as_deref(), &quota)
        .await;
    Ok(CodexQuotaObservation {
        quota,
        account_identity,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexQuotaObservation {
    #[serde(flatten)]
    quota: crate::services::subscription::SubscriptionQuota,
    account_identity: Option<String>,
}

#[cfg(test)]
mod account_tests {
    use super::*;

    #[test]
    fn opening_and_cancelling_codex_drafts_persist_nothing_and_save_persists_once(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = crate::database::Database::memory()?;
        let cancelled = new_codex_draft();
        let draft = new_codex_draft();
        assert!(cancelled.id != draft.id && draft.revision == 0);
        assert!(db.list_routing_upstreams()?.is_empty());
        let executable = discovery_provider(&db, draft.id.clone(), 0)?;
        assert!(codex_provider::is_codex(&executable));
        assert!(db.list_routing_upstreams()?.is_empty());
        let saved = save_codex_record(&db, draft.clone(), Settings::default())?;
        assert!(saved.revision == 1);
        assert!(db.list_routing_upstreams()?.len() == 1);
        assert!(save_codex_record(&db, draft, Settings::default()).is_err());
        assert!(db.list_routing_upstreams()?.len() == 1);
        Ok(())
    }

    #[test]
    fn quota_auth_errors_only_require_login_when_authorization_is_definitively_gone() {
        use crate::proxy::providers::codex_oauth_auth::CodexOAuthError;
        use crate::services::subscription::CredentialStatus;
        assert!(matches!(
            quota_auth_error(CodexOAuthError::RefreshTokenInvalid)
                .ok()
                .map(|q| q.credential_status),
            Some(CredentialStatus::Expired)
        ));
        for error in [
            CodexOAuthError::NetworkError("synthetic".into()),
            CodexOAuthError::TokenFetchFailed("synthetic".into()),
            CodexOAuthError::IoError("synthetic".into()),
        ] {
            assert!(quota_auth_error(error).is_err());
        }
        assert!(matches!(
            quota_auth_error(CodexOAuthError::AccountNotFound("synthetic".into()))
                .ok()
                .map(|q| q.credential_status),
            Some(CredentialStatus::NotFound)
        ));
        assert!(matches!(
            quota_auth_error(CodexOAuthError::ParseError("synthetic".into()))
                .ok()
                .map(|q| q.credential_status),
            Some(CredentialStatus::ParseError)
        ));
    }

    fn account(id: &str) -> Account {
        Account {
            id: id.into(),
            login: "person@example.test".into(),
            plan: None,
            native: id == "native",
            ready: true,
            aliases: vec![],
        }
    }

    #[test]
    fn duplicate_native_login_is_shown_once_without_changing_the_selected_binding() {
        for selected in [
            vec!["managed"],
            vec!["native"],
            vec!["native", "managed"],
            vec!["managed", "native"],
        ] {
            let mut settings = Settings {
                accounts: selected.iter().map(|id| (*id).into()).collect(),
                ..Default::default()
            };
            let rows = merge_native_account(
                vec![account("native"), account("managed"), account("other")],
                Some("managed"),
                &mut settings,
            );
            assert_eq!(rows.len(), 2);
            let representative = rows
                .iter()
                .find(|row| row.id != "other")
                .expect("representative");
            assert_eq!(representative.id, selected[0]);
            assert_eq!(representative.aliases.len(), 1);
            assert_eq!(settings.accounts, vec![selected[0]]);
            assert_eq!(representative.native, selected[0] == "native");
            let serialized = serde_json::to_value(&rows).expect("safe rows");
            assert!(serialized
                .as_array()
                .is_some_and(|rows| rows.iter().all(|row| {
                    row.get("identity").is_none()
                        && row.get("idToken").is_none()
                        && row.get("accessToken").is_none()
                })));
        }
    }

    #[test]
    fn same_email_is_not_evidence_and_unselected_duplicate_prefers_managed() {
        let mut settings = Settings {
            accounts: vec!["other".into()],
            ..Default::default()
        };
        let rows = merge_native_account(
            vec![account("native"), account("managed"), account("other")],
            None,
            &mut settings,
        );
        assert_eq!(rows.len(), 3);
        let rows = merge_native_account(rows, Some("managed"), &mut settings);
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .any(|row| row.id == "managed" && row.aliases == ["native"]));
        assert_eq!(settings.accounts, vec!["other"]);
    }
}
