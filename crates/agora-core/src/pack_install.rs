use crate::download;
use crate::mod_cache::ModCache;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManifest {
    pub name: String,
    pub minecraft_version: String,
    pub loader: String,
    pub loader_version: String,
    pub mods: Vec<PackModEntry>,
    pub override_source: Option<OverrideSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackModEntry {
    pub id: String,
    pub source: String,
    pub version: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub identifier: String,
    pub release_tag: String,
    pub asset_name: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInstallResult {
    pub instance_id: String,
    pub name: String,
    pub mods_installed: usize,
    pub overrides_extracted: bool,
}

fn assert_safe_path(base: &Path, candidate: &Path) -> Result<(), String> {
    let resolved = base.join(candidate);
    let canonical_base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    let canonical_resolved = std::fs::canonicalize(&resolved).unwrap_or(resolved);
    if !canonical_resolved.starts_with(&canonical_base) {
        return Err(format!("Path traversal detected: {}", candidate.display()));
    }
    Ok(())
}

async fn modrinth_version_download_url(
    client: &reqwest::Client,
    project_id: &str,
    version: Option<&str>,
) -> Result<(String, String, String), String> {
    let url = format!(
        "https://api.modrinth.com/v2/project/{}/version",
        urlencoding::encode(project_id)
    );

    let versions: Vec<serde_json::Value> = client
        .get(&url)
        .header("User-Agent", "AgoraLauncher/1.0")
        .send()
        .await
        .map_err(|e| format!("Modrinth API request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Modrinth API HTTP error: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse Modrinth versions response: {}", e))?;

    let selected = if let Some(ver) = version {
        versions
            .iter()
            .find(|v| v["version_number"].as_str() == Some(ver))
            .ok_or_else(|| format!("Version '{}' not found for project '{}'", ver, project_id))?
    } else {
        versions
            .first()
            .ok_or_else(|| format!("No versions found for project '{}'", project_id))?
    };

    let files = selected["files"]
        .as_array()
        .ok_or_else(|| "Modrinth response missing 'files' array".to_string())?;

    let primary = files
        .iter()
        .find(|f| f["primary"].as_bool() == Some(true))
        .or_else(|| files.first())
        .ok_or_else(|| "No files found in version".to_string())?;

    let download_url = primary["url"]
        .as_str()
        .ok_or_else(|| "Missing download URL".to_string())?
        .to_string();
    let filename = primary["filename"]
        .as_str()
        .ok_or_else(|| "Missing filename".to_string())?
        .to_string();
    let sha1 = primary["hashes"]["sha1"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_default();

    Ok((download_url, filename, sha1))
}

/// Parse a pack manifest from JSON string.
pub fn parse_pack_manifest(json: &str) -> Result<PackManifest, String> {
    let manifest: PackManifest =
        serde_json::from_str(json).map_err(|e| format!("Failed to parse pack manifest: {}", e))?;
    if manifest.name.trim().is_empty() {
        return Err("Pack manifest must have a non-empty 'name'".to_string());
    }
    if manifest.mods.is_empty() {
        return Err("Pack manifest must have at least one mod".to_string());
    }
    for (i, m) in manifest.mods.iter().enumerate() {
        if m.id.trim().is_empty() {
            return Err(format!("Pack manifest mod entry {} has an empty 'id'", i));
        }
        if m.source != "modrinth" && m.source != "agora" {
            return Err(format!(
                "Pack manifest mod entry {} has unsupported source '{}'",
                i, m.source
            ));
        }
        if m.status != "required" && m.status != "optional" {
            return Err(format!(
                "Pack manifest mod entry {} has unsupported status '{}'",
                i, m.status
            ));
        }
    }
    Ok(manifest)
}

/// Download the override bundle from GitHub release and extract to target_dir.
async fn download_and_extract_overrides(
    client: &reqwest::Client,
    source: &OverrideSource,
    target_dir: &Path,
) -> Result<bool, String> {
    if source.source_type != "github_release" {
        return Err(format!(
            "Unsupported override source type: {}",
            source.source_type
        ));
    }

    let url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        source.identifier, source.release_tag, source.asset_name
    );

    let resp = client
        .get(&url)
        .header("User-Agent", "AgoraLauncher/1.0")
        .send()
        .await
        .map_err(|e| format!("Failed to download override bundle: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Download override bundle returned HTTP {}",
            resp.status()
        ));
    }

    let data = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read override bundle: {}", e))?
        .to_vec();

    if let Some(expected_sha256) = &source.sha256 {
        let actual = download::sha256_hex(&data);
        if actual != *expected_sha256 {
            return Err(format!(
                "SHA-256 mismatch for override bundle: expected {} got {}",
                expected_sha256, actual
            ));
        }
    }

    let reader = std::io::Cursor::new(data);
    let mut archive =
        ZipArchive::new(reader).map_err(|e| format!("Failed to read override zip: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;

        let entry_name = entry.name().replace('\\', "/");
        if entry_name.is_empty() || entry_name.ends_with('/') {
            continue;
        }

        let candidate = Path::new(&entry_name);
        assert_safe_path(target_dir, candidate)?;

        let dest = target_dir.join(candidate);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
        }

        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("Failed to read entry {}: {}", entry_name, e))?;
        std::fs::write(&dest, &buf)
            .map_err(|e| format!("Failed to write {}: {}", dest.display(), e))?;
    }

    Ok(true)
}

