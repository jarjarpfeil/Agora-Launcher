//! Direct Java spawn module — fetches Mojang version manifests, resolves
//! libraries, constructs the classpath + arguments, and spawns the JVM.
//!
//! Replaces Mojang-launcher delegation with Agora owning the entire launch
//! process end-to-end.

use crate::db;
use crate::error::{LauncherError, LauncherResult};
use regex::Regex;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MojangVersionManifest {
    pub latest: MojangLatest,
    pub versions: Vec<MojangVersionRef>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MojangLatest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MojangVersionRef {
    pub id: String,
    pub url: String,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VersionInfo {
    pub id: String,
    pub main_class: String,
    pub arguments: Option<VersionArguments>,
    #[serde(rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    pub libraries: Vec<Library>,
    pub asset_index: Option<AssetIndex>,
    pub assets: Option<String>,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VersionArguments {
    pub jvm: Vec<serde_json::Value>,
    pub game: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Library {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    pub url: Option<String>,
    pub rules: Option<Vec<LibraryRule>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<LibraryArtifact>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibraryArtifact {
    pub path: String,
    pub url: String,
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibraryRule {
    pub action: String,
    pub os: Option<LibraryOs>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibraryOs {
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetIndex {
    pub id: String,
    pub url: String,
}

/// Full launch options constructed by the caller.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub java_path: PathBuf,
    pub mc_version: String,
    pub game_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub username: String,
    pub access_token: String,
    pub uuid: String,
    pub user_type: String,
    pub jvm_args: String,
    pub mc_args_extra: Vec<String>,
    pub loader: Option<LoaderInfo>,
}

#[derive(Debug, Clone)]
pub struct SpawnResult {
    pub pid: u32,
}

// ---------------------------------------------------------------------------
// Forge/NeoForge types
// ---------------------------------------------------------------------------

/// Forge/NeoForge install profile extracted from the installer JAR.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InstallProfile {
    #[serde(default)]
    pub version: Option<VersionInfo>,
    #[serde(default)]
    pub processors: Vec<Processor>,
    #[serde(default)]
    pub data: HashMap<String, ProcessorData>,
}

/// A single processor step in a Forge/NeoForge install profile.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Processor {
    pub jar: String,
    #[serde(default)]
    pub classpath: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub outputs: Option<HashMap<String, String>>,
    #[serde(default)]
    pub sides: Option<Vec<String>>,
}

/// A data entry in the install profile, either a direct string or a
/// client/server-pair.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum ProcessorData {
    Sided {
        #[serde(default)]
        client: Option<String>,
        #[serde(default)]
        server: Option<String>,
    },
    Direct(String),
}

/// Identifies the mod loader for launch.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LoaderInfo {
    pub loader_type: String,
    pub version: String,
    /// URL to the loader's meta API (fabric/quilt) or Maven installer (forge/neoforge).
    pub version_url: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check a network enable setting from the local state DB.
/// Opens a temporary connection to `local_state.db` under the standard data dir.
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

/// The OS name as Mojang spells it in library rules.
fn mojang_os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        _ => "linux",
    }
}

/// Classpath separator: semicolon on Windows, colon elsewhere.
fn classpath_separator() -> &'static str {
    if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    }
}

/// Natives subdirectory name for the current platform.
fn natives_subdir() -> &'static str {
    match std::env::consts::OS {
        "windows" => "natives/windows",
        "macos" => "natives/osx",
        _ => "natives/linux",
    }
}

// ---------------------------------------------------------------------------
// Manifest fetching
// ---------------------------------------------------------------------------

/// Fetch the Mojang version manifest.
pub async fn fetch_version_manifest(
    client: &reqwest::Client,
) -> LauncherResult<MojangVersionManifest> {
    check_network_enabled(
        "network_modrinth_cdn_enabled",
        "Modrinth CDN downloads are disabled in Privacy settings.",
    )?;
    let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?;
    if !resp.status().is_success() {
        return Err(LauncherError::Generic {
            code: "ERR_VERSION_MANIFEST_HTTP".into(),
            message: format!("Version manifest returned HTTP {}", resp.status()),
        });
    }
    resp.json().await.map_err(|e| LauncherError::Generic {
        code: "ERR_VERSION_MANIFEST_PARSE".into(),
        message: format!("Failed to parse version manifest: {e}"),
    })
}

/// Fetch version-specific metadata JSON.
pub async fn fetch_version_info(
    client: &reqwest::Client,
    url: &str,
) -> LauncherResult<VersionInfo> {
    check_network_enabled(
        "network_modrinth_cdn_enabled",
        "Modrinth CDN downloads are disabled in Privacy settings.",
    )?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?;
    if !resp.status().is_success() {
        return Err(LauncherError::Generic {
            code: "ERR_VERSION_INFO_HTTP".into(),
            message: format!("Version info at {url} returned HTTP {}", resp.status()),
        });
    }
    resp.json().await.map_err(|e| LauncherError::Generic {
        code: "ERR_VERSION_INFO_PARSE".into(),
        message: format!("Failed to parse version info: {e}"),
    })
}

// ---------------------------------------------------------------------------
// Caching
// ---------------------------------------------------------------------------

