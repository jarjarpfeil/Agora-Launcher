//! Path helpers for the Agora launcher.
//!
//! Most functions here are **delegating compatibility wrappers** that forward
//! to [`crate::app_paths::AppPaths`]. New code should use `AppPaths` directly
//! to avoid redundant root-path arguments.

use crate::app_paths::AppPaths;
use std::path::{Path, PathBuf};

/// Resolve the official Minecraft data directory for the current OS.
///
/// | OS | Path |
/// |---|---|
/// | Windows | `%APPDATA%\.minecraft` |
/// | macOS | `~/Library/Application Support/minecraft` |
/// | Linux | `~/.minecraft` |
pub fn minecraft_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().map(|d| d.join(".minecraft"))
    }
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir().map(|d| d.join("minecraft"))
    }
    #[cfg(target_os = "linux")]
    {
        dirs::home_dir().map(|h| h.join(".minecraft"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

// ---------------------------------------------------------------------------
// Legacy wrappers delegating to AppPaths
// ---------------------------------------------------------------------------

/// Agora-owned Minecraft runtime root — shared content for direct launch.
///
/// All shared artifacts (vanilla client JARs, Mojang version JSONs, loader
/// profiles, libraries, assets, natives, logging configs) live under this
/// directory instead of the official `.minecraft` directory.
///
/// Instance-specific content (mods, config, saves) goes under `instances/`.
///
/// This is a pure path function — it does not create directories.
pub fn minecraft_runtime_root(app_data_dir: &Path) -> PathBuf {
    AppPaths::from_root(app_data_dir.to_path_buf()).minecraft_runtime_root()
}

/// Path helper for `minecraft-runtime/versions/<version>/<version>.json`.
pub fn minecraft_version_json(app_data_dir: &Path, version_id: &str) -> anyhow::Result<PathBuf> {
    Ok(AppPaths::from_root(app_data_dir.to_path_buf()).minecraft_version_json(version_id)?)
}

/// Path helper for `minecraft-runtime/libraries/`.
pub fn minecraft_libraries_dir(app_data_dir: &Path) -> PathBuf {
    AppPaths::from_root(app_data_dir.to_path_buf()).minecraft_libraries_dir()
}

/// Path helper for `minecraft-runtime/assets/`.
pub fn minecraft_assets_dir(app_data_dir: &Path) -> PathBuf {
    AppPaths::from_root(app_data_dir.to_path_buf()).minecraft_assets_dir()
}

/// Path helper for `minecraft-runtime/logging/`.
pub fn minecraft_logging_dir(app_data_dir: &Path) -> PathBuf {
    AppPaths::from_root(app_data_dir.to_path_buf()).minecraft_logging_dir()
}

/// Path helper for `minecraft-runtime/natives/`.
pub fn minecraft_natives_dir(app_data_dir: &Path) -> PathBuf {
    AppPaths::from_root(app_data_dir.to_path_buf()).minecraft_natives_dir()
}

/// Path helper for `minecraft-runtime/launcher_profiles.json`.
pub fn minecraft_launcher_profiles_path(app_data_dir: &Path) -> PathBuf {
    AppPaths::from_root(app_data_dir.to_path_buf()).minecraft_launcher_profiles()
}

/// Path to `launcher_profiles.json` inside the official Minecraft directory.
pub fn launcher_profiles_path() -> Option<PathBuf> {
    minecraft_dir().map(|d| d.join("launcher_profiles.json"))
}

/// The root directory holding all user instances.
pub fn instances_dir(app_data_dir: &Path) -> anyhow::Result<PathBuf> {
    let dir = AppPaths::from_root(app_data_dir.to_path_buf()).instances_root();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Directory for a single instance (e.g. `instances/<instance_id>`).
pub fn instance_dir(app_data_dir: &Path, instance_id: &str) -> anyhow::Result<PathBuf> {
    Ok(AppPaths::from_root(app_data_dir.to_path_buf()).instance_dir(instance_id)?)
}

/// Path to an instance's `instance_manifest.json`.
pub fn instance_manifest_path(app_data_dir: &Path, instance_id: &str) -> anyhow::Result<PathBuf> {
    Ok(AppPaths::from_root(app_data_dir.to_path_buf()).instance_manifest(instance_id)?)
}

/// Path to the cached read-only registry database.
pub fn registry_db_path(app_data_dir: &Path) -> anyhow::Result<PathBuf> {
    Ok(AppPaths::from_root(app_data_dir.to_path_buf()).registry_db())
}

/// Path to the cached registry.db Ed25519 signature file.
pub fn registry_sig_path(app_data_dir: &Path) -> anyhow::Result<PathBuf> {
    Ok(AppPaths::from_root(app_data_dir.to_path_buf()).registry_signature())
}

/// Path to the mutable local state database.
pub fn local_state_db_path(app_data_dir: &Path) -> anyhow::Result<PathBuf> {
    Ok(AppPaths::from_root(app_data_dir.to_path_buf()).local_state_db())
}

/// Normalize an instance id so it is safe to use as a directory name.
///
/// Allows alphanumerics, `-`, and `_`. Everything else is replaced with `-`.
pub fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize_id;

    #[test]
    fn test_sanitize_id_preserves_alphanumeric() {
        assert_eq!(sanitize_id("my-instance-1"), "my-instance-1");
    }

    #[test]
    fn test_sanitize_id_removes_path_separators() {
        let result = sanitize_id("foo/bar");
        assert!(!result.contains('/'));
        assert!(!result.contains('\\'));
    }

    #[test]
    fn test_sanitize_id_removes_dot_dot() {
        let result = sanitize_id("..");
        assert!(!result.contains(".."));
    }

    #[test]
    fn test_sanitize_id_removes_dot_dot_slash() {
        let result = sanitize_id("../etc/passwd");
        assert!(!result.contains(".."));
        assert!(!result.contains('/'));
    }

    #[test]
    fn test_sanitize_id_removes_special_chars() {
        let result = sanitize_id("foo!@#bar");
        assert!(!result.contains('!'));
        assert!(!result.contains('@'));
        assert!(!result.contains('#'));
    }

    #[test]
    fn test_sanitize_id_empty() {
        let result = sanitize_id("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_sanitize_id_unicode() {
        let result = sanitize_id("café");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_sanitize_id_null_bytes() {
        let result = sanitize_id("foo\0bar");
        assert!(!result.contains('\0'));
    }
}
