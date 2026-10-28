//! Profile selection is local state; delivering it to TRAE is a separate step.
use super::{AppError, AppState, Provider};

pub(super) const PENDING_KEY: &str = "profile_trae_sync_pending";

pub(super) fn select(state: &AppState, target: &Provider) -> Result<(), AppError> {
    let official = target.category.as_deref() == Some("official");
    if !official {
        crate::services::agent_clients::validate(&target.settings_config)?;
    }
    for mut provider in state.db.get_all_providers("trae")?.into_values() {
        if provider.id == target.id {
            provider = target.clone();
        }
        provider
            .meta
            .get_or_insert_with(Default::default)
            .live_config_managed = Some(provider.id == target.id && !official);
        state.db.save_provider("trae", &provider)?;
    }
    state.db.set_current_provider("trae", &target.id)?;
    state.db.set_setting(PENDING_KEY, "true")
}

pub(super) fn sync(state: &AppState) -> bool {
    let operation = || -> Result<(), AppError> {
        // Only contact an already connected client. Do not launch, install or
        // enable its bridge or security audit as part of a scheme switch.
        crate::services::trae_bridge::profile_models()?;
        let providers = state.db.get_all_providers("trae")?;
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
            crate::services::trae_bridge::apply_connected(
                &crate::services::upstream_protocol::project(
                    state.db.as_ref(),
                    &crate::app_config::AppType::Trae,
                    provider,
                )?,
            )?;
        }
        state.db.set_setting(PENDING_KEY, "false")
    };
    if operation().is_ok() {
        true
    } else {
        log::info!("Profile selected locally; TRAE client synchronization is pending");
        false
    }
}
