//! Profiles of shared gateway connections store selectors, never gateway credentials.
use super::{error, library, AppError, AppState, AppType, ProfileChange, ProfileSelection};
use crate::services::agent_connections::{self, ModelVisibility};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Connection {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Value>,
}

fn supported(app: &AppType) -> bool {
    app != &AppType::Codex && agent_connections::APPS.contains(&app.as_str())
}

pub(super) fn available(state: &AppState, app: &AppType) -> Result<bool, AppError> {
    if !supported(app) {
        return Ok(false);
    }
    state
        .proxy_service
        .agent_connections
        .status(app.as_str())
        .map(|status| status.configured)
        .map_err(|_| error("invalidConfig"))
}

pub(super) fn prepare(state: &AppState, bundle: &super::ProfileBundle) -> Result<(), AppError> {
    for selection in &bundle.selections {
        if selection.connection.is_some() || available(state, &selection.app)? {
            // Refresh before taking client locks; forwarding keeps progressing
            // while the profile command waits on its blocking worker.
            futures::executor::block_on(state.proxy_service.ensure_global_routing())
                .map_err(|_| error("operationFailed"))?;
            break;
        }
    }
    Ok(())
}

fn menu(state: &AppState) -> Result<std::sync::Arc<crate::proxy::intent::Menu>, AppError> {
    state
        .proxy_service
        .intent_runtime
        .global_snapshot()
        .map_err(|_| error("operationFailed"))?
        .ok_or_else(|| error("noProvider"))
}

pub(super) fn capture(state: &AppState, app: &AppType) -> Result<Option<Connection>, AppError> {
    if !available(state, app)? {
        return Ok(None);
    }
    let manager = &state.proxy_service.agent_connections;
    let receipt = manager
        .load(app.as_str())
        .map_err(|_| error("invalidConfig"))?
        .ok_or_else(|| error("noProvider"))?;
    let menu = menu(state)?;
    let native = agent_connections::native::starting_model(app.as_str())
        .map_err(|_| error("invalidConfig"))?;
    let model = native
        .as_deref()
        .map(|wire| {
            receipt
                .model_map
                .get(wire)
                .map(String::as_str)
                .unwrap_or_else(|| wire.strip_prefix("varyai/").unwrap_or(wire))
        })
        .unwrap_or(&receipt.model);
    if !menu.listed_aliases().any(|alias| alias == model) {
        return Err(error("nativeModelMissing"));
    }
    Ok(Some(Connection {
        model: model.into(),
        visibility: if app == &AppType::ClaudeDesktop {
            Some(
                serde_json::to_value(
                    manager
                        .model_visibility(app.as_str())
                        .map_err(|_| error("invalidConfig"))?,
                )
                .map_err(|_| error("invalidConfig"))?,
            )
        } else {
            None
        },
    }))
}

pub(super) fn validate(
    state: &AppState,
    app: &AppType,
    connection: &Connection,
) -> Result<(), AppError> {
    if !supported(app)
        || connection.model.len() > 512
        || connection.model.chars().any(char::is_control)
    {
        return Err(error("invalidConfig"));
    }
    if !menu(state)?
        .listed_aliases()
        .any(|alias| alias == &connection.model)
    {
        return Err(error("nativeModelMissing"));
    }
    if let Some(value) = &connection.visibility {
        if app != &AppType::ClaudeDesktop {
            return Err(error("invalidConfig"));
        }
        let policy: ModelVisibility =
            serde_json::from_value(value.clone()).map_err(|_| error("invalidConfig"))?;
        policy.validate().map_err(|_| error("invalidConfig"))?;
    }
    Ok(())
}

