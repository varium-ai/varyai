//! Persist TRAE selection independently of delivery to a connected client.
use crate::{database::Database, error::AppError, provider::Provider};

pub(crate) const PENDING_KEY: &str = "profile_trae_sync_pending";

pub(crate) fn select(db: &Database, target: &Provider) -> Result<(), AppError> {
    let official = target.category.as_deref() == Some("official");
    if !official {
        crate::services::agent_clients::validate(&target.settings_config)?;
    }
    for mut provider in db.get_all_providers("trae")?.into_values() {
        if provider.id == target.id {
            provider = target.clone();
        }
        provider
            .meta
            .get_or_insert_with(Default::default)
            .live_config_managed = Some(provider.id == target.id && !official);
        db.save_provider("trae", &provider)?;
    }
    db.set_current_provider("trae", &target.id)?;
    db.set_setting(PENDING_KEY, "true")
}

pub(crate) fn sync(db: &Database) -> bool {
    let operation = || -> Result<(), AppError> {
        // Only contact an already connected client. Do not launch, install or
        // enable its bridge or security audit as part of a selection.
        crate::services::trae_bridge::profile_models()?;
        crate::services::trae_bridge::model_replacement_ready()?;
        let providers = db.get_all_providers("trae")?;
        // Include DB-only providers: an earlier offline switch may have left
        // one of them in the client. The bridge owns only VaryAI's namespace.
        for provider in providers.values().filter(|p| {
            p.category.as_deref() != Some("official")
                && p.meta.as_ref().and_then(|m| m.live_config_managed) != Some(true)
        }) {
            crate::services::trae_bridge::remove(&provider.id)?;
        }
        // Withdraw inactive entries before applying overlapping request model IDs.
        for provider in providers.values().filter(|p| {
            p.category.as_deref() != Some("official")
                && p.meta.as_ref().and_then(|m| m.live_config_managed) == Some(true)
        }) {
            crate::services::trae_bridge::replace_connected(
                &crate::services::upstream_protocol::project(
                    db,
                    &crate::app_config::AppType::Trae,
                    provider,
                )?,
            )?;
        }
        if let Some(official) = db
            .get_current_provider("trae")?
            .and_then(|id| providers.get(&id))
            .filter(|p| p.category.as_deref() == Some("official"))
        {
            crate::services::trae_bridge::replace_connected(official)?;
        }
        db.set_setting(PENDING_KEY, "false")
    };
    if operation().is_ok() {
        true
    } else {
        log::debug!("TRAE selection saved locally; client synchronization is pending");
        false
    }
}
