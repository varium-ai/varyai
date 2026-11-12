//! Visible Magpie providers and their backend-only saved authorization.
use super::*;
use crate::{
    proxy::providers::{
        codex_oauth_auth::{imported_identity, CodexOAuthManager},
        copilot_auth::{provider_import::ImportedAccount as CopilotAccount, CopilotAuthManager},
    },
    services::{
        codex_provider::Settings, native_subscription::provider_import as native,
        routing_upstreams::SubscriptionProvider,
    },
};
use std::sync::Arc;

#[derive(Clone, Serialize)]
pub(super) enum SavedSubscription {
    Codex(Value),
    Native(Box<native::ImportedAccount>),
    Copilot(CopilotAccount),
}

fn service(agent: &str) -> Option<(SubscriptionProvider, &'static str)> {
    Some(match agent {
        "claude" => (SubscriptionProvider::Claude, "Claude"),
        "chatgpt-api" => (SubscriptionProvider::ChatgptApi, "ChatGPT API"),
        "gemini" => (SubscriptionProvider::GeminiCli, "Gemini CLI"),
        "antigravity" => (SubscriptionProvider::Antigravity, "Antigravity"),
        "copilot" => (SubscriptionProvider::GithubCopilot, "Copilot"),
        _ => return None,
    })
}

fn hidden(value: &Value) -> bool {
    value == true || value.as_str().is_some_and(|s| !s.is_empty())
}

pub(super) fn read(path: &Path) -> Result<Vec<Candidate>, String> {
    let value = if path.exists() {
        json_file(path)?
    } else {
        json!({"providers":[]})
    };
    let providers = value["providers"].as_array().ok_or(INVALID)?;
    let mut candidates = if path.exists() {
        readers::provider_backup(path)?
    } else {
        vec![]
    };
    let mut policies = BTreeMap::new();
    candidates.retain(|c| {
        let id = c.reference.split_once('/').map_or("", |(_, id)| id);
        let p = providers.iter().find(|p| p["id"] == id);
        if p.is_some_and(|p| hidden(&p["hidden"])) { return false; }
        let endpoint = matches!(&c.record.source, UpstreamSource::Api { base_url, .. } if !base_url.is_empty());
        if !endpoint { policies.insert(id.to_owned(), c.record.clone()); }
        endpoint
    });
    let logins = path.with_file_name("logins.json");
    if !logins.exists() {
        return Ok(candidates);
    }
    let value = json_file(&logins)?;
    let rows = value.as_array().ok_or(INVALID)?;
    if rows.len() > MAX_ITEMS {
        return Err(INVALID.into());
    }
    for (index, row) in rows.iter().enumerate() {
        let agent = text(row, &["agent"]);
        if agent != "codex" && service(&agent).is_none() {
            continue;
        }
        if hidden(&row["hidden"])
            || providers
                .iter()
                .any(|p| p["id"] == agent && hidden(&p["hidden"]))
        {
            continue;
        }
        // Explicit API providers already represent this identity in Magpie's list.
        if candidates.iter().any(|c| {
            c.reference
                .split_once('/')
                .is_some_and(|(_, id)| id == agent)
        }) {
            continue;
        }
        let label = if agent == "codex" {
            "Codex"
        } else {
            service(&agent).ok_or(INVALID)?.1
        };
        let policy = policies.get(&agent);
        let name = policy
            .map(|p| p.name.as_str())
            .filter(|name| !name.eq_ignore_ascii_case(&agent))
            .unwrap_or(label);
        let count = rows
            .iter()
            .filter(|r| r["agent"] == agent && !hidden(&r["hidden"]))
            .count();
        let user = text(row, &["user"]);
        let name = if count > 1 && !user.is_empty() {
            format!("{name} · {user}")
        } else {
            name.into()
        };
        let mut c = candidate(
            format!("login/{index}/{}", text(row, &["id"])),
            label,
            name,
            String::new(),
            String::new(),
            "openai_responses",
            vec![],
        );
        let source_id = text(row, &["id"]);
        c.record.id = if source_id.is_empty() {
            policy
                .map(|provider| provider.id.clone())
                .unwrap_or_default()
        } else {
            source_id
        };
        let (mode, models, catalog, groups_only) = match policy.map(|p| &p.source) {
            Some(UpstreamSource::Api {
                mode,
                models,
                catalog,
                groups_only,
                ..
            }) => (mode.clone(), models.clone(), catalog.clone(), *groups_only),
            _ => (Default::default(), vec![], vec![], false),
        };
        c.record.enabled = policy.is_none_or(|p| p.enabled) && row["paused"] != true;
        c.record.icon = policy.and_then(|p| p.icon.clone()).or_else(|| {
            Some(
                match agent.as_str() {
                    "codex" | "chatgpt-api" => "openai",
                    "gemini" => "geminicli",
                    "antigravity" => "antigravity",
                    "claude" => "claude",
                    _ => "github",
                }
                .into(),
            )
        });
        if agent == "codex" {
            c.record.source = UpstreamSource::Codex;
            c.record.codex_settings = Some(Settings {
                accounts: vec![],
                mode,
                models,
                groups_only,
            });
            c.subscription = imported_identity(&row["auth"])
                .map(|_| SavedSubscription::Codex(row["auth"].clone()));
        } else {
            let provider = service(&agent).ok_or(INVALID)?.0;
            let parsed = if provider == SubscriptionProvider::GithubCopilot {
                CopilotAccount::parse(row).map(SavedSubscription::Copilot)
            } else {
                native::parse(provider, row)
                    .map(|account| SavedSubscription::Native(Box::new(account)))
            };
            let id = match &parsed {
                Ok(SavedSubscription::Native(value)) => value.account.id.clone(),
                _ => "import-pending".into(),
            };
            c.record.source = UpstreamSource::Subscription {
                provider,
                account_id: id,
                mode,
                models,
                catalog,
                groups_only,
            };
            c.subscription = parsed.ok();
            if c.subscription.is_none() {
                continue;
            }
        }
        c.skip = (c.subscription.is_none() || !text(row, &["lapsed"]).is_empty())
            .then(|| "reauth".into());
        candidates.push(c);
    }
    Ok(candidates)
}

