use std::sync::Arc;

use crate::telemetry::{TelemetryManager, TelemetryPreferences};

#[tauri::command]
pub(crate) fn get_telemetry_preferences(
    manager: tauri::State<'_, Arc<TelemetryManager>>,
) -> TelemetryPreferences {
    manager.preferences()
}

#[tauri::command]
pub(crate) async fn set_telemetry_enabled(
    manager: tauri::State<'_, Arc<TelemetryManager>>,
    enabled: bool,
    app: tauri::AppHandle,
) -> Result<TelemetryPreferences, String> {
    if enabled && !manager.preferences().available {
        return Err("Installation statistics are not configured for this build".into());
    }
    let manager = manager.inner().clone();
    let result = tokio::task::spawn_blocking(move || manager.set_enabled(enabled))
        .await
        .map_err(|_| "Could not save statistics preferences".to_string())?;
    if result.is_ok() && !enabled {
        crate::telemetry::destroy_analytics_window(&app);
    }
    result
}

#[tauri::command]
pub(crate) async fn acknowledge_analytics_report(
    manager: tauri::State<'_, Arc<TelemetryManager>>,
    window: tauri::WebviewWindow,
    token: String,
    dispatched: bool,
) -> Result<bool, String> {
    if window.label() != crate::telemetry::ANALYTICS_WINDOW_LABEL {
        return Err("Statistics acknowledgement unavailable".into());
    }
    let manager = manager.inner().clone();
    tokio::task::spawn_blocking(move || manager.complete_report(&token, dispatched))
        .await
        .map_err(|_| "Could not save statistics dispatch".to_string())?
}
