use serde::{Deserialize, Serialize};

use crate::db;
use crate::error::{LauncherError, LauncherResult};

const COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const COPILOT_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const COPILOT_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const COPILOT_USER_URL: &str = "https://api.github.com/user";
const COPILOT_INTERNAL_USER_URL: &str = "https://api.github.com/copilot_internal/user";
const COPILOT_TOKEN_EXCHANGE_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const COPILOT_INDIVIDUAL_ENDPOINT: &str = "https://api.individual.githubcopilot.com/chat/completions";
const COPILOT_ENTERPRISE_ENDPOINT: &str = "https://api.githubcopilot.com/chat/completions";
const COPILOT_KEYRING_SERVICE: &str = "agora.copilot";
const COPILOT_KEYRING_ACCOUNT: &str = "token";

/// Check a network enable setting from the local state DB.
fn check_network_enabled(setting_key: &str, disabled_msg: &str) -> LauncherResult<()> {
    let app_data_dir = dirs::data_local_dir()
        .ok_or_else(|| LauncherError::Generic {
            code: "ERR_NO_DATA_DIR".into(),
            message: "Could not determine local data directory.".into(),
        })?
        .join("agora");
    let db_path = app_data_dir.join("local_state.db");
    let conn = db::local_state_connection(&db_path).map_err(|e| LauncherError::Generic {
        code: "ERR_DB".into(),
        message: e.to_string(),
    })?;
    if !db::is_network_enabled(&conn, setting_key) {
        return Err(LauncherError::Generic {
            code: "ERR_NETWORK_DISABLED".into(),
            message: disabled_msg.into(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiContext {
    pub instance_id: Option<String>,
    pub crash_log: Option<String>,
    pub crash_signatures: Option<String>,
    pub suspects: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotDeviceFlowResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CopilotToken {
    pub access_token: String,
    pub copilot_token: Option<String>,
    pub endpoint: String,
    pub plan: String,
    pub username: String,
    pub stored_at: chrono::DateTime<chrono::Utc>,
}

impl std::fmt::Debug for CopilotToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopilotToken")
            .field("access_token", &"[REDACTED]")
            .field("copilot_token", &"[REDACTED]")
            .field("endpoint", &self.endpoint)
            .field("plan", &self.plan)
            .field("username", &self.username)
            .field("stored_at", &self.stored_at)
            .finish()
    }
}

pub fn build_system_prompt(mcp_skill_content: &str) -> String {
    format!(
        "You are Agora's built-in AI assistant for Minecraft mod crash diagnosis. \
         You help users identify which mod is causing a crash and suggest fixes.\n\n\
         ## Agora Tools\n\n\
         The following tools are available via the Agora MCP server. \
         In this chat interface, you don't call tools directly — instead, \
         analyze the context provided (crash log, instance data, suspect ranking) \
         and give the user a clear diagnosis and recommended action.\n\n\
         {}\n\n\
         ## Guidelines\n\
         - Be concise but thorough. Lead with the most likely cause.\n\
         - When you identify a suspect mod, explain WHY (cite the signal evidence).\n\
         - If you're unsure, say so — don't guess.\n\
         - If no mod-related cause is found, suggest other possibilities \
         (game engine, world corruption, shaders, GPU drivers, etc.).\n\
         - When recommending disabling a mod, mention the user can do it \
         via the Agora crash investigator UI or Settings → MCP → Approvals.",
        mcp_skill_content
    )
}

/// Start the GitHub Copilot device code flow.
pub async fn start_copilot_flow(client: &reqwest::Client) -> LauncherResult<CopilotDeviceFlowResponse> {
    check_network_enabled("network_github_oauth_enabled", "GitHub Copilot is disabled in Privacy settings.")?;
    let params = [
        ("client_id", COPILOT_CLIENT_ID),
        ("scope", "read:user"),
    ];

    let resp = client
        .post(COPILOT_DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?;

    let status = resp.status();
    if !status.is_success() {
        return Err(LauncherError::Generic {
            code: "ERR_COPILOT_DEVICE_FLOW".to_string(),
            message: format!("Device code flow returned HTTP {}", status.as_u16()),
        });
    }

    resp.json::<CopilotDeviceFlowResponse>().await.map_err(|e| LauncherError::Generic {
        code: "ERR_COPILOT_DEVICE_FLOW_PARSE".to_string(),
        message: format!("Failed to parse device flow response: {}", e),
    })
}

/// Poll the device flow until the user approves or it expires.
pub async fn poll_copilot_flow(
    client: &reqwest::Client,
    device_code: &str,
    interval: u64,
) -> LauncherResult<String> {
    check_network_enabled("network_github_oauth_enabled", "GitHub Copilot is disabled in Privacy settings.")?;
    let params = [
        ("client_id", COPILOT_CLIENT_ID),
        ("device_code", device_code),
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
    ];

    let mut current_interval = interval;

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(current_interval)).await;

        let resp = client
            .post(COPILOT_TOKEN_URL)
            .header("Accept", "application/json")
            .form(&params)
            .send()
            .await
            .map_err(|_| LauncherError::NetworkOffline)?;

        let status = resp.status();
        if !status.is_success() {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("");

            match error {
                "authorization_pending" => continue,
                "slow_down" => {
                    current_interval = current_interval.saturating_add(5);
                    continue;
                }
                "expired_token" => {
                    return Err(LauncherError::Generic {
                        code: "ERR_COPILOT_FLOW_EXPIRED".to_string(),
                        message: "Device code expired. Please restart the login process.".to_string(),
                    });
                }
                "access_denied" => {
                    return Err(LauncherError::Generic {
                        code: "ERR_COPILOT_FLOW_DENIED".to_string(),
                        message: "Login cancelled by user.".to_string(),
                    });
                }
                _ => {
                    return Err(LauncherError::Generic {
                        code: "ERR_COPILOT_FLOW_ERROR".to_string(),
                        message: format!("Device flow error: {}", error),
                    });
                }
            }
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| LauncherError::Generic {
            code: "ERR_COPILOT_POLL_PARSE".to_string(),
            message: format!("Failed to parse poll response: {}", e),
        })?;

        if let Some(token) = body.get("access_token").and_then(|v| v.as_str()) {
            return Ok(token.to_string());
        }

        let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
        match error {
            "authorization_pending" => continue,
            "slow_down" => {
                current_interval = current_interval.saturating_add(5);
                continue;
            }
            "expired_token" => {
                return Err(LauncherError::Generic {
                    code: "ERR_COPILOT_FLOW_EXPIRED".to_string(),
                    message: "Device code expired. Please restart the login process.".to_string(),
                });
            }
            "access_denied" => {
                return Err(LauncherError::Generic {
                    code: "ERR_COPILOT_FLOW_DENIED".to_string(),
                    message: "Login cancelled by user.".to_string(),
                });
            }
            _ => {
                return Err(LauncherError::Generic {
                    code: "ERR_COPILOT_FLOW_ERROR".to_string(),
                    message: format!("Device flow error: {}", error),
                });
            }
        }
    }
}

/// Detect which Copilot endpoint to use and resolve the full token.
pub async fn resolve_copilot_endpoint(
    client: &reqwest::Client,
    ghu_token: &str,
) -> LauncherResult<CopilotToken> {
    check_network_enabled("network_github_oauth_enabled", "GitHub Copilot is disabled in Privacy settings.")?;
    let resp = client
        .get(COPILOT_INTERNAL_USER_URL)
        .header("Authorization", format!("Bearer {}", ghu_token))
        .header("Accept", "application/json")
        .header("User-Agent", "Agora-Launcher/1.0")
        .send()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?;

    let status = resp.status();
    if !status.is_success() {
        return Err(LauncherError::Generic {
            code: "ERR_COPILOT_INTERNAL_USER".to_string(),
            message: format!("Copilot internal user endpoint returned HTTP {}", status.as_u16()),
        });
    }

    let internal_user: serde_json::Value = resp.json().await.map_err(|e| LauncherError::Generic {
        code: "ERR_COPILOT_INTERNAL_USER_PARSE".to_string(),
        message: format!("Failed to parse copilot_internal/user response: {}", e),
    })?;

    let plan = internal_user
        .get("copilot_plan")
        .and_then(|v| v.as_str())
        .unwrap_or("free")
        .to_string();

    let (endpoint, copilot_token) = match plan.as_str() {
        "business" | "enterprise" => {
            let resp = client
                .post(COPILOT_TOKEN_EXCHANGE_URL)
                .header("Authorization", format!("Bearer {}", ghu_token))
                .header("Accept", "application/json")
                .header("User-Agent", "Agora-Launcher/1.0")
                .send()
                .await
                .map_err(|_| LauncherError::NetworkOffline)?;

            let status = resp.status();
            if !status.is_success() {
                return Err(LauncherError::Generic {
                    code: "ERR_COPILOT_TOKEN_EXCHANGE".to_string(),
                    message: format!("Token exchange returned HTTP {}", status.as_u16()),
                });
            }

            let token_json: serde_json::Value = resp.json().await.map_err(|e| LauncherError::Generic {
                code: "ERR_COPILOT_TOKEN_EXCHANGE_PARSE".to_string(),
                message: format!("Failed to parse token exchange response: {}", e),
            })?;

            let session_token = token_json
                .get("token")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            (COPILOT_ENTERPRISE_ENDPOINT.to_string(), Some(session_token))
        }
        _ => (COPILOT_INDIVIDUAL_ENDPOINT.to_string(), None),
    };

    let resp = client
        .get(COPILOT_USER_URL)
        .header("Authorization", format!("Bearer {}", ghu_token))
        .header("Accept", "application/json")
        .header("User-Agent", "Agora-Launcher/1.0")
        .send()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?;

    let user_json: serde_json::Value = resp.json().await.map_err(|e| LauncherError::Generic {
        code: "ERR_COPILOT_USER_PARSE".to_string(),
        message: format!("Failed to parse user response: {}", e),
    })?;

    let username = user_json
        .get("login")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(CopilotToken {
        access_token: ghu_token.to_string(),
        copilot_token,
        endpoint,
        plan,
        username,
        stored_at: chrono::Utc::now(),
    })
}

/// Store the Copilot token in the OS keyring.
pub fn store_copilot_token(token: &CopilotToken) -> LauncherResult<()> {
    let entry = keyring::Entry::new(COPILOT_KEYRING_SERVICE, COPILOT_KEYRING_ACCOUNT)
        .map_err(|e| LauncherError::Generic {
            code: "ERR_COPILOT_KEYRING".to_string(),
            message: format!("Failed to access keyring: {}", e),
        })?;

    let json = serde_json::to_string(token).unwrap_or_default();
    entry.set_password(&json).map_err(|e| LauncherError::Generic {
        code: "ERR_COPILOT_KEYRING_WRITE".to_string(),
        message: format!("Failed to write token to keyring: {}", e),
    })?;

    Ok(())
}

/// Load the stored Copilot token from the OS keyring, if any.
pub fn load_copilot_token() -> LauncherResult<Option<CopilotToken>> {
    let entry = keyring::Entry::new(COPILOT_KEYRING_SERVICE, COPILOT_KEYRING_ACCOUNT)
        .map_err(|e| LauncherError::Generic {
            code: "ERR_COPILOT_KEYRING".to_string(),
            message: format!("Failed to access keyring: {}", e),
        })?;

    match entry.get_password() {
        Ok(json) => {
            let token: CopilotToken = serde_json::from_str(&json).map_err(|e| LauncherError::Generic {
                code: "ERR_COPILOT_STORED_PARSE".to_string(),
                message: format!("Failed to parse stored token: {}", e),
            })?;
            Ok(Some(token))
        }
        Err(e) => {
            if matches!(e, keyring::Error::NoEntry) {
                Ok(None)
            } else {
                Err(LauncherError::Generic {
                    code: "ERR_COPILOT_KEYRING_READ".to_string(),
                    message: format!("Failed to read keyring: {}", e),
                })
            }
        }
    }
}

/// Clear the stored Copilot token from the OS keyring.
pub fn clear_copilot_token() -> LauncherResult<()> {
    let entry = keyring::Entry::new(COPILOT_KEYRING_SERVICE, COPILOT_KEYRING_ACCOUNT)
        .map_err(|e| LauncherError::Generic {
            code: "ERR_COPILOT_KEYRING".to_string(),
            message: format!("Failed to access keyring: {}", e),
        })?;

    match entry.delete_password() {
        Ok(_) => Ok(()),
        Err(e) => {
            if matches!(e, keyring::Error::NoEntry) {
                Ok(())
            } else {
                Err(LauncherError::Generic {
                    code: "ERR_COPILOT_KEYRING_DELETE".to_string(),
                    message: format!("Failed to delete token: {}", e),
                })
            }
        }
    }
}

pub async fn chat_completion(
    messages: Vec<ChatMessage>,
    token: &CopilotToken,
) -> LauncherResult<ChatResponse> {
    check_network_enabled("network_github_oauth_enabled", "GitHub Copilot is disabled in Privacy settings.")?;
    let body = serde_json::json!({
        "messages": messages,
        "temperature": 0.3,
        "max_tokens": 2000,
    });

    let auth_token = token.copilot_token.as_deref().unwrap_or(&token.access_token);

    let client = reqwest::Client::builder()
        .user_agent("Agora-Launcher/1.0")
        .build()
        .map_err(|_| LauncherError::Generic {
            code: "ERR_AI_HTTP_CLIENT".to_string(),
            message: "Failed to build HTTP client for Copilot.".to_string(),
        })?;

    let resp = client
        .post(&token.endpoint)
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Editor-Version", "vscode/1.95.0")
        .header("User-Agent", "Agora-Launcher/1.0")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?;

    let status = resp.status();

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(LauncherError::Generic {
            code: "ERR_AI_RATE_LIMIT".to_string(),
            message: "You've reached 50 free Copilot requests/month.".to_string(),
        });
    }

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(LauncherError::Generic {
            code: "ERR_AI_AUTH_EXPIRED".to_string(),
            message: "GitHub Copilot token expired. Please re-login.".to_string(),
        });
    }

    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(LauncherError::Generic {
            code: "ERR_AI_REQUEST".to_string(),
            message: format!("Copilot returned status {}: {}", status.as_u16(), body_text),
        });
    }

    let parsed = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|_| LauncherError::Generic {
            code: "ERR_AI_PARSE".to_string(),
            message: "Failed to parse Copilot response.".to_string(),
        })?;

    let content = parsed
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    let response_model = parsed
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("copilot")
        .to_string();

    Ok(ChatResponse {
        content,
        model: response_model,
    })
}

