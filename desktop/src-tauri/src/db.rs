use tauri::Manager;

/// Expected schema version for the downloaded read-only registry database.
/// The launcher compares this against `SELECT version FROM schema_version`.
pub const REGISTRY_SCHEMA_VERSION: i64 = 1;

/// Expected schema version for the mutable local SQLite database.
/// Migrations are applied sequentially on startup.
pub const LOCAL_STATE_SCHEMA_VERSION: i64 = 1;

/// `registry.db` is read-only, downloaded from GitHub Release Assets, and is
/// cryptographically signed with an Ed25519 signature (`registry.db.sig`).
/// The launcher must never write to this file at runtime.
pub fn registry_db_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<std::path::PathBuf> {
    let dir = app.path().app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to resolve app data dir: {}", e))?;
    Ok(dir.join("registry.db"))
}

/// `local_state.db` is the mutable user database. It is created on first run
/// and stores settings, instances, crash telemetry, and MCP approval grants.
pub fn local_state_db_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<std::path::PathBuf> {
    let dir = app.path().app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to resolve app data dir: {}", e))?;
    Ok(dir.join("local_state.db"))
}

/// Ensure the application data directory exists.
pub fn ensure_app_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<std::path::PathBuf> {
    let dir = app.path().app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to resolve app data dir: {}", e))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Initialize the local SQLite database on first run.
///
/// TODO: open `local_state.db` via tauri-plugin-sql and apply migrations to
/// create user_settings, user_instances, local_crash_telemetry,
/// mcp_approval_grants, and schema_version tables.
pub async fn init_local_state<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<()> {
    let _dir = ensure_app_dir(app)?;
    let _db_path = local_state_db_path(app)?;
    // TODO: execute CREATE TABLE IF NOT EXISTS migration statements.
    Ok(())
}
