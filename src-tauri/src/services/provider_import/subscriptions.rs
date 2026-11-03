//! The reference implementation's saved Codex sign-ins live beside providers.json, not in API keys.
use super::*;
use crate::{
    proxy::providers::codex_oauth_auth::{imported_identity, CodexOAuthManager},
    services::codex_provider::Settings,
};
use std::sync::Arc;

pub(super) fn read(path: &Path) -> Result<Vec<Candidate>, String> {
    let logins = path.with_file_name("logins.json");
    let mut candidates = if path.exists() {
        readers::provider_backup(path)?
    } else {
        vec![]
    };
    if !logins.exists() {
        return Ok(candidates);
    }
    let value = json_file(&logins)?;
    let rows = value.as_array().ok_or(INVALID)?;
    if rows.len() > MAX_ITEMS {
        return Err(INVALID.into());
    }
    let policy = candidates
        .iter()
        .find(|c| c.reference.ends_with("/codex"))
        .map(|c| c.record.clone());
    let has_codex = rows.iter().any(|row| row["agent"] == "codex");
    if has_codex {
        candidates.retain(|c| !c.reference.ends_with("/codex"));
    }
    for (index, row) in rows.iter().enumerate() {
        if row["agent"] != "codex" {
            let mut c = candidate(
                format!("login/{index}"),
                "logins.json",
                text(row, &["user", "agent"]),
                String::new(),
                String::new(),
                "openai_responses",
                vec![],
            );
            c.skip = Some("subscription".into());
            candidates.push(c);
            continue;
        }
        let auth = row["auth"].clone();
        let valid = imported_identity(&auth).is_some();
        let plan = match row["plan"].as_str() {
            Some("plus") => "ChatGPT Plus",
            Some("pro") => "ChatGPT Pro",
            Some("team" | "business") => "ChatGPT Business",
            Some("enterprise") => "ChatGPT Enterprise",
            _ => "ChatGPT",
        };
        let user = text(row, &["user"]);
        let mut c = candidate(
            format!("login/{index}/{}", text(row, &["id"])),
            &format!("Codex · {plan}"),
            if user.is_empty() {
                "Codex".into()
            } else {
                format!("Codex · {user}")
            },
            String::new(),
            String::new(),
            "openai_responses",
            vec![],
        );
        let mut settings = Settings {
            accounts: vec![],
            ..Default::default()
        };
        if let Some(policy) = &policy {
            if let UpstreamSource::Api {
                mode,
                models,
                groups_only,
                ..
            } = &policy.source
            {
                settings.mode = mode.clone();
                settings.models = models.clone();
                settings.groups_only = *groups_only;
            }
            c.record.enabled = policy.enabled;
        }
        // The reference implementation's `on` is a rotation flag; the active native login may omit it.
        // An explicit import selects the account independently of that rotation.
        if row["paused"] == true {
            c.record.enabled = false;
        }
        c.record.source = UpstreamSource::Codex;
        c.record.codex_settings = Some(settings);
        c.subscription = Some(auth);
        c.skip = if !valid || !text(row, &["lapsed", "hidden"]).is_empty() {
            Some("reauth".into())
        } else {
            None
        };
        candidates.push(c);
    }
    Ok(candidates)
}

async fn observe(
    manager: &CodexOAuthManager,
    mut sources: Vec<PrivateSource>,
    have: &[RoutingUpstream],
) -> Vec<PrivateSource> {
    for source in &mut sources {
        for (candidate, item) in source.candidates.iter().zip(&mut source.public.items) {
            let Some(auth) = &candidate.subscription else {
                continue;
            };
            item.model_count = candidate
                .record
                .codex_settings
                .as_ref()
                .map_or(0, |s| s.models.len());
            if let Some(id) = manager.imported_account_id(auth).await {
                if let Some(existing) = have.iter().find(|r| {
                    r.codex_settings
                        .as_ref()
                        .is_some_and(|s| s.accounts.contains(&id))
                }) {
                    item.status = "same".into();
                    item.existing = Some(existing.name.clone());
                }
            }
        }
    }
    sources
}

pub(crate) async fn preview_with_accounts(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    paths: ImportPaths,
) -> Result<Vec<Source>, String> {
    let (sources, have) = tokio::task::spawn_blocking(move || {
        let have = db.private_routing_upstreams()?;
        Ok::<_, String>((scan(&paths, &have), have))
    })
    .await
    .map_err(|_| FAILED)??;
    Ok(observe(&manager, sources, &have)
        .await
        .into_iter()
        .map(|s| s.public)
        .collect())
}

pub(crate) async fn import_with_accounts(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    paths: ImportPaths,
    picks: Vec<Pick>,
) -> Result<ImportResult, String> {
    if picks.is_empty() || picks.len() > 256 {
        return Err(INVALID.into());
    }
    let selected = tokio::task::spawn_blocking(move || {
        let sources = scan(&paths, &db.private_routing_upstreams()?);
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
                auths.push(auth.clone());
            }
            record.id.clear();
            selected.push(Selected {
                record,
                join: matches!(pick.mode, ImportMode::Key),
            });
        }
        Ok::<_, String>((db, selected, auths))
    })
    .await
    .map_err(|_| FAILED)??;
    let (db, mut selected, auths) = selected;
    if auths.is_empty() {
        return tokio::task::spawn_blocking(move || db.import_native_providers(selected))
            .await
            .map_err(|_| FAILED)?;
    }
    manager
        .import_provider_accounts(auths, move |ids| {
            let mut ids = ids.into_iter();
            for selection in &mut selected {
                if matches!(selection.record.source, UpstreamSource::Codex) {
                    selection
                        .record
                        .codex_settings
                        .as_mut()
                        .ok_or(INVALID)?
                        .accounts = vec![ids.next().ok_or(INVALID)?];
                }
            }
            db.import_native_providers(selected)
        })
        .await
}
