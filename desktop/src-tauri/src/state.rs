use std::sync::Arc;
use tokio::sync::Mutex;

/// Lightweight shared application state.
#[derive(Default)]
pub struct AppState {
    // TODO: cache SQL connection handles, registry paths, etc.
}

/// Tauri-managed wrapper around the shared application state.
pub type LauncherState = Arc<Mutex<AppState>>;