async fn observe(
    manager: &CodexOAuthManager,
    copilot: &CopilotAuthManager,
    store: &Path,
    mut sources: Vec<PrivateSource>,
    have: &[RoutingUpstream],
) -> Result<Vec<PrivateSource>, String> {
    for source in &mut sources {
        for (candidate, item) in source.candidates.iter().zip(&mut source.public.items) {
            let id = match &candidate.subscription {
                Some(SavedSubscription::Codex(auth)) => manager.imported_account_id(auth).await,
                Some(SavedSubscription::Native(account)) => {
                    native::existing_id(store, account).await?
                }
                Some(SavedSubscription::Copilot(account)) => {
                    copilot.imported_account_id(account).await
                }
                None => continue,
            };
            let Some(id) = id else { continue };
            if let Some(existing) =
                have.iter()
                    .find(|r| match (&candidate.record.source, &r.source) {
                        (UpstreamSource::Codex, UpstreamSource::Codex) => r
                            .codex_settings
                            .as_ref()
                            .is_some_and(|s| s.accounts.contains(&id)),
                        (
                            UpstreamSource::Subscription { provider: a, .. },
                            UpstreamSource::Subscription {
                                provider: b,
                                account_id,
                                ..
                            },
                        ) => a == b && account_id == &id,
                        _ => false,
                    })
            {
                item.status = if source.public.id == "magpie" {
                    "new"
                } else {
                    "same"
                }
                .into();
                item.existing = Some(existing.name.clone());
            }
        }
    }
    Ok(sources)
}

pub(crate) async fn preview_with_managers(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    copilot: &CopilotAuthManager,
    paths: ImportPaths,
) -> Result<Vec<Source>, String> {
    let store = paths.subscription_store.clone();
    let (sources, have) = tokio::task::spawn_blocking(move || {
        let have = db.private_routing_upstreams()?;
        Ok::<_, String>((scan(&db, &paths, &have)?, have))
    })
    .await
    .map_err(|_| FAILED)??;
    Ok(observe(&manager, copilot, &store, sources, &have)
        .await?
        .into_iter()
        .map(|s| s.public)
        .collect())
}

