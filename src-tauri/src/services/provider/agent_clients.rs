use super::{ProviderService, SwitchResult};
use crate::services::trae_bridge::selection as trae_selection;
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
            let pending = state.db.get_bool_flag(trae_selection::PENDING_KEY)?;
            let projected = crate::services::upstream_protocol::project(
                state.db.as_ref(),
                &AppType::Trae,
                &provider,
            )?;
            let delivered =
                !pending && crate::services::trae_bridge::switch_provider(&projected, &[])?;
            trae_selection::select(&state.db, &provider)?;
            state.db.set_setting(
                trae_selection::PENDING_KEY,
                if delivered { "false" } else { "true" },
            )?;
            if pending {
                trae_selection::sync(&state.db);
            }
        } else {
            state.db.save_provider(app, &provider)?;
        }
        return Ok(true);
    }
    if managed && old.category.as_deref() != Some("official") {
        apply_native_provider(state, app, provider, true)?;
        return Ok(true);
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
    let provider = state
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
        let pending = state.db.get_bool_flag(trae_selection::PENDING_KEY)?;
        let delivered = !pending
            && crate::services::trae_bridge::switch_provider(&target, &projected_previous)?;
        trae_selection::select(&state.db, &provider)?;
        let delivered = if delivered {
            state.db.set_setting(trae_selection::PENDING_KEY, "false")?;
            true
        } else if pending {
            trae_selection::sync(&state.db)
        } else {
            false
        };
        return Ok(SwitchResult {
            warnings: if delivered {
                Vec::new()
            } else {
                vec!["trae_sync_pending".into()]
            },
        });
    }
    apply_native_provider(state, app, provider, false)
}

