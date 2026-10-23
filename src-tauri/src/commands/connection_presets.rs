use crate::database::ConnectionPreset;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub fn list_connection_presets(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectionPreset>, String> {
    state
        .db
        .list_connection_presets()
        .map_err(|_| "connection_presets_load_failed".into())
}

#[tauri::command]
pub fn save_connection_preset(
    state: State<'_, AppState>,
    preset: ConnectionPreset,
) -> Result<(), String> {
    state
        .db
        .save_connection_preset(&preset)
        .map_err(|_| "connection_preset_save_failed".into())
}

#[tauri::command]
pub fn delete_connection_preset(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .db
        .delete_connection_preset(&id)
        .map_err(|_| "connection_preset_delete_failed".into())
}
