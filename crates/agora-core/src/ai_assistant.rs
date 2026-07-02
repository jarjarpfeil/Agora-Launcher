use serde::{Deserialize, Serialize};

use crate::error::{LauncherError, LauncherResult};

const GITHUB_MODELS_ENDPOINT: &str = "https://models.github.ai/inference/chat/completions";
pub const DEFAULT_AI_MODEL: &str = "openai/gpt-4.1-mini";

#[derive(Debug, Clone, Serialize)]
pub struct AvailableModel {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub free_tier: bool,
}

pub const AVAILABLE_MODELS: &[AvailableModel] = &[
    AvailableModel {
        id: "openai/gpt-4.1-mini",
        name: "GPT-4.1 Mini",
        description: "Fast and accurate, decent amount of usage — recommended for crash diagnosis.",
        free_tier: true,
    },
    AvailableModel {
        id: "openai/gpt-4.1",
        name: "GPT-4.1",
        description: "Smarter analysis for complex multi-mod crashes. Slower and less usage.",
        free_tier: true,
    },
];

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

pub async fn chat_completion(
    token: String,
    messages: Vec<ChatMessage>,
    model: String,
) -> LauncherResult<ChatResponse> {

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": 0.3,
        "max_tokens": 2000,
    });

    let client = reqwest::Client::builder()
        .user_agent("agora-launcher")
        .build()
        .map_err(|_| LauncherError::Generic {
            code: "ERR_AI_HTTP_CLIENT".to_string(),
            message: "Failed to build HTTP client for GitHub Models.".to_string(),
        })?;

    let resp = client
        .post(GITHUB_MODELS_ENDPOINT)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "agora-launcher")
        .json(&body)
        .send()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?;

    let status = resp.status();

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(LauncherError::AuthExpired);
    }

    if status == reqwest::StatusCode::FORBIDDEN {
        return Err(LauncherError::Generic {
            code: "ERR_AI_FORBIDDEN".to_string(),
            message: "GitHub Models access denied. Ensure your GitHub App has models:read permission."
                .to_string(),
        });
    }

    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(LauncherError::Generic {
            code: "ERR_AI_REQUEST".to_string(),
            message: format!("GitHub Models returned status {}: {}", status.as_u16(), body_text),
        });
    }

    let parsed = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|_| LauncherError::Generic {
            code: "ERR_AI_PARSE".to_string(),
            message: "Failed to parse GitHub Models response.".to_string(),
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
        .unwrap_or(&model)
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
