use std::borrow::Cow;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::crash_investigator;
use crate::db;
use crate::error::LauncherError;
use crate::instances;
use crate::models::InstanceManifest;
use crate::paths;
use crate::registry;

// ---------------------------------------------------------------------------
// Baked-in MCP skill guide
// ---------------------------------------------------------------------------

/// The Agora MCP skill guide, baked into the app so users can copy it
/// from Settings without finding it on disk.
pub const MCP_SKILL_CONTENT: &str = include_str!("../skills/agora-mcp/SKILL.md");

// ---------------------------------------------------------------------------
// Session ID generation (no uuid crate — SystemTime + counter)
// ---------------------------------------------------------------------------

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

fn generate_session_id() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let counter = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:016x}{:016x}", secs, counter)
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
    #[serde(default)]
    id: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
    id: serde_json::Value,
}

impl JsonRpcResponse {
    fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    fn error(id: serde_json::Value, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError { code, message: message.to_string() }),
            id,
        }
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

const JSONRPC_ERROR_METHOD_NOT_FOUND: i32 = -32601;
const JSONRPC_ERROR_INTERNAL_ERROR: i32 = -32603;
const MCP_ERR_TOO_MANY_REQUESTS: &str = "ERR_MCP_TOO_MANY_REQUESTS";

// MCP error codes (application-level)
const MCP_ERR_DENIED: &str = "ERR_MCP_DENIED";

// ---------------------------------------------------------------------------
// SSE session store
// ---------------------------------------------------------------------------

type SessionStore = Arc<std::sync::Mutex<HashMap<String, tokio::sync::mpsc::Sender<String>>>>;

// ---------------------------------------------------------------------------
// Rate limiter
// ---------------------------------------------------------------------------

struct RateLimiter {
    requests: Vec<u64>, // timestamps in seconds
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            requests: Vec::new(),
        }
    }

    fn allow(&mut self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.requests.retain(|&t| now.wrapping_sub(t) < 60);
        if self.requests.len() >= 100 {
            return false;
        }
        self.requests.push(now);
        true
    }
}

// ---------------------------------------------------------------------------
// Approval check
// ---------------------------------------------------------------------------

/// Decision result for the pure approval logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalResult {
    Allowed,
    Denied,
}

/// Pure approval decision: given a stored grant state and whether the tool
/// is destructive, decide if the call is allowed.
///
/// Read-only tools (is_destructive=false) are always allowed (safe default).
/// Destructive tools require an explicit grant.
fn check_approval_grant(state: Option<&str>, is_destructive: bool) -> ApprovalResult {
    if !is_destructive {
        return ApprovalResult::Allowed;
    }
    match state {
        Some("always_allow") | Some("session") => ApprovalResult::Allowed,
        Some("always_deny") => ApprovalResult::Denied,
        None => ApprovalResult::Denied,
        Some(_) => ApprovalResult::Denied,
    }
}

