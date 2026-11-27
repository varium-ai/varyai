//! A durable outbox for TRAE. A saved intention is not a delivered catalog.
use super::{models, read, Manager, Receipt, RecoveryReason, Status, CONFLICT, FAILED};
use crate::{config::atomic_write_private, provider::Provider, proxy::intent::Menu};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, MutexGuard};

const ID: &str = "shared_catalog_connection";
const MODEL_REJECTED: &str = "agentConnections.nativeModelRejected";
static CHINA_OUTBOX: Mutex<()> = Mutex::new(());
static INTERNATIONAL_OUTBOX: Mutex<()> = Mutex::new(());
#[derive(Serialize, Deserialize)]
pub(super) struct Outbox {
    enabled: bool,
    token: String,
    #[serde(default)]
    revision: Option<String>,
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
    #[serde(default)]
    replace_pending: bool,
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
    #[cfg(test)]
    pub(crate) fn bridge_status(&self) -> Result<Status, String> {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_status()
    }
    pub(crate) fn bridge_select(&self, enabled: bool) -> Result<(), String> {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_select(enabled)
    }
    pub(crate) fn bridge_retry(&self) -> Result<(), String> {
        self.bridge_for(crate::services::trae_bridge::shared::Edition::China)
            .bridge_retry()
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
    fn lock_outbox(&self) -> Result<MutexGuard<'static, ()>, String> {
        use crate::services::trae_bridge::shared::Edition;
        match self.edition {
            Edition::China => &CHINA_OUTBOX,
            Edition::International => &INTERNATIONAL_OUTBOX,
        }
        .lock()
        .map_err(|_| FAILED.into())
    }
    fn commit<T>(
        &self,
        outbox: &Outbox,
        commit: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self.lock_outbox()?;
        if self
            .bridge_load()?
            .is_some_and(|current| current.revision == outbox.revision)
        {
            commit()
        } else {
            Err(FAILED.into())
        }
    }
    pub(crate) fn bridge_owned(&self) -> bool {
        self.bridge_load().map_or(true, |o| o.is_some())
    }
    pub(crate) fn bridge_enabled(&self) -> bool {
        self.bridge_load().ok().flatten().is_some_and(|o| o.enabled)
    }
    pub(crate) fn prepare(&self) -> Result<(), String> {
        let _guard = self.lock_outbox()?;
        if self.bridge_load()?.is_some_and(|outbox| outbox.enabled) {
            self.client().prepare().map_err(|_| FAILED.into())
        } else {
            Err(FAILED.into())
        }
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
            model: outbox
                .and_then(|o| o.model)
                .or_else(|| self.manager.remembered_model(self.edition.app())),
            restart_required: desktop_restart,
            reload_state: None,
            desktop_state,
            native_compatibility: None,
            configuration_issue: None,
        })
    }

    pub(crate) fn restart_bridge_desktop(&self) -> Result<(), String> {
        super::desktop::restart_manual(self.edition.app())
    }
    pub(crate) fn bridge_select(&self, enabled: bool) -> Result<(), String> {
        self.bridge_select_model(enabled, None)
    }
    pub(crate) fn bridge_select_model(
        &self,
        enabled: bool,
        model: Option<String>,
    ) -> Result<(), String> {
        self.set_intent(enabled, model, || {})
    }
    pub(crate) fn set_intent(
        &self,
        enabled: bool,
        model: Option<String>,
        revoke: impl FnOnce(),
    ) -> Result<(), String> {
        let _guard = self.lock_outbox()?;
        let old = self.bridge_load()?;
        if !enabled && old.is_none() {
            revoke();
            return Ok(());
        }
        let selected = model
            .or_else(|| old.as_ref().and_then(|o| o.model.clone()))
            .or_else(|| self.manager.remembered_model(self.edition.app()));
        let delivered = enabled
            && old
                .as_ref()
                .is_some_and(|o| o.enabled && o.delivered && !o.configuration_changed);
        let replace_pending = enabled
            && old
                .as_ref()
                .is_none_or(|outbox| !outbox.enabled || outbox.replace_pending);
        self.bridge_save(&Outbox {
            enabled,
            revision: Some(uuid::Uuid::new_v4().simple().to_string()),
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
            configuration_changed: false,
            model_write_failed: false,
            replace_pending,
            model: selected.clone(),
        })?;
        if !enabled {
            revoke();
        }
        self.manager
            .remember_model(self.edition.app(), selected.as_deref());
        Ok(())
    }
    pub(crate) fn bridge_retry(&self) -> Result<(), String> {
        let _guard = self.lock_outbox()?;
        let mut outbox = self.bridge_load()?.ok_or(CONFLICT)?;
        outbox.revision = Some(uuid::Uuid::new_v4().simple().to_string());
        outbox.delivered = false;
        outbox.configuration_changed = false;
        outbox.model_write_failed = false;
        outbox.replace_pending = true;
        outbox.selection_pending = outbox.model.is_some();
        self.bridge_save(&outbox)
    }
    pub(crate) fn bridge_sync(
        &self,
        menu: Option<&Menu>,
        origin: &str,
        suspend: bool,
        publish: impl Fn(Option<&Receipt>) -> Result<(), String>,
    ) -> Result<(), String> {
        self.deliver(
            menu,
            origin,
            suspend,
            |provider, replace| {
                let applied = if replace {
                    self.client().reapply(provider)
                } else {
                    self.client().apply(provider)
                };
                applied.map_err(|error| {
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
            || self.client().remove(ID).map_err(|_| FAILED.into()),
            || self.client().restore().map_err(|_| FAILED.into()),
            |model| self.client().selection_pending(ID, model) == Some(true),
            |model| self.client().select(ID, model).map_err(|_| FAILED.into()),
            |provider| self.client().matches(provider),
            publish,
        )
        .map(|_| ())
    }
    #[cfg(test)]
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
        self.deliver(
            menu,
            origin,
            suspend,
            |provider, _| apply(provider),
            remove,
            || Ok(()),
            |_| false,
            select,
            ready,
            |_| Ok(()),
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn deliver(
        &self,
        menu: Option<&Menu>,
        origin: &str,
        suspend: bool,
        apply: impl FnOnce(&Provider, bool) -> Result<(), String>,
        remove: impl FnOnce() -> Result<(), String>,
        restore: impl FnOnce() -> Result<(), String>,
        needs_selection: impl FnOnce(&str) -> bool,
        select: impl FnOnce(&str) -> Result<(), String>,
        ready: impl FnOnce(&Provider) -> Option<bool>,
        publish: impl Fn(Option<&Receipt>) -> Result<(), String>,
    ) -> Result<Option<Receipt>, String> {
        let mut outbox = {
            let _guard = self.lock_outbox()?;
            match self.bridge_load() {
                Ok(Some(outbox)) => outbox,
                Ok(None) => return Ok(None),
                Err(error) => {
                    publish(None)?;
                    return Err(error);
                }
            }
        };
        let catalog = menu.map(models).unwrap_or_default();
        if !outbox.enabled || suspend || catalog.is_empty() {
            outbox.delivered = false;
            outbox.selection_pending = outbox.enabled && outbox.model.is_some();
            self.commit(&outbox, || {
                publish(None)?;
                self.bridge_save(&outbox)
            })?;
            remove()?;
            self.commit(&outbox, || {
                restore()?;
                if !outbox.enabled {
                    std::fs::remove_file(self.bridge_path()).map_err(|_| FAILED)?;
                }
                Ok(())
            })?;
            return Ok(None);
        }
        if outbox
            .model
            .as_ref()
            .is_some_and(|selected| !catalog.iter().any(|m| &m.id == selected))
        {
            outbox.model = Some(catalog[0].id.clone());
            outbox.selection_pending = true;
            self.commit(&outbox, || self.bridge_save(&outbox))?;
        }
        // Keep an acknowledged ordering when only the starting model changes.
        let published_first = outbox
            .provider
            .as_ref()
            .filter(|_| outbox.delivered && !outbox.replace_pending)
            .and_then(|provider| provider.settings_config["models"][0]["model"].as_str())
            .filter(|id| catalog.iter().any(|model| model.id == *id));
        let first = published_first
            .or(outbox.model.as_deref())
            .unwrap_or(&catalog[0].id);
        let config = super::projections::client_config(
            &catalog,
            &format!("{origin}/connections/{}", self.edition.app()),
            &outbox.token,
            first,
        );
        let provider = Provider::with_id(ID.into(), "VaryAI".into(), config, None);
        let unchanged = !outbox.replace_pending
            && outbox.delivered
            && outbox
                .provider
                .as_ref()
                .is_some_and(|old| old.settings_config == provider.settings_config);
        let receipt = Receipt {
            app: self.edition.app().into(),
            token: outbox.token.clone(),
            model: outbox
                .model
                .clone()
                .unwrap_or_else(|| catalog[0].id.clone()),
            model_map: Default::default(),
            visible_models: None,
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        };
        let previously_delivered = unchanged && !outbox.configuration_changed;
        if previously_delivered {
            self.commit(&outbox, || publish(Some(&receipt)))?;
        }
        let mut bridge_ready = true;
        if unchanged {
            match ready(&provider) {
                Some(true) => {
                    if outbox.configuration_changed {
                        outbox.configuration_changed = false;
                        self.commit(&outbox, || self.bridge_save(&outbox))?;
                    }
                }
                Some(false) => {
                    let changed = !outbox.configuration_changed;
                    outbox.configuration_changed = true;
                    self.commit(&outbox, || {
                        publish(None)?;
                        if changed {
                            self.bridge_save(&outbox)?;
                        }
                        Ok(())
                    })?;
                    return Err(CONFLICT.into());
                }
                // An offline bridge is not evidence of an externally changed configuration.
                None if outbox.configuration_changed => return Err(CONFLICT.into()),
                None => bridge_ready = false,
            }
        } else {
            if outbox.configuration_changed {
                return Err(CONFLICT.into());
            }
            outbox.provider = Some(provider.clone());
            outbox.delivered = false;
            self.commit(&outbox, || self.bridge_save(&outbox))?;
            if let Err(error) = apply(&provider, outbox.replace_pending) {
                outbox.model_write_failed = error == MODEL_REJECTED;
                self.commit(&outbox, || self.bridge_save(&outbox))?;
                return Err(error);
            }
            outbox.model_write_failed = false;
            outbox.replace_pending = false;
            // The catalog is usable even while the preferred model awaits acknowledgement.
            outbox.delivered = true;
            self.commit(&outbox, || self.bridge_save(&outbox))?;
        }
        // A verified catalog can route while native windows restore their selections.
        if !previously_delivered {
            self.commit(&outbox, || publish(Some(&receipt)))?;
        }
        if bridge_ready
            && (outbox.selection_pending || outbox.model.as_deref().is_some_and(needs_selection))
        {
            outbox.selection_pending = true;
            self.commit(&outbox, || self.bridge_save(&outbox))?;
            if outbox
                .model
                .as_deref()
                .is_none_or(|model| select(model).is_ok())
            {
                outbox.selection_pending = false;
                self.commit(&outbox, || self.bridge_save(&outbox))?;
            }
        }
        Ok(Some(receipt))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{proxy::connections::Connections, services::trae_bridge::shared::Edition};

    #[test]
    fn published_trae_routes_survive_pending_selection_and_cannot_be_reinstalled_after_disconnect(
    ) -> Result<(), String> {
        let directory = tempfile::tempdir().map_err(|_| FAILED)?;
        let owner = Manager {
            root: directory.path().into(),
        };
        let menu = super::super::tests::fixture_menu();
        let connections = Connections::default();
        for edition in Edition::ALL {
            let bridge = owner.bridge_for(edition);
            let app = edition.app();
            let origin = "http://127.0.0.1:8123";
            let publish = |receipt: Option<&Receipt>| match receipt {
                Some(receipt) => connections.install(receipt),
                None => {
                    connections.revoke(app);
                    Ok(())
                }
            };
            let disconnect = || bridge.set_intent(false, None, || connections.revoke(app));
            bridge.bridge_select_model(true, menu.default_model.clone())?;
            let stale = bridge.deliver(
                Some(&menu),
                origin,
                false,
                |_, _| {
                    disconnect()?;
                    Ok(())
                },
                || Ok(()),
                || Ok(()),
                |_| false,
                |_| panic!("stale selection"),
                |_| None,
                publish,
            );
            assert!(stale.is_err());
            assert!(!connections.active(app));
            bridge.bridge_select_model(true, menu.default_model.clone())?;
            bridge.deliver(
                Some(&menu),
                origin,
                false,
                |_, _| Ok(()),
                || Ok(()),
                || Ok(()),
                |_| false,
                |_| {
                    assert!(connections.active(app));
                    Err(FAILED.into())
                },
                |_| None,
                publish,
            )?;
            assert!(connections.active(app));
            assert!(bridge.bridge_status()?.recovery_pending);
            connections.revoke(app); // The gateway has restarted.
            bridge.deliver(
                Some(&menu),
                origin,
                false,
                |_, _| panic!("already published"),
                || Ok(()),
                || Ok(()),
                |_| false,
                |_| panic!("offline"),
                |_| {
                    assert!(connections.active(app));
                    None
                },
                publish,
            )?;
            assert!(connections.active(app));
            assert!(bridge
                .deliver(
                    Some(&menu),
                    origin,
                    false,
                    |_, _| panic!("explicit repair required"),
                    || Ok(()),
                    || Ok(()),
                    |_| false,
                    |_| panic!("conflict"),
                    |_| Some(false),
                    publish
                )
                .is_err());
            assert!(!connections.active(app));
            assert!(bridge
                .deliver(
                    Some(&menu),
                    origin,
                    false,
                    |_, _| panic!("explicit repair required"),
                    || Ok(()),
                    || Ok(()),
                    |_| false,
                    |_| panic!("conflict"),
                    |_| None,
                    publish
                )
                .is_err());
            assert!(!connections.active(app));
            disconnect()?;
        }
        Ok(())
    }
    #[test]
    fn trae_first_catalog_prefers_the_saved_model_and_later_switches_only_select(
    ) -> Result<(), String> {
        use crate::services::trae_bridge::shared::Edition;
        let directory = tempfile::tempdir().map_err(|_| FAILED)?;
        let owner = Manager {
            root: directory.path().into(),
        };
        let mut menu = super::super::tests::fixture_menu();
        let first = menu.default_model.clone().ok_or(FAILED)?;
        let selected = "vary-auto/second";
        let mut second = menu.models[&first].clone();
        second.alias = selected.into();
        second.order = 1;
        menu.models.insert(selected.into(), second);
        menu.groups
            .insert(selected.into(), menu.groups[&first].clone());
        for edition in [Edition::China, Edition::International] {
            let manager = owner.bridge_for(edition);
            manager.bridge_select_model(true, Some(selected.into()))?;
            let origin = "http://127.0.0.1:1";
            manager.bridge_sync_with(
                Some(&menu),
                origin,
                false,
                |provider| {
                    let rows = provider.settings_config["models"]
                        .as_array()
                        .ok_or(FAILED)?;
                    assert_eq!(rows.len(), 2);
                    assert_eq!(rows[0]["model"], selected);
                    assert_eq!(rows[1]["model"], first);
                    Ok(())
                },
                || panic!("catalog is available"),
                |model| {
                    assert_eq!(model, selected);
                    Err(FAILED.into())
                },
                |_| None,
            )?;
            manager.bridge_select_model(true, Some(first.clone()))?;
            manager.bridge_sync_with(
                Some(&menu),
                origin,
                false,
                |_| panic!("a model switch must not rewrite the catalog"),
                || panic!("catalog is available"),
                |model| {
                    assert_eq!(model, first);
                    Ok(())
                },
                |_| Some(true),
            )?;
            assert!(!manager.bridge_load()?.ok_or(FAILED)?.selection_pending);
        }
        Ok(())
    }
    #[test]
    fn both_trae_editions_persist_explicit_replacement_without_forcing_model_selection(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::trae_bridge::shared::Edition;
        let directory = tempfile::tempdir()?;
        let owner = Manager {
            root: directory.path().into(),
        };
        for edition in [Edition::China, Edition::International] {
            let manager = owner.bridge_for(edition);
            manager.bridge_select_model(true, Some("synthetic-model".into()))?;
            let read = || -> Result<serde_json::Value, Box<dyn std::error::Error>> {
                Ok(serde_json::from_slice(&std::fs::read(
                    manager.bridge_path(),
                )?)?)
            };
            assert_eq!(read()?["replace_pending"], true);
            let mut delivered = read()?;
            delivered["replace_pending"] = false.into();
            delivered["delivered"] = true.into();
            std::fs::write(manager.bridge_path(), serde_json::to_vec(&delivered)?)?;
            manager.bridge_select_model(true, Some("synthetic-second".into()))?;
            assert_eq!(read()?["replace_pending"], false);
            manager.bridge_retry()?;
            assert_eq!(read()?["replace_pending"], true);
            assert_eq!(read()?["delivered"], false);
        }
        Ok(())
    }

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
        let offline = manager
            .bridge_sync_with(
                Some(&menu),
                "http://127.0.0.1:1",
                false,
                |_| panic!("an offline client must not trigger configuration replacement"),
                || Ok(()),
                |_| panic!("an offline client cannot acknowledge a selection"),
                |_| None,
            )?
            .ok_or(FAILED)?;
        assert_eq!(offline.token, receipt.token);
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
    fn disconnected_trae_editions_remember_their_model_after_outbox_cleanup() -> Result<(), String>
    {
        use crate::services::trae_bridge::shared::Edition;
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().into(),
        };
        for (edition, model) in [
            (Edition::China, "vary-auto/china-second"),
            (Edition::International, "vary-auto/international-second"),
        ] {
            let bridge = manager.bridge_for(edition);
            bridge.bridge_select_model(true, Some(model.into()))?;
            bridge.bridge_select(false)?;
            bridge.bridge_sync_with(
                None,
                "http://127.0.0.1:1",
                false,
                |_| Ok(()),
                || Ok(()),
                |_| Ok(()),
                |_| Some(true),
            )?;
            let reopened = Manager {
                root: dir.path().into(),
            };
            let bridge = reopened.bridge_for(edition);
            let status = bridge.bridge_status_with_desktop(None, |_, _| None)?;
            assert_eq!(status.model.as_deref(), Some(model));
            assert!(!status.enabled && !status.configured && !status.recovery_pending);
            assert!(!bridge.bridge_owned());
            bridge.bridge_select(true)?;
            assert_eq!(
                bridge.bridge_load()?.ok_or(FAILED)?.model.as_deref(),
                Some(model)
            );
        }
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
        for bridge in [&china, &international] {
            let mut outbox = bridge.bridge_load()?.ok_or(FAILED)?;
            outbox.delivered = true;
            outbox.configuration_changed = true;
            bridge.bridge_save(&outbox)?;
            bridge.bridge_select_model(true, None)?;
            let reapplied = bridge.bridge_load()?.ok_or(FAILED)?;
            assert!(reapplied.enabled);
            assert!(!reapplied.delivered);
            assert!(!reapplied.configuration_changed);
            assert!(reapplied.selection_pending);
        }
        international.bridge_select(false)?;
        assert!(china.bridge_enabled());
        assert!(!international.bridge_enabled());
        Ok(())
    }
}