/// Install a simple pack (Tier 1) — just the mod list.
pub async fn install_simple_pack(
    client: &reqwest::Client,
    manifest: &PackManifest,
    target_dir: &Path,
) -> Result<PackInstallResult, String> {
    let cache_dir = target_dir.join(".cache");
    let cache = ModCache::new(cache_dir);
    let mods_dir = target_dir.join("mods");
    std::fs::create_dir_all(&mods_dir)
        .map_err(|e| format!("Failed to create mods directory: {}", e))?;

    let mut mods_installed = 0;

    for entry in &manifest.mods {
        if entry.status == "optional" {
            continue;
        }

        match entry.source.as_str() {
            "modrinth" => {
                let (download_url, filename, sha1) = modrinth_version_download_url(
                    client,
                    &entry.id,
                    entry.version.as_deref(),
                )
                .await?;

                let resp = client
                    .get(&download_url)
                    .header("User-Agent", "AgoraLauncher/1.0")
                    .send()
                    .await
                    .map_err(|e| format!("Failed to download mod {}: {}", entry.id, e))?;

                if !resp.status().is_success() {
                    return Err(format!(
                        "Download mod {} returned HTTP {}",
                        entry.id,
                        resp.status()
                    ));
                }

                let data = resp
                    .bytes()
                    .await
                    .map_err(|e| format!("Failed to read mod {}: {}", entry.id, e))?
                    .to_vec();

                if !sha1.is_empty() {
                    let actual_sha1 = download::sha1_hex(&data);
                    if actual_sha1 != sha1.to_lowercase() {
                        return Err(format!(
                            "SHA-1 mismatch for mod '{}': expected {} got {}",
                            entry.id, sha1, actual_sha1
                        ));
                    }
                }

                let jar_path = mods_dir.join(&filename);
                let actual_sha256 = download::sha256_hex(&data);

                std::fs::write(&jar_path, &data)
                    .map_err(|e| format!("Failed to write mod file: {}", e))?;

                cache
                    .store_jar(&jar_path, &actual_sha256)
                    .map_err(|e| format!("Failed to cache mod: {}", e))?;

                mods_installed += 1;
            }
            "agora" => {
                return Err(format!(
                    "Agora registry source not yet supported for pack install (mod '{}')",
                    entry.id
                ));
            }
            _ => {
                return Err(format!("Unsupported mod source '{}'", entry.source));
            }
        }
    }

    let instance_id = crate::paths::sanitize_id(&manifest.name);

    Ok(PackInstallResult {
        instance_id,
        name: manifest.name.clone(),
        mods_installed,
        overrides_extracted: false,
    })
}