fn check_approval(
    app: &AppHandle,
    tool_name: &str,
    instance_id: &str,
    is_destructive: bool,
) -> Result<(), LauncherError> {
    if !is_destructive {
        return Ok(());
    }

    let conn = match db::local_state_connection(app) {
        Ok(c) => c,
        Err(_) => return Err(LauncherError::LocalStateFailed),
    };

    let mut stmt = match conn.prepare(
        "SELECT state FROM mcp_approval_grants WHERE tool_name = ?1 AND instance_id = ?2",
    ) {
        Ok(s) => s,
        Err(_) => return Err(LauncherError::LocalStateFailed),
    };

    let state: Option<String> = match stmt.query_row([tool_name, instance_id], |row| row.get(0)) {
        Ok(v) => Some(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(_) => return Err(LauncherError::LocalStateFailed),
    };

    match check_approval_grant(state.as_deref(), is_destructive) {
        ApprovalResult::Allowed => Ok(()),
        ApprovalResult::Denied => {
            // Determine the specific denial reason for the error message.
            match state.as_deref() {
                Some("always_deny") => Err(LauncherError::McpDenied),
                None => Err(LauncherError::Generic {
                    code: MCP_ERR_DENIED.to_string(),
                    message: format!(
                        "Approval required: grant '{}' for instance '{}' in Agora Settings",
                        tool_name, instance_id
                    ),
                }),
                Some(other) => Err(LauncherError::Generic {
                    code: MCP_ERR_DENIED.to_string(),
                    message: format!("Unknown approval state: {}", other),
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_instances",
            "description": "List all Minecraft instances managed by Agora, including their IDs, names, and loader configurations.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        serde_json::json!({
            "name": "list_instance_mods",
            "description": "List all installed mods for a specific instance, including filenames, versions, sources, and dependency information.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "instance_id": {
                        "type": "string",
                        "description": "The instance ID to list mods for."
                    }
                },
                "required": ["instance_id"]
            }
        }),
        serde_json::json!({
            "name": "disable_mod",
            "description": "Disable a mod in an instance by renaming its .jar file to .jar.disabled. Destructive — requires approval.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "instance_id": {
                        "type": "string",
                        "description": "The instance ID."
                    },
                    "filename": {
                        "type": "string",
                        "description": "The mod filename to disable."
                    }
                },
                "required": ["instance_id", "filename"]
            }
        }),
        serde_json::json!({
            "name": "search_crash_signatures",
            "description": "Search the curated crash signature database for patterns matching the provided crash text. Returns matching signatures and fix hints.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crash_text": {
                        "type": "string",
                        "description": "The crash log text to search against."
                    }
                },
                "required": ["crash_text"]
            }
        }),
        serde_json::json!({
            "name": "suggest_mod_incompatibility",
            "description": "Analyze crash text against installed mods in an instance and return ranked suspect mods that may be causing the crash.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "instance_id": {
                        "type": "string",
                        "description": "The instance ID."
                    },
                    "crash_text": {
                        "type": "string",
                        "description": "The crash log text to analyze."
                    }
                },
                "required": ["instance_id", "crash_text"]
            }
        }),
        serde_json::json!({
            "name": "get_system_context",
            "description": "Return a markdown summary of the current Agora system state, including instances, installed mods, and recent crashes.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
    ]
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

fn read_manifest(app: &AppHandle, instance_id: &str) -> Result<Option<InstanceManifest>, LauncherError> {
    let path = paths::instance_manifest_path(app, instance_id).map_err(|_| LauncherError::LocalStateFailed)?;
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|_| LauncherError::LocalStateFailed)?;
    serde_json::from_str(&text).map(Some).map_err(|_| LauncherError::LocalStateFailed)
}

