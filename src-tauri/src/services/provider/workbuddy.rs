use super::{ProviderService, SwitchResult};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::store::AppState;
use indexmap::IndexMap;
use serde_json::Value;

const APP: &str = "workbuddy";
const OFFICIAL_ID: &str = "workbuddy-official";

pub(super) fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    if !state.db.get_bool_flag("workbuddy_official_seeded")? {
        if state.db.get_provider_by_id(OFFICIAL_ID, APP)?.is_none() {
            let mut official = Provider::with_id(
                OFFICIAL_ID.into(),
                "WorkBuddy Official".into(),
                serde_json::json!({}),
                Some("https://www.workbuddy.cn".into()),
            );
            official.category = Some("official".into());
            state.db.save_provider(APP, &official)?;
        }
        state.db.set_setting("workbuddy_official_seeded", "true")?;
    }
    state.db.get_all_providers(APP)
}

pub(super) fn current(state: &AppState) -> Result<String, AppError> {
    if let Some(id) = state.db.get_current_provider(APP)? {
        return Ok(id);
    }
    Ok(
        if state.db.get_provider_by_id(OFFICIAL_ID, APP)?.is_some() {
            OFFICIAL_ID.into()
        } else {
            String::new()
        },
    )
}

fn models(provider: &Provider) -> Result<Vec<Value>, AppError> {
    crate::workbuddy_config::configured_models(&provider.settings_config)
}

fn check_unique(state: &AppState, provider: &Provider) -> Result<(), AppError> {
    let incoming = models(provider)?;
    for saved in state.db.get_all_providers(APP)?.values().filter(|saved| {
        saved.id != provider.id
            && saved.category.as_deref() != Some("official")
            && ProviderService::provider_live_config_managed(saved) != Some(false)
    }) {
        if models(saved)?.iter().any(|model| {
            incoming
                .iter()
                .any(|incoming| incoming["id"] == model["id"])
        }) {
            return Err(AppError::localized(
                "workbuddy.duplicate_model",
                "该模型 ID 已有 WorkBuddy 配置，请编辑已有配置",
                "This WorkBuddy model ID already has a configuration; edit the existing one",
            ));
        }
    }
    Ok(())
}

fn active_backup(
    state: &AppState,
) -> Result<Option<crate::services::workbuddy_proxy::Backup>, AppError> {
    futures::executor::block_on(state.db.get_live_backup(APP))?
        .map(|saved| {
            serde_json::from_str(&saved.original_config)
                .map_err(|_| AppError::Config("WorkBuddy recovery backup is invalid".into()))
        })
        .transpose()
}

fn live_models(state: &AppState, provider: &Provider) -> Result<Vec<Value>, AppError> {
    let mut models = models(provider)?;
    if let Some(backup) = active_backup(state)? {
        let local = backup.model_url(&provider.id).map_err(AppError::Config)?;
        for model in &mut models {
            model["url"] = Value::String(local.clone());
        }
    }
    Ok(models)
}

// Local routing addresses belong only in the client file, never in saved upstream settings.
fn native_models(state: &AppState) -> Result<Vec<Value>, AppError> {
    let mut native = crate::workbuddy_config::read_models()?;
    if let Some(backup) = active_backup(state)? {
        for provider in state
            .db
            .get_all_providers(APP)?
            .values()
            .filter(|p| p.category.as_deref() != Some("official"))
        {
            let local = backup.model_url(&provider.id).map_err(AppError::Config)?;
            for saved in models(provider)? {
                for model in &mut native {
                    if model["id"] == saved["id"] && model["url"].as_str() == Some(&local) {
                        model["url"] = saved["url"].clone();
                    }
                }
            }
        }
    }
    Ok(native)
}

