use super::codex_oauth::CodexOAuthState;
use crate::services::intent_routing::catalog::{prepare_providers, CatalogStatus};
use crate::{
    services::intent_routing::{eligible_provider, PlanPreview, RoutingCatalog, RoutingGroup},
    store::AppState,
};
use serde::Serialize;
use tauri::State;

#[tauri::command]
pub(crate) fn routing_trace_snapshot(
    state: State<'_, AppState>,
) -> crate::proxy::intent::trace::Snapshot {
    let runtime = &state.proxy_service.intent_runtime;
    let requests = runtime.traces.snapshot();
    let accounts = runtime.traces.live_accounts(&requests);
    crate::proxy::intent::trace::Snapshot {
        requests,
        accounts,
        capacity: crate::proxy::intent::trace::CAPACITY,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderOption {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_type: Option<String>,
    task_eligible: bool,
    classifier_eligible: bool,
    models: Vec<ModelOption>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelOption {
    id: String,
    name: String,
    context_window: Option<u64>,
    reasoning_levels: Vec<String>,
    #[serde(flatten)]
    facts: ModelFacts,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    output_price: Option<f64>,
    maker: bool,
    released: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<bool>,
}
fn model_facts(provider: &crate::provider::Provider, model: &str) -> ModelFacts {
    let fact = &provider.settings_config["routingModelFacts"][model];
    let capability = provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(serde_json::Value::as_array)
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["model"].as_str() == Some(model))
        })
        .map(crate::services::intent_routing::effort::Capability::parse)
        .unwrap_or_default();
    ModelFacts {
        output_price: fact["outputPrice"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0),
        maker: fact["maker"].as_bool().unwrap_or(false),
        released: fact["released"]
            .as_str()
            .filter(|s| s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok())
            .unwrap_or_default()
            .into(),
        images: capability.images.or_else(|| fact["images"].as_bool()),
        reasoning: if capability.quiet {
            Some(false)
        } else if !capability.levels.is_empty() {
            Some(true)
        } else {
            fact["reasoning"].as_bool()
        },
    }
}

#[tauri::command]
pub(crate) async fn list_intent_routing_providers(
    state: State<'_, AppState>,
) -> Result<Vec<ProviderOption>, String> {
    let db = state.db.clone();
    let (prepared, _) =
        prepare_providers(db.clone(), state.codex_oauth_manager.clone(), false).await?;
    let providers = prepared
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect::<std::collections::HashMap<_, _>>();
    let records = tauri::async_runtime::spawn_blocking(move || {
        let records = db.list_routing_upstreams()?;
        Ok::<_, String>(records)
    })
    .await
    .map_err(|_| "intentRouting.failed")?
    .map_err(|_| "intentRouting.failed")?;
    let mut options = Vec::new();
    for record in records {
        let provider = providers.get(&record.id);
        let task_eligible = if let Some(p) = provider {
            crate::services::intent_routing::catalog::task_configured(p, &state.codex_oauth_manager)
                .await
        } else {
            false
        };
        options.push(ProviderOption {
            icon: picker_icon(
                record
                    .preset_id
                    .as_deref()
                    .or_else(|| provider.and_then(|p| p.icon.as_deref())),
            ),
            app_type: matches!(
                record.source,
                crate::services::routing_upstreams::UpstreamSource::Codex
            )
            .then(|| "codex".into()),
            id: record.id,
            name: record.name,
            task_eligible,
            classifier_eligible: provider.is_some_and(|p| eligible_provider(p, true)),
            models: provider.map(model_options).unwrap_or_default(),
        });
    }
    options.sort_by(|a, b| (&a.app_type, &a.name, &a.id).cmp(&(&b.app_type, &b.name, &b.id)));
    Ok(options)
}

fn picker_icon(value: Option<&str>) -> Option<String> {
    value
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 80
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .map(str::to_owned)
}

