use crate::db;
use crate::error::{LauncherError, LauncherResult};

pub use crate::mcp::MCP_SKILL_CONTENT;
pub use agora_core::ai_assistant::{
    AiContext, AvailableModel, AVAILABLE_MODELS, ChatMessage, ChatResponse, DEFAULT_AI_MODEL,
    build_context_message,
};

pub fn build_system_prompt() -> String {
    agora_core::ai_assistant::build_system_prompt(MCP_SKILL_CONTENT)
}

pub async fn chat_completion(
    app: &tauri::AppHandle,
    messages: Vec<ChatMessage>,
    model: Option<String>,
) -> LauncherResult<ChatResponse> {
    let token = crate::auth::get_token(app).ok_or(LauncherError::AuthRequired)?;
    let model = match model {
        Some(m) if !m.is_empty() => m,
        _ => {
            let conn = crate::db::local_state_connection(app).ok();
            conn.as_ref()
                .and_then(|c| db::get_setting(c, "ai_model").ok())
                .flatten()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| agora_core::ai_assistant::DEFAULT_AI_MODEL.to_string())
        }
    };
    agora_core::ai_assistant::chat_completion(token, messages, model).await
}

pub fn build_context_message_with_app(app: &tauri::AppHandle, context: &AiContext) -> String {
    let manifest_path = context.instance_id.as_ref().and_then(|id| {
        crate::paths::instance_manifest_path(app, id).ok()
    });
    agora_core::ai_assistant::build_context_message_with_app(manifest_path, context)
}
