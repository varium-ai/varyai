use super::{ProviderService, SwitchResult};
use crate::{
    app_config::AppType, error::AppError, provider::Provider, qoder_config as native,
    store::AppState,
};
use indexmap::IndexMap;
use serde_json::{json, Value};
const APP: &str = "qoder";
const OFFICIAL: &str = "qoder-official";
pub(super) fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    if !state.db.get_bool_flag("qoder_official_seeded")? {
        if state.db.get_provider_by_id(OFFICIAL, APP)?.is_none() {
            let mut p = Provider::with_id(
                OFFICIAL.into(),
                "Qoder CN Official".into(),
                json!({}),
                Some("https://qoder.cn".into()),
            );
            p.category = Some("official".into());
            state.db.save_provider(APP, &p)?;
        }
        state.db.set_setting("qoder_official_seeded", "true")?;
    }
    state.db.get_all_providers(APP)
}
pub(super) fn current(state: &AppState) -> Result<String, AppError> {
    Ok(state
        .db
        .get_current_provider(APP)?
        .unwrap_or_else(|| OFFICIAL.into()))
}
pub(super) fn add(
    state: &AppState,
    provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    ProviderService::validate_provider_settings(&AppType::Qoder, &provider)?;
    if state.db.get_provider_by_id(&provider.id, APP)?.is_some() {
        return Err(native::invalid());
    }
    if add_to_live && provider.category.as_deref() != Some("official") {
        let config = projected(state, &provider)?;
        native::change_provider(
            &native::native_id(&provider.id),
            None,
            Some(&config),
            || {
                state.db.save_provider(APP, &provider)?;
                Ok(true)
            },
        )
    } else {
        state.db.save_provider(APP, &provider)?;
        Ok(true)
    }
}
pub(super) fn update(
    state: &AppState,
    original_id: Option<&str>,
    mut provider: Provider,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    if original_id.is_some_and(|id| id != provider.id) {
        return Err(native::invalid());
    }
    let previous = state
        .db
        .get_provider_by_id(&provider.id, APP)?
        .ok_or_else(native::invalid)?;
    if previous.category.as_deref() == Some("official")
        && provider.category.as_deref() != Some("official")
    {
        return Err(native::invalid());
    }
    ProviderService::validate_provider_settings(&AppType::Qoder, &provider)?;
    if ProviderService::provider_live_config_managed(&previous) == Some(false) {
        ProviderService::set_provider_live_config_managed(&mut provider, false);
    }
    let id = native::native_id(&provider.id);
    let old = native::providers()?.get(&id).cloned();
    if provider.category.as_deref() == Some("official")
        || old.is_none()
        || ProviderService::provider_live_config_managed(&previous) == Some(false)
    {
        state.db.save_provider(APP, &provider)?;
        return Ok(true);
    }
    let old = old.ok_or_else(native::invalid)?;
    ensure_owned(state, &previous, &old)?;
    let replacement = merge_native(&old, &projected(state, &provider)?);
    native::change_provider(&id, Some(&old), Some(&replacement), || {
        state.db.save_provider(APP, &provider)?;
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
            "qoder.current_in_use",
            "请先切换到其他 Qoder 配置",
            "Switch Qoder to another configuration first",
        ));
    }
    let commit = || {
        if delete {
            state.db.delete_provider(APP, id)?;
        }
        Ok(())
    };
    let key = native::native_id(id);
    if provider.category.as_deref() != Some("official")
        && ProviderService::provider_live_config_managed(&provider) != Some(false)
    {
        if let Some(old) = native::providers()?.get(&key).cloned() {
            ensure_owned(state, &provider, &old)?;
            return native::change_provider(&key, Some(&old), None, commit);
        }
    }
    commit()
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
    let mut provider = state
        .db
        .get_provider_by_id(id, APP)?
        .ok_or_else(native::invalid)?;
    ProviderService::validate_provider_settings(&AppType::Qoder, &provider)?;
    if provider.category.as_deref() == Some("official") {
        state.db.set_current_provider(APP, id)?;
        return Ok(SwitchResult::default());
    }
    let key = native::native_id(id);
    let old = native::providers()?.get(&key).cloned();
    if let Some(old) = &old {
        ensure_owned(state, &provider, old)?;
    }
    let config = projected(state, &provider)?;
    let config = old
        .as_ref()
        .map(|old| merge_native(old, &config))
        .unwrap_or(config);
    ProviderService::set_provider_live_config_managed(&mut provider, true);
    native::change_provider(&key, old.as_ref(), Some(&config), || {
        state.db.save_provider(APP, &provider)?;
        state.db.set_current_provider(APP, id)?;
        Ok(SwitchResult::default())
    })
}
fn merge_native(old: &Value, new: &Value) -> Value {
    let mut merged = old.clone();
    if let (Some(target), Some(source)) = (merged.as_object_mut(), new.as_object()) {
        target.extend(source.clone());
    }
    if let Some(models) = merged.get_mut("models").and_then(Value::as_array_mut) {
        for model in models {
            if let Some(previous) = old["models"]
                .as_array()
                .and_then(|ms| ms.iter().find(|p| p["model"] == model["model"]))
            {
                let mut item = previous.clone();
                if let (Some(t), Some(s)) = (item.as_object_mut(), model.as_object()) {
                    t.extend(s.clone());
                }
                *model = item;
            }
        }
    }
    merged
}
fn ensure_owned(state: &AppState, provider: &Provider, native: &Value) -> Result<(), AppError> {
    let projected = projected(state, provider)?;
    if native["baseUrl"] != provider.settings_config["baseUrl"]
        && native["baseUrl"] != projected["baseUrl"]
    {
        return Err(crate::qoder_config::invalid());
    }
    Ok(())
}
pub(crate) fn projected(state: &AppState, provider: &Provider) -> Result<Value, AppError> {
    let mut config = provider.settings_config.clone();
    config["displayName"] = json!(provider.name);
    if let Some(saved) = futures::executor::block_on(state.db.get_live_backup(APP))? {
        let backup: Value =
            serde_json::from_str(&saved.original_config).map_err(|_| native::invalid())?;
        let origin = backup["custom_origin"]
            .as_str()
            .ok_or_else(native::invalid)?;
        config["baseUrl"] = json!(model_url(origin, &provider.id)?);
        config["apiKey"] = json!("varyai-local");
    }
    Ok(config)
}
pub(crate) fn model_url(origin: &str, id: &str) -> Result<String, AppError> {
    let mut url = url::Url::parse(origin).map_err(|_| native::invalid())?;
    url.path_segments_mut()
        .map_err(|_| native::invalid())?
        .clear()
        .extend(["qoder", "providers", id, "v1"]);
    Ok(url.into())
}
pub(crate) fn live_ids(state: &AppState) -> Result<Vec<String>, AppError> {
    let native = native::providers()?;
    Ok(state
        .db
        .get_all_providers(APP)?
        .values()
        .filter(|p| {
            p.category.as_deref() != Some("official")
                && ProviderService::provider_live_config_managed(p) != Some(false)
                && native.contains_key(&crate::qoder_config::native_id(&p.id))
        })
        .map(|p| p.id.clone())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_config::AppType, database::Database, provider::Provider, services::ProviderService,
        store::AppState,
    };
    use serde_json::json;
    use std::sync::Arc;
    #[test]
    fn qoder_saved_copies_stay_local_and_multiple_models_coexist_without_modifying_login() {
        let dir = tempfile::tempdir().expect("dir");
        crate::qoder_config::with_test_dir(dir.path(), || {
            let state = AppState::new(Arc::new(Database::memory().expect("db")));
            std::fs::write(
                dir.path().join("settings.json"),
                r#"{"enabledPlugins":{"keep":true}}"#,
            )
            .expect("native");
            std::fs::write(dir.path().join("credentials.json"), "native-login").expect("login");
            let saved = list(&state).expect("seed");
            assert!(saved.contains_key("qoder-official"));
            let config = json!({"baseUrl":"https://api.example/v1","apiKey":"synthetic-key","type":"openai-compatible","protocol":"openai","authType":"bearer","model":"a","models":[{"model":"a"},{"model":"b"}]});
            let source = Provider::with_id("source".into(), "API".into(), config.clone(), None);
            add(&state, source.clone(), false).expect("save only");
            assert!(crate::qoder_config::providers().expect("native").is_empty());
            enable(&state, &source.id).expect("enable");
            assert_eq!(live_ids(&state).expect("ids"), vec!["source"]);
            let original = std::fs::read(crate::qoder_config::path()).expect("native");
            let copy = ProviderService::duplicate(&state, AppType::Qoder, &source.id, "Copy")
                .expect("copy");
            assert_eq!(
                std::fs::read(crate::qoder_config::path()).expect("native"),
                original
            );
            enable(&state, &copy.id).expect("same model IDs under independent providers");
            assert_eq!(crate::qoder_config::providers().expect("native").len(), 2);
            enable(&state, "qoder-official").expect("official");
            remove(&state, &copy.id, true).expect("remove copy");
            assert_eq!(
                std::fs::read_to_string(dir.path().join("credentials.json")).expect("login"),
                "native-login"
            );
            assert_eq!(
                crate::qoder_config::read_at(&crate::qoder_config::path())
                    .expect("native")
                    .0["enabledPlugins"],
                json!({"keep":true})
            );
        });
    }
}