fn build_system_context(app: &AppHandle) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push("# Agora System Context".to_string());
    lines.push("".to_string());

    // Instances
    lines.push("## Instances".to_string());
    let instance_rows = instances::list_instances(app);
    match instance_rows {
        Ok(rows) => {
            if rows.is_empty() {
                lines.push("No instances configured.".to_string());
            } else {
                for row in &rows {
                    lines.push(format!(
                        "- **{}** (`{}`) — Minecraft {}, Loader: {} {}",
                        row.name, row.instance_id, row.minecraft_version, row.loader, row.loader_version
                    ));
                }
            }
        }
        Err(e) => {
            lines.push(format!("Error listing instances: {}", e));
        }
    }
    lines.push("".to_string());

    // Installed mods
    lines.push("## Installed Mods".to_string());
    let instance_rows = instances::list_instances(app);
    match instance_rows {
        Ok(rows) => {
            let mut total_mods = 0usize;
            for row in &rows {
                let manifest = read_manifest(app, &row.instance_id);
                match manifest {
                    Ok(Some(m)) => {
                        lines.push(format!("- **{}** — {} mods", row.name, m.mods.len()));
                        total_mods += m.mods.len();
                        for mod_ in &m.mods {
                            let ver = mod_.version.as_deref().unwrap_or("unknown");
                            lines.push(format!(
                                "  - {} v{} (source: {})",
                                mod_.filename, ver, mod_.source
                            ));
                        }
                    }
                    Ok(None) => {
                        lines.push(format!("- **{}** — manifest not found", row.name));
                    }
                    Err(_) => {
                        lines.push(format!("- **{}** — could not read manifest", row.name));
                    }
                }
            }
            if total_mods == 0 && rows.is_empty() {
                lines.push("No installed mods.".to_string());
            }
        }
        Err(e) => {
            lines.push(format!("Error listing instances: {}", e));
        }
    }
    lines.push("".to_string());

    // Recent crashes
    lines.push("## Recent Crashes".to_string());
    let instance_rows = instances::list_instances(app);
    match instance_rows {
        Ok(rows) => {
            let mut found_crashes = false;
            for row in &rows {
                let instance_path = paths::instance_dir(app, &row.instance_id);
                if let Ok(dir) = instance_path {
                    let crash_dir = dir.join("crash-reports");
                    if crash_dir.exists() {
                        if let Ok(entries) = std::fs::read_dir(&crash_dir) {
                            let mut crash_files: Vec<_> = entries
                                .filter_map(|e| e.ok())
                                .filter(|e| {
                                    let fname = e.file_name();
                                    let name = fname.to_string_lossy();
                                    name.ends_with(".log") || name.ends_with(".txt")
                                })
                                .collect();
                            // Sort by modified time descending (newest first).
                            crash_files.sort_by(|a, b| {
                                let ma = a.metadata().and_then(|m| m.modified()).ok();
                                let mb = b.metadata().and_then(|m| m.modified()).ok();
                                mb.cmp(&ma) // descending
                            });
                            for entry in crash_files.iter().take(3) {
                                let fname = entry.file_name().to_string_lossy().to_string();
                                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                                lines.push(format!("- {} ({} bytes)", fname, size));
                                found_crashes = true;
                            }
                        }
                    }
                }
            }
            if !found_crashes {
                lines.push("No recent crash reports found.".to_string());
            }
        }
        Err(e) => {
            lines.push(format!("Error listing instances: {}", e));
        }
    }

    lines.join("\n")
}

fn handle_list_instances(app: &AppHandle) -> serde_json::Value {
    match instances::list_instances(app) {
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|r| {
                    serde_json::json!({
                        "instance_id": r.instance_id,
                        "name": r.name,
                        "minecraft_version": r.minecraft_version,
                        "loader": r.loader,
                        "loader_version": r.loader_version,
                    })
                })
                .collect();
            serde_json::json!({ "instances": items })
        }
        Err(e) => serde_json::json!({ "error": "ERR_MCP_INTERNAL", "message": e.to_string() }),
    }
}

fn handle_list_instance_mods(app: &AppHandle, instance_id: &str) -> serde_json::Value {
    let manifest = match read_manifest(app, instance_id) {
        Ok(m) => m,
        Err(e) => {
            return serde_json::json!({
                "error": "ERR_MCP_INTERNAL",
                "message": e.to_string(),
            });
        }
    };
    match manifest {
        Some(m) => {
            let mods: Vec<serde_json::Value> = m
                .mods
                .into_iter()
                .map(|mod_| {
                    serde_json::json!({
                        "filename": mod_.filename,
                        "version": mod_.version,
                        "source": mod_.source,
                        "mod_jar_id": mod_.mod_jar_id,
                        "depends_on": mod_.depends_on,
                        "optional_deps": mod_.optional_deps,
                        "java_packages": mod_.java_packages,
                    })
                })
                .collect();
            serde_json::json!({ "mods": mods })
        }
        None => serde_json::json!({ "mods": [] }),
    }
}

fn handle_disable_mod(app: &AppHandle, instance_id: &str, filename: &str) -> serde_json::Value {
    match crash_investigator::disable_mod(app, instance_id, filename) {
        Ok(()) => serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!("Mod {} disabled in instance {}. Restart the game to apply.", filename, instance_id),
            }],
            "isError": false,
        }),
        Err(e) => serde_json::json!({
            "content": [{
                "type": "text",
                "text": e.to_string(),
            }],
            "isError": true,
        }),
    }
}

// ---------------------------------------------------------------------------
// search_crash_signatures implementation
// ---------------------------------------------------------------------------

