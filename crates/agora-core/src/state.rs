use crate::msa::LoginFlow;
use crate::browse_cache::SharedBrowseCache;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Information about the current directly-launched Minecraft process.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunningProcess {
    pub instance_id: String,
    pub pid: u32,
    /// Monotonically increasing launch session ID, used to disambiguate
    /// late events from a previous launch of the same instance.
    pub session_id: u64,
}

/// Lightweight shared application state.
pub struct AppState {
    /// Shared HTTP client for all network operations (MSA, Modrinth, etc.)
    pub client: reqwest::Client,
    /// In-flight MSA login flow (ephemeral — only alive between begin/finish).
    /// If the app crashes, the flow is lost and the user re-authenticates.
    pub login_flow: Option<LoginFlow>,
    /// Shared browse cache for paginated Modrinth + registry results.
    pub browse_cache: SharedBrowseCache,
    /// Tracked directly-launched process, stored so the frontend can recover
    /// running state after navigation or reload.
    pub running_process: Option<RunningProcess>,
    /// Session counter incremented on every direct launch.
    pub launch_session_counter: u64,
}

impl AppState {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            login_flow: None,
            browse_cache: crate::browse_cache::new_cache(),
            running_process: None,
            launch_session_counter: 0,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Tauri-managed wrapper around the shared application state.
pub type LauncherState = Arc<Mutex<AppState>>;
