use crate::ai_assistant::{self, ChatMessage, ChatResponse};
use crate::auth::{DeviceFlowResponse, GithubProfile};
use crate::crash_diagnostics::{self, CrashReportInfo, CrashTriageResult};
use crate::crash_investigator;
use crate::db;
use crate::dependency_ops;
use crate::error::{LauncherError, LauncherResult};
use crate::instances::{self, CreateInstanceRequest, InstanceDetail, LoaderVersionSummary};
use crate::loader_manifests;
use crate::mcp;
use crate::mod_install;
use crate::models::{InstanceManifest, InstanceRow, InstalledMod, ModVersionCandidate};
use crate::modrinth_raw;
use crate::paths;
use crate::registry::{self, AuditLogEntry, CategoryInfo, ModReview, PackModRow, RegistryItem, SortOption, UnderReviewItem};
use crate::state::LauncherState;
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Current status of the MCP server.
#[derive(Debug, Clone, serde::Serialize)]
pub struct McpStatus {
    pub running: bool,
    pub url: String,
}

#[tauri::command]
pub async fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}

/// Browse registry items with typed filters (replaces raw-SQL queryRegistry).
///
/// When `modrinth_enabled` is false, items with `download_strategy = 'modrinth_id'`
/// are excluded from results.
#[tauri::command]
pub async fn browse_items(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    content_type: Option<String>,
    category: Option<String>,
    sort: Option<SortOption>,
    modrinth_enabled: Option<bool>,
    mc_version: Option<String>,
    loader: Option<String>,
    limit: Option<i64>,
) -> LauncherResult<Vec<RegistryItem>> {
    tokio::task::spawn_blocking(move || {
        let conn = registry::open_registry(&app)?;
        registry::browse_items(
            &conn,
            content_type.as_deref(),
            category.as_deref(),
            &sort.unwrap_or_default(),
            modrinth_enabled.unwrap_or(false),
            mc_version.as_deref(),
            loader.as_deref(),
            limit.unwrap_or(100),
        )
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_REGISTRY_QUERY".to_string(),
        message: "Registry query task failed.".to_string(),
    })?
}

/// "For You" recommendations: boost uninstalled mods whose categories overlap
/// with the user's installed mods (Â§6.2). Honors the user's selected MC version
/// and loader compatibility filters when supplied.
#[tauri::command]
pub async fn for_you_items(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    modrinth_enabled: Option<bool>,
    mc_version: Option<String>,
    loader: Option<String>,
    limit: Option<i64>,
) -> LauncherResult<Vec<RegistryItem>> {
    let modrinth_enabled = modrinth_enabled.unwrap_or(false);
    let limit = limit.unwrap_or(50).clamp(1, 500);
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        registry::for_you_items(
            &app,
            modrinth_enabled,
            mc_version.as_deref(),
            loader.as_deref(),
            limit,
        )
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_REGISTRY_QUERY".to_string(),
        message: "Registry query task failed.".to_string(),
    })?
}

/// Fetch a single registry item by ID.
#[tauri::command]
pub async fn get_registry_item(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    item_id: String,
) -> LauncherResult<Option<RegistryItem>> {
    tokio::task::spawn_blocking(move || {
        let conn = registry::open_registry(&app)?;
        registry::get_item_by_id(&conn, &item_id)
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_REGISTRY_QUERY".to_string(),
        message: "Registry query task failed.".to_string(),
    })?
}