pub(super) fn add(
    state: &AppState,
    provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    ProviderService::validate_provider_settings(&AppType::WorkBuddy, &provider)?;
    if state.db.get_provider_by_id(&provider.id, APP)?.is_some() {
        return Err(AppError::InvalidInput(
            "WorkBuddy configuration already exists".into(),
        ));
    }
    if provider.category.as_deref() == Some("official") {
        state.db.save_provider(APP, &provider)?;
        return Ok(true);
    }
    check_unique(state, &provider)?;
    if native_models(state)?.iter().any(|native| {
        models(&provider)
            .map(|models| models.iter().any(|model| model["id"] == native["id"]))
            .unwrap_or(false)
    }) {
        return Err(AppError::localized(
            "workbuddy.duplicate_model",
            "WorkBuddy 中已有该模型 ID，请先导入后编辑",
            "This model ID already exists in WorkBuddy; import it before editing",
        ));
    }
    if add_to_live {
        crate::workbuddy_config::change_models(&[], &live_models(state, &provider)?, || {
            state.db.save_provider(APP, &provider)?;
            Ok(true)
        })
    } else {
        state.db.save_provider(APP, &provider)?;
        Ok(true)
    }
}

pub(super) fn update(
    state: &AppState,
    original_id: Option<&str>,
    provider: Provider,
) -> Result<bool, AppError> {
    update_with_lock(state, original_id, provider, false)
}

pub(super) fn update_with_lock(
    state: &AppState,
    original_id: Option<&str>,
    mut provider: Provider,
    lock_held: bool,
) -> Result<bool, AppError> {
    let _guard = if lock_held {
        None
    } else {
        Some(futures::executor::block_on(
            state.proxy_service.lock_switch_for_app(APP),
        ))
    };
    let original_id = original_id.unwrap_or(&provider.id);
    if original_id != provider.id {
        return Err(AppError::InvalidInput(
            "WorkBuddy configuration keys cannot be renamed".into(),
        ));
    }
    let previous = state
        .db
        .get_provider_by_id(original_id, APP)?
        .ok_or_else(|| AppError::InvalidInput("WorkBuddy configuration not found".into()))?;
    ProviderService::validate_provider_settings(&AppType::WorkBuddy, &provider)?;
    if previous.category.as_deref() == Some("official") {
        if provider.category.as_deref() != Some("official") {
            return Err(AppError::InvalidInput(
                "Cannot change WorkBuddy official configuration type".into(),
            ));
        }
        state.db.save_provider(APP, &provider)?;
        return Ok(true);
    }
    if ProviderService::provider_live_config_managed(&previous) == Some(false) {
        ProviderService::set_provider_live_config_managed(&mut provider, false);
        state.db.save_provider(APP, &provider)?;
        return Ok(true);
    }
    if ProviderService::provider_live_config_managed(&previous) == Some(true) {
        ProviderService::set_provider_live_config_managed(&mut provider, true);
    }
    check_unique(state, &provider)?;
    let previous_models = models(&previous)?;
    let native = native_models(state)?;
    let old_ids: Vec<String> = native
        .iter()
        .filter(|model| previous_models.iter().any(|old| old["id"] == model["id"]))
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect();
    if old_ids.is_empty() {
        state.db.save_provider(APP, &provider)?;
        return Ok(true);
    }
    let mut replacements = live_models(state, &provider)?;
    for (index, model) in replacements.iter_mut().enumerate() {
        let existing = native
            .iter()
            .find(|old| old["id"] == model["id"])
            .or_else(|| {
                previous_models
                    .get(index)
                    .and_then(|previous| native.iter().find(|old| old["id"] == previous["id"]))
            });
        if let Some(existing) = existing {
            let mut merged = existing.clone();
            if let (Some(target), Some(source)) = (merged.as_object_mut(), model.as_object()) {
                target.extend(source.clone());
                for key in ["maxInputTokens", "maxOutputTokens"] {
                    if !source.contains_key(key) {
                        target.remove(key);
                    }
                }
            }
            *model = merged;
        }
    }
    // Retain client-added fields in the saved upstream representation too.
    let mut synced = provider.clone();
    let mut upstream = replacements.clone();
    let saved = models(&provider)?;
    for model in &mut upstream {
        if let Some(original) = saved.iter().find(|saved| saved["id"] == model["id"]) {
            model["url"] = original["url"].clone();
        }
    }
    if provider.settings_config.get("models").is_some() {
        synced.settings_config["models"] = Value::Array(
            upstream
                .into_iter()
                .map(|mut model| {
                    if let Some(fields) = model.as_object_mut() {
                        fields.retain(|key, value| {
                            key == "id"
                                || key == "name"
                                || (key != "url"
                                    && key != "apiKey"
                                    && provider.settings_config.get(key) != Some(value))
                        });
                    }
                    model
                })
                .collect(),
        );
    } else if let Some(model) = upstream.into_iter().next() {
        synced.settings_config = model;
    }
    crate::workbuddy_config::change_models(&old_ids, &replacements, || {
        state.db.save_provider(APP, &synced)?;
        Ok(true)
    })
}