/// Download a file to `cache_path`, optionally verifying its SHA-1 hash.
/// Skips the download if the file exists and the hash matches.
async fn download_to_path(
    client: &reqwest::Client,
    url: &str,
    cache_path: &Path,
    expected_sha1: Option<&str>,
) -> LauncherResult<PathBuf> {
    if cache_path.is_file() {
        if let Some(sha1) = expected_sha1 {
            if let Ok(data) = std::fs::read(cache_path) {
                let actual = sha1_hex(&data);
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

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?;
    if !resp.status().is_success() {
        return Err(LauncherError::Generic {
            code: "ERR_DOWNLOAD_HTTP".into(),
            message: format!("Download {url} returned HTTP {}", resp.status()),
        });
    }
    let data = resp
        .bytes()
        .await
        .map_err(|_| LauncherError::NetworkOffline)?
        .to_vec();

    if let Some(sha1) = expected_sha1 {
        let actual = sha1_hex(&data);
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

fn sha1_hex(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Maven path conversion
// ---------------------------------------------------------------------------

/// Convert Maven `group:artifact:version` to a relative jar path.
///
/// `net.minecraft:launchwrapper:1.12` →
/// `net/minecraft/launchwrapper/launchwrapper-1.12.jar`
pub fn name_to_path(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return name.replace(':', "/") + ".jar";
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    format!("{group}/{artifact}/{version}/{artifact}-{version}.jar")
}

// ---------------------------------------------------------------------------
// Library filtering
// ---------------------------------------------------------------------------

/// Check whether a library should be included based on its OS rules.
fn check_lib_allowed(lib: &Library) -> bool {
    let Some(rules) = &lib.rules else {
        return true;
    };

    let os_name = mojang_os_name();
    let mut allowed = false;
    let mut has_allow = false;

    for rule in rules {
        match rule.action.as_str() {
            "deny" => {
                if let Some(os) = &rule.os {
                    if os.name == os_name {
                        return false;
                    }
                }
            }
            "allow" => {
                has_allow = true;
                if rule.os.as_ref().map_or(true, |os| os.name == os_name) {
                    allowed = true;
                }
            }
            _ => {}
        }
    }

    if has_allow {
        allowed
    } else {
        true
    }
}

/// Filter a library list by OS rules.
pub fn filter_libraries(libs: &[Library]) -> Vec<Library> {
    libs.iter()
        .filter(|lib| check_lib_allowed(lib))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Classpath construction
// ---------------------------------------------------------------------------

/// Build a classpath string from filtered library artifact paths.
/// Uses `;` on Windows, `:` otherwise.
pub fn build_classpath(libs: &[Library]) -> String {
    let sep = classpath_separator();
    filter_libraries(libs)
        .iter()
        .filter_map(|lib| {
            lib.downloads
                .as_ref()?
                .artifact
                .as_ref()
                .map(|a| a.path.clone())
        })
        .collect::<Vec<_>>()
        .join(sep)
}

// ---------------------------------------------------------------------------
// Argument substitution
// ---------------------------------------------------------------------------

fn substitute_template(s: &str, options: &LaunchOptions, version: &VersionInfo) -> String {
    let natives_dir = options.game_dir.join(natives_subdir());
    let sep = classpath_separator();
    let uuid_no_dashes = options.uuid.replace('-', "");
    let assets_index_name = version
        .asset_index
        .as_ref()
        .map(|a| a.id.as_str())
        .unwrap_or("");

    s.replace("${auth_player_name}", &options.username)
        .replace("${auth_access_token}", &options.access_token)
        .replace("${auth_uuid}", &uuid_no_dashes)
        .replace("${user_type}", &options.user_type)
        .replace("${version_name}", &options.mc_version)
        .replace(
            "${game_directory}",
            &options.game_dir.to_string_lossy().as_ref(),
        )
        .replace(
            "${assets_root}",
            &options.assets_dir.to_string_lossy().as_ref(),
        )
        .replace("${assets_index_name}", assets_index_name)
        .replace("${version_type}", &version.type_)
        .replace(
            "${natives_directory}",
            &natives_dir.to_string_lossy().as_ref(),
        )
        .replace("${classpath_separator}", sep)
        .replace("${launcher_name}", "agora")
        .replace("${launcher_version}", "1.0.0")
        .replace("${library_directory}", "")
}

/// Check if a rules-based argument element is allowed on this platform.
fn arg_rules_pass(rules: &[LibraryRule]) -> bool {
    let os_name = mojang_os_name();
    for rule in rules {
        match rule.action.as_str() {
            "allow" => {
                if let Some(os) = &rule.os {
                    if os.name != os_name {
                        return false;
                    }
                }
            }
            "deny" => {
                if let Some(os) = &rule.os {
                    if os.name == os_name {
                        return false;
                    }
                }
            }
            _ => {}
        }
    }
    true
}

fn expand_game_argument(
    value: &serde_json::Value,
    options: &LaunchOptions,
    version: &VersionInfo,
    out: &mut Vec<String>,
) {
    if let Some(s) = value.as_str() {
        out.push(substitute_template(s, options, version));
    } else if let Some(obj) = value.as_object() {
        let rules_opt = obj
            .get("rules")
            .and_then(|v| serde_json::from_value::<Vec<LibraryRule>>(v.clone()).ok());
        if let Some(rules) = rules_opt {
            if !arg_rules_pass(&rules) {
                return;
            }
        }
        let value_val = match obj.get("value") {
            Some(v) => v,
            None => return,
        };
        if let Some(s) = value_val.as_str() {
            out.push(substitute_template(s, options, version));
        } else if let Some(arr) = value_val.as_array() {
            for item in arr {
                if let Some(s) = item.as_str() {
                    out.push(substitute_template(s, options, version));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Launch command construction
// ---------------------------------------------------------------------------

/// Construct the full argument list for spawning the JVM.
pub fn build_launch_command(
    options: &LaunchOptions,
    version_info: &VersionInfo,
    classpath: &str,
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    // 1. JVM args from the options (space-split)
    for part in options.jvm_args.split_whitespace() {
        if !part.is_empty() {
            args.push(part.to_string());
        }
    }

    // 2. Standard JVM flags
    let natives_dir = options.game_dir.join(natives_subdir());
    args.push(format!(
        "-Djava.library.path={}",
        natives_dir.to_string_lossy()
    ));
    args.push("-cp".into());
    args.push(classpath.to_string());

    // 3. Main class
    args.push(version_info.main_class.clone());

    // 4. Game arguments
    if let Some(arguments) = &version_info.arguments {
        for elem in &arguments.game {
            expand_game_argument(elem, options, version_info, &mut args);
        }
    } else if let Some(legacy) = &version_info.minecraft_arguments {
        let substituted = substitute_template(legacy, options, version_info);
        for part in substituted.split_whitespace() {
            if !part.is_empty() {
                args.push(part.to_string());
            }
        }
    }

    // 5. Extra user arguments
    for extra in &options.mc_args_extra {
        if !extra.is_empty() {
            args.push(extra.clone());
        }
    }

    args
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Full launch flow: fetch manifest → resolve version → download libs →
/// build classpath → construct args → spawn Java.
async fn spawn_java_child(options: &LaunchOptions) -> LauncherResult<tokio::process::Child> {
    check_network_enabled(
        "network_adoptium_enabled",
        "Java runtime downloads are disabled in Privacy settings.",
    )?;
    let client = reqwest::Client::new();

    // 1. Fetch version manifest
    let manifest = fetch_version_manifest(&client).await?;

    // 2. Find version entry
    let version_ref = manifest
        .versions
        .iter()
        .find(|v| v.id == options.mc_version)
        .ok_or_else(|| LauncherError::VersionNotFound)?;

    // 3. Fetch version info
    let version_info = fetch_version_info(&client, &version_ref.url).await?;

    // 4. Fetch and cache libraries
    let cache_dir = dirs::data_local_dir()
        .ok_or_else(|| LauncherError::Generic {
            code: "ERR_NO_DATA_DIR".into(),
            message: "Could not determine local data directory.".into(),
        })?
        .join("agora")
        .join("lib_cache");

    let filtered = filter_libraries(&version_info.libraries);

    // Pre-create natives dir
    let natives_dir = options.game_dir.join(natives_subdir());
    std::fs::create_dir_all(&natives_dir).map_err(|e| LauncherError::Generic {
        code: "ERR_NATIVES_DIR".into(),
        message: format!("Failed to create natives directory: {e}"),
    })?;

    for lib in &filtered {
        if let Some(downloads) = &lib.downloads {
            if let Some(artifact) = &downloads.artifact {
                let cache_path = cache_dir.join(&artifact.path);
                download_to_path(
                    &client,
                    &artifact.url,
                    &cache_path,
                    artifact.sha1.as_deref(),
                )
                .await?;
            }
        }
    }

    // 5. Build classpath (absolute paths)
    let rel_cp = build_classpath(&version_info.libraries);
    let abs_cp = if rel_cp.is_empty() {
        String::new()
    } else {
        let sep = classpath_separator();
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

    // 6. Build launch command
    let full_args = build_launch_command(options, &version_info, &abs_cp);

    // 7. Spawn Java
    let mut cmd = tokio::process::Command::new(&options.java_path);
    cmd.args(&full_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let child = cmd.spawn().map_err(|e| LauncherError::Generic {
        code: "ERR_SPAWN".into(),
        message: format!("Failed to spawn Java: {e}"),
    })?;

    child.id().ok_or_else(|| LauncherError::Generic {
        code: "ERR_NO_PID".into(),
        message: "Spawned process has no PID.".into(),
    })?;

    Ok(child)
}

/// Spawn and detach, preserving the original launcher API.
pub async fn spawn_java(options: &LaunchOptions) -> LauncherResult<SpawnResult> {
    let mut child = spawn_java_child(options).await?;
    let pid = child.id().ok_or_else(|| LauncherError::Generic {
        code: "ERR_NO_PID".into(),
        message: "Spawned process has no PID.".into(),
    })?;

    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    Ok(SpawnResult { pid })
}

/// Spawn Java and wait for its complete outcome. CLI launch uses this form so
/// the process result can be classified and tied to an exact LKG snapshot.
pub async fn spawn_java_and_wait(
    options: &LaunchOptions,
) -> LauncherResult<(SpawnResult, crate::lkg::LaunchOutcome)> {
    let child = spawn_java_child(options).await?;
    let pid = child.id().ok_or_else(|| LauncherError::Generic {
        code: "ERR_NO_PID".into(),
        message: "Spawned process has no PID.".into(),
    })?;
    let started = std::time::Instant::now();
    let launched_at = std::time::SystemTime::now();
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| LauncherError::Generic {
            code: "ERR_WAIT".into(),
            message: format!("Failed while waiting for Java: {error}"),
        })?;
    let mut captured = String::from_utf8_lossy(&output.stdout).into_owned();
    captured.push_str(&String::from_utf8_lossy(&output.stderr));
    let crash_report_found = options
        .game_dir
        .join("crash-reports")
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .is_some_and(|modified| modified >= launched_at)
        });
    let outcome = crate::lkg::classify_launch(&crate::lkg::LaunchEvents {
        exit_code: output.status.code(),
        runtime_ms: started.elapsed().as_millis() as u64,
        was_user_cancelled: false,
        crash_report_found,
        log_crash_signature_matched: crate::crash_diagnostics::triage(&captured).matched,
    });
    Ok((SpawnResult { pid }, outcome))
}

// ---------------------------------------------------------------------------
// Maven name conversion (extended)
// ---------------------------------------------------------------------------

/// Convert Maven `group:artifact:version[:classifier]` to a relative jar path.
///
/// `net.minecraftforge:forge:1.20.1-47.1.0:installer` →
/// `net/minecraftforge/forge/1.20.1-47.1.0/forge-1.20.1-47.1.0-installer.jar`
pub fn maven_name_to_path(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = if parts.len() > 3 {
        Some(parts[3])
    } else {
        None
    };
    let jar_name = match classifier {
        Some(c) => format!("{artifact}-{version}-{c}.jar"),
        None => format!("{artifact}-{version}.jar"),
    };
    format!("{group}/{artifact}/{version}/{jar_name}")
}

// ---------------------------------------------------------------------------
// Forge/NeoForge install-profile loading
// ---------------------------------------------------------------------------

/// Download and extract `install_profile.json` from a Forge/NeoForge installer
/// JAR. Caches the JAR under `cache_dir/forge_installers/`.
pub async fn load_install_profile(
    client: &reqwest::Client,
    installer_jar_url: &str,
    cache_dir: &Path,
) -> LauncherResult<InstallProfile> {
    check_network_enabled(
        "network_modrinth_cdn_enabled",
        "Modrinth CDN downloads are disabled in Privacy settings.",
    )?;
    let installer_dir = cache_dir.join("forge_installers");
    std::fs::create_dir_all(&installer_dir).map_err(|e| LauncherError::Generic {
        code: "ERR_FORGE_CACHE_DIR".into(),
        message: format!("Failed to create forge installer cache: {e}"),
    })?;

    let jar_name = installer_jar_url
        .rsplit('/')
        .next()
        .unwrap_or("installer.jar");
    let jar_name: String = jar_name
        .chars()
        .filter(|&c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        .collect();
    if jar_name.is_empty() {
        return Err(LauncherError::Generic {
            code: "ERR_FORGE_INVALID_FILENAME".into(),
            message: "Installer URL produced an empty filename after sanitization.".into(),
        });
    }
    let jar_path = installer_dir.join(&jar_name);

    download_to_path(client, installer_jar_url, &jar_path, None).await?;

    let file = std::fs::File::open(&jar_path).map_err(|e| LauncherError::Generic {
        code: "ERR_FORGE_OPEN_JAR".into(),
        message: format!("Failed to open installer JAR: {e}"),
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| LauncherError::Generic {
        code: "ERR_FORGE_ZIP".into(),
        message: format!("Failed to read installer JAR as zip: {e}"),
    })?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| LauncherError::Generic {
            code: "ERR_FORGE_ZIP_ENTRY".into(),
            message: format!("Failed to read zip entry: {e}"),
        })?;
        if entry.name() == "install_profile.json" {
            let mut buf = String::new();
            entry
                .read_to_string(&mut buf)
                .map_err(|e| LauncherError::Generic {
                    code: "ERR_FORGE_READ_PROFILE".into(),
                    message: format!("Failed to read install_profile.json: {e}"),
                })?;
            return serde_json::from_str(&buf).map_err(|e| LauncherError::Generic {
                code: "ERR_FORGE_PARSE_PROFILE".into(),
                message: format!("Failed to parse install_profile.json: {e}"),
            });
        }
    }

    Err(LauncherError::Generic {
        code: "ERR_FORGE_NO_PROFILE".into(),
        message: "install_profile.json not found in installer JAR.".into(),
    })
}

// ---------------------------------------------------------------------------
// Processor dependency downloads
// ---------------------------------------------------------------------------

const FORGE_MAVEN_REPOS: &[&str] = &[
    "https://maven.minecraftforge.net/",
    "https://maven.neoforged.net/releases/",
    "https://repo1.maven.org/maven2/",
];

/// Download all unique processor JARs (jar + classpath entries) to the Maven
/// cache under `cache_dir/forge_maven/`. Returns the list of cached paths.
pub async fn download_processor_deps(
    client: &reqwest::Client,
    profile: &InstallProfile,
    cache_dir: &Path,
) -> LauncherResult<Vec<PathBuf>> {
    check_network_enabled(
        "network_modrinth_cdn_enabled",
        "Modrinth CDN downloads are disabled in Privacy settings.",
    )?;
    let maven_cache = cache_dir.join("forge_maven");
    let mut unique_names: Vec<String> = Vec::new();

    for proc in &profile.processors {
        let name = &proc.jar;
        if !unique_names.iter().any(|n| n == name) {
            unique_names.push(name.clone());
        }
        for cp in &proc.classpath {
            if !unique_names.iter().any(|n| n == cp) {
                unique_names.push(cp.clone());
            }
        }
    }

    let mut paths = Vec::with_capacity(unique_names.len());

    for name in &unique_names {
        let rel = maven_name_to_path(name);
        let cache_path = maven_cache.join(&rel);

        if cache_path.is_file() {
            paths.push(cache_path);
            continue;
        }

        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| LauncherError::Generic {
                code: "ERR_FORGE_MAVEN_CACHE".into(),
                message: format!("Failed to create maven cache dir: {e}"),
            })?;
        }

        let mut downloaded = false;
        for repo in FORGE_MAVEN_REPOS {
            let url = format!("{repo}{rel}");
            let resp = client.get(&url).send().await;
            if let Ok(r) = resp {
                if r.status().is_success() {
                    let data = r.bytes().await.map_err(|_| LauncherError::NetworkOffline)?;
                    std::fs::write(&cache_path, &data).map_err(|e| LauncherError::Generic {
                        code: "ERR_FORGE_MAVEN_WRITE".into(),
                        message: format!("Failed to write {}: {e}", cache_path.display()),
                    })?;
                    downloaded = true;
                    break;
                }
            }
        }

        if !downloaded {
            return Err(LauncherError::Generic {
                code: "ERR_FORGE_MAVEN_DOWNLOAD".into(),
                message: format!("Failed to download {name} from any Maven repo"),
            });
        }

        paths.push(cache_path);
    }

    Ok(paths)
}

// ---------------------------------------------------------------------------
// Processor argument substitution
// ---------------------------------------------------------------------------

fn resolve_data_value(
    data: &HashMap<String, ProcessorData>,
    key: &str,
    side: &str,
) -> Option<String> {
    match data.get(key)? {
        ProcessorData::Sided { client, server } => match side {
            "client" => client.clone(),
            "server" => server.clone(),
            _ => None,
        },
        ProcessorData::Direct(s) => Some(s.clone()),
    }
}

/// Replace `${variable_name}` and `${data:VARIABLE_NAME}` placeholders in
/// processor args using the install profile's `data` section.
fn re_data_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$\{data:([^}]+)\}").unwrap())
}

fn re_direct_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").unwrap())
}

pub fn substitute_processor_args(
    args: &[String],
    data: &HashMap<String, ProcessorData>,
    side: &str,
) -> Vec<String> {
    let re_data = re_data_pattern();
    let re_direct = re_direct_pattern();
    args.iter()
        .map(|arg| {
            let mut result = arg.clone();
            result = re_data
                .replace_all(&result, |caps: &regex::Captures| {
                    resolve_data_value(data, &caps[1], side)
                        .unwrap_or_else(|| format!("${{data:{}}}", &caps[1]))
                })
                .to_string();
            result = re_direct
                .replace_all(&result, |caps: &regex::Captures| {
                    let key = &caps[1];
                    if data.contains_key(key) {
                        resolve_data_value(data, key, side).unwrap_or_else(|| format!("${{{key}}}"))
                    } else {
                        format!("${{{key}}}")
                    }
                })
                .to_string();
            result
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Processor execution
// ---------------------------------------------------------------------------

/// Run all processors from a Forge/NeoForge install profile.
///
/// Skips processors whose `sides` list does not include `"client"`.
pub async fn run_processors(
    client: &reqwest::Client,
    profile: &InstallProfile,
    java_path: &Path,
    game_dir: &Path,
    cache_dir: &Path,
) -> LauncherResult<()> {
    let maven_cache = cache_dir.join("forge_maven");

    // Ensure processor deps are downloaded first
    download_processor_deps(client, profile, cache_dir).await?;

    for (idx, proc) in profile.processors.iter().enumerate() {
        // Check sides
        if let Some(sides) = &proc.sides {
            if !sides.iter().any(|s| s == "client") {
                continue;
            }
        }

        // Resolve jar path
        let jar_rel = maven_name_to_path(&proc.jar);
        let jar_path = maven_cache.join(&jar_rel);
        if !jar_path.is_file() {
            return Err(LauncherError::Generic {
                code: "ERR_FORGE_PROCESSOR_JAR".into(),
                message: format!(
                    "Processor JAR not found: {} (resolved to {})",
                    proc.jar,
                    jar_path.display()
                ),
            });
        }

        // Build classpath
        let mut classpath: Vec<PathBuf> = proc
            .classpath
            .iter()
            .map(|cp| maven_cache.join(maven_name_to_path(cp)))
            .collect();
        classpath.push(jar_path);

        // Verify all classpath entries exist
        for cp in &classpath {
            if !cp.is_file() {
                return Err(LauncherError::Generic {
                    code: "ERR_FORGE_CP_MISSING".into(),
                    message: format!("Processor classpath entry missing: {}", cp.display()),
                });
            }
        }

        let sep = classpath_separator();
        let cp_str = classpath
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(sep);

        // Substitute args
        let side = "client";
        let substituted = substitute_processor_args(&proc.args, &profile.data, side);

        if substituted.is_empty() {
            continue;
        }

        // Build command
        let mut cmd = tokio::process::Command::new(java_path);
        cmd.arg("-cp").arg(&cp_str);

        // First arg is the main class; rest are its arguments
        let main_class = &substituted[0];
        cmd.arg(main_class);
        for a in &substituted[1..] {
            cmd.arg(a);
        }

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(game_dir);

        let output = cmd.output().await.map_err(|e| LauncherError::Generic {
            code: "ERR_FORGE_PROCESSOR_SPAWN".into(),
            message: format!("Failed to run processor {idx}: {e}"),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LauncherError::Generic {
                code: "ERR_FORGE_PROCESSOR_FAILED".into(),
                message: format!(
                    "Processor {idx} ({main_class}) failed ({}): {stderr}",
                    output.status
                ),
            });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Version merging
// ---------------------------------------------------------------------------

/// Merge a partial version info (from a Forge/NeoForge install_profile) with
/// the base Mojang version. The base version takes priority for most fields.
pub fn merge_forge_version(partial: &VersionInfo, base: &VersionInfo) -> VersionInfo {
    let mut libraries = base.libraries.clone();

    // Add partial libraries whose name doesn't already appear in base
    for plib in &partial.libraries {
        let exists = libraries.iter().any(|blib| {
            let bp = blib.name.split(':').take(2).collect::<Vec<_>>().join(":");
            let pp = plib.name.split(':').take(2).collect::<Vec<_>>().join(":");
            bp == pp
        });
        if !exists {
            libraries.push(plib.clone());
        }
    }

    let main_class = if partial.main_class.is_empty() {
        base.main_class.clone()
    } else {
        partial.main_class.clone()
    };

    let arguments = match (&partial.arguments, &base.arguments) {
        (Some(p), Some(b)) => {
            let mut jvm = b.jvm.clone();
            jvm.extend(p.jvm.clone());
            let mut game = b.game.clone();
            game.extend(p.game.clone());
            Some(VersionArguments { jvm, game })
        }
        (Some(p), None) => Some(p.clone()),
        (None, b) => b.clone(),
    };

    let minecraft_arguments = partial
        .minecraft_arguments
        .clone()
        .or_else(|| base.minecraft_arguments.clone());

    let assets = base.assets.clone();
    let asset_index = base.asset_index.clone();

    // For other fields, partial wins where present; fall back to base
    let type_ = if partial.type_.is_empty() {
        base.type_.clone()
    } else {
        partial.type_.clone()
    };

    VersionInfo {
        id: base.id.clone(),
        main_class,
        arguments,
        minecraft_arguments,
        libraries,
        asset_index,
        assets,
        type_,
    }
}

// ---------------------------------------------------------------------------
// Loader preparation
// ---------------------------------------------------------------------------

/// Fetch the base Mojang version, apply loader-specific patches, and return
/// a final `VersionInfo` ready for launch.
///
/// For Fabric/Quilt: fetches the partial version JSON from `loader.version_url`
/// and merges with the base version.
///
/// For Forge/NeoForge: downloads the installer JAR, runs all processors, then
/// merges the patched version with the base.
pub async fn prepare_loader(
    client: &reqwest::Client,
    loader: &LoaderInfo,
    mc_version: &str,
    game_dir: &Path,
    java_path: &Path,
    cache_dir: &Path,
) -> LauncherResult<VersionInfo> {
    check_network_enabled(
        "network_adoptium_enabled",
        "Java runtime downloads are disabled in Privacy settings.",
    )?;
    let manifest = fetch_version_manifest(client).await?;
    let version_ref = manifest
        .versions
        .iter()
        .find(|v| v.id == mc_version)
        .ok_or_else(|| LauncherError::VersionNotFound)?;
    let base_version = fetch_version_info(client, &version_ref.url).await?;

    match loader.loader_type.as_str() {
        "forge" | "neoforge" => {
            let profile = load_install_profile(client, &loader.version_url, cache_dir).await?;
            run_processors(client, &profile, java_path, game_dir, cache_dir).await?;

            match &profile.version {
                Some(partial) => Ok(merge_forge_version(partial, &base_version)),
                None => Err(LauncherError::Generic {
                    code: "ERR_FORGE_NO_VERSION".into(),
                    message: "install_profile.json has no version section.".into(),
                }),
            }
        }
        "fabric" | "quilt" => {
            let partial = fetch_version_info(client, &loader.version_url).await?;
            Ok(merge_forge_version(&partial, &base_version))
        }
        other => Err(LauncherError::Generic {
            code: "ERR_UNSUPPORTED_LOADER".into(),
            message: format!("Unknown loader type: {other}"),
        }),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_to_path_standard() {
        assert_eq!(
            name_to_path("net.minecraft:launchwrapper:1.12"),
            "net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
        );
    }

    #[test]
    fn name_to_path_three_parts() {
        assert_eq!(
            name_to_path("org.ow2.asm:asm-tree:9.7"),
            "org/ow2/asm/asm-tree/9.7/asm-tree-9.7.jar"
        );
    }

    #[test]
    fn name_to_path_short() {
        assert_eq!(name_to_path("a:b:1"), "a/b/1/b-1.jar");
    }

    #[test]
    fn name_to_path_no_version() {
        let result = name_to_path("a:b");
        assert!(result.ends_with(".jar"));
    }

    #[test]
    fn filter_libraries_no_rules_included() {
        let lib = Library {
            name: "test:lib:1.0".into(),
            downloads: None,
            url: None,
            rules: None,
        };
        let result = filter_libraries(&[lib]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn filter_libraries_allow_current_os() {
        let lib = Library {
            name: "test:lib:1.0".into(),
            downloads: None,
            url: None,
            rules: Some(vec![LibraryRule {
                action: "allow".into(),
                os: Some(LibraryOs {
                    name: mojang_os_name().into(),
                }),
            }]),
        };
        let result = filter_libraries(&[lib]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn filter_libraries_deny_current_os() {
        let lib = Library {
            name: "test:lib:1.0".into(),
            downloads: None,
            url: None,
            rules: Some(vec![LibraryRule {
                action: "deny".into(),
                os: Some(LibraryOs {
                    name: mojang_os_name().into(),
                }),
            }]),
        };
        let result = filter_libraries(&[lib]);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn filter_libraries_deny_other_os() {
        let other_os = if mojang_os_name() == "windows" {
            "osx"
        } else {
            "windows"
        };
        let lib = Library {
            name: "test:lib:1.0".into(),
            downloads: None,
            url: None,
            rules: Some(vec![LibraryRule {
                action: "deny".into(),
                os: Some(LibraryOs {
                    name: other_os.into(),
                }),
            }]),
        };
        let result = filter_libraries(&[lib]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn filter_libraries_allow_without_os_includes() {
        let lib = Library {
            name: "test:lib:1.0".into(),
            downloads: None,
            url: None,
            rules: Some(vec![LibraryRule {
                action: "allow".into(),
                os: None,
            }]),
        };
        let result = filter_libraries(&[lib]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn filter_libraries_multiple_rules_allow_current_deny_other() {
        let other_os = if mojang_os_name() == "windows" {
            "osx"
        } else {
            "windows"
        };
        let lib = Library {
            name: "test:lib:1.0".into(),
            downloads: None,
            url: None,
            rules: Some(vec![
                LibraryRule {
                    action: "allow".into(),
                    os: Some(LibraryOs {
                        name: mojang_os_name().into(),
                    }),
                },
                LibraryRule {
                    action: "deny".into(),
                    os: Some(LibraryOs {
                        name: other_os.into(),
                    }),
                },
            ]),
        };
        let result = filter_libraries(&[lib]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn classpath_separator_is_not_empty() {
        let sep = classpath_separator();
        assert!(!sep.is_empty());
        assert!(sep == ";" || sep == ":");
    }

    #[test]
    fn mojang_os_name_is_recognized() {
        let name = mojang_os_name();
        assert!(name == "windows" || name == "osx" || name == "linux");
    }

    #[test]
    fn build_classpath_empty_libs() {
        let cp = build_classpath(&[]);
        assert_eq!(cp, "");
    }

    #[test]
    fn build_classpath_skips_libs_without_downloads() {
        let lib = Library {
            name: "test:lib:1.0".into(),
            downloads: None,
            url: None,
            rules: None,
        };
        let cp = build_classpath(&[lib]);
        assert_eq!(cp, "");
    }

    #[test]
    fn build_classpath_with_artifact() {
        let lib = Library {
            name: "test:lib:1.0".into(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: "test/lib/1.0/lib-1.0.jar".into(),
                    url: "https://example.com/lib-1.0.jar".into(),
                    sha1: None,
                }),
            }),
            url: None,
            rules: None,
        };
        let cp = build_classpath(&[lib]);
        assert_eq!(cp, "test/lib/1.0/lib-1.0.jar");
    }

    #[test]
    fn build_classpath_multiple_artifacts() {
        let libs = vec![
            Library {
                name: "a:a:1".into(),
                downloads: Some(LibraryDownloads {
                    artifact: Some(LibraryArtifact {
                        path: "a/a/1/a-1.jar".into(),
                        url: "".into(),
                        sha1: None,
                    }),
                }),
                url: None,
                rules: None,
            },
            Library {
                name: "b:b:2".into(),
                downloads: Some(LibraryDownloads {
                    artifact: Some(LibraryArtifact {
                        path: "b/b/2/b-2.jar".into(),
                        url: "".into(),
                        sha1: None,
                    }),
                }),
                url: None,
                rules: None,
            },
        ];
        let sep = classpath_separator();
        let cp = build_classpath(&libs);
        assert_eq!(cp, format!("a/a/1/a-1.jar{sep}b/b/2/b-2.jar"));
    }

    #[test]
    fn substitute_template_auth_player_name() {
        let options = LaunchOptions {
            java_path: "java".into(),
            mc_version: "1.21".into(),
            game_dir: PathBuf::from("/game"),
            assets_dir: PathBuf::from("/assets"),
            username: "Player123".into(),
            access_token: "tok".into(),
            uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
            user_type: "msa".into(),
            jvm_args: "-Xmx2G".into(),
            mc_args_extra: vec![],
            loader: None,
        };
        let version = VersionInfo {
            id: "1.21".into(),
            main_class: "net.minecraft.client.Main".into(),
            arguments: None,
            minecraft_arguments: None,
            libraries: vec![],
            asset_index: None,
            assets: None,
            type_: "release".into(),
        };
        let result = substitute_template("${auth_player_name}", &options, &version);
        assert_eq!(result, "Player123");
    }

    #[test]
    fn substitute_template_auth_uuid_no_dashes() {
        let options = LaunchOptions {
            java_path: "java".into(),
            mc_version: "1.21".into(),
            game_dir: PathBuf::from("/game"),
            assets_dir: PathBuf::from("/assets"),
            username: "p".into(),
            access_token: "tok".into(),
            uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
            user_type: "msa".into(),
            jvm_args: "".into(),
            mc_args_extra: vec![],
            loader: None,
        };
        let version = VersionInfo {
            id: "1.21".into(),
            main_class: "x".into(),
            arguments: None,
            minecraft_arguments: None,
            libraries: vec![],
            asset_index: None,
            assets: None,
            type_: "release".into(),
        };
        let result = substitute_template("${auth_uuid}", &options, &version);
        assert_eq!(result, "550e8400e29b41d4a716446655440000");
    }

    #[test]
    fn build_launch_command_contains_main_class() {
        let options = LaunchOptions {
            java_path: "java".into(),
            mc_version: "1.21".into(),
            game_dir: PathBuf::from("/game"),
            assets_dir: PathBuf::from("/assets"),
            username: "p".into(),
            access_token: "tok".into(),
            uuid: "u".into(),
            user_type: "msa".into(),
            jvm_args: "-Xmx2G".into(),
            mc_args_extra: vec![],
            loader: None,
        };
        let version = VersionInfo {
            id: "1.21".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            arguments: None,
            minecraft_arguments: None,
            libraries: vec![],
            asset_index: None,
            assets: None,
            type_: "release".into(),
        };
        let args = build_launch_command(&options, &version, "a.jar");
        assert!(args.contains(&"net.minecraft.client.main.Main".to_string()));
    }

    #[test]
    fn build_launch_command_legacy_minecraft_arguments() {
        let options = LaunchOptions {
            java_path: "java".into(),
            mc_version: "1.12.2".into(),
            game_dir: PathBuf::from("/game"),
            assets_dir: PathBuf::from("/assets"),
            username: "Player".into(),
            access_token: "tok".into(),
            uuid: "u".into(),
            user_type: "msa".into(),
            jvm_args: "".into(),
            mc_args_extra: vec![],
            loader: None,
        };
        let version = VersionInfo {
            id: "1.12.2".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            arguments: None,
            minecraft_arguments: Some(
                "--username ${auth_player_name} --accessToken ${auth_access_token}".into(),
            ),
            libraries: vec![],
            asset_index: None,
            assets: None,
            type_: "release".into(),
        };
        let args = build_launch_command(&options, &version, "");
        assert!(args.contains(&"--username".to_string()));
        assert!(args.contains(&"Player".to_string()));
        assert!(args.contains(&"--accessToken".to_string()));
        assert!(args.contains(&"tok".to_string()));
    }

    #[test]
    fn natives_subdir_is_valid() {
        let sub = natives_subdir();
        assert!(sub.starts_with("natives/"));
        assert!(sub == "natives/windows" || sub == "natives/osx" || sub == "natives/linux");
    }

    // -----------------------------------------------------------------------
    // Forge/NeoForge tests
    // -----------------------------------------------------------------------

    #[test]
    fn maven_name_to_path_no_classifier() {
        assert_eq!(
            maven_name_to_path("net.minecraft:launchwrapper:1.12"),
            "net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
        );
    }

    #[test]
    fn maven_name_to_path_with_classifier() {
        assert_eq!(
            maven_name_to_path("net.minecraftforge:forge:1.20.1-47.1.0:installer"),
            "net/minecraftforge/forge/1.20.1-47.1.0/forge-1.20.1-47.1.0-installer.jar"
        );
    }

    #[test]
    fn maven_name_to_path_short() {
        assert_eq!(maven_name_to_path("a:b:1:classy"), "a/b/1/b-1-classy.jar");
    }

    #[test]
    fn substitute_processor_args_data_client() {
        use std::collections::HashMap;
        let mut data: HashMap<String, ProcessorData> = HashMap::new();
        data.insert(
            "BINPATCH".into(),
            ProcessorData::Sided {
                client: Some("binpatch/client.lzma".into()),
                server: Some("binpatch/server.lzma".into()),
            },
        );
        data.insert(
            "MCP_CONFIG".into(),
            ProcessorData::Direct("mcp/config.zip".into()),
        );

        let args = vec![
            "net.minecraftforge.Main".into(),
            "--input".into(),
            "${data:BINPATCH}".into(),
            "--config".into(),
            "${data:MCP_CONFIG}".into(),
        ];

        let result = substitute_processor_args(&args, &data, "client");
        assert_eq!(result[0], "net.minecraftforge.Main");
        assert_eq!(result[1], "--input");
        assert_eq!(result[2], "binpatch/client.lzma");
        assert_eq!(result[3], "--config");
        assert_eq!(result[4], "mcp/config.zip");
    }

    #[test]
    fn substitute_processor_args_server_side() {
        use std::collections::HashMap;
        let mut data: HashMap<String, ProcessorData> = HashMap::new();
        data.insert(
            "BINPATCH".into(),
            ProcessorData::Sided {
                client: Some("client.lzma".into()),
                server: Some("server.lzma".into()),
            },
        );

        let args = vec!["--file".into(), "${data:BINPATCH}".into()];
        let result = substitute_processor_args(&args, &data, "server");
        assert_eq!(result[1], "server.lzma");
    }

    #[test]
    fn substitute_processor_args_direct_key() {
        use std::collections::HashMap;
        let mut data: HashMap<String, ProcessorData> = HashMap::new();
        data.insert(
            "SIDE".into(),
            ProcessorData::Sided {
                client: Some("client".into()),
                server: Some("server".into()),
            },
        );

        let args = vec!["--side".into(), "${SIDE}".into()];
        let result = substitute_processor_args(&args, &data, "client");
        assert_eq!(result[1], "client");
    }

    #[test]
    fn substitute_processor_args_unknown_token_left_untouched() {
        use std::collections::HashMap;
        let data: HashMap<String, ProcessorData> = HashMap::new();
        let args = vec!["--dir".into(), "${game_directory}".into()];
        let result = substitute_processor_args(&args, &data, "client");
        // Unknown tokens (not in data) are left as-is
        assert_eq!(result[1], "${game_directory}");
    }

    #[test]
    fn merge_forge_version_dedup_libraries() {
        let base = VersionInfo {
            id: "1.21".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            arguments: None,
            minecraft_arguments: None,
            libraries: vec![Library {
                name: "net.minecraft:minecraft:1.21".into(),
                downloads: None,
                url: None,
                rules: None,
            }],
            asset_index: Some(AssetIndex {
                id: "1.21".into(),
                url: "https://example.com/1.21.json".into(),
            }),
            assets: Some("1.21".into()),
            type_: "release".into(),
        };

        let partial = VersionInfo {
            id: String::new(),
            main_class: "net.minecraftforge.Main".into(),
            arguments: None,
            minecraft_arguments: None,
            libraries: vec![
                Library {
                    name: "net.minecraft:minecraft:1.21".into(),
                    downloads: None,
                    url: None,
                    rules: None,
                },
                Library {
                    name: "net.minecraftforge:forge:47.1.0".into(),
                    downloads: None,
                    url: None,
                    rules: None,
                },
            ],
            asset_index: None,
            assets: None,
            type_: String::new(),
        };

        let merged = merge_forge_version(&partial, &base);
        // base id wins
        assert_eq!(merged.id, "1.21");
        // partial main_class wins
        assert_eq!(merged.main_class, "net.minecraftforge.Main");
        // dedup: minecraft library should appear only once (from base)
        assert_eq!(merged.libraries.len(), 2);
        assert!(merged
            .libraries
            .iter()
            .any(|l| l.name == "net.minecraft:minecraft:1.21"));
        assert!(merged
            .libraries
            .iter()
            .any(|l| l.name == "net.minecraftforge:forge:47.1.0"));
        // base assets win
        assert_eq!(merged.assets.as_deref(), Some("1.21"));
    }

    #[test]
    fn merge_forge_version_main_class_fallback() {
        let base = VersionInfo {
            id: "1.21".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            arguments: None,
            minecraft_arguments: None,
            libraries: vec![],
            asset_index: None,
            assets: None,
            type_: "release".into(),
        };
        let partial = VersionInfo {
            id: String::new(),
            main_class: String::new(),
            arguments: None,
            minecraft_arguments: None,
            libraries: vec![],
            asset_index: None,
            assets: None,
            type_: String::new(),
        };
        let merged = merge_forge_version(&partial, &base);
        // When partial has empty main_class, the base one is used
        assert_eq!(merged.main_class, "net.minecraft.client.main.Main");
    }

    #[test]
    fn merge_forge_version_arguments_concat() {
        let base = VersionInfo {
            id: "1.21".into(),
            main_class: "x".into(),
            arguments: Some(VersionArguments {
                jvm: vec![serde_json::json!("-Xmx2G")],
                game: vec![serde_json::json!("--username")],
            }),
            minecraft_arguments: None,
            libraries: vec![],
            asset_index: None,
            assets: None,
            type_: "release".into(),
        };
        let partial = VersionInfo {
            id: String::new(),
            main_class: "y".into(),
            arguments: Some(VersionArguments {
                jvm: vec![serde_json::json!("-Dforge=true")],
                game: vec![serde_json::json!("--accessToken")],
            }),
            minecraft_arguments: None,
            libraries: vec![],
            asset_index: None,
            assets: None,
            type_: String::new(),
        };
        let merged = merge_forge_version(&partial, &base);
        let args = merged.arguments.unwrap();
        assert_eq!(args.jvm.len(), 2);
        assert_eq!(args.game.len(), 2);
    }

    #[test]
    fn load_install_profile_with_minimal_zip() {
        // Create an in-memory zip containing install_profile.json
        use std::io::Write;
        let profile_json = r#"{
            "version": {
                "id": "",
                "main_class": "net.minecraftforge.Main",
                "libraries": [],
                "type": ""
            },
            "processors": [],
            "data": {}
        }"#;

        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("install_profile.json", options).unwrap();
            zip.write_all(profile_json.as_bytes()).unwrap();
            zip.finish().unwrap();
        }

        let data = buf.into_inner();
        let reader = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(reader).unwrap();

        let mut found = String::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            if entry.name() == "install_profile.json" {
                entry.read_to_string(&mut found).unwrap();
            }
        }

        let profile: InstallProfile = serde_json::from_str(&found).unwrap();
        assert!(profile.version.is_some());
        assert_eq!(
            profile.version.as_ref().unwrap().main_class,
            "net.minecraftforge.Main"
        );
        assert!(profile.processors.is_empty());
        assert!(profile.data.is_empty());
    }
}
