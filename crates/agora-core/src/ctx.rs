use std::path::PathBuf;

/// The dependency bag passed to every `agora-core` operation (plan C3).
///
/// Constructed once per host: the Tauri GUI builds it from `AppHandle`
/// (resolving `app_data_dir`); the future `agora` CLI builds it from the
/// configured data dir; the MCP listener inherits it from its host.
///
/// Constraint: this struct MUST NOT carry any `tauri::` type. It owns only
/// plain, UI-agnostic resources.
pub struct Ctx {
    /// Resolved app data directory (`%APPDATA%/com.agoramc.app` on Windows).
    pub app_data_dir: PathBuf,
    /// Shared HTTP client (rustls, no native OpenSSL).
    pub client: reqwest::Client,
}

impl Ctx {
    pub fn new(app_data_dir: PathBuf, client: reqwest::Client) -> Self {
        Self { app_data_dir, client }
    }
}