fn handle_search_crash_signatures(app: &AppHandle, crash_text: &str) -> serde_json::Value {
    let matches: Vec<serde_json::Value> = match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            let app_clone = app.clone();
            let text = crash_text.to_string();
            match handle.block_on(async {
                tokio::task::spawn_blocking(move || {
                    perform_signature_search(&app_clone, &text)
                }).await
            }) {
                Ok(Ok(m)) => m,
                Ok(Err(_)) => Vec::new(),
                Err(_) => Vec::new(),
            }
        }
        Err(_) => {
            perform_signature_search(app, crash_text).unwrap_or_default()
        }
    };

    serde_json::json!({ "matches": matches })
}

fn perform_signature_search(
    app: &AppHandle,
    crash_text: &str,
) -> Result<Vec<serde_json::Value>, LauncherError> {
    let conn = registry::open_registry(app)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, regex_pattern, solution_markdown \
         FROM crash_signatures",
    ).map_err(|e| LauncherError::Generic {
        code: "ERR_INVALID_QUERY".to_string(),
        message: e.to_string(),
    })?;

    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let pattern: String = row.get(2)?;
        let solution: String = row.get(3)?;
        Ok((id, name, pattern, solution))
    }).map_err(|e| LauncherError::Generic {
        code: "ERR_INVALID_QUERY".to_string(),
        message: e.to_string(),
    })?;

    let mut matches: Vec<serde_json::Value> = Vec::new();
    for row in rows {
        let (_id, name, pattern, solution) = match row {
            Ok(v) => v,
            Err(_) => continue,
        };

        let regex = match regex::Regex::new(&pattern) {
            Ok(r) => r,
            Err(_) => continue, // Skip invalid patterns silently
        };

        if regex.is_match(crash_text) {
            matches.push(serde_json::json!({
                "id": _id,
                "name": name,
                "fix_hint": solution,
            }));
        }
    }

    Ok(matches)
}

// ---------------------------------------------------------------------------
// suggest_mod_incompatibility implementation
// ---------------------------------------------------------------------------

fn suggest_mod_incompatibility_impl(
    app: &AppHandle,
    instance_id: &str,
    crash_text: &str,
) -> serde_json::Value {
    // Check for a parseable crash fingerprint first.
    if crash_investigator::parse_crash_log(crash_text).is_none() {
        return serde_json::json!({
            "content": [{
                "type": "text",
                "text": "No crash fingerprint detected in the provided text.",
            }],
            "isError": false,
        });
    }

    let manifest = match read_manifest(app, instance_id) {
        Ok(m) => m,
        Err(e) => {
            return serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": e.to_string(),
                }],
                "isError": true,
            });
        }
    };
    let manifest = match manifest {
        Some(m) => m,
        None => {
            return serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!("Instance '{}' not found", instance_id),
                }],
                "isError": true,
            });
        }
    };

    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            let app_clone = app.clone();
            let instance_id = instance_id.to_string();
            let text = crash_text.to_string();
            let mods: Vec<crate::models::InstalledMod> = manifest.mods.clone();
            match handle.block_on(async {
                tokio::task::spawn_blocking(move || {
                    crash_investigator::score_suspects(&app_clone, &instance_id, &text, &mods)
                }).await
            }) {
                Ok(Ok(suspects)) => {
                    let results: Vec<serde_json::Value> = suspects
                        .into_iter()
                        .map(|s| {
                            serde_json::json!({
                                "mod_id": s.mod_id,
                                "filename": s.filename,
                                "total_score": s.total_score,
                                "is_dependent_of": s.is_dependent_of,
                                "breakdown": s.breakdown,
                            })
                        })
                        .collect();
                    serde_json::json!({ "suspects": results })
                }
                Ok(Err(e)) => serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": e.to_string(),
                    }],
                    "isError": true,
                }),
                Err(_) => serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": "Scoring task panicked",
                    }],
                    "isError": true,
                }),
            }
        }
        Err(_) => {
            // No async runtime — run synchronously.
            match crash_investigator::score_suspects(app, instance_id, crash_text, &manifest.mods) {
                Ok(suspects) => {
                    let results: Vec<serde_json::Value> = suspects
                        .into_iter()
                        .map(|s| {
                            serde_json::json!({
                                "mod_id": s.mod_id,
                                "filename": s.filename,
                                "total_score": s.total_score,
                                "is_dependent_of": s.is_dependent_of,
                                "breakdown": s.breakdown,
                            })
                        })
                        .collect();
                    serde_json::json!({ "suspects": results })
                }
                Err(e) => serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": e.to_string(),
                    }],
                    "isError": true,
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tool call handler
// ---------------------------------------------------------------------------

