use super::{ProviderService, SwitchResult};
use crate::{
    app_config::AppType, error::AppError, provider::Provider, services::agent_clients as native,
    store::AppState,
};
use indexmap::IndexMap;
use serde_json::json;

pub(super) fn list(state: &AppState, app: &str) -> Result<IndexMap<String, Provider>, AppError> {
    let id = format!("{app}-official");
    if !state.db.get_bool_flag(&format!("{app}_official_seeded"))? {
        let label = match app {
            "kimi" => "Kimi Code",
            "qwen" => "Qwen Code",
            "zcode" => "ZCode",
            "trae" => "TRAE CN",
            _ => app,
        };
        let mut p = Provider::with_id(id, format!("{label} Official"), json!({}), None);
        p.category = Some("official".into());
        state.db.save_provider(app, &p)?;
        state
            .db
            .set_setting(&format!("{app}_official_seeded"), "true")?;
    }
    state.db.get_all_providers(app)
}
pub(super) fn current(state: &AppState, app: &str) -> Result<String, AppError> {
    Ok(state
        .db
        .get_current_provider(app)?
        .unwrap_or_else(|| format!("{app}-official")))
}
pub(super) fn add(state: &AppState, app: &str, mut provider: Provider) -> Result<bool, AppError> {
    if provider.category.as_deref() != Some("official") {
        native::validate(&provider.settings_config)?;
    }
    ProviderService::set_provider_live_config_managed(&mut provider, false);
    state.db.save_provider(app, &provider)?;
    Ok(true)
}
pub(super) fn update(
    state: &AppState,
    app: &str,
    mut provider: Provider,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(app));
    if provider.category.as_deref() != Some("official") {
        native::validate(&provider.settings_config)?;
    }
    let old = state
        .db
        .get_provider_by_id(&provider.id, app)?
        .ok_or_else(|| AppError::InvalidInput("Provider not found".into()))?;
    if old.category.as_deref() == Some("official") && provider.category != old.category {
        return Err(AppError::InvalidInput(
            "Official login must remain owned by the client".into(),
        ));
    }
    let managed = ProviderService::provider_live_config_managed(&old) == Some(true);
    ProviderService::set_provider_live_config_managed(&mut provider, managed);
    if app == "trae" {
        if managed && provider.category.as_deref() != Some("official") {
            crate::services::trae_bridge::apply(&crate::services::upstream_protocol::project(
                state.db.as_ref(),
                &AppType::Trae,
                &provider,
            )?)?;
        }
        state.db.save_provider(app, &provider)?;
        return Ok(true);
    }
    if managed && old.category.as_deref() != Some("official") {
        let backup = futures::executor::block_on(state.db.get_live_backup(app))?
            .map(|saved| serde_json::from_str::<native::Backup>(&saved.original_config))
            .transpose()
            .map_err(|_| AppError::InvalidInput("Invalid route backup".into()))?;
        let origin = backup
            .as_ref()
            .and_then(|backup| backup.routes.first())
            .and_then(|route| route.local.split("/agents/").next())
            .map(str::to_owned);
        futures::executor::block_on(native::restore(&state.db, app))?;
        let operation = (|| {
            let path = native::config_path(app)?;
            let mut doc = native::Document::read(app, &path)?;
            // Validate ownership without removing client-added options from the live document.
            let mut ownership = native::Document::parse(app, &doc.encode()?)?;
            ownership.remove(app, &old.id, &old.settings_config)?;
            let selection = doc.selection(app);
            let selected_other = !doc.selection_owned(app, Some(&old.id));
            doc.apply(app, &provider.id, &provider.name, &provider.settings_config)?;
            if selected_other || doc.selection_available(app, &selection) {
                doc.set_selection(app, &selection)?;
            }
            doc.write(&path)
        })();
        if let Some(origin) = origin {
            if let Err(error) = futures::executor::block_on(native::enable(&state.db, app, &origin))
            {
                state.db.set_setting(&native::enabled_key(app), "false")?;
                return Err(error);
            }
        }
        operation?;
    }
    state.db.save_provider(app, &provider)?;
    Ok(true)
}
pub(super) fn enable(state: &AppState, app: &str, id: &str) -> Result<SwitchResult, AppError> {
    enable_with_lock(state, app, id, false)
}
pub(super) fn enable_with_lock(
    state: &AppState,
    app: &str,
    id: &str,
    lock_held: bool,
) -> Result<SwitchResult, AppError> {
    let _guard = if lock_held {
        None
    } else {
        Some(futures::executor::block_on(
            state.proxy_service.lock_switch_for_app(app),
        ))
    };
    let mut provider = state
        .db
        .get_provider_by_id(id, app)?
        .ok_or_else(|| AppError::InvalidInput("Provider not found".into()))?;
    if app == "trae" {
        let official = provider.category.as_deref() == Some("official");
        let previous: Vec<_> = state
            .db
            .get_all_providers(app)?
            .into_values()
            .filter(|p| {
                p.id != id
                    && ProviderService::provider_live_config_managed(p) == Some(true)
                    && p.category.as_deref() != Some("official")
            })
            .collect();
        let target = if official {
            provider.clone()
        } else {
            crate::services::upstream_protocol::project(
                state.db.as_ref(),
                &AppType::Trae,
                &provider,
            )?
        };
        let projected_previous = previous
            .iter()
            .map(|other| {
                crate::services::upstream_protocol::project(
                    state.db.as_ref(),
                    &AppType::Trae,
                    other,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        crate::services::trae_bridge::switch_provider(&target, &projected_previous)?;
        for mut other in previous {
            ProviderService::set_provider_live_config_managed(&mut other, false);
            state.db.save_provider(app, &other)?;
        }
        ProviderService::set_provider_live_config_managed(&mut provider, !official);
        state.db.save_provider(app, &provider)?;
        state.db.set_current_provider(app, id)?;
        return Ok(SwitchResult::default());
    }
    if provider.category.as_deref() == Some("official") {
        if native::supported(app) {
            let path = native::config_path(app)?;
            let mut doc = native::Document::read(app, &path)?;
            let mut withdrawn = Vec::new();
            let mut backup = if app == "zcode" {
                futures::executor::block_on(state.db.get_live_backup(app))?
                    .map(|saved| serde_json::from_str::<native::Backup>(&saved.original_config))
                    .transpose()
                    .map_err(|_| AppError::InvalidInput("Invalid route backup".into()))?
            } else {
                None
            };
            if app == "zcode" {
                for provider in state.db.get_all_providers(app)?.into_values().filter(|p| {
                    p.category.as_deref() != Some("official")
                        && ProviderService::provider_live_config_managed(p) == Some(true)
                }) {
                    let mut expected = provider.settings_config.clone();
                    if let Some(route) = backup
                        .as_ref()
                        .and_then(|b| b.routes.iter().find(|r| r.provider_id == provider.id))
                    {
                        expected["baseUrl"] = json!(route.local);
                    }
                    doc.remove(app, &provider.id, &expected)?;
                    withdrawn.push(provider);
                }
            }
            let mut changed = !withdrawn.is_empty();
            if doc.selection_owned(app, None) {
                let saved = state
                    .db
                    .get_setting(&format!("{app}_official_model_selection"))?;
                if saved.is_some() || app == "zcode" {
                    let selection = saved
                        .map(|saved| serde_json::from_str(&saved))
                        .transpose()
                        .map_err(|_| {
                            AppError::InvalidInput("Invalid model selection backup".into())
                        })?
                        .unwrap_or(serde_json::Value::Null);
                    doc.set_selection(app, &selection)?;
                    changed = true;
                }
            }
            if changed {
                doc.write(&path)?;
            }
            if !withdrawn.is_empty() {
                if let Some(backup) = backup.as_mut() {
                    backup
                        .routes
                        .retain(|route| !withdrawn.iter().any(|p| p.id == route.provider_id));
                    // Preserve active route IDs while model removal shifts array positions.
                    if let Some(rules) =
                        doc.value["config"]["providerConfigRules"]["providerRules"].as_array()
                    {
                        for (index, rule) in rules.iter().enumerate() {
                            for route in &mut backup.routes {
                                if rule.pointer("/config/api/baseUrl") == Some(&json!(route.local))
                                {
                                    route.pointer = format!("/config/providerConfigRules/providerRules/{index}/config/api/baseUrl");
                                }
                            }
                        }
                    }
                    if backup.routes.is_empty() {
                        futures::executor::block_on(state.db.delete_live_backup(app))?;
                        state.db.set_setting(&native::enabled_key(app), "false")?;
                    } else {
                        let encoded = serde_json::to_string(backup).map_err(|_| {
                            AppError::InvalidInput("Cannot encode route backup".into())
                        })?;
                        futures::executor::block_on(state.db.save_live_backup(app, &encoded))?;
                    }
                }
                for mut provider in withdrawn {
                    ProviderService::set_provider_live_config_managed(&mut provider, false);
                    state.db.save_provider(app, &provider)?;
                }
            }
        }
        state.db.set_current_provider(app, id)?;
        return Ok(SwitchResult::default());
    }
    let was_enabled = state.db.get_bool_flag(&native::enabled_key(app))?;
    let backup = futures::executor::block_on(state.db.get_live_backup(app))?;
    let origin = backup
        .as_ref()
        .and_then(|b| serde_json::from_str::<native::Backup>(&b.original_config).ok())
        .and_then(|b| {
            b.routes.first().map(|r| {
                r.local
                    .split("/agents/")
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
        });
    futures::executor::block_on(native::restore(&state.db, app))?;
    let path = native::config_path(app)?;
    let mut doc = native::Document::read(app, &path)?;
    if !doc.selection_owned(app, None) {
        state.db.set_setting(
            &format!("{app}_official_model_selection"),
            &doc.selection(app).to_string(),
        )?;
    }
    doc.apply(app, id, &provider.name, &provider.settings_config)?;
    doc.write(&path)?;
    ProviderService::set_provider_live_config_managed(&mut provider, true);
    state.db.save_provider(app, &provider)?;
    state.db.set_current_provider(app, id)?;
    if was_enabled {
        if let Some(origin) = origin {
            futures::executor::block_on(native::enable(&state.db, app, &origin))?;
        }
    }
    Ok(SwitchResult::default())
}
pub(super) fn remove(state: &AppState, app: &str, id: &str, delete: bool) -> Result<(), AppError> {
    remove_with_lock(state, app, id, delete, false)
}
pub(super) fn remove_with_lock(
    state: &AppState,
    app: &str,
    id: &str,
    delete: bool,
    lock_held: bool,
) -> Result<(), AppError> {
    let _guard = if lock_held {
        None
    } else {
        Some(futures::executor::block_on(
            state.proxy_service.lock_switch_for_app(app),
        ))
    };
    let Some(mut provider) = state.db.get_provider_by_id(id, app)? else {
        return Ok(());
    };
    if provider.category.as_deref() == Some("official") {
        return Err(AppError::localized(
            "agents.keep_official",
            "请保留客户端官方账号入口",
            "Keep the native official-account entry",
        ));
    }
    if app == "trae" {
        if ProviderService::provider_live_config_managed(&provider) == Some(true) {
            crate::services::trae_bridge::remove(id)?;
            ProviderService::set_provider_live_config_managed(&mut provider, false);
            state.db.save_provider(app, &provider)?;
        }
        if state.db.get_current_provider(app)?.as_deref() == Some(id) {
            state.db.set_current_provider(app, "trae-official")?;
        }
        if delete {
            state.db.delete_provider(app, id)?;
        }
        return Ok(());
    }
    if ProviderService::provider_live_config_managed(&provider) == Some(true) {
        let saved = futures::executor::block_on(state.db.get_live_backup(app))?;
        let origin = saved
            .as_ref()
            .and_then(|saved| serde_json::from_str::<native::Backup>(&saved.original_config).ok())
            .and_then(|backup| {
                backup.routes.first().map(|r| {
                    r.local
                        .split("/agents/")
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                })
            });
        let was_enabled = state.db.get_bool_flag(&native::enabled_key(app))?;
        futures::executor::block_on(native::restore(&state.db, app))?;
        let operation = (|| {
            let path = native::config_path(app)?;
            let mut doc = native::Document::read(app, &path)?;
            if doc.selection_owned(app, Some(id)) {
                if let Some(saved) = state
                    .db
                    .get_setting(&format!("{app}_official_model_selection"))?
                {
                    doc.set_selection(
                        app,
                        &serde_json::from_str(&saved).map_err(|_| {
                            AppError::InvalidInput("Invalid model selection backup".into())
                        })?,
                    )?;
                }
            }
            doc.remove(app, id, &provider.settings_config)?;
            doc.write(&path)
        })();
        if was_enabled {
            if let Some(origin) = origin {
                if let Err(error) =
                    futures::executor::block_on(native::enable(&state.db, app, &origin))
                {
                    state.db.set_setting(&native::enabled_key(app), "false")?;
                    return Err(error);
                }
            }
        }
        operation?;
        ProviderService::set_provider_live_config_managed(&mut provider, false);
        state.db.save_provider(app, &provider)?;
    }
    if state.db.get_current_provider(app)?.as_deref() == Some(id) {
        state
            .db
            .set_current_provider(app, &format!("{app}-official"))?;
    }
    if delete {
        state.db.delete_provider(app, id)?;
    }
    Ok(())
}

pub(super) fn is_agent(app: &AppType) -> bool {
    matches!(
        app,
        AppType::Kimi | AppType::Qwen | AppType::ZCode | AppType::Trae
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn zcode_official_switch_withdraws_applied_models_and_preserves_saved_configs() {
        for routed in [false, true] {
            let dir = tempfile::tempdir().expect("dir");
            let path = dir.path().join("provider_config.json");
            let official = json!({"providerId":"account:zai-start-plan","modelId":"glm-test"});
            let original = json!({"schemaVersion":1,"config":{
                "defaultModelSelection":official,
                "providerConfigRules":{"providerRules":[{"providerId":"account:zai-start-plan","config":{"api":{"type":"anthropic-messages","baseUrl":"https://official.example/anthropic"}}}]},
                "modelConfigRules":{"providerModelRules":[],"manualProviderModelRules":[]}
            }});
            std::fs::write(&path, original.to_string()).expect("config");
            native::with_test_config(&path, || {
                let state =
                    AppState::new(Arc::new(crate::database::Database::memory().expect("db")));
                list(&state, "zcode").expect("official");
                for id in ["first", "second", "unused"] {
                    add(&state, "zcode", Provider::with_id(
                        id.into(), id.into(),
                        json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-key","protocol":"openai","models":[{"model":"alpha"}]}), None,
                    )).expect("save");
                    if id != "unused" {
                        enable(&state, "zcode", id).expect("apply");
                    }
                }
                let mut doc = native::Document::read("zcode", &path).expect("read");
                doc.value["config"]["providerConfigRules"]["providerRules"]
                    .as_array_mut().expect("rules")
                    .push(json!({"providerId":"user-custom","config":{"api":{"type":"openai-chat-completions","baseUrl":"https://user.example/v1"},"personalModelIds":["user-model"],"customOption":"keep"}}));
                if routed {
                    let routes = doc.route("zcode", "http://127.0.0.1:1234").expect("route");
                    let backup = native::Backup {
                        path: path.clone(),
                        routes,
                    };
                    futures::executor::block_on(state.db.save_live_backup(
                        "zcode",
                        &serde_json::to_string(&backup).expect("encode"),
                    ))
                    .expect("backup");
                    state
                        .db
                        .set_setting(&native::enabled_key("zcode"), "true")
                        .expect("enabled");
                }
                let native_rule =
                    doc.value["config"]["providerConfigRules"]["providerRules"][3].clone();
                doc.write(&path).expect("write");

                enable(&state, "zcode", "zcode-official").expect("switch official");

                let doc = native::Document::read("zcode", &path).expect("read official");
                let rules = doc.value["config"]["providerConfigRules"]["providerRules"]
                    .as_array()
                    .expect("rules");
                assert_eq!(
                    rules.len(),
                    2,
                    "only official and user-created providers remain"
                );
                assert_eq!(rules[1], native_rule);
                assert_eq!(doc.selection("zcode"), official);
                assert!(
                    doc.value["config"]["modelConfigRules"]["providerModelRules"]
                        .as_array()
                        .expect("models")
                        .is_empty()
                );
                for id in ["first", "second", "unused"] {
                    let saved = state
                        .db
                        .get_provider_by_id(id, "zcode")
                        .expect("lookup")
                        .expect("saved config remains");
                    assert_eq!(saved.settings_config["apiKey"], "synthetic-key");
                    assert_eq!(
                        ProviderService::provider_live_config_managed(&saved),
                        Some(false)
                    );
                }
                if routed {
                    let saved = futures::executor::block_on(state.db.get_live_backup("zcode"))
                        .expect("backup")
                        .expect("audit remains enabled");
                    let backup: native::Backup =
                        serde_json::from_str(&saved.original_config).expect("decode");
                    assert_eq!(backup.routes.len(), 2);
                    assert!(state
                        .db
                        .get_bool_flag(&native::enabled_key("zcode"))
                        .expect("flag"));
                    for route in &backup.routes {
                        assert_eq!(
                            doc.value.pointer(&route.pointer),
                            Some(&json!(route.local)),
                            "remaining route metadata follows its provider"
                        );
                    }
                    assert!(
                        doc.value["config"]["providerConfigRules"]["providerRules"][0]["config"]
                            ["api"]["baseUrl"]
                            .as_str()
                            .expect("official route")
                            .starts_with("http://127.0.0.1:1234/agents/zcode/")
                    );
                    futures::executor::block_on(native::restore(&state.db, "zcode"))
                        .expect("restore");
                    state
                        .db
                        .set_setting(&native::enabled_key("zcode"), "false")
                        .expect("disabled");
                }
                enable(&state, "zcode", "first").expect("apply saved config again");
                let doc = native::Document::read("zcode", &path).expect("read reapplied");
                assert_eq!(doc.selection("zcode")["providerId"], "varyai-first");
                assert_eq!(
                    doc.value["config"]["providerConfigRules"]["providerRules"]
                        .as_array()
                        .expect("rules")
                        .len(),
                    3
                );
            });
        }
    }

    #[test]
    fn zcode_official_switch_refuses_to_remove_externally_edited_models() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("provider_config.json");
        std::fs::write(&path, "{}").expect("config");
        native::with_test_config(&path, || {
            let state = AppState::new(Arc::new(crate::database::Database::memory().expect("db")));
            list(&state, "zcode").expect("official");
            for id in ["first", "edited"] {
                add(&state, "zcode", Provider::with_id(
                    id.into(), id.into(),
                    json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-key","protocol":"openai","models":[{"model":"alpha"}]}), None,
                )).expect("save");
                enable(&state, "zcode", id).expect("apply");
            }
            let mut doc = native::Document::read("zcode", &path).expect("read");
            doc.value["config"]["providerConfigRules"]["providerRules"][1]["config"]["access"]
                ["apiKey"] = json!("synthetic-user-edit");
            doc.write(&path).expect("external edit");
            let before = std::fs::read(&path).expect("before");
            assert!(enable(&state, "zcode", "zcode-official").is_err());
            assert_eq!(
                std::fs::read(&path).expect("after"),
                before,
                "failed switch must not partially remove models"
            );
            assert_eq!(current(&state, "zcode").expect("current"), "edited");
        });
    }

    #[test]
    fn zcode_official_switch_clears_missing_default_without_changing_native_selection() {
        for native_selection in [
            serde_json::Value::Null,
            json!({"providerId":"account:zai-start-plan","modelId":"glm-test"}),
        ] {
            let dir = tempfile::tempdir().expect("dir");
            let path = dir.path().join("provider_config.json");
            std::fs::write(&path, "{}").expect("config");
            native::with_test_config(&path, || {
                let state =
                    AppState::new(Arc::new(crate::database::Database::memory().expect("db")));
                list(&state, "zcode").expect("official");
                let mut provider = Provider::with_id(
                    "saved".into(),
                    "Gateway".into(),
                    json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-key","protocol":"openai","models":[{"model":"alpha"}]}),
                    None,
                );
                ProviderService::set_provider_live_config_managed(&mut provider, true);
                state.db.save_provider("zcode", &provider).expect("save");
                let mut doc = native::Document::read("zcode", &path).expect("read");
                doc.apply(
                    "zcode",
                    &provider.id,
                    &provider.name,
                    &provider.settings_config,
                )
                .expect("existing applied model");
                doc.write(&path).expect("write");
                if !native_selection.is_null() {
                    let mut doc = native::Document::read("zcode", &path).expect("read");
                    doc.set_selection("zcode", &native_selection)
                        .expect("native selection");
                    doc.write(&path).expect("write");
                }
                enable(&state, "zcode", "zcode-official").expect("official");
                let doc = native::Document::read("zcode", &path).expect("read official");
                assert_eq!(doc.selection("zcode"), native_selection);
                assert!(doc.value["config"]["providerConfigRules"]["providerRules"]
                    .as_array()
                    .expect("rules")
                    .is_empty());
            });
        }
    }

    #[test]
    fn editing_applied_provider_updates_native_key_without_changing_selected_model() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "default_model='native'\n[providers.native]\ntype='kimi'\nbase_url='https://api.kimi.com/coding/v1'\n[models.native]\nprovider='native'\nmodel='native'\n").expect("write");
        native::with_test_config(&path, || {
            let state = AppState::new(Arc::new(crate::database::Database::memory().expect("db")));
            list(&state, "kimi").expect("seed");
            let p = Provider::with_id(
                "saved".into(),
                "Test".into(),
                json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-old","protocol":"openai","models":[{"model":"alpha"}]}),
                None,
            );
            add(&state, "kimi", p).expect("add");
            enable(&state, "kimi", "saved").expect("enable");
            let mut doc = native::Document::read("kimi", &path).expect("doc");
            doc.set_selection("kimi", &json!("native"))
                .expect("selection");
            doc.write(&path).expect("write");
            let mut edited = state
                .db
                .get_provider_by_id("saved", "kimi")
                .expect("lookup")
                .expect("exists");
            edited.settings_config["apiKey"] = json!("synthetic-new");
            update(&state, "kimi", edited).expect("update");
            let doc = native::Document::read("kimi", &path).expect("doc");
            assert_eq!(doc.value["providers"]["Test"]["api_key"], "synthetic-new");
            assert_eq!(doc.value["default_model"], "native");
        });
    }

    #[test]
    fn applied_models_can_be_removed_without_deleting_native_login() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path,"default_model='native'\n[providers.native]\ntype='kimi'\nbase_url='https://api.kimi.com/coding/v1'\n[models.native]\nprovider='native'\nmodel='native'\n").expect("config");
        native::with_test_config(&path, || {
            let state = AppState::new(Arc::new(crate::database::Database::memory().expect("db")));
            list(&state, "kimi").expect("official");
            let provider = Provider::with_id(
                "saved".into(),
                "Test".into(),
                json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-key","protocol":"openai","models":[{"model":"alpha"}]}),
                None,
            );
            add(&state, "kimi", provider).expect("add");
            enable(&state, "kimi", "saved").expect("apply");
            remove(&state, "kimi", "saved", true).expect("remove applied");
            let doc = native::Document::read("kimi", &path).expect("read");
            assert!(doc.value["providers"].get("varyai-saved").is_none());
            assert_eq!(doc.value["default_model"], "native");
            assert_eq!(
                doc.value["providers"]["native"]["base_url"],
                "https://api.kimi.com/coding/v1"
            );
            assert!(state
                .db
                .get_provider_by_id("saved", "kimi")
                .expect("db")
                .is_none());
        });
    }
}
