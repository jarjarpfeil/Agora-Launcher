//! Governance network actions: pure logic layer.
//!
//! Tauri-coupled functions that require `&tauri::AppHandle` (fetch_triage_poll,
//! flag_review) live in the desktop crate's governance shim. This module hosts
//! the constants, types, and pure DB-bound logic.

use crate::db::FlagRateLimit;
use crate::error::{LauncherError, LauncherResult};
use rusqlite::Connection;
use serde::Serialize;

// --- Constants ---

/// The governance repo (where mod review issues, triage discussions, and
/// reactions live) IS the registry repo itself. Configurable at build time
/// via AGORA_REGISTRY_REPO. An empty value disables repository-backed actions.
pub const AGORA_GOVERNANCE_REPO: &str = match option_env!("AGORA_REGISTRY_REPO") {
    Some(v) => v,
    None => "",
};

/// Admin-alerts repo is where curator flag issues are filed. Configurable at
/// build time via AGORA_ADMIN_ALERTS_REPO; defaults to the same owner as the
/// governance/registry repo.
pub const AGORA_ADMIN_ALERTS_REPO: &str = match option_env!("AGORA_ADMIN_ALERTS_REPO") {
    Some(v) => v,
    None => "jarjarpfeil/admin-alerts",
};

// --- Types ---

/// A live triage poll for a given mod, fetched from GitHub Discussions.
#[derive(Debug, Serialize, Clone)]
pub struct TriagePoll {
    pub discussion_url: Option<String>,
    pub keep_votes: i64,
    pub remove_votes: i64,
}

// --- Rate limit status ---

/// Return the current flag rate-limit status for a local state connection.
pub fn get_flag_rate_limit(conn: &Connection) -> LauncherResult<FlagRateLimit> {
    let now_unix = chrono::Utc::now().timestamp();
    crate::db::get_flag_rate_limit_status(conn, now_unix)
        .map_err(|_| LauncherError::LocalStateFailed)
}
