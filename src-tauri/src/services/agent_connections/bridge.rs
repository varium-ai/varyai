//! A durable outbox for TRAE. A saved intention is not a delivered catalog.
use super::{models, read, Manager, Receipt, RecoveryReason, Status, CONFLICT, FAILED};
use crate::{config::atomic_write_private, provider::Provider, proxy::intent::Menu};
use serde::{Deserialize, Serialize};

const ID: &str = "shared_catalog_connection";
const MODEL_REJECTED: &str = "agentConnections.nativeModelRejected";
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
    #[serde(default)]
    configuration_changed: bool,
    #[serde(default)]
    model_write_failed: bool,
}
impl Manager {
    pub(crate) fn bridge_for(
        &self,
        edition: crate::services::trae_bridge::shared::Edition,
    ) -> TraeBridge<'_> {
        TraeBridge {
            manager: self,
            edition,
        }
    }
    pub(crate) fn bridge_owned(&self) -> bool {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_owned()
    }
    pub(crate) fn bridge_enabled(&self) -> bool {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_enabled()
    }
    pub(crate) fn bridge_status(&self) -> Result<Status, String> {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_status()
    }
    pub(crate) fn bridge_select(&self, enabled: bool) -> Result<(), String> {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_select(enabled)
    }
    pub(crate) fn bridge_select_model(
        &self,
        enabled: bool,
        model: Option<String>,
    ) -> Result<(), String> {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_select_model(enabled, model)
    }
    pub(crate) fn bridge_retry(&self) -> Result<(), String> {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_retry()
    }
    pub(crate) fn bridge_sync(
        &self,
        menu: Option<&Menu>,
        origin: &str,
        suspend: bool,
    ) -> Result<Option<Receipt>, String> {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_sync(menu, origin, suspend)
    }
}
pub(crate) struct TraeBridge<'a> {
    manager: &'a Manager,
    edition: crate::services::trae_bridge::shared::Edition,
}
impl TraeBridge<'_> {
    fn client(&self) -> crate::services::trae_bridge::shared::Client {
        crate::services::trae_bridge::shared::Client(self.edition)
    }
    fn bridge_path(&self) -> std::path::PathBuf {
        self.manager.root.join(match self.edition {
            crate::services::trae_bridge::shared::Edition::China => "trae-shared.json",
            crate::services::trae_bridge::shared::Edition::International => "trae-intl-shared.json",
        })
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
        self.bridge_status_with_desktop(self.client().applied_at_ms(), super::desktop::state)
    }
    fn bridge_status_with_desktop(
        &self,
        applied_ms: Option<i64>,
        state: impl FnOnce(&str, Option<i64>) -> Option<super::desktop::State>,
    ) -> Result<Status, String> {
        let outbox = self.bridge_load()?;
        let enabled = outbox.as_ref().is_some_and(|o| o.enabled);
        let delivered = outbox.as_ref().is_some_and(|o| o.delivered);
        let configuration_changed = outbox.as_ref().is_some_and(|o| o.configuration_changed);
        // Delivery retries update the outbox but do not change client launch settings.
        let desktop_state = enabled
            .then(|| state(self.edition.app(), applied_ms))
            .flatten();
        let desktop_restart = desktop_state == Some(super::desktop::State::RestartRequired);
        Ok(Status {
            app: self.edition.app().into(),
            enabled,
            configured: enabled && delivered && !configuration_changed,
            recovery_pending: outbox
                .as_ref()
                .is_some_and(|o| !delivered || o.selection_pending || configuration_changed),
            recovery_reason: if outbox.as_ref().is_some_and(|o| o.model_write_failed) {
                Some(RecoveryReason::BridgeModelRejected)
            } else if configuration_changed {
                Some(RecoveryReason::ConfigurationChanged)
            } else if delivered && outbox.as_ref().is_some_and(|o| o.selection_pending) {
                Some(RecoveryReason::BridgeSelectionPending)
            } else {
                outbox
                    .as_ref()
                    .is_some_and(|o| !delivered || o.selection_pending)
                    .then_some(RecoveryReason::BridgePending)
            },
            model: outbox.and_then(|o| o.model),
            restart_required: desktop_restart,
            reload_state: None,
            desktop_state,
            native_compatibility: None,
            configuration_issue: None,
        })
    }

    pub(crate) fn restart_bridge_desktop(&self) -> Result<(), String> {
        let outbox = self.bridge_load()?.ok_or(CONFLICT)?;
        if !outbox.enabled {
            return Err(CONFLICT.into());
        }
        let path = self.bridge_path();
        let applied_ms = self.client().applied_at_ms();
        let expected = std::fs::metadata(&path)
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| {
                time.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|duration| duration.as_millis() as i64)
            });
        let expected_token = outbox.token.clone();
        super::desktop::restart(self.edition.app(), applied_ms, || {
            self.bridge_load().is_ok_and(|current| {
                current.is_some_and(|current| {
                    current.enabled
                        && current.token == expected_token
                        && self.client().applied_at_ms() == applied_ms
                        && std::fs::metadata(&path)
                            .ok()
                            .and_then(|meta| meta.modified().ok())
                            .and_then(|time| {
                                time.duration_since(std::time::UNIX_EPOCH)
                                    .ok()
                                    .map(|duration| duration.as_millis() as i64)
                            })
                            == expected
                })
            })
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
        let configuration_changed =
            enabled && old.as_ref().is_some_and(|o| o.configuration_changed);
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
            configuration_changed,
            model_write_failed: false,
            model: selected,
        })
    }
    pub(crate) fn bridge_retry(&self) -> Result<(), String> {
        let mut outbox = self.bridge_load()?.ok_or(CONFLICT)?;
        outbox.delivered = false;
        outbox.configuration_changed = false;
        outbox.model_write_failed = false;
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
                self.client().apply(provider).map_err(|error| {
                    if matches!(
                        error,
                        crate::error::AppError::Localized {
                            key: "agents.trae_model_configuration"
                                | "agents.trae_recovery_unavailable",
                            ..
                        }
                    ) {
                        MODEL_REJECTED.into()
                    } else {
                        FAILED.into()
                    }
                })
            },
            || {
                self.client().remove(ID).map_err(|_| FAILED)?;
                self.client().restore().map_err(|_| FAILED.into())
            },
            |model| self.client().select(ID, model).map_err(|_| FAILED.into()),
            |provider| self.client().matches(provider),
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
        ready: impl FnOnce(&Provider) -> Option<bool>,
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
            &format!("{origin}/connections/{}", self.edition.app()),
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
        let provider = Provider::with_id(ID.into(), "VaryAI".into(), config, None);
        let unchanged = outbox.delivered
            && outbox
                .provider
                .as_ref()
                .is_some_and(|old| old.settings_config == provider.settings_config);
        if unchanged {
            match ready(&provider) {
                Some(true) => {
                    if outbox.configuration_changed {
                        outbox.configuration_changed = false;
                        self.bridge_save(&outbox)?;
                    }
                }
                Some(false) => {
                    if !outbox.configuration_changed {
                        outbox.configuration_changed = true;
                        self.bridge_save(&outbox)?;
                    }
                    return Err(CONFLICT.into());
                }
                // An offline bridge is not evidence of an externally changed configuration.
                None => return Err(FAILED.into()),
            }
            if outbox.selection_pending {
                self.bridge_save(&outbox)?;
                outbox.selection_pending = outbox
                    .model
                    .as_ref()
                    .is_some_and(|model| select(model).is_err());
                self.bridge_save(&outbox)?;
            }
            return Ok(Some(Receipt {
                app: self.edition.app().into(),
                token: outbox.token,
                model: outbox.model.unwrap_or_else(|| catalog[0].id.clone()),
                model_map: Default::default(),
                visible_models: None,
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            }));
        }
        if outbox.configuration_changed {
            return Err(CONFLICT.into());
        }
        outbox.provider = Some(provider.clone());
        outbox.delivered = false;
        self.bridge_save(&outbox)?;
        if let Err(error) = apply(&provider) {
            outbox.model_write_failed = error == MODEL_REJECTED;
            self.bridge_save(&outbox)?;
            return Err(error);
        }
        outbox.model_write_failed = false;
        // Catalog delivery remains usable when a native chat window cannot yet save its default.
        outbox.delivered = true;
        self.bridge_save(&outbox)?;
        if outbox.selection_pending {
            outbox.selection_pending = outbox
                .model
                .as_ref()
                .is_some_and(|model| select(model).is_err());
        }
        self.bridge_save(&outbox)?;
        Ok(Some(Receipt {
            app: self.edition.app().into(),
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
    fn pending_delivery_does_not_require_restart_after_a_manual_reopen() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let owner = Manager {
            root: dir.path().into(),
        };
        let manager = owner.bridge_for(crate::services::trae_bridge::shared::Edition::China);
        manager.bridge_select(true)?;
        let observe = |_: &str, applied: Option<i64>| {
            Some(if applied.is_some_and(|time| time >= 2000) {
                super::super::desktop::State::RestartRequired
            } else {
                super::super::desktop::State::Current
            })
        };
        let status = manager.bridge_status_with_desktop(Some(1000), observe)?;
        assert!(status.recovery_pending);
        assert_eq!(
            status.desktop_state,
            Some(super::super::desktop::State::Current)
        );
        assert!(!status.restart_required);
        manager.bridge_retry()?;
        let status = manager.bridge_status_with_desktop(Some(1000), observe)?;
        assert!(!status.restart_required);
        let stale = manager.bridge_status_with_desktop(Some(3000), observe)?;
        assert!(stale.restart_required);
        Ok(())
    }
    #[test]
    fn bridge_delivery_refreshes_global_models_and_suspend_preserves_intent() -> Result<(), String>
    {
        use crate::{
            proxy::intent::{MenuModel, Session, Target},
            services::intent_routing::RoutingGroup,
        };
        use std::{collections::HashMap, sync::Arc, time::Instant};
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let owner = Manager {
            root: dir.path().into(),
        };
        let manager = owner.bridge_for(crate::services::trae_bridge::shared::Edition::China);
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
                |_| Some(true)
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
                |_| Some(true),
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
                |_| Some(true),
            )?
            .is_some());
        assert!(manager.bridge_status()?.configured);
        assert!(matches!(
            manager.bridge_status()?.recovery_reason,
            Some(RecoveryReason::BridgeSelectionPending)
        ));
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
            |_| Some(true),
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
            |_| Some(true),
        )?;
        assert!(manager
            .bridge_sync_with(
                Some(&menu),
                "http://127.0.0.1:1",
                false,
                |_| panic!("an offline client must not trigger configuration replacement"),
                || Ok(()),
                |_| Ok(()),
                |_| None,
            )
            .is_err());
        assert!(manager.bridge_status()?.recovery_reason.is_none());
        assert!(manager
            .bridge_sync_with(
                Some(&menu),
                "http://127.0.0.1:1",
                false,
                |_| panic!("external edits require explicit retry"),
                || Ok(()),
                |_| Ok(()),
                |_| Some(false)
            )
            .is_err());
        assert!(matches!(
            manager.bridge_status()?.recovery_reason,
            Some(RecoveryReason::ConfigurationChanged)
        ));
        manager.bridge_retry()?;
        manager.bridge_sync_with(
            Some(&menu),
            "http://127.0.0.1:1",
            false,
            |_| Ok(()),
            || Ok(()),
            |_| Ok(()),
            |_| Some(false),
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
            |_| Some(true),
        )?;
        manager.bridge_sync_with(
            Some(&menu),
            "http://127.0.0.1:1",
            true,
            |_| panic!("suspension cannot publish"),
            || Ok(()),
            |_| Ok(()),
            |_| Some(true),
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
        let owner = Manager {
            root: dir.path().into(),
        };
        let manager = owner.bridge_for(crate::services::trae_bridge::shared::Edition::China);
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
        let owner = Manager {
            root: dir.path().into(),
        };
        let manager = owner.bridge_for(crate::services::trae_bridge::shared::Edition::China);
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
                |_| Some(true)
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
            |_| Some(true),
        )?;
        assert!(!manager.bridge_owned());
        Ok(())
    }
    #[test]
    fn editions_keep_shared_catalog_intentions_separate() -> Result<(), String> {
        use crate::services::trae_bridge::shared::Edition;
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().into(),
        };
        let china = manager.bridge_for(Edition::China);
        let international = manager.bridge_for(Edition::International);
        china.bridge_select_model(true, Some("synthetic-cn-model".into()))?;
        international.bridge_select_model(true, Some("synthetic-global-model".into()))?;
        assert_ne!(china.bridge_path(), international.bridge_path());
        assert_eq!(china.bridge_status()?.app, "trae");
        assert_eq!(international.bridge_status()?.app, "trae-intl");
        assert_eq!(
            china.bridge_status()?.model.as_deref(),
            Some("synthetic-cn-model")
        );
        assert_eq!(
            international.bridge_status()?.model.as_deref(),
            Some("synthetic-global-model")
        );
        international.bridge_select(false)?;
        assert!(china.bridge_enabled());
        assert!(!international.bridge_enabled());
        Ok(())
    }
}