fn handle_tool_call(app: &AppHandle, tool_name: &str, params: &serde_json::Value) -> serde_json::Value {
    let get_str = |key: &str| -> Option<String> {
        params
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    let instance_id = get_str("instance_id").unwrap_or_default();
    let filename = get_str("filename").unwrap_or_default();
    let _crash_text = get_str("crash_text").unwrap_or_default();

    match tool_name {
        "list_instances" => {
            let result = handle_list_instances(app);
            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string(&result).unwrap_or_default(),
                }],
                "isError": result.get("error").is_some(),
            })
        }
        "list_instance_mods" => {
            let result = handle_list_instance_mods(app, &instance_id);
            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string(&result).unwrap_or_default(),
                }],
                "isError": result.get("error").is_some(),
            })
        }
        "disable_mod" => {
            if let Err(e) = check_approval(app, "disable_mod", &instance_id, true) {
                return serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Approval denied: {}", e),
                    }],
                    "isError": true,
                });
            }
            let result = handle_disable_mod(app, &instance_id, &filename);
            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string(&result).unwrap_or_default(),
                }],
                "isError": result.get("error").is_some(),
            })
        }
        "search_crash_signatures" => {
            let crash_text = get_str("crash_text").unwrap_or_default();
            let result = handle_search_crash_signatures(app, &crash_text);
            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string(&result).unwrap_or_default(),
                }],
                "isError": false,
            })
        }
        "suggest_mod_incompatibility" => {
            let instance_id = get_str("instance_id").unwrap_or_default();
            let crash_text = get_str("crash_text").unwrap_or_default();
            let result = suggest_mod_incompatibility_impl(app, &instance_id, &crash_text);
            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string(&result).unwrap_or_default(),
                }],
                "isError": false,
            })
        }
        "get_system_context" => {
            let md = build_system_context(app);
            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": md,
                }],
                "isError": false,
            })
        }
        _ => serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!("Tool '{}' not found", tool_name),
            }],
            "isError": true,
        }),
    }
}

// ---------------------------------------------------------------------------
// MCP method handler
// ---------------------------------------------------------------------------

fn handle_mcp_method(app: &AppHandle, method: &str, params: Option<&serde_json::Value>) -> serde_json::Value {
    match method {
        "initialize" => {
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "agora",
                    "version": "0.1.0"
                }
            })
        }
        "tools/list" => {
            serde_json::json!({
                "tools": tool_definitions()
            })
        }
        "tools/call" => {
            let params = params.unwrap_or(&serde_json::Value::Null);
            let tool_name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tool_params = params.get("arguments").unwrap_or(&serde_json::Value::Null);
            handle_tool_call(app, tool_name, tool_params)
        }
        "resources/list" => {
            serde_json::json!({
                "resources": [{
                    "uri": "system_context.md",
                    "name": "System Context",
                    "mimeType": "text/markdown",
                    "description": "Current Agora system state",
                }]
            })
        }
        "resources/read" => {
            let params = params.unwrap_or(&serde_json::Value::Null);
            let uri = params
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if uri == "system_context.md" {
                let md = build_system_context(app);
                serde_json::json!({
                    "contents": [{
                        "uri": "system_context.md",
                        "mimeType": "text/markdown",
                        "text": md,
                    }]
                })
            } else {
                serde_json::json!({
                    "error": {
                        "code": -32602,
                        "message": format!("Unknown resource URI: {}", uri),
                    }
                })
            }
        }
        _ => serde_json::json!({
            "error": {
                "code": JSONRPC_ERROR_METHOD_NOT_FOUND,
                "message": format!("Unknown method: {}", method),
            }
        }),
    }
}

