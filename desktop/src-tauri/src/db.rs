//! Database access shim — delegates to `agora_core::db` after resolving
//! Tauri-specific paths from [`&tauri::AppHandle`].
//!
//! This module preserves the original public signatures so that callers
//! across `commands.rs`, `instances.rs`, `lib.rs` setup hook, and
//! `crash_diagnostics.rs` resolve unchanged.

// ---------------------------------------------------------------------------
// Path-resolving functions (Tauri-coupled — resolve path then delegate)
// ---------------------------------------------------------------------------

/// Open a connection to the mutable local state database, creating it if needed.
pub fn local_state_connection<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> anyhow::Result<rusqlite::Connection> {
    let path = crate::paths::local_state_db_path(app)?;
    agora_core::db::local_state_connection(&path)
}

/// Initialize the local SQLite database on first run and apply migrations.
pub fn init_local_state_db<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<()> {
    let path = crate::paths::local_state_db_path(app)?;
    agora_core::db::init_local_state_db(&path)
}

/// Open a read-only connection to the cached registry database.
pub fn registry_connection<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> anyhow::Result<rusqlite::Connection> {
    let path = crate::paths::registry_db_path(app)?;
    agora_core::db::registry_connection(&path)
}

// ---------------------------------------------------------------------------
// Pure query/CRUD functions — re-export verbatim from agora_core
// ---------------------------------------------------------------------------

pub use agora_core::db::count_flags_since;
pub use agora_core::db::count_instances_by_loader_version;
pub use agora_core::db::CrashAttribution;
pub use agora_core::db::delete_instance;
pub use agora_core::db::FlagRateLimit;
pub use agora_core::db::get_confirmed_attribution;
pub use agora_core::db::get_flag_rate_limit_status;
pub use agora_core::db::get_instance;
pub use agora_core::db::get_mod_survival_count;
pub use agora_core::db::get_pair_survival_count;
pub use agora_core::db::get_ruled_out_mods;
pub use agora_core::db::get_setting;
pub use agora_core::db::get_total_survival_count;
pub use agora_core::db::increment_confirmation;
pub use agora_core::db::init_local_state_db as _init_local_state_db;
pub use agora_core::db::insert_crash_event;
pub use agora_core::db::insert_survival;
pub use agora_core::db::is_ruled_out;
pub use agora_core::db::list_instances;
pub use agora_core::db::LOCAL_STATE_SCHEMA_VERSION;
pub use agora_core::db::normalize_pair;
pub use agora_core::db::purge_stale_crash_telemetry;
pub use agora_core::db::REGISTRY_SCHEMA_VERSION;
pub use agora_core::db::record_co_crash;
pub use agora_core::db::record_flag_submission;
pub use agora_core::db::set_locked;
pub use agora_core::db::set_setting;
pub use agora_core::db::touch_last_launched;
pub use agora_core::db::upsert_instance;
pub use agora_core::db::add_ruled_out;