pub(super) fn remove(state: &AppState, id: &str, delete: bool) -> Result<(), AppError> {
    remove_with_lock(state, id, delete, false)
}
pub(super) fn remove_with_lock(
    state: &AppState,
    id: &str,
    delete: bool,
    lock_held: bool,
) -> Result<(), AppError> {
    let _guard = if lock_held {
        None
    } else {
        Some(futures::executor::block_on(
            state.proxy_service.lock_switch_for_app(APP),
        ))
    };
    let Some(provider) = state.db.get_provider_by_id(id, APP)? else {
        return Ok(());
    };
    if current(state)? == id {
        return Err(AppError::localized(
            "workbuddy.current_in_use",
            "请先切换到其他配置，再移除当前 WorkBuddy 配置",
            "Switch WorkBuddy to another provider before removing the current configuration",
        ));
    }
    if provider.category.as_deref() == Some("official") {
        if delete {
            state.db.delete_provider(APP, id)?;
        }
        return Ok(());
    }
    if ProviderService::provider_live_config_managed(&provider) == Some(false) {
        if delete {
            state.db.delete_provider(APP, id)?;
        }
        return Ok(());
    }
    let saved = models(&provider)?;
    let native = native_models(state)?;
    let old_ids: Vec<String> = native
        .iter()
        .filter(|model| saved.iter().any(|saved| saved["id"] == model["id"]))
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect();
    crate::workbuddy_config::change_models(&old_ids, &[], || {
        if delete {
            state.db.delete_provider(APP, id)?;
        }
        Ok(())
    })
}

pub(super) fn enable(state: &AppState, id: &str) -> Result<SwitchResult, AppError> {
    enable_with_lock(state, id, false)
}
pub(super) fn enable_with_lock(
    state: &AppState,
    id: &str,
    lock_held: bool,
) -> Result<SwitchResult, AppError> {
    let _guard = if lock_held {
        None
    } else {
        Some(futures::executor::block_on(
            state.proxy_service.lock_switch_for_app(APP),
        ))
    };
    enable_model(state, id)
}