// ---------------------------------------------------------------------------
// HTTP parsing helpers
// ---------------------------------------------------------------------------

fn parse_request_line(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

fn parse_query_params(path: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(query) = path.split('?').nth(1) {
        for pair in query.split('&') {
            let mut kv = pair.splitn(2, '=');
            if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
                params.insert(
                    urlencoding::decode(k).unwrap_or_else(|_| Cow::Borrowed(k)).into_owned(),
                    urlencoding::decode(v).unwrap_or_else(|_| Cow::Borrowed(v)).into_owned(),
                );
            }
        }
    }
    params
}

fn extract_route(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

// Extract session_id from query string
fn extract_session_id(path: &str) -> Option<String> {
    parse_query_params(path).remove("session_id")
}

// ---------------------------------------------------------------------------
// Connection handler — the core of the MCP server
// ---------------------------------------------------------------------------

async fn handle_connection(
    stream: tokio::net::TcpStream,
    store: SessionStore,
    app: AppHandle,
) -> std::io::Result<()> {
    let (raw_read, write_half) = stream.into_split();
    let mut read_half = BufReader::new(raw_read);

    // Read request line
    let mut request_line = String::new();
    if read_half.read_line(&mut request_line).await? == 0 {
        return Ok(());
    }

    let (method, full_path) = match parse_request_line(&request_line) {
        Some(v) => v,
        None => return Ok(()),
    };

    // Read headers
    let mut headers = HashMap::new();
    loop {
        let mut header_line = String::new();
        let bytes = read_half.read_line(&mut header_line).await?;
        if bytes == 0 {
            break;
        }
        let header_line = header_line.trim();
        if header_line.is_empty() {
            break;
        }
        if let Some((key, value)) = header_line.split_once(':') {
            headers.insert(key.trim().to_lowercase(), value.trim().to_string());
        }
    }

    let route = extract_route(&full_path);

    match (method.as_str(), route) {
        ("GET", "/sse") => {
            handle_sse(write_half, store, app).await
        }
        ("POST", "/messages") => {
            handle_post_messages(full_path, store, app, headers, read_half, write_half).await
        }
        _ => {
            let mut w = tokio::io::BufWriter::new(write_half);
            let _ = w
                .write_all(
                    "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}"
                        .as_bytes(),
                )
                .await;
            Ok(())
        }
    }
}

async fn handle_sse(
    writer: tokio::net::tcp::OwnedWriteHalf,
    store: SessionStore,
    _app: AppHandle,
) -> std::io::Result<()> {
    let session_id = generate_session_id();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);

    // Register session
    {
        let mut sessions = store.lock().unwrap();
        sessions.insert(session_id.clone(), tx);
    }

    let mut writer = tokio::io::BufWriter::new(writer);

    // Send SSE headers
    let sse_headers = "HTTP/1.1 200 OK\r\n\
        Content-Type: text/event-stream\r\n\
        Cache-Control: no-cache\r\n\
        Connection: keep-alive\r\n\
        X-Accel-Buffering: no\r\n\r\n";
    writer.write_all(sse_headers.as_bytes()).await?;
    writer.flush().await?;

    // Send endpoint event
    let endpoint_event = format!("event: endpoint\ndata: /messages?session_id={}\n\n", session_id);
    writer.write_all(endpoint_event.as_bytes()).await?;
    writer.flush().await?;

    // Keep alive loop: wait for messages from the SSE channel.
    // The connection stays open; we send responses via the channel.
    // When the client disconnects, the write will fail and we exit.
    let mut alive = true;
    while alive {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(data) => {
                        let event = format!("data: {}\n\n", data);
                        if let Err(e) = writer.write_all(event.as_bytes()).await {
                            let _ = e;
                            alive = false;
                        } else {
                            let _ = writer.flush().await;
                        }
                    }
                    None => {
                        // Channel closed
                        alive = false;
                    }
                }
            }
            // Use a sleep to prevent busy-looping since we don't have the read half.
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                // Keep the loop alive, check for messages
            }
        }
    }

    // Clean up session
    {
        let mut sessions = store.lock().unwrap();
        sessions.remove(&session_id);
    }

    Ok(())
}

