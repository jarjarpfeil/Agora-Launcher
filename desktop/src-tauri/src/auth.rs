use crate::error::LauncherResult;

pub use agora_core::auth::{
    log_line, start_device_flow, poll_device_flow, get_github_user,
    AGORA_OAUTH_CLIENT_ID, DeviceFlowResponse, GithubProfile,
};

pub fn store_token(_app: &tauri::AppHandle, token: &str) -> LauncherResult<()> {
    agora_core::auth::store_token(token)
}

pub fn get_token(_app: &tauri::AppHandle) -> Option<String> {
    agora_core::auth::get_token()
}

pub fn clear_token(_app: &tauri::AppHandle) -> Result<(), String> {
    agora_core::auth::clear_token()
}

pub fn is_authenticated(_app: &tauri::AppHandle) -> bool {
    agora_core::auth::is_authenticated()
}