pub(crate) async fn import_with_managers(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    copilot: &CopilotAuthManager,
    paths: ImportPaths,
    picks: Vec<Pick>,
) -> Result<ImportResult, String> {
    if picks.is_empty() || picks.len() > 256 {
        return Err(INVALID.into());
    }
    let store = paths.subscription_store.clone();
    let (db, mut selected, auths) = tokio::task::spawn_blocking(move || {
        let sources = scan(&db, &paths, &db.private_routing_upstreams()?)?;
        let mut seen = HashSet::new();
        let mut selected = vec![];
        let mut auths = vec![];
        for pick in picks {
            if !seen.insert((pick.source.clone(), pick.reference.clone())) {
                return Err(INVALID.into());
            }
            let candidate = sources
                .iter()
                .find(|s| s.public.id == pick.source)
                .and_then(|s| s.candidates.iter().find(|c| c.reference == pick.reference))
                .ok_or(CONFLICT)?;
            if candidate.skip.is_some() || fingerprint(candidate)? != pick.fingerprint {
                return Err(CONFLICT.into());
            }
            let mut record = candidate.record.clone();
            if let Some(name) = pick.name.filter(|name| !name.trim().is_empty()) {
                record.name = name.trim().into();
            }
            record.validate()?;
            if let Some(auth) = &candidate.subscription {
                if matches!(pick.mode, ImportMode::Key) {
                    return Err(INVALID.into());
                }
                auths.push((selected.len(), auth.clone()));
            }
            if pick.source != "magpie" {
                record.id.clear();
            }
            selected.push(Selected {
                record,
                join: matches!(pick.mode, ImportMode::Key),
                overwrite: pick.source == "magpie",
            });
        }
        Ok::<_, String>((db, selected, auths))
    })
    .await
    .map_err(|_| FAILED)??;
    let mut native_values = vec![];
    let mut copilot_values = vec![];
    let mut codex_values = vec![];
    let mut native_indices = vec![];
    let mut copilot_indices = vec![];
    let mut codex_indices = vec![];
    for (index, auth) in auths {
        match auth {
            SavedSubscription::Native(value) => {
                native_indices.push(index);
                native_values.push(*value);
            }
            SavedSubscription::Copilot(value) => {
                copilot_indices.push(index);
                copilot_values.push(value);
            }
            SavedSubscription::Codex(value) => {
                codex_indices.push(index);
                codex_values.push(value);
            }
        }
    }
    native::import_accounts(store, native_values, |native_ids| async move {
        for (index, id) in native_indices.into_iter().zip(native_ids) {
            if let UpstreamSource::Subscription { account_id, .. } =
                &mut selected[index].record.source
            {
                *account_id = id;
            }
        }
        copilot
            .import_provider_accounts(copilot_values, |copilot_ids| async move {
                for (index, id) in copilot_indices.into_iter().zip(copilot_ids) {
                    if let UpstreamSource::Subscription { account_id, .. } =
                        &mut selected[index].record.source
                    {
                        *account_id = id;
                    }
                }
                let publish = move |ids: Vec<String>| {
                    for (index, id) in codex_indices.into_iter().zip(ids) {
                        selected[index]
                            .record
                            .codex_settings
                            .as_mut()
                            .ok_or(INVALID)?
                            .accounts = vec![id];
                    }
                    db.import_native_providers(selected)
                };
                if codex_values.is_empty() {
                    tokio::task::spawn_blocking(move || publish(vec![]))
                        .await
                        .map_err(|_| FAILED)?
                } else {
                    manager
                        .import_provider_accounts(codex_values, publish)
                        .await
                }
            })
            .await
    })
    .await
}

#[cfg(test)]
pub(crate) async fn preview_with_accounts(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    paths: ImportPaths,
) -> Result<Vec<Source>, String> {
    let copilot = CopilotAuthManager::new(paths.home.join("copilot-store"));
    preview_with_managers(db, manager, &copilot, paths).await
}

#[cfg(test)]
pub(crate) async fn import_with_accounts(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    paths: ImportPaths,
    picks: Vec<Pick>,
) -> Result<ImportResult, String> {
    let copilot = CopilotAuthManager::new(paths.home.join("copilot-store"));
    import_with_managers(db, manager, &copilot, paths, picks).await
}