fn model_options(provider: &crate::provider::Provider) -> Vec<ModelOption> {
    let mut rows = std::collections::BTreeMap::new();
    if let Some(entries) = provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(serde_json::Value::as_array)
    {
        for entry in entries {
            let Some(id) = entry["model"].as_str().map(str::trim).filter(|id| {
                !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control)
            }) else {
                continue;
            };
            let name =
                crate::services::intent_routing::model_preferences::name(entry).unwrap_or(id);
            rows.insert(
                id.to_owned(),
                ModelOption {
                    id: id.into(),
                    name: name.into(),
                    reasoning_levels: crate::services::intent_routing::effort::Capability::parse(
                        entry,
                    )
                    .levels,
                    facts: model_facts(provider, id),
                    context_window:
                        crate::services::intent_routing::context::Metadata::for_provider(
                            &provider.settings_config,
                            id,
                        )
                        .context_window,
                },
            );
        }
    }
    if let Some(config) = provider.settings_config["config"]
        .as_str()
        .and_then(|s| s.parse::<toml::Value>().ok())
    {
        if let Some(id) = config
            .get("model")
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control))
        {
            rows.entry(id.into()).or_insert_with(|| ModelOption {
                id: id.into(),
                name: id.into(),
                reasoning_levels: Vec::new(),
                facts: model_facts(provider, id),
                context_window: crate::services::intent_routing::context::Metadata::for_provider(
                    &provider.settings_config,
                    id,
                )
                .context_window,
            });
        }
    }
    rows.into_values().collect()
}

#[cfg(test)]
mod model_picker_tests {
    #[test]
    fn model_preferences_override_picker_names_images_and_reasoning() {
        let provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            serde_json::json!({"modelCatalog":{"models":[{"model":"exact", "displayName":"Vendor", "reasoningLevels":["low","high"], "inputModalities":["text","image"], "modelPreferences":{"name":"Custom", "reasoningLevels":[], "images":false}}]}}),
            None,
        );
        let value = serde_json::to_value(super::model_options(&provider)).expect("safe projection");
        assert_eq!(value[0]["name"], "Custom");
        assert_eq!(value[0]["images"], false);
        assert_eq!(value[0]["reasoningLevels"], serde_json::json!([]));
        assert_eq!(value[0]["reasoning"], false);
        assert!(value[0].get("modelPreferences").is_none());
    }
    #[test]
    fn template_projection_exposes_only_validated_public_facts() {
        let provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            serde_json::json!({"modelCatalog":{"models":[{"model":"a"},{"model":"b"}]}, "routingModelFacts":{
                "a":{"outputPrice":15,"maker":true,"released":"2026-01-01","images":true,"reasoning":false,"private":"synthetic-private"},
                "b":{"outputPrice":-10,"maker":false,"released":"synthetic-private","images":"synthetic-private"}
            }}),
            None,
        );
        let value =
            serde_json::to_value(super::model_options(&provider)).expect("synthetic projection");
        assert_eq!(value[0]["outputPrice"], 15.0);
        assert_eq!(value[0]["released"], "2026-01-01");
        assert_eq!(value[0]["images"], true);
        assert!(value[1].get("outputPrice").is_none());
        assert_eq!(value[1]["released"], "");
        assert!(value[1].get("images").is_none());
        assert!(!value.to_string().contains("synthetic-private"));
    }
    #[test]
    fn picker_icons_are_local_asset_names_never_paths_or_remote_urls() {
        assert_eq!(super::picker_icon(Some("openai")), Some("openai".into()));
        for icon in [
            "/synthetic/private/icon.svg",
            "https://synthetic.example/icon.svg",
            "../icon",
            "",
            "bad\nicon",
        ] {
            assert!(super::picker_icon(Some(icon)).is_none());
        }
    }
    #[test]
    fn picker_projects_only_safe_model_metadata_without_a_menu_size_cap() {
        let entries = (0..300).map(|n| serde_json::json!({"model":format!("model-{n}"),"contextWindow":1000000,"privateConfig":"synthetic-private"})).collect::<Vec<_>>();
        let provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            serde_json::json!({"modelCatalog":{"models":entries},"auth":{"OPENAI_API_KEY":"synthetic-private"}}),
            None,
        );
        let options = super::model_options(&provider);
        assert_eq!(options.len(), 300);
        let value = serde_json::to_value(options).expect("safe fixture serialization");
        assert_eq!(value[0]["contextWindow"], 1000000);
        assert!(!value.to_string().contains("synthetic-private"));
        assert!(value[0].get("privateConfig").is_none());
    }
}

#[tauri::command]
pub(crate) async fn list_intent_routing_groups(
    state: State<'_, AppState>,
    oauth: State<'_, CodexOAuthState>,
) -> Result<RoutingCatalog, String> {
    let db = state.db.clone();
    let (providers, _) = prepare_providers(db.clone(), oauth.0.clone(), false).await?;
    tauri::async_runtime::spawn_blocking(move || {
        db.intent_routing_catalog_with_providers(Some(providers))
    })
    .await
    .map_err(|_| "intentRouting.failed")?
}