fn enable_model(state: &AppState, id: &str) -> Result<SwitchResult, AppError> {
    let provider = state
        .db
        .get_provider_by_id(id, APP)?
        .ok_or_else(|| AppError::InvalidInput("WorkBuddy configuration not found".into()))?;
    ProviderService::validate_provider_settings(&AppType::WorkBuddy, &provider)?;
    if provider.category.as_deref() == Some("official") {
        state.db.set_current_provider(APP, id)?;
        return Ok(SwitchResult::default());
    }
    let desired = models(&provider)?;
    let native = native_models(state)?;
    let saved = state.db.get_all_providers(APP)?;
    let mut old_ids = Vec::new();
    for model in native
        .iter()
        .filter(|model| desired.iter().any(|new| new["id"] == model["id"]))
    {
        let managed = saved
            .values()
            .filter(|saved| {
                saved.category.as_deref() != Some("official")
                    && ProviderService::provider_live_config_managed(saved) != Some(false)
            })
            .any(|saved| {
                models(saved)
                    .map(|models| models.iter().any(|saved| saved == model))
                    .unwrap_or(false)
            });
        if !desired.iter().any(|new| new == model) && !managed {
            return Err(AppError::localized("workbuddy.duplicate_model", "WorkBuddy 中已有不同内容的同名模型，请先处理冲突配置", "WorkBuddy already has a different configuration for this model ID; resolve the conflicting configuration first"));
        }
        old_ids.push(
            model["id"]
                .as_str()
                .ok_or_else(|| AppError::InvalidInput("Missing WorkBuddy model ID".into()))?
                .to_string(),
        );
    }
    // Replacing an overlapping group also removes its other owned models.
    for saved in saved.values().filter(|saved| {
        saved.id != provider.id
            && saved.category.as_deref() != Some("official")
            && ProviderService::provider_live_config_managed(saved) != Some(false)
    }) {
        let owned = models(saved)?;
        if owned
            .iter()
            .any(|model| desired.iter().any(|new| new["id"] == model["id"]))
        {
            for model in native.iter().filter(|model| owned.contains(model)) {
                if let Some(id) = model["id"].as_str() {
                    if !old_ids.iter().any(|old| old == id) {
                        old_ids.push(id.to_string());
                    }
                }
            }
        }
    }
    let ids: Vec<String> = desired
        .iter()
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect();
    crate::workbuddy_config::change_models(&old_ids, &live_models(state, &provider)?, || {
        state.db.activate_workbuddy_models(&provider.id, &ids)?;
        Ok(SwitchResult::default())
    })
}

pub(super) fn import(state: &AppState) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let saved = state.db.get_all_providers(APP)?;
    let mut imported = false;
    for model in native_models(state)? {
        if crate::workbuddy_config::validate_model(&model).is_err() {
            log::warn!("Skipping an incomplete WorkBuddy native model");
            continue;
        }
        let Some(native_id) = model["id"].as_str() else {
            continue;
        };
        let mut provider = match saved.values().find(|provider| {
            ProviderService::provider_live_config_managed(provider) != Some(false)
                && provider.category.as_deref() != Some("official")
                && models(provider)
                    .map(|models| {
                        models
                            .iter()
                            .any(|model| model["id"].as_str() == Some(native_id))
                    })
                    .unwrap_or(false)
        }) {
            Some(provider) => provider.clone(),
            None => Provider::with_id(
                format!("workbuddy-{native_id}"),
                model["name"].as_str().unwrap_or(native_id).to_string(),
                model.clone(),
                None,
            ),
        };
        if provider.settings_config.get("models").is_some() {
            imported = true;
            continue;
        }
        provider.settings_config = model;
        state.db.save_provider(APP, &provider)?;
        imported = true;
    }
    if let Some(backup) = active_backup(state)? {
        backup
            .project_models(&state.db, true)
            .map_err(AppError::Config)?;
    }
    Ok(imported)
}

