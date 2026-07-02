use serde::{Deserialize, Serialize};
use std::io::Write;
use std::time::Duration;

use crate::error::{LauncherError, LauncherResult};

pub fn log_line(line: &str) {
    let stamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let entry = format!("[{stamp}] {line}\n");
    let path = std::env::temp_dir().join("agora-device-flow.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(entry.as_bytes());
        let _ = f.flush();
    }
}

pub const AGORA_OAUTH_CLIENT_ID: &str = match option_env!("AGORA_OAUTH_CLIENT_ID") {
    Some(v) => v,
    None => "Iv23ctVA40Yy1ZUkvemh",
};

const KEYRING_SERVICE: &str = "com.agoramc";
const KEYRING_ACCOUNT: &str = "github-token";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceFlowResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GithubProfile {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Debug, Deserialize)]
struct DeviceFlowPollResponse {
    access_token: Option<String>,
    error: Option<String>,
    interval: Option<u64>,
}

pub async fn start_device_flow() -> LauncherResult<DeviceFlowResponse> {
    if AGORA_OAUTH_CLIENT_ID.is_empty() {
        return Err(LauncherError::Generic {
            code: "ERR_AUTH_NOT_CONFIGURED".to_string(),
            message: "GitHub OAuth is not configured. Set the AGORA_OAUTH_CLIENT_ID environment \
                      variable before building/running Tauri (e.g. \
                      `$env:AGORA_OAUTH_CLIENT_ID='Iv1.xxxxxxxx'; npm run tauri:dev`). Register \
                      an OAuth app at https://github.com/settings/developers (Authorization type: \
                      GitHub App, Device Flow enabled)."
                .to_string(),
        });
    }

    let client = reqwest::Client::builder()
        .user_agent("agora-launcher")
        .build()
        .map_err(|_| LauncherError::Generic {
            code: "ERR_AUTH_HTTP_CLIENT".to_string(),
            message: "Failed to build HTTP client for device flow.".to_string(),
        })?;

    let params = [("client_id", AGORA_OAUTH_CLIENT_ID)];

    let resp = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|e| {
            log_line(&format!(
                "device-code request network error: {e}"
            ));
            LauncherError::NetworkOffline
        })?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    log_line(&format!(
        "device-code response status={status} body={body}"
    ));

    if !status.is_success() {
        return Err(LauncherError::Generic {
            code: "ERR_AUTH_DEVICE_CODE".to_string(),
            message: format!("GitHub rejected the device code request (status {status})."),
        });
    }

    serde_json::from_str::<DeviceFlowResponse>(&body).map_err(|e| {
        log_line(&format!("device-code parse error: {e}"));
        LauncherError::Generic {
            code: "ERR_AUTH_DEVICE_CODE".to_string(),
            message: "Failed to parse GitHub device code response.".to_string(),
        }
    })
}

pub async fn poll_device_flow(device_code: String, mut interval: u64) -> LauncherResult<Option<String>> {
    log_line(&format!(
        "poll_device_flow ENTERED device_code_len={} interval={}s",
        device_code.len(),
        interval
    ));
    let client = reqwest::Client::builder()
        .user_agent("agora-launcher")
        .build()
        .map_err(|e| {
            log_line(&format!("poll HTTP client build error: {e}"));
            LauncherError::Generic {
                code: "ERR_AUTH_HTTP_CLIENT".to_string(),
                message: "Failed to build HTTP client for token polling.".to_string(),
            }
        })?;

    let deadline = std::time::Instant::now() + Duration::from_secs(1200);

    loop {
        if std::time::Instant::now() >= deadline {
            return Ok(None);
        }

        let params = [
            ("client_id", AGORA_OAUTH_CLIENT_ID),
            ("device_code", &device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ];

        let resp = client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&params)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                log_line(&format!("poll status={status} body={body}"));

                let parsed: Option<DeviceFlowPollResponse> =
                    serde_json::from_str(&body).ok();

                if let Some(parsed) = parsed {
                    if let Some(token) = parsed.access_token {
                        log_line("token obtained");
                        return Ok(Some(token));
                    }
                    if let Some(err) = parsed.error.as_deref() {
                        match err {
                            "authorization_pending" => {
                                log_line(&format!(
                                    "awaiting user authorization (interval={})",
                                    parsed.interval.unwrap_or(interval)
                                ));
                                if let Some(next) = parsed.interval {
                                    interval = next;
                                }
                            }
                            "slow_down" => {
                                interval = interval.saturating_add(5);
                                log_line(&format!(
                                    "slow_down; interval now {interval}s"
                                ));
                            }
                            "expired_token" => {
                                log_line("device code expired");
                                return Ok(None);
                            }
                            "access_denied" => {
                                log_line("user denied authorization");
                                return Ok(None);
                            }
                            other => {
                                log_line(&format!(
                                    "unknown error from GitHub: {other}"
                                ));
                            }
                        }
                    } else if let Some(next) = parsed.interval {
                        interval = next;
                    }
                } else {
                    log_line("could not parse poll response as JSON");
                }
            }
            Err(e) => {
                log_line(&format!("network error during poll: {e}"));
            }
        }

        tokio::time::sleep(Duration::from_secs(interval.max(1))).await;
    }
}

pub fn store_token(token: &str) -> LauncherResult<()> {
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
        Ok(entry) => entry
            .set_password(token)
            .map_err(|_| LauncherError::Generic {
                code: "ERR_AUTH_KEYRING_WRITE".to_string(),
                message: "Failed to store GitHub token in the OS keyring.".to_string(),
            }),
        Err(_) => Err(LauncherError::Generic {
            code: "ERR_AUTH_KEYRING_UNAVAILABLE".to_string(),
            message: "OS keyring is unavailable and no fallback is implemented yet.".to_string(),
        }),
    }
}

pub fn get_token() -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).ok()?;
    entry.get_password().ok()
}

pub fn clear_token() -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("Failed to open keyring entry: {}", e))?;
    match entry.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to delete GitHub token: {}", e)),
    }
}

pub fn is_authenticated() -> bool {
    get_token().is_some()
}

pub async fn get_github_user(token: String) -> LauncherResult<GithubProfile> {
    let client = reqwest::Client::builder()
        .user_agent("agora-launcher")
        .build()
        .map_err(|_| LauncherError::Generic {
            code: "ERR_AUTH_HTTP_CLIENT".to_string(),
            message: "Failed to build HTTP client for GitHub profile.".to_string(),
        })?;

    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(LauncherError::AuthExpired);
    }
    if !resp.status().is_success() {
        return Err(LauncherError::Generic {
            code: "ERR_AUTH_PROFILE".to_string(),
            message: "GitHub rejected the profile request.".to_string(),
        });
    }

    #[derive(Debug, Deserialize)]
    struct GithubUserJson {
        login: String,
        avatar_url: String,
    }

    let parsed = resp
        .json::<GithubUserJson>()
        .await
        .map_err(|_| LauncherError::Generic {
            code: "ERR_AUTH_PROFILE".to_string(),
            message: "Failed to parse GitHub profile response.".to_string(),
        })?;

    Ok(GithubProfile {
        login: parsed.login,
        avatar_url: parsed.avatar_url,
    })
}