#[tauri::command]
pub(crate) async fn set_intent_routing_auto_create(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || db.set_intent_routing_auto_create(enabled))
        .await
        .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn restore_discovered_routing_group(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.restore_discovered_routing_group(&id))
        .await
        .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn get_routing_context_settings(
    state: State<'_, AppState>,
) -> Result<crate::services::intent_routing::context::Settings, String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.routing_context_settings())
        .await
        .map_err(|_| "intentRouting.failed")?
}

#[tauri::command]
pub(crate) async fn get_routing_auto_group_skip(
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.routing_auto_group_skip())
        .await
        .map_err(|_| "intentRouting.failed")?
}

#[tauri::command]
pub(crate) async fn set_routing_auto_group_skip(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<(), String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.set_routing_auto_group_skip(ids))
        .await
        .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn set_routing_context_settings(
    state: State<'_, AppState>,
    settings: crate::services::intent_routing::context::Settings,
) -> Result<(), String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.set_routing_context_settings(&settings))
        .await
        .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn arrange_routing_groups(
    state: State<'_, AppState>,
    order: Vec<String>,
) -> Result<(), String> {
    let db = state.db.clone();
    let (providers, _) =
        prepare_providers(db.clone(), state.codex_oauth_manager.clone(), false).await?;
    tokio::task::spawn_blocking(move || db.set_routing_group_order(order, Some(providers)))
        .await
        .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn save_intent_routing_group(
    state: State<'_, AppState>,
    group: RoutingGroup,
) -> Result<RoutingGroup, String> {
    let db = state.db.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || db.save_intent_routing_group(group))
        .await
        .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(saved)
}

#[tauri::command]
pub(crate) async fn delete_intent_routing_group(
    state: State<'_, AppState>,
    id: String,
    revision: u32,
    oauth: State<'_, CodexOAuthState>,
) -> Result<(), String> {
    let db = state.db.clone();
    let (providers, _) = prepare_providers(db.clone(), oauth.0.clone(), false).await?;
    tauri::async_runtime::spawn_blocking(move || {
        db.delete_intent_routing_group_with_providers(&id, revision, Some(providers))
    })
    .await
    .map_err(|_| "intentRouting.failed")??;
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn resolve_routing_group_matches(
    state: State<'_, AppState>,
    group: RoutingGroup,
) -> Result<Vec<crate::services::intent_routing::Member>, String> {
    let (providers, _) =
        prepare_providers(state.db.clone(), state.codex_oauth_manager.clone(), false).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let group = crate::services::intent_routing::membership::resolve(group, &providers)?;
        Ok(group
            .members
            .into_iter()
            .filter(|member| group.matched.contains(&member.id))
            .collect())
    })
    .await
    .map_err(|_| "intentRouting.failed")?
}

#[tauri::command]
pub(crate) async fn preview_intent_routing_plan(
    state: State<'_, AppState>,
    group: RoutingGroup,
    classification: String,
) -> Result<PlanPreview, String> {
    let db = state.db.clone();
    let (providers, _) =
        prepare_providers(db.clone(), state.codex_oauth_manager.clone(), false).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let group = crate::services::intent_routing::membership::resolve(group, &providers)?;
        group.validate()?;
        db.validate_intent_routing_providers(&group)?;
        if group.routing == crate::services::intent_routing::RoutingStrategy::Manual {
            return group.preview(None).map_err(String::from);
        }
        let intent = group
            .parse_classification(&classification)
            .map_err(|_| "intentRouting.invalidOutput")?;
        group.preview(intent.as_deref()).map_err(String::from)
    })
    .await
    .map_err(|_| "intentRouting.failed")?
}

#[tauri::command]
pub(crate) async fn refresh_intent_routing_catalogs(
    state: State<'_, AppState>,
    oauth: State<'_, CodexOAuthState>,
    missing_only: Option<bool>,
) -> Result<Vec<CatalogStatus>, String> {
    let statuses = if missing_only == Some(true) {
        crate::services::intent_routing::catalog::prepare_missing_catalogs(
            state.db.clone(),
            oauth.0.clone(),
        )
        .await?
    } else {
        prepare_providers(state.db.clone(), oauth.0.clone(), true)
            .await?
            .1
    };
    crate::services::intent_routing::menu::refresh_active(&state.proxy_service).await;
    Ok(statuses)
}

