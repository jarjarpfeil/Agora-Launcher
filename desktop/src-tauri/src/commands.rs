use crate::error::LauncherResult;
use crate::state::LauncherState;

#[tauri::command]
pub async fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}

/// Placeholder: query the curated registry database.
#[tauri::command]
pub async fn query_registry(
    _state: tauri::State<'_, LauncherState>,
    _sql: String,
) -> LauncherResult<Vec<serde_json::Value>> {
    // TODO: execute parameterized SELECT against registry.db via tauri-plugin-sql.
    Ok(vec![])
}

/// Placeholder: list user instances from local_state.db.
#[tauri::command]
pub async fn list_instances(
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<Vec<serde_json::Value>> {
    // TODO: SELECT * FROM user_instances ORDER BY last_launched_at DESC.
    Ok(vec![])
}

/// Placeholder: read a setting from local_state.db / store.
#[tauri::command]
pub async fn get_settings(
    _state: tauri::State<'_, LauncherState>,
    _key: String,
) -> LauncherResult<Option<serde_json::Value>> {
    // TODO: lookup key in user_settings.
    Ok(None)
}

/// Placeholder: write a setting to local_state.db / store.
#[tauri::command]
pub async fn set_settings(
    _state: tauri::State<'_, LauncherState>,
    _key: String,
    _value: serde_json::Value,
) -> LauncherResult<()> {
    // TODO: upsert key/value_json in user_settings.
    Ok(())
}
