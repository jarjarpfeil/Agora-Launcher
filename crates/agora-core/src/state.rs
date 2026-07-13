use crate::browse_cache::SharedBrowseCache;
use crate::install_pipeline::{CancellationToken, ResolvedInstallPlan};
use crate::msa::LoginFlow;
use std::collections::{HashMap, HashSet};
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

/// Reservation held while a direct launch is preparing network assets and the
/// Java command.  It closes the check-then-spawn race without keeping the
/// application-state mutex locked across asynchronous work.
#[derive(Debug, Clone)]
pub struct LaunchReservation {
    pub instance_id: String,
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
    /// A launch that has exclusive ownership but has not spawned Java yet.
    pub launch_reservation: Option<LaunchReservation>,
    /// Sessions for which the user explicitly requested termination.  The exit
    /// classifier consumes these so a user stop is never reported as a crash.
    pub user_cancelled_launches: HashSet<u64>,
    /// Session counter incremented on every direct launch.
    pub launch_session_counter: u64,
    /// Backend-owned plans keyed by fingerprint. Clients submit only the id for
    /// execution, preventing plan-body tampering between resolve and apply.
    pub resolved_install_plans: HashMap<String, ResolvedInstallPlan>,
    /// Per-plan cancellation flags shared with active executors.
    pub install_cancellations: HashMap<String, CancellationToken>,
    /// Instance IDs with an active install transaction.
    pub active_install_instances: HashSet<String>,
    /// Per-instance serialization for LKG read/modify/write promotion. Delegated
    /// monitors can overlap a newer direct launch, so the global launch lock is
    /// not sufficient for protecting `lkg.json`.
    pub lkg_locks: HashMap<String, Arc<Mutex<()>>>,
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
            launch_reservation: None,
            user_cancelled_launches: HashSet::new(),
            launch_session_counter: 0,
            resolved_install_plans: HashMap::new(),
            install_cancellations: HashMap::new(),
            active_install_instances: HashSet::new(),
            lkg_locks: HashMap::new(),
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