/// List all categories from the registry.
#[tauri::command]
pub async fn list_categories(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<Vec<CategoryInfo>> {
    tokio::task::spawn_blocking(move || {
        let conn = registry::open_registry(&app)?;
        registry::list_categories(&conn)
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_REGISTRY_QUERY".to_string(),
        message: "Registry query task failed.".to_string(),
    })?
}

/// List all mods in a pack.
#[tauri::command]
pub async fn list_pack_mods(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    pack_id: String,
) -> LauncherResult<Vec<PackModRow>> {
    tokio::task::spawn_blocking(move || {
        let conn = registry::open_registry(&app)?;
        registry::pack_mods_for_pack(&conn, &pack_id)
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_REGISTRY_QUERY".to_string(),
        message: "Pack mods query task failed.".to_string(),
    })?
}

/// List audit log entries from the registry DB (Â§4.6).
#[tauri::command]
pub async fn list_audit_log(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    limit: Option<i64>,
) -> LauncherResult<Vec<AuditLogEntry>> {
    let limit = limit.unwrap_or(200).clamp(1, 1000);
    tokio::task::spawn_blocking(move || {
        let conn = registry::open_registry(&app)?;
        registry::list_audit_log(&conn, limit)
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_REGISTRY_QUERY".to_string(),
        message: "Audit log query task failed.".to_string(),
    })?
}

/// List all user instances from `local_state.db`.
#[tauri::command]
pub async fn list_instances(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<Vec<InstanceRow>> {
    tokio::task::spawn_blocking(move || instances::list_instances(&app))
        .await
        .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Fetch a single instance plus its on-disk manifest.
#[tauri::command]
pub async fn get_instance_detail(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<Option<InstanceDetail>> {
    tokio::task::spawn_blocking(move || instances::get_instance_detail(&app, &instance_id))
        .await
        .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Create a custom instance and inject its modloader.
#[tauri::command]
pub async fn create_instance(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    request: CreateInstanceRequest,
) -> LauncherResult<InstanceRow> {
    instances::create_instance(app, request).await
}

/// Delete an instance, moving its directory to the OS trash.
#[tauri::command]
pub async fn delete_instance(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<()> {
    tokio::task::spawn_blocking(move || instances::delete_instance(&app, &instance_id))
        .await
        .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Unlock a locked pack instance for manual mod management (Â§6.5).
#[tauri::command]
pub async fn unlock_instance(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<()> {
    instances::unlock_instance(&app, &instance_id).await
}

/// Lock an unlocked pack instance, discarding the lock snapshot.
#[tauri::command]
pub async fn lock_instance(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<()> {
    instances::lock_instance(&app, &instance_id).await
}

/// Revert an unlocked instance to its lock snapshot.
#[tauri::command]
pub async fn revert_instance(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<()> {
    instances::revert_instance(&app, &instance_id).await
}

/// Launch an instance via the official Mojang launcher delegation.
#[tauri::command]
pub async fn launch_instance(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<()> {
    tokio::task::spawn_blocking(move || instances::launch_instance(&app, &instance_id))
        .await
        .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Direct Java spawn — Agora owns the launch process instead of delegating to Mojang launcher.
#[tauri::command]
pub async fn launch_instance_direct(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<u32> {
    use tauri::Emitter;
    use tokio::io::AsyncBufReadExt;

    let sanitized = paths::sanitize_id(&instance_id);

    let conn = db::local_state_connection(&app)
        .map_err(|e| LauncherError::Generic { code: "ERR_DB".into(), message: e.to_string() })?;
    let row = db::get_instance(&conn, &sanitized)
        .map_err(|_| LauncherError::LocalStateFailed)?
        .ok_or(LauncherError::LaunchFailed)?;
    drop(conn);

    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_INSTANCE_PATH".into(), message: e.to_string() })?;

    let mc_version = match row.loader.as_str() {
        "fabric" => format!("fabric-loader-{}-{}", row.loader_version, row.minecraft_version),
        "quilt" => format!("quilt-loader-{}-{}", row.loader_version, row.minecraft_version),
        "neoforge" => format!("neoforge-{}", row.loader_version),
        "forge" => format!("forge-{}-{}", row.minecraft_version, row.loader_version),
        _ => format!("{}-{}-{}", row.loader, row.loader_version, row.minecraft_version),
    };

    let java_paths = agora_core::java::detect_installed_jres();
    let java_path: PathBuf = {
        let conn2 = db::local_state_connection(&app)
            .map_err(|e| LauncherError::Generic { code: "ERR_DB".into(), message: e.to_string() })?;
        let user_override = db::get_setting(&conn2, "java_path")
            .map_err(|e| LauncherError::Generic { code: "ERR_DB".into(), message: e.to_string() })?
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        drop(conn2);
        if let Some(p) = user_override {
            PathBuf::from(p)
        } else if let Some(inst) = java_paths.first() {
            inst.path.clone()
        } else {
            return Err(LauncherError::Generic {
                code: "ERR_NO_JAVA".into(),
                message: "No Java installation found. Install Java 17+ or set the path in Settings.".into(),
            });
        }
    };

    let heap_mb = row.jvm_memory_mb.max(1024);
    let gc = agora_core::gc::compute_gc(
        java_paths.first().map(|j| j.version).unwrap_or(21),
        heap_mb,
        &row.jvm_custom_args,
        None,
    );

    let assets_dir = instance_dir.parent().unwrap_or(&instance_dir).join("assets");

    let (username, access_token, uuid, user_type) =
        if let Ok(Some(creds)) = agora_core::msa::load_credentials() {
            (creds.username, creds.access_token, creds.uuid, "msa".to_string())
        } else {
            ("Player".to_string(), "0".to_string(), "00000000-0000-0000-0000-000000000000".to_string(), "mojang".to_string())
        };

    let opts = agora_core::launch::LaunchOptions {
        java_path: java_path.clone(),
        mc_version,
        game_dir: instance_dir.clone(),
        assets_dir,
        username,
        access_token,
        uuid,
        user_type,
        jvm_args: gc.jvm_args,
        mc_args_extra: vec![],
        loader: None,
    };

    let client = reqwest::Client::new();
    let manifest = agora_core::launch::fetch_version_manifest(&client).await?;

    let version_ref = manifest
        .versions
        .iter()
        .find(|v| v.id == opts.mc_version)
        .ok_or(LauncherError::VersionNotFound)?;

    let version_info = agora_core::launch::fetch_version_info(&client, &version_ref.url).await?;

    let cache_dir = dirs::data_local_dir()
        .ok_or_else(|| LauncherError::Generic {
            code: "ERR_NO_DATA_DIR".into(),
            message: "Could not determine local data directory.".into(),
        })?
        .join("agora")
        .join("lib_cache");

    let filtered = agora_core::launch::filter_libraries(&version_info.libraries);

    let natives_subdir = match std::env::consts::OS {
        "windows" => "natives/windows",
        "macos" => "natives/osx",
        _ => "natives/linux",
    };
    let natives_dir = instance_dir.join(natives_subdir);
    std::fs::create_dir_all(&natives_dir).map_err(|e| LauncherError::Generic {
        code: "ERR_NATIVES_DIR".into(),
        message: format!("Failed to create natives directory: {e}"),
    })?;

    for lib in &filtered {
        if let Some(downloads) = &lib.downloads {
            if let Some(artifact) = &downloads.artifact {
                let cache_path = cache_dir.join(&artifact.path);
                download_lib(&client, &artifact.url, &cache_path, artifact.sha1.as_deref()).await?;
            }
        }
    }

    let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
    let rel_cp = agora_core::launch::build_classpath(&version_info.libraries);
    let abs_cp = if rel_cp.is_empty() {
        String::new()
    } else {
        rel_cp
            .split(sep)
            .map(|p| {
                if p.is_empty() {
                    p.to_string()
                } else {
                    cache_dir.join(p).to_string_lossy().to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(sep)
    };

    let full_args = agora_core::launch::build_launch_command(&opts, &version_info, &abs_cp);

    let mut child = tokio::process::Command::new(&opts.java_path)
        .args(&full_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(&opts.game_dir)
        .spawn()
        .map_err(|e| LauncherError::Generic {
            code: "ERR_LAUNCH_SPAWN".into(),
            message: e.to_string(),
        })?;

    let pid = child.id().unwrap_or(0);

    let app1 = app.clone();
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let sanitized = agora_core::log_sanitizer::sanitize_log(&line);
                let _ = app1.emit("game-log", serde_json::json!({"line": sanitized, "stream": "stdout"}));
            }
        });
    }

    let app2 = app.clone();
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let sanitized = agora_core::log_sanitizer::sanitize_log(&line);
                let _ = app2.emit("game-log", serde_json::json!({"line": sanitized, "stream": "stderr"}));
            }
        });
    }

    let app3 = app.clone();
    let inst_id = instance_id.clone();
    tokio::spawn(async move {
        let status = child.wait().await;
        let exit_code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1);
        let _ = app3.emit("game-exited", serde_json::json!({
            "instance_id": inst_id,
            "exit_code": exit_code
        }));

        if let Some(win) = app3.get_webview_window("main") {
            let _ = win.show();
            let _ = win.set_focus();
        }

        if exit_code != 0 {
            let _ = app3.emit("crash-detected", serde_json::json!({
                "instance_id": inst_id,
                "exit_code": exit_code
            }));
        }
    });

    Ok(pid)
}

async fn download_lib(
    client: &reqwest::Client,
    url: &str,
    cache_path: &Path,
    expected_sha1: Option<&str>,
) -> LauncherResult<PathBuf> {
    if cache_path.is_file() {
        if let Some(sha1) = expected_sha1 {
            if let Ok(data) = std::fs::read(cache_path) {
                use sha1::Digest;
                let actual = hex::encode(sha1::Sha1::digest(&data));
                if actual == sha1 {
                    return Ok(cache_path.to_path_buf());
                }
            }
        } else {
            return Ok(cache_path.to_path_buf());
        }
    }

    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| LauncherError::Generic {
            code: "ERR_CACHE_CREATE_DIR".into(),
            message: format!("Failed to create cache directory {}: {e}", parent.display()),
        })?;
    }

    let resp = client.get(url).send().await.map_err(|_| LauncherError::NetworkOffline)?;
    if !resp.status().is_success() {
        return Err(LauncherError::Generic {
            code: "ERR_DOWNLOAD_HTTP".into(),
            message: format!("Download {url} returned HTTP {}", resp.status()),
        });
    }
    let data = resp.bytes().await.map_err(|_| LauncherError::NetworkOffline)?.to_vec();

    if let Some(sha1) = expected_sha1 {
        use sha1::Digest;
        let actual = hex::encode(sha1::Sha1::digest(&data));
        if actual != sha1 {
            return Err(LauncherError::HashMismatch);
        }
    }

    std::fs::write(cache_path, &data).map_err(|e| LauncherError::Generic {
        code: "ERR_CACHE_WRITE".into(),
        message: format!("Failed to write cache file {}: {e}", cache_path.display()),
    })?;

    Ok(cache_path.to_path_buf())
}

/// Run the pre-launch health scan on an instance. Returns a [`HealthReport`]
/// with blockers (must resolve before launch) and warnings (may override).
#[tauri::command]
pub async fn check_instance_health(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<agora_core::health::HealthReport> {
    tokio::task::spawn_blocking(move || {
        let sanitized = paths::sanitize_id(&instance_id);
        let instance_dir = paths::instance_dir(&app, &sanitized)
            .map_err(|e| LauncherError::Generic { code: "ERR_INSTANCE_PATH".into(), message: e.to_string() })?;
        let manifest = load_manifest(&app, &sanitized)?;

        // Registry DB for curated known_conflicts â€” optional (Phase 3: never required)
        let reg_path = paths::registry_db_path(&app).ok();

        Ok(agora_core::health::health(
            &instance_dir,
            &manifest,
            reg_path.as_deref(),
        ))
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// List pinned loader versions for a loader + Minecraft version.
#[tauri::command]
pub async fn list_loader_versions(
    _state: tauri::State<'_, LauncherState>,
    loader: String,
    mc_version: String,
) -> LauncherResult<Vec<LoaderVersionSummary>> {
    Ok(instances::list_loader_versions(&loader, &mc_version))
}

/// Distinct loader names present in the embedded loader manifests.
#[tauri::command]
pub async fn list_manifest_loaders(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<Vec<String>> {
    Ok(loader_manifests::list_loaders().iter().map(|s| s.to_string()).collect())
}

/// Distinct Minecraft versions across all loaders (or one loader when supplied).
#[tauri::command]
pub async fn list_manifest_mc_versions(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    loader: Option<String>,
) -> LauncherResult<Vec<String>> {
    Ok(loader_manifests::list_mc_versions(loader.as_deref()))
}

/// Read a JSON-encoded setting from `local_state.db`.
#[tauri::command]
pub async fn get_setting(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    key: String,
) -> LauncherResult<Option<serde_json::Value>> {
    tokio::task::spawn_blocking(move || {
        let conn = db::local_state_connection(&app).map_err(|_| LauncherError::LocalStateFailed)?;
        db::get_setting(&conn, &key).map_err(|_| LauncherError::LocalStateFailed)
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Upsert a JSON-encoded setting into `local_state.db`.
#[tauri::command]
pub async fn set_setting(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    key: String,
    value: serde_json::Value,
) -> LauncherResult<()> {
    tokio::task::spawn_blocking(move || {
        let conn = db::local_state_connection(&app).map_err(|_| LauncherError::LocalStateFailed)?;
        db::set_setting(&conn, &key, &value).map_err(|_| LauncherError::LocalStateFailed)
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Check GitHub Releases for a registry.db update and download + verify it.
#[tauri::command]
pub async fn check_registry_update(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    force: Option<bool>,
) -> LauncherResult<crate::registry_sync::RegistryStatus> {
    crate::registry_sync::check_and_download_update(&app, force.unwrap_or(false)).await
}

/// Return current registry status without network check.
#[tauri::command]
pub async fn get_registry_status(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<crate::registry_sync::RegistryStatus> {
    Ok(crate::registry_sync::get_status(&app))
}

/// Extract a pack override zip into an instance directory with full sanitization.
///
/// Implements Â§7.2: directory whitelist, zip-bomb limits, banned extensions,
/// and Zip Slip protection.
#[tauri::command]
pub async fn extract_overrides(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    zip_path: String,
    instance_id: String,
) -> LauncherResult<crate::override_sanitizer::ExtractionResult> {
    let zip = std::path::PathBuf::from(zip_path);
    let dest = crate::paths::instance_dir(&app, &instance_id)
        .map_err(|_| LauncherError::InstanceCreateFailed)?;
    tokio::task::spawn_blocking(move || {
        crate::override_sanitizer::extract_overrides(&zip, &dest)
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_OVERRIDE_FAILED".to_string(),
        message: "Extraction task failed.".to_string(),
    })?
}

/// Begin the GitHub OAuth Device Flow and return the code the user must enter.
#[tauri::command]
pub async fn github_login() -> LauncherResult<DeviceFlowResponse> {
    crate::auth::start_device_flow().await
}

/// Poll the GitHub token endpoint until the user authorizes the device.
/// Returns true if the token was obtained and stored; false if still pending.
#[tauri::command]
pub async fn github_login_poll(
    app: tauri::AppHandle,
    device_code: String,
    interval: u64,
) -> LauncherResult<bool> {
    crate::auth::log_line(&format!(
        "github_login_poll command ENTERED device_code_len={} interval={}",
        device_code.len(),
        interval
    ));
    let token = crate::auth::poll_device_flow(device_code, interval).await?;
    if let Some(t) = token {
        crate::auth::store_token(&app, &t)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Sign out by deleting any stored GitHub token.
#[tauri::command]
pub async fn github_logout(app: tauri::AppHandle) -> Result<(), String> {
    crate::auth::clear_token(&app)
}

/// Whether a GitHub token is currently stored.
#[tauri::command]
pub async fn get_auth_status(app: tauri::AppHandle) -> bool {
    crate::auth::is_authenticated(&app)
}

/// Fetch the authenticated user's GitHub profile, if signed in.
#[tauri::command]
pub async fn get_github_profile(app: tauri::AppHandle) -> LauncherResult<Option<GithubProfile>> {
    match crate::auth::get_token(&app) {
        Some(token) => Ok(Some(crate::auth::get_github_user(token).await?)),
        None => Ok(None),
    }
}

/// Check whether a fresh crash report appeared after the instance's last launch.
#[tauri::command]
pub async fn check_instance_crash(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<Option<CrashReportInfo>> {
    tokio::task::spawn_blocking(move || crash_diagnostics::check_for_crash(&app, &instance_id))
        .await
        .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Triage a crash log against curated signatures from the registry.
#[tauri::command]
pub async fn triage_crash_report(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    filename: String,
) -> LauncherResult<CrashTriageResult> {
    tokio::task::spawn_blocking(move || {
        crash_diagnostics::triage_crash(&app, &instance_id, &filename)
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// List all crash report files for an instance.
#[tauri::command]
pub async fn list_crash_reports_cmd(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
) -> LauncherResult<Vec<CrashReportInfo>> {
    tokio::task::spawn_blocking(move || crash_diagnostics::list_crash_reports(&app, &instance_id))
        .await
        .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Read the content of a specific crash report file.
#[tauri::command]
pub async fn read_crash_log_cmd(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    filename: String,
) -> LauncherResult<String> {
    tokio::task::spawn_blocking(move || {
        crash_diagnostics::read_crash_log(&app, &instance_id, &filename)
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// List available mod versions for a registry item, resolving live data from
/// the upstream source (GitHub Releases or Modrinth).
#[tauri::command]
pub async fn list_mod_versions(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    item_id: String,
) -> LauncherResult<Vec<ModVersionCandidate>> {
    mod_install::list_mod_versions(&app, &instance_id, &item_id).await
}

/// Install a specific mod version into an instance's `mods/` directory.
#[tauri::command]
pub async fn install_mod_version(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    item_id: String,
    candidate: ModVersionCandidate,
) -> LauncherResult<InstalledMod> {
    mod_install::install_mod_version(&app, &instance_id, &item_id, &candidate).await
}

/// Remove a mod from an instance's `mods/` directory and update the manifest.
#[tauri::command]
pub async fn remove_mod_from_instance(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    filename: String,
) -> LauncherResult<()> {
    mod_install::remove_mod_from_instance(&app, &instance_id, &filename).await
}

/// Add a manually-dropped .jar file into an instance's `mods/` folder (Â§6.5b).
#[tauri::command]
pub async fn add_manual_mod(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    source_path: String,
) -> LauncherResult<InstalledMod> {
    mod_install::add_manual_mod(&app, &instance_id, &source_path).await
}

/// Open a native file picker and return the chosen file path, or `None` if cancelled.
#[tauri::command]
pub async fn pick_open_file(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    title: String,
    extensions: Vec<String>,
) -> LauncherResult<Option<String>> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(&title);
    if !extensions.is_empty() {
        let exts: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
        dialog = dialog.add_filter("Allowed", &exts);
    }
    let picked = dialog.pick_file().await;
    Ok(picked.map(|h| h.path().to_string_lossy().to_string()))
}

/// Export an instance as a shareable pack file (Â§6.5c).
#[tauri::command]
pub async fn export_instance_pack(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    format: String,
) -> LauncherResult<String> {
    mod_install::export_instance_pack(&app, &instance_id, &format).await
}

/// Import an instance from a pack file (.mrpack or .agora-pack.json).
#[tauri::command]
pub async fn import_instance_pack(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    source_path: String,
) -> LauncherResult<String> {
    mod_install::import_instance_pack(&app, &source_path).await
}

/// Whether the Modrinth integration is currently enabled (Â§6.3 toggle).
#[tauri::command]
pub async fn is_modrinth_enabled(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<bool> {
    Ok(modrinth_raw::is_modrinth_enabled(&app))
}

/// Live search of all of Modrinth (uncurated, Â§6.3). Gated by the
/// `modrinth_enabled` setting; returns `Err(ModrinthDisabled)` when off.
#[tauri::command]
pub async fn search_modrinth(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    params: modrinth_raw::ModrinthSearchParams,
) -> LauncherResult<modrinth_raw::ModrinthSearchPage> {
    modrinth_raw::search_modrinth(&app, &params).await
}

/// List Modrinth category tags for the filter UI.
#[tauri::command]
pub async fn list_modrinth_categories(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<Vec<modrinth_raw::ModrinthCategoryInfo>> {
    modrinth_raw::list_modrinth_categories(&app).await
}

/// List Modrinth loader tags for the filter UI.
#[tauri::command]
pub async fn list_modrinth_loaders(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<Vec<modrinth_raw::ModrinthLoaderInfo>> {
    modrinth_raw::list_modrinth_loaders(&app).await
}

/// List Modrinth game version tags for the filter UI.
#[tauri::command]
pub async fn list_modrinth_game_versions(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<Vec<modrinth_raw::ModrinthGameVersionInfo>> {
    modrinth_raw::list_modrinth_game_versions(&app).await
}

/// List raw Modrinth versions for a project, optionally scoped to an
/// instance's Minecraft version and loader.
#[tauri::command]
pub async fn list_raw_modrinth_versions(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: Option<String>,
    project_id: String,
) -> LauncherResult<Vec<modrinth_raw::RawModrinthVersionCandidate>> {
    modrinth_raw::list_raw_modrinth_versions(&app, instance_id.as_deref(), &project_id).await
}

/// Install an uncurated Modrinth mod file, verified against the SHA-1 hash
/// published by Modrinth's API (Â§6.3).
#[tauri::command]
pub async fn install_raw_modrinth(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    project_id: String,
    candidate: modrinth_raw::RawModrinthVersionCandidate,
    project_type: Option<String>,
) -> LauncherResult<InstalledMod> {
    modrinth_raw::install_raw_modrinth(&app, &instance_id, &project_id, &candidate, project_type.as_deref().unwrap_or("mod")).await
}

/// Fetch a single Modrinth project's full details (including body markdown).
#[tauri::command]
pub async fn fetch_modrinth_project(
    app: tauri::AppHandle,
    project_id: String,
) -> Result<modrinth_raw::ModrinthProjectFull, LauncherError> {
    modrinth_raw::fetch_project_full(&app, &project_id).await
}

/// List registry items whose status is `under_review`, ordered by net_score.
#[tauri::command]
pub async fn list_under_review_items(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<Vec<UnderReviewItem>> {
    tokio::task::spawn_blocking(move || {
        let conn = registry::open_registry(&app)?;
        registry::list_under_review_items(&conn)
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_REGISTRY_QUERY".to_string(),
        message: "Under-review query task failed.".to_string(),
    })?
}

/// List recent triage resolutions from the audit log.
#[tauri::command]
pub async fn list_recent_resolutions(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    limit: Option<u32>,
) -> LauncherResult<Vec<AuditLogEntry>> {
    let limit = limit.unwrap_or(50);
    tokio::task::spawn_blocking(move || {
        let conn = registry::open_registry(&app)?;
        registry::list_recent_resolutions(&conn, limit)
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_REGISTRY_QUERY".to_string(),
        message: "Recent resolutions query task failed.".to_string(),
    })?
}

/// Load parsed curator reviews for a single registry item.
#[tauri::command]
pub async fn list_mod_reviews(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    item_id: String,
) -> LauncherResult<Vec<ModReview>> {
    tokio::task::spawn_blocking(move || {
        let conn = registry::open_registry(&app)?;
        registry::list_mod_reviews(&conn, item_id)
    })
    .await
    .map_err(|_| LauncherError::Generic {
        code: "ERR_REGISTRY_QUERY".to_string(),
        message: "Mod reviews query task failed.".to_string(),
    })?
}

/// Fetch the live triage poll for a mod from GitHub Discussions.
#[tauri::command]
pub async fn fetch_triage_poll(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    mod_id: String,
) -> LauncherResult<crate::governance::TriagePoll> {
    crate::governance::fetch_triage_poll(&app, mod_id).await
}

/// Submit a comment-flag for a mod, creating a GitHub issue.
#[tauri::command]
pub async fn flag_review(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    mod_id: String,
    mod_name: String,
    issue_number: i64,
    author: String,
    quoted_text: String,
    reporter_login: String,
) -> LauncherResult<String> {
    crate::governance::flag_review(&app, mod_id, mod_name, issue_number, author, quoted_text, reporter_login).await
}

/// Return the current flag rate-limit status for the local state database.
#[tauri::command]
pub async fn get_flag_rate_limit(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<agora_core::db::FlagRateLimit> {
    crate::governance::get_flag_rate_limit(&app)
}

/// Load the instance manifest for the given instance_id.
fn load_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: &str,
) -> LauncherResult<InstanceManifest> {
    let manifest_path = paths::instance_manifest_path(app, instance_id)
        .map_err(|_| LauncherError::InstanceCreateFailed)?;
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|_| LauncherError::Generic {
            code: "ERR_MANIFEST_MISSING".to_string(),
            message: format!("Instance manifest not found for '{}'.", instance_id),
        })?;
    serde_json::from_str(&text)
        .map_err(|_| LauncherError::Generic {
            code: "ERR_MANIFEST_PARSE".to_string(),
            message: "Failed to parse instance manifest.".to_string(),
        })
}

/// Investigate a crash for an instance using the auto-detected or provided
/// crash log filename. Runs the full guided-isolation pipeline.
#[tauri::command]
pub async fn investigate_crash(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    filename: Option<String>,
) -> LauncherResult<crash_investigator::InvestigationResult> {
    tokio::task::spawn_blocking(move || {
        // Determine the crash log filename.
        let filename = match filename {
            Some(f) => f,
            None => {
                let report = crash_diagnostics::check_for_crash(&app, &instance_id)
                    .map_err(|_| LauncherError::LocalStateFailed)?;
                report.ok_or_else(|| LauncherError::Generic {
                    code: "ERR_NO_CRASH_LOG".to_string(),
                    message: "No crash log detected for this instance.".to_string(),
                })?
                .filename
            }
        };

        // Read the crash log text.
        let crash_text = crash_diagnostics::read_crash_log(&app, &instance_id, &filename)
            .map_err(|_| LauncherError::Generic {
                code: "ERR_CRASH_LOG_READ".to_string(),
                message: "Could not read the crash log file.".to_string(),
            })?;

        // Load the instance manifest for installed mods.
        let manifest = load_manifest(&app, &instance_id)?;

        // Run the investigation pipeline.
        let fingerprint = match crash_investigator::parse_crash_log(&crash_text) {
            Some(fp) => fp,
            None => {
                // Can't parse â€” return empty investigation.
                return Ok(crash_investigator::InvestigationResult {
                    fingerprint: None,
                    signature_name: None,
                    suspects: Vec::new(),
                    suggested_action: crash_investigator::SuggestedAction::NoSuspects,
                    ruled_out: Vec::new(),
                });
            }
        };

        crash_investigator::continue_investigation(
            &app,
            &instance_id,
            &fingerprint,
            &manifest.mods,
            &crash_text,
        )
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Investigate a crash using a manually-provided crash log text.
#[tauri::command]
pub async fn investigate_manual(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    log_text: String,
) -> LauncherResult<crash_investigator::InvestigationResult> {
    tokio::task::spawn_blocking(move || {
        let manifest = load_manifest(&app, &instance_id)?;

        let fingerprint = match crash_investigator::parse_crash_log(&log_text) {
            Some(fp) => fp,
            None => {
                return Ok(crash_investigator::InvestigationResult {
                    fingerprint: None,
                    signature_name: None,
                    suspects: Vec::new(),
                    suggested_action: crash_investigator::SuggestedAction::NoSuspects,
                    ruled_out: Vec::new(),
                });
            }
        };

        crash_investigator::continue_investigation(
            &app,
            &instance_id,
            &fingerprint,
            &manifest.mods,
            &log_text,
        )
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Temporarily disable a mod by renaming it to `<filename>.disabled`.
#[tauri::command]
pub async fn disable_mod_for_test(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    filename: String,
) -> LauncherResult<()> {
    tokio::task::spawn_blocking(move || {
        crash_investigator::disable_mod(&app, &instance_id, &filename)
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Re-enable a previously disabled mod (rename back).
#[tauri::command]
pub async fn enable_mod_for_test(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    filename: String,
) -> LauncherResult<()> {
    tokio::task::spawn_blocking(move || {
        crash_investigator::enable_mod(&app, &instance_id, &filename)
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Confirm that a mod was the cause of a crash (for telemetry).
#[tauri::command]
pub async fn confirm_crash_fix(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    fingerprint: crash_investigator::CrashFingerprint,
    mod_id: String,
) -> LauncherResult<()> {
    tokio::task::spawn_blocking(move || {
        crash_investigator::confirm_attribution(&app, &fingerprint, &mod_id)
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Report that the crash persists after disabling the top suspect.
/// Rules out the mod and re-runs the investigation to find the next suspect.
#[tauri::command]
pub async fn report_still_crashing(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    fingerprint: crash_investigator::CrashFingerprint,
    ruled_out_mod_id: String,
    crash_log_text: String,
) -> LauncherResult<crash_investigator::InvestigationResult> {
    tokio::task::spawn_blocking(move || {
        // Rule out the mod.
        crash_investigator::rule_out(&app, &fingerprint, &ruled_out_mod_id)
            .map_err(|_| LauncherError::LocalStateFailed)?;

        // Reload the manifest and re-investigate.
        let manifest = load_manifest(&app, &instance_id)?;

        crash_investigator::continue_investigation(
            &app,
            &instance_id,
            &fingerprint,
            &manifest.mods,
            &crash_log_text,
        )
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Build a disable plan for a mod: which other installed mods would be affected
/// if this mod is disabled (renamed to `.disabled`).
#[tauri::command]
pub async fn get_disable_plan(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    filename: String,
) -> LauncherResult<dependency_ops::DisablePlan> {
    tokio::task::spawn_blocking(move || {
        let manifest = load_manifest(&app, &instance_id)?;
        let target = manifest
            .mods
            .iter()
            .find(|m| m.filename == filename)
            .ok_or_else(|| LauncherError::Generic {
                code: "ERR_MOD_NOT_FOUND".to_string(),
                message: format!("Mod '{}' not found in instance manifest.", filename),
            })?
            .clone();
        Ok(dependency_ops::build_disable_plan(&manifest.mods, &target))
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Build a removal plan for a mod: which other installed mods would break if
/// this mod is removed entirely.
#[tauri::command]
pub async fn get_removal_plan(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    filename: String,
) -> LauncherResult<dependency_ops::RemovalPlan> {
    tokio::task::spawn_blocking(move || {
        let manifest = load_manifest(&app, &instance_id)?;
        let target = manifest
            .mods
            .iter()
            .find(|m| m.filename == filename)
            .ok_or_else(|| LauncherError::Generic {
                code: "ERR_MOD_NOT_FOUND".to_string(),
                message: format!("Mod '{}' not found in instance manifest.", filename),
            })?
            .clone();
        Ok(dependency_ops::build_removal_plan(&manifest.mods, &target))
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Build an install plan for a target mod: which dependencies are missing,
/// which are optional, and whether there are any conflicts between jar and
/// manifest declarations.
#[tauri::command]
pub async fn get_install_plan(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    item_id: String,
    jar_path: String,
) -> LauncherResult<dependency_ops::InstallPlan> {
    tokio::task::spawn_blocking(move || {
        // Fetch the target mod's manifest-declared dependencies from the registry.
        let conn = registry::open_registry(&app)?;
        let manifest_deps = registry::get_manifest_dependencies(&conn, item_id)?;

        // Parse the jar for declared dependencies (defensive: bad path â†’ empty deps).
        let jar_metadata = crash_investigator::parse_jar_metadata(std::path::Path::new(&jar_path));

        // Load the target instance's installed mods to determine which deps are missing.
        let manifest = load_manifest(&app, &instance_id)?;

        Ok(dependency_ops::build_install_plan(
            manifest_deps,
            &jar_metadata,
            &manifest.mods,
        ))
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Enable a mod by renaming `<filename>.disabled` â†’ `<filename>` and
/// auto-re-enable any previously-disabled required dependencies.
///
/// Returns the list of filenames that were auto-enabled (toast messages).
/// Best-effort: individual enable failures are logged but do not abort the
/// entire operation.
#[tauri::command]
pub async fn enable_mod_with_auto_deps(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    filename: String,
) -> LauncherResult<Vec<String>> {
    tokio::task::spawn_blocking(move || {
        let manifest = load_manifest(&app, &instance_id)?;

        let target = manifest
            .mods
            .iter()
            .find(|m| m.filename == filename)
            .ok_or_else(|| LauncherError::Generic {
                code: "ERR_MOD_NOT_FOUND".to_string(),
                message: format!("Mod '{}' not found in instance manifest.", filename),
            })?;

        let mut auto_enabled: Vec<String> = Vec::new();

        // Resolve the target mod's required deps from jar metadata.
        let depends_on = match &target.mod_jar_id {
            Some(_) => &target.depends_on,
            None => &Vec::new(),
        };

        // For each required dep, find the corresponding installed mod and check
        // if it's disabled (`.disabled` file exists). If so, enable it.
        for dep_jar_id in depends_on {
            let dep_lower = dep_jar_id.to_lowercase();

            // Find the installed mod whose mod_jar_id matches this dep.
            let dep_mod = manifest.mods.iter().find(|m| {
                m.mod_jar_id
                    .as_ref()
                    .map(|jid| jid.to_lowercase() == dep_lower)
                    .unwrap_or(false)
            });

            let dep_mod = match dep_mod {
                Some(m) => m,
                None => continue, // Missing entirely â€” skip silently (can't auto-install).
            };

            // Check if the dep's jar file is disabled.
            let mods_dir = paths::instance_dir(&app, &instance_id)
                .map_err(|_| LauncherError::InstanceCreateFailed)?
                .join("mods");
            let disabled_path = mods_dir.join(format!("{}.disabled", dep_mod.filename));

            if !disabled_path.exists() {
                continue; // Already enabled.
            }

            // Best-effort enable: continue past individual failures.
            if let Err(e) = crash_investigator::enable_mod(&app, &instance_id, &dep_mod.filename) {
                crate::auth::log_line(&format!(
                    "enable_mod_with_auto_deps: failed to enable dep '{}': {}",
                    dep_mod.filename, e
                ));
                continue;
            }

            auto_enabled.push(dep_mod.filename.clone());
        }

        // Now enable the target mod itself.
        if let Err(e) = crash_investigator::enable_mod(&app, &instance_id, &filename) {
            crate::auth::log_line(&format!(
                "enable_mod_with_auto_deps: failed to enable target '{}': {}",
                filename, e
            ));
            // Still return the auto-enabled deps we managed; the target failure
            // is surfaced via the Err path below.
            return Err(LauncherError::Generic {
                code: "ERR_ENABLE_FAILED".to_string(),
                message: format!("Failed to enable '{}': {}", filename, e),
            });
        }

        Ok(auto_enabled)
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Start the MCP server if not already running.
/// Checks the `ai_mcp_enabled` setting and manages the server in Tauri state.
/// Returns the server URL.
#[tauri::command]
pub async fn start_mcp_server(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<McpStatus> {
    // If already running, return existing status.
    if let Some(server) = app.try_state::<mcp::McpServer>() {
        return Ok(McpStatus {
            running: true,
            url: format!("http://127.0.0.1:{}", server.port()),
        });
    }

    // Check if the feature is enabled.
    let conn = db::local_state_connection(&app).map_err(|_| LauncherError::LocalStateFailed)?;
    let enabled = match db::get_setting(&conn, "ai_mcp_enabled") {
        Ok(Some(val)) => val == serde_json::json!("true"),
        _ => false,
    };
    if !enabled {
        return Ok(McpStatus {
            running: false,
            url: String::new(),
        });
    }

    // Start the server.
    let app_for_start = app.clone();
    match mcp::start_server(app_for_start).await {
        Ok(server) => {
            app.manage(server);
            Ok(McpStatus {
                running: true,
                url: "http://127.0.0.1:39741".to_string(),
            })
        }
        Err(e) => Err(LauncherError::Generic {
            code: "ERR_MCP_START_FAILED".to_string(),
            message: format!("Failed to start MCP server: {}", e),
        }),
    }
}

/// Stop the MCP server if running.
#[tauri::command]
pub async fn stop_mcp_server(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<()> {
    if let Some(server) = app.try_state::<mcp::McpServer>() {
        server.stop();
    }
    Ok(())
}

/// Return the current MCP server status.
#[tauri::command]
pub async fn get_mcp_status(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<McpStatus> {
    if let Some(server) = app.try_state::<mcp::McpServer>() {
        Ok(McpStatus {
            running: true,
            url: format!("http://127.0.0.1:{}", server.port()),
        })
    } else {
        Ok(McpStatus {
            running: false,
            url: String::new(),
        })
    }
}

/// Return the baked-in MCP skill guide content.
#[tauri::command]
pub fn get_mcp_skill_content() -> String {
    crate::mcp::MCP_SKILL_CONTENT.to_string()
}

/// Record a user approval grant for an MCP tool + instance pair.
/// `state` is one of: "always_allow", "always_deny", "session".
#[tauri::command]
pub async fn set_mcp_approval(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    tool_name: String,
    instance_id: String,
    state: String,
) -> LauncherResult<()> {
    tokio::task::spawn_blocking(move || {
        let conn = db::local_state_connection(&app).map_err(|_| LauncherError::LocalStateFailed)?;
        let now = chrono::Utc::now().to_rfc3339();
        let expires_at = if state == "session" {
            // Session grants expire after 24 hours.
            Some((chrono::Utc::now() + chrono::Duration::hours(24)).to_rfc3339())
        } else {
            None
        };
        conn.execute(
            "INSERT INTO mcp_approval_grants (tool_name, instance_id, state, granted_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(tool_name, instance_id) DO UPDATE SET
                 state = excluded.state,
                 granted_at = excluded.granted_at,
                 expires_at = excluded.expires_at",
            rusqlite::params![tool_name, instance_id, state, now, expires_at],
        )
        .map_err(|_| LauncherError::LocalStateFailed)?;
        Ok(())
    })
    .await
    .map_err(|_| LauncherError::LocalStateFailed)?
}

/// Send a chat message to the AI assistant and return the response.
///
/// If `context` is provided and the messages don't already contain a context
/// message, one is prepended. A system prompt is always inserted as the first
/// message.
#[tauri::command]
pub async fn ai_chat(
    app: tauri::AppHandle,
    messages: Vec<ChatMessage>,
    context: Option<serde_json::Value>,
    model: Option<String>,
) -> Result<ChatResponse, LauncherError> {
    let mut messages = messages;

    // Build context message if context JSON is provided and not already present.
    if let Some(ctx_val) = &context {
        let has_context = messages.iter().any(|m| {
            m.role == "system"
                || (m.role == "user"
                    && (m.content.contains("## Crash Log")
                        || m.content.contains("## Ranked Suspect Mods")
                        || m.content.contains("## Curated Crash Signatures")))
        });
        if !has_context {
            // Manually extract AiContext fields from JSON (AiContext lacks Deserialize).
            let instance_id = ctx_val
                .get("instance_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let crash_log = ctx_val
                .get("crash_log")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let crash_signatures = ctx_val
                .get("crash_signatures")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let suspects = ctx_val
                .get("suspects")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let ctx = ai_assistant::AiContext {
                instance_id,
                crash_log,
                crash_signatures,
                suspects,
            };
            let context_text = ai_assistant::build_context_message(&ctx);
            messages.insert(0, ChatMessage {
                role: "user".to_string(),
                content: context_text,
            });
        }
    }

    // Ensure system prompt is first.
    if messages.is_empty() || messages[0].role != "system" {
        messages.insert(0, ChatMessage {
            role: "system".to_string(),
            content: ai_assistant::build_system_prompt(),
        });
    }

    ai_assistant::chat_completion(&app, messages, model).await
}

/// Return the list of available AI models (curated free-tier list).
#[tauri::command]
pub fn ai_get_models() -> Vec<ai_assistant::AvailableModel> {
    ai_assistant::AVAILABLE_MODELS.to_vec()
}

/// Return the default AI model ID.
#[tauri::command]
pub fn ai_get_default_model() -> String {
    ai_assistant::DEFAULT_AI_MODEL.to_string()
}

// ---------------------------------------------------------------------------
// Phase 5: MSA auth + GC architect
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct MsaBeginLoginResponse {
    pub auth_uri: String,
}

/// Begin the Microsoft Account login flow. Returns a URL the frontend should
/// open in a browser/webview. After the user completes login, call
/// `msa_finish_login` with the `?code=` from the redirect URL.
#[tauri::command]
pub async fn msa_begin_login(
    _app: tauri::AppHandle,
    state: tauri::State<'_, LauncherState>,
) -> LauncherResult<MsaBeginLoginResponse> {
    let mut s = state.lock().await;
    let flow = agora_core::msa::begin_login(&s.client).await?;
    let auth_uri = flow.auth_uri.clone();
    s.login_flow = Some(flow);
    Ok(MsaBeginLoginResponse { auth_uri })
}

/// Complete the MSA login flow with the auth code from the browser redirect.
#[tauri::command]
pub async fn msa_finish_login(
    _app: tauri::AppHandle,
    state: tauri::State<'_, LauncherState>,
    code: String,
    oauth_state: Option<String>,
) -> LauncherResult<agora_core::msa::MsaCredentials> {
    let mut s = state.lock().await;
    let flow = s.login_flow.take().ok_or_else(|| LauncherError::Generic {
        code: "ERR_MSA_NO_FLOW".into(),
        message: "No login flow in progress. Call msa_begin_login first.".into(),
    })?;
    let creds = agora_core::msa::finish_login(&s.client, &code, &flow, oauth_state.as_deref()).await?;
    Ok(creds)
}

/// Return the current MSA login status, or None if not authenticated.
#[tauri::command]
pub async fn msa_get_status(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<Option<agora_core::msa::MsaCredentials>> {
    agora_core::msa::load_credentials()
}
/// Refresh expired MSA credentials.
#[tauri::command]
pub async fn msa_refresh(
    _app: tauri::AppHandle,
    state: tauri::State<'_, LauncherState>,
) -> LauncherResult<agora_core::msa::MsaCredentials> {
    let s = state.lock().await;
    let creds = agora_core::msa::load_credentials()?.ok_or_else(|| LauncherError::Generic {
        code: "ERR_MSA_NOT_AUTHENTICATED".into(),
        message: "Not signed in. Use msa_begin_login first.".into(),
    })?;
    let refreshed = agora_core::msa::refresh_credentials(&s.client, &creds).await?;
    Ok(refreshed)
}

/// Sign out and clear stored MSA credentials.
#[tauri::command]
pub async fn msa_logout(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
) -> LauncherResult<()> {
    agora_core::msa::clear_credentials()
}

/// Compute optimal JVM GC flags for an instance.
#[tauri::command]
pub fn compute_gc_args(
    _state: tauri::State<'_, LauncherState>,
    java_version: u32,
    requested_heap_mb: i64,
    manual_args: String,
    override_profile: Option<agora_core::gc::GcProfile>,
) -> agora_core::gc::GcResult {
    agora_core::gc::compute_gc(java_version, requested_heap_mb, &manual_args, override_profile)
}

// ---------------------------------------------------------------------------
// Phase 6: Instance lifecycle — snapshots, loadouts, import, clone, export
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_snapshots(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, instance_id: String) -> LauncherResult<Vec<agora_core::snapshot::Snapshot>> {
    let sanitized = paths::sanitize_id(&instance_id);
    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    agora_core::snapshot::list_snapshots(&instance_dir)
        .map_err(|e| LauncherError::Generic { code: "ERR_SNAPSHOT".into(), message: e })
}

#[tauri::command]
pub async fn create_snapshot(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, instance_id: String, label: Option<String>) -> LauncherResult<agora_core::snapshot::Snapshot> {
    let sanitized = paths::sanitize_id(&instance_id);
    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    agora_core::snapshot::create_snapshot(&instance_dir, label.as_deref())
        .map_err(|e| LauncherError::Generic { code: "ERR_SNAPSHOT".into(), message: e })
}

#[tauri::command]
pub async fn restore_snapshot(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, instance_id: String, snapshot_id: String) -> LauncherResult<()> {
    let sanitized = paths::sanitize_id(&instance_id);
    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    agora_core::snapshot::restore_snapshot(&instance_dir, &snapshot_id)
        .map_err(|e| LauncherError::Generic { code: "ERR_SNAPSHOT".into(), message: e })
}

#[tauri::command]
pub async fn delete_snapshot(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, instance_id: String, snapshot_id: String) -> LauncherResult<()> {
    let sanitized = paths::sanitize_id(&instance_id);
    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    agora_core::snapshot::delete_snapshot(&instance_dir, &snapshot_id)
        .map_err(|e| LauncherError::Generic { code: "ERR_SNAPSHOT".into(), message: e })
}

#[tauri::command]
pub async fn list_loadout_profiles(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, instance_id: String) -> LauncherResult<Vec<agora_core::loadout::LoadoutProfile>> {
    let sanitized = paths::sanitize_id(&instance_id);
    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    agora_core::loadout::list_profiles(&instance_dir)
        .map_err(|e| LauncherError::Generic { code: "ERR_LOADOUT".into(), message: e })
}

#[tauri::command]
pub async fn create_loadout_profile(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, instance_id: String, name: String) -> LauncherResult<agora_core::loadout::LoadoutProfile> {
    let sanitized = paths::sanitize_id(&instance_id);
    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    agora_core::loadout::create_profile(&instance_dir, &name)
        .map_err(|e| LauncherError::Generic { code: "ERR_LOADOUT".into(), message: e })
}

#[tauri::command]
pub async fn apply_loadout_profile(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, instance_id: String, profile_name: String) -> LauncherResult<()> {
    let sanitized = paths::sanitize_id(&instance_id);
    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    agora_core::loadout::apply_profile(&instance_dir, &profile_name)
        .map_err(|e| LauncherError::Generic { code: "ERR_LOADOUT".into(), message: e })
}

#[tauri::command]
pub async fn delete_loadout_profile(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, instance_id: String, profile_name: String) -> LauncherResult<()> {
    let sanitized = paths::sanitize_id(&instance_id);
    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    agora_core::loadout::delete_profile(&instance_dir, &profile_name)
        .map_err(|e| LauncherError::Generic { code: "ERR_LOADOUT".into(), message: e })
}

#[tauri::command]
pub async fn import_instance(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, source_path: String, symlink_saves: bool) -> LauncherResult<agora_core::import::ImportResult> {
    let source = std::path::PathBuf::from(&source_path);
    let app_data = paths::app_data_dir(&app)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    let instances_dir = app_data.join("instances");
    std::fs::create_dir_all(&instances_dir).ok();

    if source_path.ends_with(".mrpack") {
        agora_core::import::import_mrpack(&source, &instances_dir, symlink_saves)
    } else if source_path.ends_with(".zip") {
        agora_core::import::import_prism_zip(&source, &instances_dir, symlink_saves)
    } else {
        agora_core::import::import_directory(&source, &instances_dir, symlink_saves)
    }.map_err(|e| LauncherError::Generic { code: "ERR_IMPORT".into(), message: e.to_string() })
}

#[tauri::command]
pub fn detect_launchers(_app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>) -> LauncherResult<Vec<agora_core::import::DetectedLauncher>> {
    Ok(agora_core::import::auto_detect_launchers())
}

#[tauri::command]
pub async fn clone_instance_cmd(app: tauri::AppHandle, _state: tauri::State<'_, LauncherState>, instance_id: String, new_name: String, prefs: agora_core::clone::ClonePrefs) -> LauncherResult<String> {
    let sanitized = paths::sanitize_id(&instance_id);
    let src_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    let app_data = paths::app_data_dir(&app)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    let new_id = paths::sanitize_id(&new_name);
    let dest_dir = app_data.join("instances").join(&new_id);
    agora_core::clone::clone_instance(&src_dir, &dest_dir, &prefs)
        .map_err(|e| LauncherError::Generic { code: "ERR_CLONE".into(), message: e })
}

/// Export an instance as a server environment — filters client-only mods,
/// downloads server loader, writes start scripts.
#[tauri::command]
pub async fn export_server_environment(
    app: tauri::AppHandle,
    _state: tauri::State<'_, LauncherState>,
    instance_id: String,
    dest_path: String,
) -> LauncherResult<agora_core::server_export::ExportResult> {
    let sanitized = paths::sanitize_id(&instance_id);
    let instance_dir = paths::instance_dir(&app, &sanitized)
        .map_err(|e| LauncherError::Generic { code: "ERR_PATH".into(), message: e.to_string() })?;
    let manifest = load_manifest(&app, &sanitized)?;
    let dest = std::path::PathBuf::from(&dest_path);
    std::fs::create_dir_all(&dest).ok();
    agora_core::server_export::export_server_environment(
        &instance_dir, &dest, &manifest.loader, &manifest.minecraft_version,
    ).map_err(|e| LauncherError::Generic { code: "ERR_EXPORT".into(), message: e.to_string() })
}