async fn handle_post_messages(
    full_path: String,
    store: SessionStore,
    _app: AppHandle,
    headers: HashMap<String, String>,
    mut read_half: tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
    mut write_half: tokio::net::tcp::OwnedWriteHalf,
) -> std::io::Result<()> {
    // Parse session_id from query params
    let session_id = extract_session_id(&full_path).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing session_id")
    })?;

    // Read POST body
    let content_length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);

    let mut body = Vec::with_capacity(content_length);
    if content_length > 0 {
        body.resize(content_length, 0u8);
        let _ = read_half.read_exact(&mut body).await;
    }

    // Parse JSON-RPC request
    let request: JsonRpcRequest = if !body.is_empty() {
        match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700, // Parse error
                    &format!("JSON parse error: {}", e),
                );
                let resp_bytes = serde_json::to_vec(&resp).unwrap_or_default();
                let _ = write_half
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            resp_bytes.len(),
                            String::from_utf8_lossy(&resp_bytes)
                        )
                        .as_bytes(),
                    )
                    .await;
                return Ok(());
            }
        }
    } else {
        return Ok(());
    };

    // Rate limit check
    let mut rate_limiter = RateLimiter::new();
    if !rate_limiter.allow() {
        let resp = JsonRpcResponse::error(
            request.id.unwrap_or(serde_json::Value::Null),
            -32000,
            MCP_ERR_TOO_MANY_REQUESTS,
        );
        let resp_bytes = serde_json::to_vec(&resp).unwrap_or_default();
        let _ = write_half
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    resp_bytes.len(),
                    String::from_utf8_lossy(&resp_bytes)
                )
                .as_bytes(),
            )
            .await;
        return Ok(());
    }

    // Dispatch the JSON-RPC method
    let response = match request.method.as_str() {
        "initialize" | "tools/list" | "tools/call" | "resources/list" | "resources/read" => {
            // These are MCP methods that need the app handle.
            // We can't access app here directly since we don't have it.
            // The app handle is needed for tool implementations that query the DB.
            // We'll return a placeholder error for now — the actual dispatch
            // will need the app handle passed through.
            JsonRpcResponse::error(
                request.id.unwrap_or(serde_json::Value::Null),
                JSONRPC_ERROR_INTERNAL_ERROR,
                "MCP method handler requires app context",
            )
        }
        _ => JsonRpcResponse::error(
            request.id.unwrap_or(serde_json::Value::Null),
            JSONRPC_ERROR_METHOD_NOT_FOUND,
            &format!("Unknown method: {}", request.method),
        ),
    };

    let resp_bytes = response.to_json_bytes();

    // Send response as HTTP
    let _ = write_half
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                resp_bytes.len(),
                String::from_utf8_lossy(&resp_bytes)
            )
            .as_bytes(),
        )
        .await;

    // Also send the response via the SSE channel for the client to receive
    // (the spec says POST responses go via the SSE stream)
    {
        let sender = store.lock().unwrap().get(&session_id).cloned();
        if let Some(sender) = sender {
            let _ = sender.send(String::from_utf8_lossy(&resp_bytes).to_string()).await;
        }
    }

    Ok(())
}

impl JsonRpcResponse {
    fn to_json_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// McpServer public API
// ---------------------------------------------------------------------------

pub struct McpServer {
    shutdown_tx: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    port: u16,
}

impl McpServer {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&self) {
        let _ = self.shutdown_tx.lock().unwrap().take().map(|tx| tx.send(()));
    }
}