pub(crate) fn live_ids(state: &AppState) -> Result<Vec<String>, AppError> {
    let models = crate::workbuddy_config::visible_model_ids()?;
    Ok(state
        .db
        .get_all_providers(APP)?
        .values()
        .filter(|provider| {
            if ProviderService::provider_live_config_managed(provider) == Some(false) {
                return false;
            }
            if provider.category.as_deref() == Some("official") {
                return false;
            }
            self::models(provider)
                .map(|configured| {
                    configured.iter().all(|model| {
                        models
                            .iter()
                            .any(|id| model["id"].as_str() == Some(id.as_str()))
                    })
                })
                .unwrap_or(false)
        })
        .map(|provider| provider.id.clone())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::workbuddy_config::{get_workbuddy_config_path, read_models, with_test_dir};
    use serde_json::json;
    use std::sync::Arc;

    fn provider(id: &str, model_id: &str) -> Provider {
        Provider::with_id(
            id.into(),
            "WorkBuddy model".into(),
            json!({
                "id":model_id,"name":"WorkBuddy model","url":"https://example.com/v1/chat/completions","apiKey":"test-key"
            }),
            None,
        )
    }

    #[test]
    fn workbuddy_multi_model_lifecycle_preserves_group_and_restores_every_endpoint() {
        let dir = tempfile::tempdir().expect("dir");
        with_test_dir(dir.path(), || {
            let state = AppState::new(Arc::new(Database::memory().expect("db")));
            std::fs::write(
                dir.path().join("settings.json"),
                r#"{"env":{"CODEBUDDY_BASE_URL":"https://official.example/v2"}}"#,
            )
            .expect("settings");
            let backup = crate::services::workbuddy_proxy::Backup::prepare(
                dir.path(),
                "http://127.0.0.1:19999/workbuddy/v1",
            )
            .expect("backup");
            futures::executor::block_on(
                state
                    .db
                    .save_live_backup(APP, &serde_json::to_string(&backup).expect("json")),
            )
            .expect("save");
            let mut group = provider("group", "model-a");
            group.settings_config["models"] =
                json!([{"id":"model-a","name":"A"},{"id":"model-b","name":"B","custom":true}]);
            add(&state, group.clone(), false).expect("save only");
            assert!(read_models().expect("models").is_empty());
            enable(&state, &group.id).expect("apply group");
            let models = read_models().expect("models");
            assert_eq!(models.len(), 2);
            assert!(models
                .iter()
                .all(|m| m["url"] == backup.model_url(&group.id).expect("url")
                    && m.get("models").is_none()));
            import(&state).expect("import preserves group");
            assert_eq!(state.db.get_all_providers(APP).expect("db").len(), 1);
            backup.project_models(&state.db, false).expect("restore");
            assert!(read_models()
                .expect("models")
                .iter()
                .all(|m| m["url"] == group.settings_config["url"]));
            backup.project_models(&state.db, true).expect("project");
            let copy = ProviderService::duplicate(&state, AppType::WorkBuddy, &group.id, "Copy")
                .expect("copy");
            enable(&state, &copy.id).expect("switch copy");
            assert!(read_models()
                .expect("models")
                .iter()
                .all(|m| m["url"] == backup.model_url(&copy.id).expect("copy URL")));
            let mut edited = copy.clone();
            edited.settings_config["models"] =
                json!([{"id":"model-a","name":"A"},{"id":"model-c","name":"C"}]);
            update(&state, None, edited).expect("replace model list");
            assert_eq!(
                read_models()
                    .expect("models")
                    .iter()
                    .map(|m| m["id"].as_str().expect("id").to_string())
                    .collect::<Vec<_>>(),
                vec!["model-a", "model-c"]
            );
            state
                .db
                .set_current_provider(APP, "")
                .expect("clear current");
            remove(&state, &copy.id, true).expect("delete group");
            assert!(read_models().expect("models").is_empty());
        });
    }

    #[test]
    fn workbuddy_managed_native_edits_and_imports_keep_real_upstreams_in_database() {
        let dir = tempfile::tempdir().expect("dir");
        with_test_dir(dir.path(), || {
            let state = AppState::new(Arc::new(Database::memory().expect("db")));
            std::fs::write(
                dir.path().join("settings.json"),
                r#"{"env":{"CODEBUDDY_BASE_URL":"https://official.example/v2"}}"#,
            )
            .expect("settings");
            let backup = crate::services::workbuddy_proxy::Backup::prepare(
                dir.path(),
                "http://127.0.0.1:19999/workbuddy/v1",
            )
            .expect("backup");
            futures::executor::block_on(
                state
                    .db
                    .save_live_backup(APP, &serde_json::to_string(&backup).expect("JSON")),
            )
            .expect("save backup");
            let custom = provider("api-provider", "model-a");
            add(&state, custom.clone(), true).expect("add to native");
            let local = backup.model_url(&custom.id).expect("local url");
            assert_eq!(read_models().expect("models")[0]["url"], local);
            enable(&state, &custom.id).expect("enable existing local model");
            import(&state).expect("import projected model");
            assert_eq!(
                state
                    .db
                    .get_provider_by_id(&custom.id, APP)
                    .expect("db")
                    .expect("saved")
                    .settings_config,
                custom.settings_config
            );
            let mut edited = custom.clone();
            edited.settings_config["id"] = json!("model-b");
            edited.settings_config["url"] = json!("https://new.example/v1/chat/completions");
            edited.settings_config["apiKey"] = json!("synthetic-new-key");
            update(&state, None, edited.clone()).expect("edit live model");
            let native = read_models().expect("models");
            assert_eq!(native[0]["id"], "model-b");
            assert_eq!(native[0]["url"], local);
            assert_eq!(native[0]["apiKey"], "synthetic-new-key");
            import(&state).expect("import edited model");
            assert_eq!(
                state
                    .db
                    .get_provider_by_id(&custom.id, APP)
                    .expect("db")
                    .expect("saved")
                    .settings_config,
                edited.settings_config
            );
            state
                .db
                .set_current_provider(APP, "other")
                .expect("current");
            remove(&state, &custom.id, false).expect("remove from live");
            assert!(read_models().expect("models").is_empty());
            assert_eq!(
                state
                    .db
                    .get_provider_by_id(&custom.id, APP)
                    .expect("db")
                    .expect("saved")
                    .settings_config["url"],
                edited.settings_config["url"]
            );
            enable(&state, &custom.id).expect("enable again");
            backup
                .project_models(&state.db, false)
                .expect("restore URLs");
            assert_eq!(read_models().expect("models")[0], edited.settings_config);
        });
    }

    #[test]
    fn workbuddy_copy_stays_detached_until_selected_and_switches_same_model_safely() {
        let dir = tempfile::tempdir().expect("dir");
        with_test_dir(dir.path(), || {
            let state = AppState::new(Arc::new(Database::memory().expect("db")));
            std::fs::write(
                dir.path().join("settings.json"),
                r#"{"env":{"CODEBUDDY_BASE_URL":"https://official.example/v2"}}"#,
            )
            .expect("settings");
            let backup = crate::services::workbuddy_proxy::Backup::prepare(
                dir.path(),
                "http://127.0.0.1:19999/workbuddy/v1",
            )
            .expect("backup");
            futures::executor::block_on(
                state
                    .db
                    .save_live_backup(APP, &serde_json::to_string(&backup).expect("JSON")),
            )
            .expect("backup");
            let source = provider("source", "same-model");
            add(&state, source.clone(), true).expect("add");
            enable(&state, &source.id).expect("enable");
            let path = get_workbuddy_config_path();
            let original = std::fs::read(&path).expect("models");
            let settings = std::fs::read(dir.path().join("settings.json")).expect("settings");
            let mut copy =
                ProviderService::duplicate(&state, AppType::WorkBuddy, &source.id, "Copy")
                    .expect("duplicate");
            assert_eq!(std::fs::read(&path).expect("models"), original);
            assert_eq!(
                std::fs::read(dir.path().join("settings.json")).expect("settings"),
                settings
            );
            assert_eq!(
                live_ids(&state).expect("membership"),
                vec![source.id.clone()]
            );
            copy.settings_config["url"] = json!("https://copy.example/v1/chat/completions");
            copy.settings_config["apiKey"] = json!("synthetic-copy-key");
            update(&state, None, copy.clone()).expect("edit detached copy");
            assert_eq!(std::fs::read(&path).expect("models"), original);
            backup.project_models(&state.db, true).expect("project");
            assert_eq!(
                read_models().expect("models")[0]["url"],
                backup.model_url(&source.id).expect("source route")
            );
            import(&state).expect("import");
            assert_eq!(
                state
                    .db
                    .get_provider_by_id(&copy.id, APP)
                    .expect("db")
                    .expect("copy")
                    .settings_config,
                copy.settings_config
            );
            enable(&state, &copy.id).expect("apply copy");
            assert_eq!(read_models().expect("models").len(), 1);
            assert_eq!(live_ids(&state).expect("membership"), vec![copy.id.clone()]);
            assert_eq!(
                read_models().expect("models")[0]["url"],
                backup.model_url(&copy.id).expect("copy route")
            );
            backup.project_models(&state.db, false).expect("restore");
            assert_eq!(
                read_models().expect("models")[0]["url"],
                copy.settings_config["url"]
            );
            backup
                .project_models(&state.db, true)
                .expect("restart routing");
            assert_eq!(
                read_models().expect("models")[0]["url"],
                backup.model_url(&copy.id).expect("copy route")
            );
            enable(&state, &source.id).expect("switch back");
            let applied = std::fs::read(&path).expect("models");
            remove(&state, &copy.id, true).expect("delete detached copy");
            assert_eq!(std::fs::read(&path).expect("models"), applied);
            assert!(state
                .db
                .get_provider_by_id(&copy.id, APP)
                .expect("db")
                .is_none());
            backup
                .project_models(&state.db, false)
                .expect("restore original");
            assert_eq!(read_models().expect("models")[0], source.settings_config);
        });
    }

    #[test]
    fn workbuddy_switches_between_default_official_and_saved_api_configuration() {
        let dir = tempfile::tempdir().expect("temp dir");
        with_test_dir(dir.path(), || {
            let state = AppState::new(Arc::new(Database::memory().expect("database")));
            let saved = ProviderService::list(&state, AppType::WorkBuddy).expect("list");
            let official = saved
                .values()
                .find(|p| p.category.as_deref() == Some("official"))
                .expect("official seed");
            let official_id = official.id.clone();
            assert_eq!(
                ProviderService::current(&state, AppType::WorkBuddy).expect("current"),
                official_id
            );
            let custom = provider("api-provider", "custom-model");
            ProviderService::add(&state, AppType::WorkBuddy, custom.clone(), false).expect("add");
            ProviderService::switch(&state, AppType::WorkBuddy, &custom.id).expect("switch API");
            assert_eq!(
                ProviderService::current(&state, AppType::WorkBuddy).expect("current"),
                custom.id
            );
            assert!(ProviderService::delete(&state, AppType::WorkBuddy, &custom.id).is_err());
            ProviderService::switch(&state, AppType::WorkBuddy, &official_id)
                .expect("switch official");
            assert_eq!(
                ProviderService::current(&state, AppType::WorkBuddy).expect("current"),
                official_id
            );
            assert_eq!(
                ProviderService::list(&state, AppType::WorkBuddy)
                    .expect("list twice")
                    .len(),
                2
            );
            let mut edited = official.clone();
            edited.name = "My official account".into();
            ProviderService::update(&state, AppType::WorkBuddy, None, edited)
                .expect("edit official");
            assert_eq!(
                ProviderService::list(&state, AppType::WorkBuddy).expect("list")[&official_id].name,
                "My official account"
            );
            assert_eq!(read_models().expect("native models").len(), 1);
        });
    }

    #[test]
    fn workbuddy_native_lifecycle_preserves_unrelated_models_and_client_fields() {
        let dir = tempfile::tempdir().expect("temp dir");
        with_test_dir(dir.path(), || {
            let state = AppState::new(Arc::new(Database::memory().expect("database")));
            let path = get_workbuddy_config_path();
            let unrelated = provider("unrelated", "keep").settings_config;
            std::fs::write(&path, json!({"models":[unrelated.clone()],"customSetting":true,"availableModels":["keep"]}).to_string()).expect("seed");
            let input = provider("saved-model", "model-a");
            ProviderService::add(&state, AppType::WorkBuddy, input.clone(), false).expect("save");
            assert!(live_ids(&state).expect("membership").is_empty());
            ProviderService::switch(&state, AppType::WorkBuddy, &input.id).expect("enable");
            assert_eq!(
                live_ids(&state).expect("membership"),
                vec![input.id.clone()]
            );
            assert_eq!(
                ProviderService::current(&state, AppType::WorkBuddy).expect("current"),
                input.id
            );
            let mut document: Value =
                serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
            document["models"][1]["relatedModels"] = json!({"lite":"keep"});
            std::fs::write(&path, document.to_string()).expect("native edit");
            let mut edited = input.clone();
            edited.settings_config["id"] = json!("model-b");
            ProviderService::update(&state, AppType::WorkBuddy, None, edited).expect("edit");
            assert_eq!(
                read_models().expect("models")[1]["relatedModels"],
                json!({"lite":"keep"})
            );
            ProviderService::list(&state, AppType::WorkBuddy).expect("seed official");
            ProviderService::switch(&state, AppType::WorkBuddy, OFFICIAL_ID)
                .expect("official before removal");
            ProviderService::remove_from_live_config(&state, AppType::WorkBuddy, &input.id)
                .expect("remove");
            assert_eq!(read_models().expect("models"), vec![unrelated.clone()]);
            assert!(state
                .db
                .get_provider_by_id(&input.id, APP)
                .expect("saved")
                .is_some());
            ProviderService::switch(&state, AppType::WorkBuddy, &input.id).expect("reenable");
            assert_eq!(read_models().expect("models")[1]["id"], "model-b");
            ProviderService::switch(&state, AppType::WorkBuddy, OFFICIAL_ID)
                .expect("official before delete");
            ProviderService::delete(&state, AppType::WorkBuddy, &input.id).expect("delete");
            assert!(state
                .db
                .get_provider_by_id(&input.id, APP)
                .expect("deleted")
                .is_none());
            let final_document: Value =
                serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
            assert_eq!(final_document["models"], json!([unrelated]));
            assert_eq!(final_document["customSetting"], true);
        });
    }

    #[test]
    fn workbuddy_import_and_enable_respect_the_clients_model_filter() {
        let dir = tempfile::tempdir().expect("temp dir");
        with_test_dir(dir.path(), || {
            let state = AppState::new(Arc::new(Database::memory().expect("database")));
            let model = provider("unused", "native-model").settings_config;
            std::fs::write(
                get_workbuddy_config_path(),
                json!({"models":[model],"availableModels":["built-in"]}).to_string(),
            )
            .expect("seed");
            assert!(import(&state).expect("import"));
            let saved = ProviderService::list(&state, AppType::WorkBuddy).expect("list");
            assert_eq!(saved.len(), 2);
            assert!(live_ids(&state).expect("membership").is_empty());
            let id = saved
                .keys()
                .find(|id| id.as_str() != OFFICIAL_ID)
                .expect("imported ID");
            ProviderService::switch(&state, AppType::WorkBuddy, id).expect("enable filtered model");
            assert_eq!(live_ids(&state).expect("membership"), vec![id.clone()]);
            assert_eq!(read_models().expect("models").len(), 1);
            assert!(import(&state).expect("repeat import"));
            assert_eq!(
                ProviderService::list(&state, AppType::WorkBuddy)
                    .expect("list")
                    .len(),
                2
            );
        });
    }

    #[test]
    fn workbuddy_rejects_conflicting_model_ids_without_saving_or_overwriting() {
        let dir = tempfile::tempdir().expect("temp dir");
        with_test_dir(dir.path(), || {
            let state = AppState::new(Arc::new(Database::memory().expect("database")));
            let original =
                json!({"models":[provider("native", "same-model").settings_config]}).to_string();
            let path = get_workbuddy_config_path();
            std::fs::write(&path, &original).expect("seed");
            let input = provider("saved", "same-model");
            assert!(ProviderService::add(&state, AppType::WorkBuddy, input.clone(), true).is_err());
            assert!(state
                .db
                .get_provider_by_id(&input.id, APP)
                .expect("saved")
                .is_none());
            assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
            assert!(
                ProviderService::add(&state, AppType::WorkBuddy, input.clone(), false).is_err()
            );
            let input = provider("saved", "other-model");
            ProviderService::add(&state, AppType::WorkBuddy, input.clone(), false)
                .expect("save disabled");
            assert!(ProviderService::add(
                &state,
                AppType::WorkBuddy,
                provider("other", "other-model"),
                false
            )
            .is_err());
        });
    }
}
