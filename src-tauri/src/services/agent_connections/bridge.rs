//! A durable outbox for TRAE. A saved intention is not a delivered catalog.
use super::{models, read, Manager, Receipt, RecoveryReason, Status, CONFLICT, FAILED};
use crate::{config::atomic_write_private, provider::Provider, proxy::intent::Menu};
use serde::{Deserialize, Serialize};

const ID: &str = "shared_catalog_connection";
#[derive(Serialize, Deserialize)]
pub(super) struct Outbox {
    enabled: bool,
    token: String,
    provider: Option<Provider>,
    delivered: bool,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    selection_pending: bool,
}
impl Manager {
    fn bridge_path(&self) -> std::path::PathBuf {
        self.root.join("trae-shared.json")
    }
    fn bridge_load(&self) -> Result<Option<Outbox>, String> {
        read(&self.bridge_path())?
            .map(|text| serde_json::from_str(&text).map_err(|_| FAILED.into()))
            .transpose()
    }
    fn bridge_save(&self, outbox: &Outbox) -> Result<(), String> {
        atomic_write_private(
            &self.bridge_path(),
            &serde_json::to_vec(outbox).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED.into())
    }
    pub(crate) fn bridge_owned(&self) -> bool {
        self.bridge_load().map_or(true, |o| o.is_some())
    }
    pub(crate) fn bridge_enabled(&self) -> bool {
        self.bridge_load().ok().flatten().is_some_and(|o| o.enabled)
    }
    pub(crate) fn bridge_status(&self) -> Result<Status, String> {
        let outbox = self.bridge_load()?;
        let enabled = outbox.as_ref().is_some_and(|o| o.enabled);
        let delivered = outbox.as_ref().is_some_and(|o| o.delivered);
        Ok(Status {
            app: "trae".into(),
            enabled,
            configured: enabled
                && delivered
                && !outbox.as_ref().is_some_and(|o| o.selection_pending),
            recovery_pending: outbox
                .as_ref()
                .is_some_and(|o| !delivered || o.selection_pending),
            recovery_reason: outbox
                .as_ref()
                .is_some_and(|o| !delivered || o.selection_pending)
                .then_some(RecoveryReason::BridgePending),
            model: outbox.and_then(|o| o.model),
            restart_required: enabled && !delivered,
            desktop_state: None,
        })
    }
    pub(crate) fn bridge_select(&self, enabled: bool) -> Result<(), String> {
        self.bridge_select_model(enabled, None)
    }
    pub(crate) fn bridge_select_model(
        &self,
        enabled: bool,
        model: Option<String>,
    ) -> Result<(), String> {
        let old = self.bridge_load()?;
        if !enabled && old.is_none() {
            return Ok(());
        }
        let selected = model.or_else(|| old.as_ref().and_then(|o| o.model.clone()));
        let delivered = enabled && old.as_ref().is_some_and(|o| o.enabled && o.delivered);
        self.bridge_save(&Outbox {
            enabled,
            token: old.as_ref().map(|o| o.token.clone()).unwrap_or_else(|| {
                format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                )
            }),
            provider: old.and_then(|o| o.provider),
            delivered,
            selection_pending: enabled && selected.is_some(),
            model: selected,
        })
    }
    pub(crate) fn bridge_retry(&self) -> Result<(), String> {
        let mut outbox = self.bridge_load()?.ok_or(CONFLICT)?;
        outbox.delivered = false;
        outbox.selection_pending = outbox.model.is_some();
        self.bridge_save(&outbox)
    }
    pub(crate) fn bridge_sync(
        &self,
        menu: Option<&Menu>,
        origin: &str,
        suspend: bool,
    ) -> Result<Option<Receipt>, String> {
        self.bridge_sync_with(
            menu,
            origin,
            suspend,
            |provider| {
                crate::services::trae_bridge::apply_connected(provider).map_err(|_| FAILED.into())
            },
            || {
                crate::services::trae_bridge::remove(ID).map_err(|_| FAILED)?;
                crate::services::trae_bridge::restore_shared().map_err(|_| FAILED.into())
            },
            |model| {
                crate::services::trae_bridge::select_starting_model(ID, model)
                    .map_err(|_| FAILED.into())
            },
            crate::services::trae_bridge::shared_matches,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn bridge_sync_with(
        &self,
        menu: Option<&Menu>,
        origin: &str,
        suspend: bool,
        apply: impl FnOnce(&Provider) -> Result<(), String>,
        remove: impl FnOnce() -> Result<(), String>,
        select: impl FnOnce(&str) -> Result<(), String>,
        ready: impl FnOnce(&Provider) -> bool,
    ) -> Result<Option<Receipt>, String> {
        let Some(mut outbox) = self.bridge_load()? else {
            return Ok(None);
        };
        let catalog = menu.map(models).unwrap_or_default();
        if !outbox.enabled || suspend || catalog.is_empty() {
            outbox.delivered = false;
            outbox.selection_pending = outbox.enabled && outbox.model.is_some();
            self.bridge_save(&outbox)?;
            remove()?;
            if !outbox.enabled {
                std::fs::remove_file(self.bridge_path()).map_err(|_| FAILED)?;
            }
            return Ok(None);
        }
        let config = super::projections::client_config(
            &catalog,
            &format!("{origin}/connections/trae"),
            &outbox.token,
            &catalog[0].id,
        );
        if outbox
            .model
            .as_ref()
            .is_some_and(|selected| !catalog.iter().any(|m| &m.id == selected))
        {
            outbox.model = Some(catalog[0].id.clone());
            outbox.selection_pending = true;
        }
        let provider = Provider::with_id(ID.into(), "Vary AI".into(), config, None);
        let unchanged = outbox.delivered
            && outbox
                .provider
                .as_ref()
                .is_some_and(|old| old.settings_config == provider.settings_config);
        if unchanged {
            if !ready(&provider) {
                return Err(CONFLICT.into());
            }
            if outbox.selection_pending {
                self.bridge_save(&outbox)?;
                if let Some(model) = &outbox.model {
                    select(model)?;
                }
                outbox.selection_pending = false;
                self.bridge_save(&outbox)?;
            }
            return Ok(Some(Receipt {
                app: "trae".into(),
                token: outbox.token,
                model: outbox.model.unwrap_or_else(|| catalog[0].id.clone()),
                model_map: Default::default(),
                visible_models: None,
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            }));
        }
        outbox.provider = Some(provider.clone());
        outbox.delivered = false;
        self.bridge_save(&outbox)?;
        apply(&provider)?;
        if outbox.selection_pending {
            if let Some(model) = &outbox.model {
                select(model)?;
            }
            outbox.selection_pending = false;
        }
        outbox.delivered = true;
        self.bridge_save(&outbox)?;
        Ok(Some(Receipt {
            app: "trae".into(),
            token: outbox.token,
            model: outbox.model.unwrap_or_else(|| catalog[0].id.clone()),
            model_map: Default::default(),
            visible_models: None,
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_delivery_refreshes_global_models_and_suspend_preserves_intent() -> Result<(), String>
    {
        use crate::{
            proxy::intent::{MenuModel, Session, Target},
            services::intent_routing::RoutingGroup,
        };
        use std::{collections::HashMap, sync::Arc, time::Instant};
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().into(),
        };
        manager.bridge_select(true)?;
        let group: RoutingGroup = serde_json::from_value(serde_json::json!({"id":"fixture","name":"Fixture","revision":0,"enabled":true,"members":[],"defaultMemberId":"a","fallbackMemberIds":[],"intentRecognitionEnabled":false,"classifierTarget":null,"classificationBudgetMs":2000,"rules":[]})).map_err(|_| FAILED)?;
        let alias = group.alias();
        let target = Target {
            provider_id: "fixture".into(),
            model: "exact".into(),
            upstream: None,
            effort: Default::default(),
            url: "http://127.0.0.1:1/responses".parse().map_err(|_| FAILED)?,
            headers: Default::default(),
            official: false,
            credentials: None,
            scheduling: Arc::default(),
            fixed_effort: None,
        };
        let mut menu = Menu {
            token: String::new(),
            models: HashMap::from([(
                alias.clone(),
                MenuModel {
                    alias: alias.clone(),
                    label: "Fixture".into(),
                    description: "VaryAI routing group",
                    order: 0,
                },
            )]),
            default_model: Some(alias.clone()),
            omitted_entries: 0,
            context_settings: Default::default(),
            groups: HashMap::from([(
                alias.clone(),
                Arc::new(Session {
                    group,
                    targets: vec![target],
                    classifier: None,
                    client: reqwest::Client::new(),
                    created: Instant::now(),
                    bindings: Arc::default(),
                }),
            )]),
        };
        assert!(manager
            .bridge_sync_with(
                Some(&menu),
                "http://127.0.0.1:1",
                false,
                |_| Err(FAILED.into()),
                || Ok(()),
                |_| Ok(()),
                |_| true
            )
            .is_err());
        assert!(!manager.bridge_status()?.configured);
        let receipt = manager
            .bridge_sync_with(
                Some(&menu),
                "http://127.0.0.1:1",
                false,
                |provider| {
                    assert_eq!(provider.settings_config["models"][0]["model"], alias);
                    assert_eq!(
                        provider.settings_config["baseUrl"],
                        "http://127.0.0.1:1/connections/trae/v1"
                    );
                    Ok(())
                },
                || Ok(()),
                |_| Ok(()),
                |_| true,
            )?
            .ok_or(FAILED)?;
        assert_eq!(receipt.app, "trae");
        assert!(manager.bridge_status()?.configured);
        manager.bridge_select_model(true, Some(alias.clone()))?;
        assert_eq!(manager.bridge_status()?.model, Some(alias.clone()));
        assert!(manager.bridge_status()?.recovery_pending);
        assert!(manager
            .bridge_sync_with(
                Some(&menu),
                "http://127.0.0.1:1",
                false,
                |_| panic!("selection must not rewrite a delivered catalog"),
                || Ok(()),
                |_| Err(FAILED.into()),
                |_| true,
            )
            .is_err());
        assert!(!manager.bridge_status()?.configured);
        assert!(manager.bridge_status()?.recovery_pending);
        manager.bridge_sync_with(
            Some(&menu),
            "http://127.0.0.1:1",
            false,
            |_| panic!("selection retry must not rewrite the catalog"),
            || Ok(()),
            |selected| {
                assert_eq!(selected, alias);
                Ok(())
            },
            |_| true,
        )?;
        assert!(manager.bridge_status()?.configured);
        assert!(!manager.bridge_status()?.recovery_pending);
        manager.bridge_sync_with(
            Some(&menu),
            "http://127.0.0.1:1",
            false,
            |_| panic!("unchanged catalog must not be rewritten"),
            || Ok(()),
            |_| panic!("ordinary refresh must preserve the native selection"),
            |_| true,
        )?;
        assert!(manager
            .bridge_sync_with(
                Some(&menu),
                "http://127.0.0.1:1",
                false,
                |_| panic!("external edits require explicit retry"),
                || Ok(()),
                |_| Ok(()),
                |_| false
            )
            .is_err());
        manager.bridge_retry()?;
        manager.bridge_sync_with(
            Some(&menu),
            "http://127.0.0.1:1",
            false,
            |_| Ok(()),
            || Ok(()),
            |_| Ok(()),
            |_| false,
        )?;
        menu.models.get_mut(&alias).ok_or(FAILED)?.label = "Renamed".into();
        manager.bridge_sync_with(
            Some(&menu),
            "http://127.0.0.1:1",
            false,
            |provider| {
                assert_eq!(
                    provider.settings_config["models"][0]["displayName"],
                    "Renamed"
                );
                Ok(())
            },
            || Ok(()),
            |_| Ok(()),
            |_| true,
        )?;
        manager.bridge_sync_with(
            Some(&menu),
            "http://127.0.0.1:1",
            true,
            |_| panic!("suspension cannot publish"),
            || Ok(()),
            |_| Ok(()),
            |_| true,
        )?;
        assert!(manager.bridge_enabled());
        assert!(!manager.bridge_status()?.configured);
        assert_eq!(manager.bridge_status()?.model, Some(alias.clone()));
        assert!(manager.bridge_load()?.is_some_and(|o| o.selection_pending));
        Ok(())
    }
    #[test]
    fn legacy_outbox_does_not_invent_a_starting_selection() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().into(),
        };
        std::fs::write(
            manager.bridge_path(),
            r#"{"enabled":true,"token":"fixture","provider":null,"delivered":true}"#,
        )
        .map_err(|_| FAILED)?;
        assert!(manager.bridge_status()?.configured);
        assert!(manager.bridge_status()?.model.is_none());
        assert!(!manager.bridge_load()?.is_some_and(|o| o.selection_pending));
        Ok(())
    }
    #[test]
    fn offline_bridge_cleanup_is_retained_until_acknowledged() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().into(),
        };
        manager.bridge_select(true)?;
        assert!(manager.bridge_status()?.recovery_pending);
        assert!(!manager.bridge_status()?.configured);
        manager.bridge_select(false)?;
        assert!(manager
            .bridge_sync_with(
                None,
                "http://127.0.0.1:1",
                false,
                |_| Ok(()),
                || Err(FAILED.into()),
                |_| Ok(()),
                |_| true
            )
            .is_err());
        assert!(manager.bridge_owned());
        assert!(!manager.bridge_status()?.enabled);
        manager.bridge_sync_with(
            None,
            "http://127.0.0.1:1",
            false,
            |_| Ok(()),
            || Ok(()),
            |_| Ok(()),
            |_| true,
        )?;
        assert!(!manager.bridge_owned());
        Ok(())
    }
}