#[tauri::command]
pub(crate) async fn trial_intent_routing_classification(
    state: State<'_, AppState>,
    group: RoutingGroup,
    text: String,
    consent: bool,
) -> Result<crate::services::intent_routing::classifier::TrialResult, String> {
    if !consent || text.len() > 65536 || text.trim().is_empty() {
        return Err("intentRouting.invalid".into());
    }
    group.validate()?;
    let db = state.db.clone();
    let (prepared, _) =
        prepare_providers(db.clone(), state.codex_oauth_manager.clone(), false).await?;
    let (group, classifier) = tauri::async_runtime::spawn_blocking(move || {
        let group = crate::services::intent_routing::membership::resolve(group, &prepared)?;
        let providers = prepared
            .into_iter()
            .map(|provider| (provider.id.clone(), provider))
            .collect::<std::collections::HashMap<_, _>>();
        if group
            .members
            .iter()
            .any(|member| !providers.contains_key(&member.provider_id))
        {
            return Err("intentRouting.providerUnavailable".to_string());
        }
        if group.routing == crate::services::intent_routing::RoutingStrategy::Manual {
            return Ok((group, None));
        }
        let target = group
            .classifier_target
            .as_ref()
            .ok_or("intentRouting.providerUnavailable")?;
        let classifier = providers
            .get(&target.provider_id)
            .cloned()
            .ok_or("intentRouting.providerUnavailable")?;
        Ok((group, Some(classifier)))
    })
    .await
    .map_err(|_| "intentRouting.failed")??;
    match classifier {
        Some(classifier) => {
            crate::services::intent_routing::classifier::trial(group, classifier, text).await
        }
        None => Ok(crate::services::intent_routing::classifier::TrialResult {
            plan: group.preview(None)?,
            intent_id: None,
            outcome: "manual",
        }),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoutingSession {
    token: String,
    base_url: String,
    model: String,
    model_catalog_path: String,
    expires_in_seconds: u32,
}

/// Explicit temporary execution: no client files or current-provider writes.
#[tauri::command]
pub(crate) async fn start_intent_routing_session(
    state: State<'_, AppState>,
    group: RoutingGroup,
    consent: bool,
) -> Result<RoutingSession, String> {
    if !consent {
        return Err("intentRouting.invalid".into());
    }
    let session = crate::services::intent_routing::execution::prepare(
        state.db.clone(),
        state.codex_oauth_manager.clone(),
        group,
    )
    .await?;
    let model = session.group.alias();
    let status = state.proxy_service.get_status().await?;
    // Starting a temporary session must not implicitly enable the global proxy.
    if !status.running {
        return Err("intentRouting.proxyRequired".into());
    }
    let db = state.db.clone();
    let settings = tokio::task::spawn_blocking(move || db.routing_context_settings())
        .await
        .map_err(|_| "intentRouting.failed")??;
    let catalog = crate::services::intent_routing::execution::temporary_catalog_with_settings(
        &session, &settings,
    )
    .await?;
    let model_catalog_path = catalog.to_str().ok_or("intentRouting.failed")?.to_owned();
    let token = state
        .proxy_service
        .intent_runtime
        .install(session, Some(catalog))?;
    Ok(RoutingSession {
        token,
        base_url: format!("http://127.0.0.1:{}/intent/v1", status.port),
        model,
        model_catalog_path,
        expires_in_seconds: 3600,
    })
}

#[tauri::command]
pub(crate) fn stop_intent_routing_session(state: State<'_, AppState>, token: String) {
    state.proxy_service.intent_runtime.revoke(&token);
}

#[tauri::command]
pub(crate) async fn intent_routing_client_status(
    state: State<'_, AppState>,
) -> Result<crate::services::intent_routing::client::Status, String> {
    let manager = state.proxy_service.intent_client.clone();
    let mut status = tokio::task::spawn_blocking(move || manager.status())
        .await
        .map_err(|_| "intentRouting.clientFailed")??;
    status.provider_enabled = state
        .db
        .get_proxy_config_for_app("codex")
        .await
        .map_err(|_| "intentRouting.clientFailed")?
        .enabled;
    Ok(status)
}

#[tauri::command]
pub(crate) async fn connect_intent_routing_client(
    state: State<'_, AppState>,
    consent: bool,
) -> Result<crate::services::intent_routing::client::Status, String> {
    if !consent {
        return Err("intentRouting.invalid".into());
    }
    state.proxy_service.connect_codex_menu(true, true).await
}

#[tauri::command]
pub(crate) async fn disconnect_intent_routing_client(
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .proxy_service
        .set_takeover_for_app("codex", false)
        .await
}

#[tauri::command]
pub(crate) async fn cleanup_intent_routing_orphan(
    state: State<'_, AppState>,
    consent: bool,
) -> Result<(), String> {
    let _guard = state.proxy_service.lock_switch_for_app("codex").await;
    let manager = state.proxy_service.intent_client.clone();
    tokio::task::spawn_blocking(move || manager.cleanup_orphan(consent))
        .await
        .map_err(|_| "intentRouting.clientFailed")?
}