pub fn build_context_message(context: &AiContext) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(ref crash_log) = context.crash_log {
        parts.push(format!("## Crash Log\n\n```\n{}\n```", crash_log));
    }

    if let Some(ref crash_signatures) = context.crash_signatures {
        parts.push(format!(
            "## Curated Crash Signatures Matched\n\n{}",
            crash_signatures
        ));
    }

    if let Some(ref suspects) = context.suspects {
        parts.push(format!("## Ranked Suspect Mods\n\n{}", suspects));
    }

    if parts.is_empty() {
        return "I need help diagnosing a Minecraft mod crash.".to_string();
    }

    parts.push(
        "## Your Task\n\nBased on the above, identify the most likely cause of the crash and recommend a fix."
            .to_string(),
    );

    parts.join("\n\n")
}

pub fn build_context_message_with_app(
    manifest_path: Option<std::path::PathBuf>,
    context: &AiContext,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(ref crash_log) = context.crash_log {
        parts.push(format!("## Crash Log\n\n```\n{}\n```", crash_log));
    }

    if let Some(ref crash_signatures) = context.crash_signatures {
        parts.push(format!(
            "## Curated Crash Signatures Matched\n\n{}",
            crash_signatures
        ));
    }

    if let Some(ref suspects) = context.suspects {
        parts.push(format!("## Ranked Suspect Mods\n\n{}", suspects));
    }

    if let Some(ref manifest_path) = manifest_path {
        if manifest_path.exists() {
            if let Ok(text) = std::fs::read_to_string(manifest_path) {
                if let Ok(manifest) = serde_json::from_str::<crate::models::InstanceManifest>(&text) {
                    let mut mod_lines: Vec<String> = Vec::new();
                    for mod_ in &manifest.mods {
                        let ver = mod_.version.as_deref().unwrap_or("unknown");
                        mod_lines.push(format!(
                            "- {} v{} (source: {})",
                            mod_.filename, ver, mod_.source
                        ));
                    }
                    if !mod_lines.is_empty() {
                        parts.push(format!(
                            "## Instance Mods\n\n{}\n\n### {}\n\n{}",
                            mod_lines.join("\n"),
                            manifest.name,
                            context.instance_id.as_deref().unwrap_or("unknown"),
                        ));
                    }
                }
            }
        }
    }

    if parts.is_empty() {
        return "I need help diagnosing a Minecraft mod crash.".to_string();
    }

    parts.push(
        "## Your Task\n\nBased on the above, identify the most likely cause of the crash and recommend a fix."
            .to_string(),
    );

    parts.join("\n\n")
}