fn apply_native_provider(
    state: &AppState,
    app: &str,
    mut provider: Provider,
    preserve_selection: bool,
) -> Result<SwitchResult, AppError> {
    let target_id = provider.id.clone();
    let id = target_id.as_str();
    let official = provider.category.as_deref() == Some("official");
    let was_enabled = state.db.get_bool_flag(&native::enabled_key(app))?;
    let backup = futures::executor::block_on(state.db.get_live_backup(app))?
        .map(|saved| serde_json::from_str::<native::Backup>(&saved.original_config))
        .transpose()
        .map_err(|_| AppError::InvalidInput("Invalid route backup".into()))?;
    let origin = backup.as_ref().and_then(|backup| {
        backup.routes.first().map(|route| {
            route
                .local
                .split("/agents/")
                .next()
                .unwrap_or_default()
                .to_string()
        })
    });
    let path = native::config_path(app)?;
    let mut doc = native::Document::read(app, &path)?;
    // Stage restoration, selection and routing without withdrawing the active route on failure.
    if let Some(backup) = &backup {
        if backup.path != path {
            return Err(AppError::InvalidInput(
                "Agent configuration directory changed while routing was active".into(),
            ));
        }
        doc.restore(&backup.routes)?;
    }
    let selection = doc.selection(app);
    let selected_owned = doc.selection_owned(app, None);
    let official_selection = (!selected_owned).then(|| doc.selection(app));
    let mut withdrawn = Vec::new();
    let mut ownership = native::Document::parse(app, &doc.encode()?)?;
    for previous in state.db.get_all_providers(app)?.into_values().filter(|p| {
        p.category.as_deref() != Some("official")
            && ProviderService::provider_live_config_managed(p) == Some(true)
    }) {
        ownership.remove(app, &previous.id, &previous.settings_config)?;
        if previous.id != id {
            withdrawn.push(previous);
        }
    }
    let before_clear = doc.value.clone();
    doc.clear_managed_models(app)?;
    if official && !selected_owned && doc.value == before_clear {
        for mut previous in withdrawn {
            ProviderService::set_provider_live_config_managed(&mut previous, false);
            state.db.save_provider(app, &previous)?;
        }
        ProviderService::set_provider_live_config_managed(&mut provider, false);
        state.db.save_provider(app, &provider)?;
        state.db.set_current_provider(app, id)?;
        return Ok(SwitchResult::default());
    }
    if official {
        if selected_owned {
            let saved = state
                .db
                .get_setting(&format!("{app}_official_model_selection"))?
                .map(|value| serde_json::from_str(&value))
                .transpose()
                .map_err(|_| AppError::InvalidInput("Invalid model selection backup".into()))?
                .unwrap_or_else(|| {
                    if app == "kimi" {
                        doc.value["models"]
                            .as_object()
                            .and_then(|models| {
                                models
                                    .iter()
                                    .find(|(_, model)| {
                                        model["provider"].as_str().is_some_and(|provider| {
                                            doc.value["providers"][provider]["type"] == "kimi"
                                        })
                                    })
                                    .map(|(id, _)| json!(id))
                            })
                            .unwrap_or_default()
                    } else {
                        serde_json::Value::Null
                    }
                });
            doc.set_selection(app, &saved)?;
        }
    } else {
        doc.apply(app, id, &provider.name, &provider.settings_config)?;
        if preserve_selection && doc.selection_available(app, &selection) {
            doc.set_selection(app, &selection)?;
        }
    }
    let next_backup = if let Some(origin) = origin.filter(|_| was_enabled) {
        let added = if app == "zcode" && !official {
            doc.prepare_zcode_official(&native::zcode_builtin()?)?
        } else {
            Vec::new()
        };
        let mut routes = match doc.route(app, &origin) {
            Ok(routes) => routes,
            Err(AppError::Localized { key, .. }) if official && key == "agents.no_models" => {
                Vec::new()
            }
            Err(error) => return Err(error),
        };
        for route in &mut routes {
            if let Some(rule) = route
                .pointer
                .strip_suffix("/config/api/baseUrl")
                .and_then(|path| doc.value.pointer(path))
            {
                route.created_provider = added
                    .iter()
                    .find(|created| created["providerId"] == rule["providerId"])
                    .cloned();
            }
        }
        if routes.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&native::Backup {
                    path: path.clone(),
                    routes,
                })
                .map_err(|_| AppError::InvalidInput("Cannot encode route backup".into()))?,
            )
        }
    } else {
        None
    };
    doc.write(&path)?;
    if let Some(next_backup) = next_backup {
        futures::executor::block_on(state.db.save_live_backup(app, &next_backup))?;
    } else if backup.is_some() {
        futures::executor::block_on(state.db.delete_live_backup(app))?;
        state.db.set_setting(&native::enabled_key(app), "false")?;
    }
    if let Some(selection) = official_selection.filter(|_| !official) {
        state.db.set_setting(
            &format!("{app}_official_model_selection"),
            &selection.to_string(),
        )?;
    }
    for mut previous in withdrawn {
        ProviderService::set_provider_live_config_managed(&mut previous, false);
        state.db.save_provider(app, &previous)?;
    }
    ProviderService::set_provider_live_config_managed(&mut provider, !official);
    state.db.save_provider(app, &provider)?;
    state.db.set_current_provider(app, id)?;
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
    fn rejected_kimi_switch_keeps_existing_routes_and_selection() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "default_model='native'\n[providers.native]\ntype='kimi'\nbase_url='https://native.example/v1'\n[models.native]\nprovider='native'\nmodel='native'\n").expect("config");
        native::with_test_config(&path, || {
            let state = AppState::new(Arc::new(crate::database::Database::memory().expect("db")));
            list(&state, "kimi").expect("official");
            let config = json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-key","protocol":"openai","models":[{"model":"alpha"}]});
            for (id, name) in [("first", "Company"), ("conflict", "native")] {
                add(
                    &state,
                    "kimi",
                    Provider::with_id(id.into(), name.into(), config.clone(), None),
                )
                .expect("add");
            }
            enable(&state, "kimi", "first").expect("first");
            futures::executor::block_on(native::enable(&state.db, "kimi", "http://127.0.0.1:1"))
                .expect("route");
            let before = std::fs::read(&path).expect("before");
            let backup = futures::executor::block_on(state.db.get_live_backup("kimi"))
                .expect("backup")
                .expect("active")
                .original_config;
            assert!(enable(&state, "kimi", "conflict").is_err());
            assert_eq!(
                std::fs::read(&path).expect("after"),
                before,
                "a rejected switch must leave the active route intact"
            );
            assert_eq!(
                futures::executor::block_on(state.db.get_live_backup("kimi"))
                    .expect("backup")
                    .expect("still active")
                    .original_config,
                backup
            );
            assert_eq!(current(&state, "kimi").expect("current"), "first");
        });
    }

    #[test]
    fn kimi_switches_between_saved_suppliers_with_the_same_name() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("config.toml");
        native::with_test_config(&path, || {
            let state = AppState::new(Arc::new(crate::database::Database::memory().expect("db")));
            let config = json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-key","protocol":"openai","models":[{"model":"alpha"}]});
            for id in ["first", "second"] {
                add(
                    &state,
                    "kimi",
                    Provider::with_id(id.into(), "Company".into(), config.clone(), None),
                )
                .expect("add");
            }
            enable(&state, "kimi", "first").expect("first");
            futures::executor::block_on(native::enable(&state.db, "kimi", "http://127.0.0.1:1"))
                .expect("route");
            for id in ["second", "first", "second"] {
                enable(&state, "kimi", id).expect("switch same-name supplier");
                let doc = native::Document::read("kimi", &path).expect("doc");
                assert!(doc.selection_owned("kimi", Some(id)));
                assert_eq!(doc.value["models"].as_object().expect("models").len(), 1);
                assert!(doc.value["providers"]["Company"]["base_url"]
                    .as_str()
                    .expect("route")
                    .starts_with("http://127.0.0.1:1/agents/kimi/"));
            }
        });
    }

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
                let native_rule = doc.value["config"]["providerConfigRules"]["providerRules"]
                    .as_array()
                    .expect("rules")
                    .iter()
                    .find(|rule| rule["providerId"] == "user-custom")
                    .expect("native rule")
                    .clone();
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
            doc.value["config"]["providerConfigRules"]["providerRules"]
                .as_array_mut()
                .expect("rules")
                .iter_mut()
                .find(|rule| rule["providerId"] == "varyai-edited")
                .expect("edited rule")["config"]["access"]["apiKey"] = json!("synthetic-user-edit");
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
    fn switch_replaces_orphaned_managed_models_after_database_reset() {
        for app in ["kimi", "qwen", "zcode"] {
            let dir = tempfile::tempdir().expect("dir");
            let path = dir.path().join(if app == "kimi" {
                "config.toml"
            } else {
                "settings.json"
            });
            let config = json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-old","protocol":"openai","models":[{"model":"alpha"},{"model":"obsolete"}]});
            let mut doc =
                native::Document::parse(app, if app == "kimi" { "" } else { "{}" }).expect("empty");
            doc.apply(app, "lost", "Supplier", &config)
                .expect("old installation");
            doc.value["mcpServers"] = json!({"manual":{"command":"synthetic"}});
            std::fs::write(&path, doc.encode().expect("encode")).expect("native");
            native::with_test_config(&path, || {
                let state = AppState::new(Arc::new(
                    crate::database::Database::memory().expect("fresh database"),
                ));
                let mut target = config.clone();
                target["apiKey"] = json!("synthetic-new");
                target["models"] = json!([{"model":"alpha"}]);
                add(
                    &state,
                    app,
                    Provider::with_id("new".into(), "Supplier".into(), target, None),
                )
                .expect("new supplier");
                enable(&state, app, "new").expect("replace orphaned models");
                let doc = native::Document::read(app, &path).expect("read");
                assert!(!doc.encode().expect("encode").contains("obsolete"));
                assert_eq!(doc.value["mcpServers"]["manual"]["command"], "synthetic");
                assert!(doc.selection_owned(app, Some("new")));
                assert_eq!(current(&state, app).expect("current"), "new");
            });
        }
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

    #[test]
    fn qwen_supplier_copies_replace_overlapping_models_and_credentials() {
        for protocol in ["openai", "openai-responses", "anthropic"] {
            for routed in [false, true] {
                let dir = tempfile::tempdir().expect("dir");
                let path = dir.path().join("settings.json");
                std::fs::write(&path, r#"{"env":{"USER_KEY":"synthetic-user"},"modelProviders":{"openai":[{"id":"manual","envKey":"USER_KEY","baseUrl":"https://user.example/v1"}]},"mcpServers":{"fixture":{"command":"synthetic"}}}"#).expect("fixture");
                native::with_test_config(&path, || {
                    let state =
                        AppState::new(Arc::new(crate::database::Database::memory().expect("db")));
                    let config = json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-original","protocol":protocol,"models":[{"model":"alpha"},{"model":"beta"}]});
                    let mut copy = config.clone();
                    copy["apiKey"] = json!("synthetic-copy");
                    copy["models"] = json!([{"model":"alpha"}]);
                    let mut other = config.clone();
                    other["baseUrl"] = json!("https://other.example/v1");
                    for (id, config) in [
                        ("original", config.clone()),
                        ("copy", copy.clone()),
                        ("other", other),
                    ] {
                        add(
                            &state,
                            "qwen",
                            Provider::with_id(id.into(), id.into(), config, None),
                        )
                        .expect("add");
                    }
                    enable(&state, "qwen", "other").expect("unrelated provider");
                    enable(&state, "qwen", "original").expect("original");
                    if routed {
                        futures::executor::block_on(native::enable(
                            &state.db,
                            "qwen",
                            "http://127.0.0.1:1",
                        ))
                        .expect("route projection");
                    }
                    for (id, previous, expected) in [
                        ("copy", "original", &copy),
                        ("original", "copy", &config),
                        ("copy", "original", &copy),
                    ] {
                        enable(&state, "qwen", id).expect("switch overlapping supplier");
                        let mut doc = native::Document::read("qwen", &path).expect("read");
                        assert!(doc.selection_owned("qwen", Some(id)));
                        if routed {
                            let saved =
                                futures::executor::block_on(state.db.get_live_backup("qwen"))
                                    .expect("backup")
                                    .expect("active backup");
                            let backup: native::Backup =
                                serde_json::from_str(&saved.original_config).expect("decode");
                            doc.restore(&backup.routes)
                                .expect("inspect upstream configuration");
                        }
                        let env = format!("VARYAI_{id}_API_KEY");
                        let previous_env = format!("VARYAI_{previous}_API_KEY");
                        assert_eq!(doc.value["env"][&env], expected["apiKey"]);
                        assert!(doc.value["env"].get(&previous_env).is_none());
                        let entries: Vec<_> = doc.value["modelProviders"]
                            .as_object()
                            .expect("groups")
                            .values()
                            .flat_map(|models| models.as_array().expect("models"))
                            .collect();
                        assert_eq!(
                            entries.iter().filter(|m| m["envKey"] == env).count(),
                            expected["models"]
                                .as_array()
                                .expect("expected models")
                                .len()
                        );
                        assert!(!entries.iter().any(|m| m["envKey"] == previous_env));
                        assert!(entries
                            .iter()
                            .all(|m| m["envKey"] != "VARYAI_other_API_KEY"));
                        assert!(entries.iter().any(|m| m["envKey"] == "USER_KEY"));
                        assert_eq!(doc.value["mcpServers"]["fixture"]["command"], "synthetic");
                        assert_eq!(
                            state
                                .db
                                .get_current_provider("qwen")
                                .expect("current")
                                .as_deref(),
                            Some(id)
                        );
                        for (provider_id, managed) in
                            [(id, true), (previous, false), ("other", false)]
                        {
                            assert_eq!(
                                ProviderService::provider_live_config_managed(
                                    &state
                                        .db
                                        .get_provider_by_id(provider_id, "qwen")
                                        .expect("provider")
                                        .expect("saved provider")
                                ),
                                Some(managed)
                            );
                        }
                    }
                    assert_eq!(
                        state
                            .db
                            .get_provider_by_id("original", "qwen")
                            .expect("db")
                            .expect("original retained")
                            .settings_config,
                        config
                    );
                });
            }
        }
    }

    #[test]
    fn qwen_failed_copy_switch_preserves_native_models_and_current_supplier() {
        for external_edit in [false, true] {
            let dir = tempfile::tempdir().expect("dir");
            let path = dir.path().join("settings.json");
            native::with_test_config(&path, || {
                let state =
                    AppState::new(Arc::new(crate::database::Database::memory().expect("db")));
                let config = json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-original","protocol":"openai","models":[{"model":"alpha"}]});
                let mut copy = config.clone();
                copy["models"] = json!([{"model":"alpha"},{"model":"manual"}]);
                for (id, config) in [("original", config), ("copy", copy)] {
                    add(
                        &state,
                        "qwen",
                        Provider::with_id(id.into(), id.into(), config, None),
                    )
                    .expect("add");
                }
                enable(&state, "qwen", "original").expect("original");
                let mut doc = native::Document::read("qwen", &path).expect("read");
                if external_edit {
                    doc.value["env"]["VARYAI_original_API_KEY"] = json!("synthetic-external-edit");
                } else {
                    doc.value["modelProviders"]["openai"].as_array_mut().expect("models").push(json!({"id":"manual","envKey":"USER_KEY","baseUrl":"https://models.example/v1"}));
                }
                doc.write(&path).expect("external changes");
                let before = std::fs::read(&path).expect("before");
                assert!(enable(&state, "qwen", "copy").is_err());
                assert_eq!(std::fs::read(&path).expect("after"), before);
                assert_eq!(
                    state
                        .db
                        .get_current_provider("qwen")
                        .expect("current")
                        .as_deref(),
                    Some("original")
                );
                assert_eq!(
                    ProviderService::provider_live_config_managed(
                        &state
                            .db
                            .get_provider_by_id("original", "qwen")
                            .expect("db")
                            .expect("provider")
                    ),
                    Some(true)
                );
                assert_eq!(
                    ProviderService::provider_live_config_managed(
                        &state
                            .db
                            .get_provider_by_id("copy", "qwen")
                            .expect("db")
                            .expect("provider")
                    ),
                    Some(false)
                );
            });
        }
    }
}