/// Install a complex pack (Tier 2) — mod list + override bundle.
pub async fn install_complex_pack(
    client: &reqwest::Client,
    manifest: &PackManifest,
    target_dir: &Path,
) -> Result<PackInstallResult, String> {
    let mut result = install_simple_pack(client, manifest, target_dir).await?;

    if let Some(ref overrides) = manifest.override_source {
        let extracted = download_and_extract_overrides(client, overrides, target_dir).await?;
        result.overrides_extracted = extracted;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pack_manifest_valid() {
        let json = r#"{
            "name": "Optimized Vanilla",
            "minecraft_version": "1.21",
            "loader": "fabric",
            "loader_version": "0.15.11",
            "mods": [
                { "id": "AANobbMI", "source": "modrinth", "version": "0.6.0", "status": "required" },
                { "id": "P7dR8mSH", "source": "modrinth", "version": "0.100.4", "status": "required" }
            ]
        }"#;
        let manifest = parse_pack_manifest(json).unwrap();
        assert_eq!(manifest.name, "Optimized Vanilla");
        assert_eq!(manifest.minecraft_version, "1.21");
        assert_eq!(manifest.loader, "fabric");
        assert_eq!(manifest.mods.len(), 2);
        assert!(manifest.override_source.is_none());
    }

    #[test]
    fn test_parse_pack_manifest_with_overrides() {
        let json = r#"{
            "name": "My Modpack",
            "minecraft_version": "1.21",
            "loader": "fabric",
            "loader_version": "0.15.11",
            "mods": [
                { "id": "AANobbMI", "source": "modrinth", "version": "0.6.0", "status": "required" }
            ],
            "override_source": {
                "type": "github_release",
                "identifier": "owner/repo",
                "release_tag": "v1.0.0",
                "asset_name": "overrides.zip",
                "sha256": "abcdef1234567890"
            }
        }"#;
        let manifest = parse_pack_manifest(json).unwrap();
        let overrides = manifest.override_source.unwrap();
        assert_eq!(overrides.source_type, "github_release");
        assert_eq!(overrides.identifier, "owner/repo");
        assert_eq!(overrides.sha256, Some("abcdef1234567890".to_string()));
    }

    #[test]
    fn test_parse_pack_manifest_empty_name() {
        let json = r#"{
            "name": "",
            "minecraft_version": "1.21",
            "loader": "fabric",
            "loader_version": "0.15.11",
            "mods": [
                { "id": "AANobbMI", "source": "modrinth", "version": "0.6.0", "status": "required" }
            ]
        }"#;
        let err = parse_pack_manifest(json).unwrap_err();
        assert!(err.contains("name"));
    }

    #[test]
    fn test_parse_pack_manifest_empty_mods() {
        let json = r#"{
            "name": "Empty Pack",
            "minecraft_version": "1.21",
            "loader": "fabric",
            "loader_version": "0.15.11",
            "mods": []
        }"#;
        let err = parse_pack_manifest(json).unwrap_err();
        assert!(err.contains("at least one mod"));
    }

    #[test]
    fn test_parse_pack_manifest_missing_field() {
        let json = r#"{
            "name": "Broken Pack",
            "mods": []
        }"#;
        let err = parse_pack_manifest(json).unwrap_err();
        assert!(err.contains("Failed to parse"));
    }

    #[test]
    fn test_parse_pack_manifest_bad_source() {
        let json = r#"{
            "name": "Bad Source Pack",
            "minecraft_version": "1.21",
            "loader": "fabric",
            "loader_version": "0.15.11",
            "mods": [
                { "id": "test", "source": "curseforge", "version": "1.0", "status": "required" }
            ]
        }"#;
        let err = parse_pack_manifest(json).unwrap_err();
        assert!(err.contains("unsupported source"));
    }

    #[test]
    fn test_parse_pack_manifest_bad_status() {
        let json = r#"{
            "name": "Bad Status Pack",
            "minecraft_version": "1.21",
            "loader": "fabric",
            "loader_version": "0.15.11",
            "mods": [
                { "id": "test", "source": "modrinth", "version": "1.0", "status": "invalid" }
            ]
        }"#;
        let err = parse_pack_manifest(json).unwrap_err();
        assert!(err.contains("unsupported status"));
    }

    #[test]
    fn test_assert_safe_path_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("instance");
        std::fs::create_dir_all(base.join("mods")).unwrap();
        std::fs::write(base.join("mods").join("test.jar"), b"test").unwrap();
        assert_safe_path(&base, Path::new("mods/test.jar")).unwrap();
    }

    #[test]
    fn test_assert_safe_path_traversal_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("instance");
        std::fs::create_dir_all(&base).unwrap();
        let err = assert_safe_path(&base, Path::new("../outside.txt")).unwrap_err();
        assert!(err.contains("Path traversal"));
    }

    #[test]
    fn test_download_and_extract_overrides_with_test_zip() {
        use std::io::Write;

        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("instance");
        std::fs::create_dir_all(&target_dir).unwrap();

        let zip_path = tmp.path().join("overrides.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip_writer = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip_writer
            .start_file("config/test.cfg", options)
            .unwrap();
        zip_writer.write_all(b"test config").unwrap();
        zip_writer.finish().unwrap();

        let zip_data = std::fs::read(&zip_path).unwrap();
        let sha256 = crate::download::sha256_hex(&zip_data);

        let source = OverrideSource {
            source_type: "github_release".to_string(),
            identifier: "owner/repo".to_string(),
            release_tag: "v1.0.0".to_string(),
            asset_name: "overrides.zip".to_string(),
            sha256: Some(sha256),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();

        let result = rt.block_on(async {
            download_and_extract_overrides(&client, &source, &target_dir).await
        });

        assert!(result.is_err(), "expected network error since URL is fake");
    }
}