pub(super) fn exists(
    state: &AppState,
    app: &AppType,
    connection: &Connection,
) -> Result<bool, AppError> {
    match validate(state, app, connection) {
        Ok(()) => Ok(true),
        Err(AppError::Message(message))
            if [
                "profiles.bundle.nativeModelMissing",
                "profiles.bundle.noProvider",
            ]
            .contains(&message.as_str()) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn preview(
    state: &AppState,
    selection: &ProfileSelection,
    connection: &Connection,
) -> Result<ProfileChange, AppError> {
    validate(state, &selection.app, connection)?;
    let menu = menu(state)?;
    let catalog = agent_connections::models(&menu);
    let label = |id: &str| {
        catalog
            .iter()
            .find(|model| model.id == id)
            .map(|model| model.name.clone())
            .unwrap_or_else(|| id.into())
    };
    if let Some(library) = &selection.library {
        library.validate(state, &selection.app)?;
    }
    let current = agent_connections::native::starting_model(selection.app.as_str())
        .map_err(|_| error("invalidConfig"))?;
    let receipt = state
        .proxy_service
        .agent_connections
        .load(selection.app.as_str())
        .map_err(|_| error("invalidConfig"))?;
    let current = current.or_else(|| receipt.as_ref().map(|receipt| receipt.model.clone()));
    let current = current.map(|wire| {
        label(
            receipt
                .as_ref()
                .and_then(|receipt| receipt.model_map.get(&wire))
                .map(String::as_str)
                .unwrap_or_else(|| wire.strip_prefix("varyai/").unwrap_or(&wire)),
        )
    });
    Ok(ProfileChange {
        app: selection.app.as_str().into(),
        current_provider: if receipt.is_some() {
            "Vary AI".into()
        } else {
            String::new()
        },
        target_provider: "Vary AI".into(),
        models: vec![label(&connection.model)],
        enabled_providers: vec!["Vary AI".into()],
        current_models: current.into_iter().collect(),
        client_sync_pending: false,
        library: selection
            .library
            .as_ref()
            .map(|library| library.preview(state, &selection.app))
            .transpose()?,
        current_library: Some(
            library::LibrarySelection::capture(state, &selection.app)?
                .preview(state, &selection.app)?,
        ),
    })
}

pub(super) fn apply(
    state: &AppState,
    app: &AppType,
    connection: &Connection,
) -> Result<(), AppError> {
    validate(state, app, connection)?;
    let menu = menu(state)?;
    let status = futures::executor::block_on(state.proxy_service.get_status())
        .map_err(|_| error("operationFailed"))?;
    if !status.running {
        return Err(error("sharedGatewayRequired"));
    }
    let manager = &state.proxy_service.agent_connections;
    if let Some(value) = &connection.visibility {
        manager
            .write_model_visibility(
                app.as_str(),
                &serde_json::from_value(value.clone()).map_err(|_| error("invalidConfig"))?,
            )
            .map_err(|_| error("operationFailed"))?;
    }
    let origin = format!("http://127.0.0.1:{}", status.port);
    let receipt = if manager
        .load(app.as_str())
        .map_err(|_| error("invalidConfig"))?
        .is_some()
    {
        manager
            .update(app.as_str(), &menu, &origin, Some(&connection.model))
            .map_err(|_| error("operationFailed"))?
            .ok_or_else(|| error("noProvider"))?
    } else {
        let receipt = manager
            .build(app.as_str(), &menu, &origin, &connection.model)
            .map_err(|_| error("operationFailed"))?;
        manager
            .install(receipt.clone())
            .map_err(|_| error("operationFailed"))?;
        receipt
    };
    state
        .proxy_service
        .intent_runtime
        .connections
        .install(&receipt)
        .map_err(|_| error("operationFailed"))
}

pub(super) fn paths(state: &AppState, app: &AppType) -> Result<Vec<PathBuf>, AppError> {
    if !supported(app) {
        return Ok(vec![]);
    }
    state
        .proxy_service
        .agent_connections
        .profile_paths(app.as_str())
        .map_err(|_| error("invalidConfig"))
}

pub(super) fn restore(state: &AppState, app: &AppType) -> Result<(), AppError> {
    if !supported(app) {
        return Ok(());
    }
    state
        .proxy_service
        .intent_runtime
        .connections
        .revoke(app.as_str());
    if available(state, app)? {
        let receipt = state
            .proxy_service
            .agent_connections
            .load(app.as_str())
            .map_err(|_| error("restoreFailed"))?
            .ok_or_else(|| error("restoreFailed"))?;
        state
            .proxy_service
            .intent_runtime
            .connections
            .install(&receipt)
            .map_err(|_| error("restoreFailed"))?;
    }
    Ok(())
}