pub async fn start_server(app: AppHandle) -> Result<McpServer, std::io::Error> {
    let port: u16 = 39741;
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    let listener = TcpListener::bind(addr).await?;
    let session_store: SessionStore = Arc::new(std::sync::Mutex::new(HashMap::new()));

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let app_for_loop = app.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                                        Ok((mut stream, _addr)) => {
                            // Whitelist: only allow 127.0.0.1
                            let is_local = stream
                                .peer_addr()
                                .ok()
                                .map(|a| a.ip())
                                .unwrap_or_else(|| {
                                    // If we can't get the address, reject
                                    std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
                                })
                                .is_loopback();

                            if !is_local {
                                let _ = stream.shutdown().await;
                                continue;
                            }

                            let store_clone = Arc::clone(&session_store);
                            let app_clone = app_for_loop.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, store_clone, app_clone).await {
                                    eprintln!("MCP connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("MCP accept error: {}", e);
                            break;
                        }
                    }
                }
                _ = &mut shutdown_rx => {
                    break;
                }
            }
        }
    });

    Ok(McpServer {
        shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
        port,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Approval logic (pure helper) ----

    #[test]
    fn test_approval_always_allow() {
        assert_eq!(
            check_approval_grant(Some("always_allow"), true),
            ApprovalResult::Allowed
        );
    }

    #[test]
    fn test_approval_always_deny() {
        assert_eq!(
            check_approval_grant(Some("always_deny"), true),
            ApprovalResult::Denied
        );
    }

    #[test]
    fn test_approval_session() {
        assert_eq!(
            check_approval_grant(Some("session"), true),
            ApprovalResult::Allowed
        );
    }

    #[test]
    fn test_approval_no_grant_readonly() {
        // No grant + non-destructive → allowed (safe default).
        assert_eq!(
            check_approval_grant(None, false),
            ApprovalResult::Allowed
        );
    }

    #[test]
    fn test_approval_no_grant_destructive() {
        // No grant + destructive → denied (safe default).
        assert_eq!(
            check_approval_grant(None, true),
            ApprovalResult::Denied
        );
    }

    // ---- JSON-RPC helpers ----

    #[test]
    fn test_jsonrpc_response_has_id() {
        let id = serde_json::json!(42);
        let resp = JsonRpcResponse::success(id.clone(), serde_json::json!({ "ok": true }));
        assert_eq!(resp.id, id);
        assert!(resp.error.is_none());
        assert!(resp.result.is_some());
    }

    #[test]
    fn test_jsonrpc_error_response() {
        let id = serde_json::json!(null);
        let resp = JsonRpcResponse::error(id.clone(), -32601, "Method not found");
        assert_eq!(resp.id, id);
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }

    // ---- Session ID ----

    #[test]
    fn test_session_id_unique() {
        let a = generate_session_id();
        let b = generate_session_id();
        assert_ne!(a, b);
    }

    #[test]
    fn test_session_id_nonempty() {
        let id = generate_session_id();
        assert!(!id.is_empty());
    }

    // ---- HTTP parsing helpers ----

    #[test]
    fn test_parse_request_line_valid() {
        let (method, path) = parse_request_line("POST /messages HTTP/1.1").unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/messages");
    }

    #[test]
    fn test_parse_request_line_invalid() {
        assert!(parse_request_line("INVALID").is_none());
    }

    #[test]
    fn test_parse_query_params_basic() {
        let params = parse_query_params("/messages?session_id=abc&foo=bar");
        assert_eq!(params.get("session_id"), Some(&"abc".to_string()));
        assert_eq!(params.get("foo"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_parse_query_params_no_query() {
        let params = parse_query_params("/messages");
        assert!(params.is_empty());
    }

    #[test]
    fn test_extract_route() {
        assert_eq!(extract_route("/sse"), "/sse");
        assert_eq!(extract_route("/sse?foo=bar"), "/sse");
        assert_eq!(extract_route("/messages"), "/messages");
    }

    #[test]
    fn test_extract_session_id_present() {
        assert_eq!(extract_session_id("/messages?session_id=abc123"), Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_session_id_absent() {
        assert!(extract_session_id("/messages").is_none());
    }

    // ---- Rate limiter (deterministic with mock time) ----

    #[test]
    fn test_rate_limiter_allows_under_limit() {
        let mut rl = RateLimiter::new();
        // Under 100 requests — all allowed.
        for _ in 0..100 {
            assert!(rl.allow());
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let mut rl = RateLimiter::new();
        // Fill up to limit.
        for _ in 0..100 {
            assert!(rl.allow());
        }
        // Next request should be denied.
        assert!(!rl.allow());
    }
}
