//! Persist TRAE selection independently of delivery to a connected client.
use crate::{database::Database, error::AppError, provider::Provider};

pub(crate) const PENDING_KEY: &str = "profile_trae_sync_pending";
pub(crate) const ROUTE_PENDING_KEY: &str = "trae_responses_route_pending";

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

pub(crate) fn sync_response_route(db: &Database) -> bool {
    sync_response_route_with(db, |provider| {
        // Reattach to a user-opened client even when a pending route removal
        // no longer requests inspection. Do not enable auditing or launch TRAE.
        if !db.get_bool_flag(crate::services::trae_bridge::CONSENT_KEY)? {
            return Err(super::unavailable());
        }
        super::ensure_ready()?;
        super::apply_connected(provider)
    })
}

fn sync_response_route_with(
    db: &Database,
    mut apply: impl FnMut(&Provider) -> Result<(), AppError>,
) -> bool {
    let mut operation = || -> Result<(), AppError> {
        for provider in db.get_all_providers("trae")?.into_values().filter(|p| {
            p.meta.as_ref().and_then(|m| m.live_config_managed) == Some(true)
                && crate::services::upstream_protocol::trae_native_responses(p)
        }) {
            // Update owned models in place so existing conversations keep their IDs.
            apply(&crate::services::upstream_protocol::project(
                db,
                &crate::app_config::AppType::Trae,
                &provider,
            )?)?;
        }
        db.set_setting(ROUTE_PENDING_KEY, "false")
    };
    if operation().is_ok() {
        true
    } else {
        log::debug!("TRAE Responses routing synchronization is pending");
        false
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn response_route_sync_preserves_selection_and_retries_delivery_without_auditing(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = Database::memory()?;
        db.update_security_settings(&crate::database::SecurityAuditSettings { enabled: false })?;
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({
                "protocol":"openai-responses", "baseUrl":"https://upstream.example/v1",
                "apiKey":"synthetic-key", "models":[{"model":"fixture"}]
            }),
            None,
        );
        provider
            .meta
            .get_or_insert_with(Default::default)
            .live_config_managed = Some(true);
        db.save_provider("trae", &provider)?;
        db.set_current_provider("trae", &provider.id)?;
        db.set_setting(crate::services::trae_bridge::ENABLED_KEY, "true")?;
        db.set_setting(ROUTE_PENDING_KEY, "true")?;
        assert!(!sync_response_route_with(&db, |_| Err(AppError::Config(
            "synthetic unavailable".into()
        ))));
        assert!(db.get_bool_flag(ROUTE_PENDING_KEY)?);
        let mut delivered = Vec::new();
        assert!(sync_response_route_with(&db, |p| {
            delivered.push(p.clone());
            Ok(())
        }));
        assert!(delivered[0].settings_config["baseUrl"]
            .as_str()
            .is_some_and(|url| url.contains("/upstream/trae/providers/fixture/v1")));
        assert!(!db.get_bool_flag(ROUTE_PENDING_KEY)?);
        db.set_setting(crate::services::trae_bridge::ENABLED_KEY, "false")?;
        db.set_setting(ROUTE_PENDING_KEY, "true")?;
        assert!(sync_response_route_with(&db, |p| {
            delivered.push(p.clone());
            Ok(())
        }));
        assert_eq!(delivered[1].settings_config, provider.settings_config);
        assert_eq!(db.get_current_provider("trae")?, Some(provider.id.clone()));
        assert_eq!(
            db.get_provider_by_id(&provider.id, "trae")?
                .unwrap()
                .settings_config,
            provider.settings_config
        );
        assert!(!db.get_security_settings()?.enabled);
        provider.category = Some("official".into());
        db.save_provider("trae", &provider)?;
        assert!(sync_response_route_with(&db, |_| panic!(
            "official model must remain untouched"
        )));
        provider.category = None;
        provider.meta.as_mut().unwrap().live_config_managed = Some(false);
        db.save_provider("trae", &provider)?;
        assert!(sync_response_route_with(&db, |_| panic!(
            "inactive model must remain untouched"
        )));
        Ok(())
    }
}
