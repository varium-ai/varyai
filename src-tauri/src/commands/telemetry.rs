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
) -> Result<TelemetryPreferences, String> {
    if enabled && !manager.preferences().available {
        return Err("Optional statistics are not configured for this build".into());
    }
    let manager = manager.inner().clone();
    tokio::task::spawn_blocking(move || manager.set_enabled(enabled))
        .await
        .map_err(|_| "Could not save statistics preferences".to_string())?
}
