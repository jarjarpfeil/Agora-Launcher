use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadoutProfile {
    pub name: String,
    pub enabled_mods: Vec<String>,
    pub created_at: String,
}

fn loadouts_dir(instance_dir: &Path) -> std::path::PathBuf {
    instance_dir.join(".agora_loadouts")
}

/// Create a new loadout profile from the current enabled mods in an instance.
pub fn create_profile(instance_dir: &Path, name: &str) -> Result<LoadoutProfile, String> {
    let mods_dir = instance_dir.join("mods");
    if !mods_dir.is_dir() {
        return Err("Instance has no mods directory".to_string());
    }

    let enabled_mods: Vec<String> = fs::read_dir(&mods_dir)
        .map_err(|e| format!("Cannot read mods directory: {e}"))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| !name.ends_with(".disabled"))
        .collect();

    let profile = LoadoutProfile {
        name: name.to_string(),
        enabled_mods,
        created_at: Utc::now().to_rfc3339(),
    };

    let profiles_dir = loadouts_dir(instance_dir);
    fs::create_dir_all(&profiles_dir)
        .map_err(|e| format!("Cannot create loadouts dir: {e}"))?;

    let profile_path = profiles_dir.join(format!("{}.json", name));
    let json = serde_json::to_string_pretty(&profile)
        .map_err(|e| format!("Cannot serialize profile: {e}"))?;
    fs::write(&profile_path, json).map_err(|e| format!("Cannot write profile: {e}"))?;

    Ok(profile)
}

/// List all loadout profiles for an instance.
pub fn list_profiles(instance_dir: &Path) -> Result<Vec<LoadoutProfile>, String> {
    let profiles_dir = loadouts_dir(instance_dir);
    if !profiles_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut profiles = Vec::new();
    let entries = fs::read_dir(&profiles_dir)
        .map_err(|e| format!("Cannot read loadouts dir: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Cannot read entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let content =
            fs::read_to_string(&path).map_err(|e| format!("Cannot read {:?}: {e}", path))?;
        let profile: LoadoutProfile =
            serde_json::from_str(&content).map_err(|e| format!("Invalid profile JSON: {e}"))?;
        profiles.push(profile);
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}

/// Apply a loadout profile: enable mods in the profile, disable others.
/// Renames `<mod>.jar` ↔ `<mod>.jar.disabled`.
pub fn apply_profile(instance_dir: &Path, profile_name: &str) -> Result<(), String> {
    let profiles_dir = loadouts_dir(instance_dir);
    let profile_path = profiles_dir.join(format!("{profile_name}.json"));

    let content =
        fs::read_to_string(&profile_path).map_err(|e| format!("Cannot read profile: {e}"))?;
    let profile: LoadoutProfile =
        serde_json::from_str(&content).map_err(|e| format!("Invalid profile JSON: {e}"))?;

    let mods_dir = instance_dir.join("mods");
    if !mods_dir.is_dir() {
        return Ok(());
    }

    let enabled_set: std::collections::HashSet<String> =
        profile.enabled_mods.iter().cloned().collect();

    let entries: Vec<_> = fs::read_dir(&mods_dir)
        .map_err(|e| format!("Cannot read mods directory: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();

    for entry in entries {
        let filename = entry.file_name().to_string_lossy().to_string();

        let (base_name, is_disabled) = if let Some(stripped) = filename.strip_suffix(".disabled") {
            (stripped.to_string(), true)
        } else {
            (filename.clone(), false)
        };

        let should_be_enabled = enabled_set.contains(&base_name);

        if should_be_enabled && is_disabled {
            let src = entry.path();
            let dst = src.with_extension("jar");
            fs::rename(&src, &dst).map_err(|e| format!("Cannot enable {filename}: {e}"))?;
        } else if !should_be_enabled && !is_disabled {
            let src = entry.path();
            let dst = mods_dir.join(format!("{base_name}.disabled"));
            fs::rename(&src, &dst).map_err(|e| format!("Cannot disable {filename}: {e}"))?;
        }
    }

    Ok(())
}

/// Delete a loadout profile.
pub fn delete_profile(instance_dir: &Path, profile_name: &str) -> Result<(), String> {
    let profiles_dir = loadouts_dir(instance_dir);
    let profile_path = profiles_dir.join(format!("{profile_name}.json"));

    if !profile_path.exists() {
        return Err(format!("Profile '{profile_name}' not found"));
    }

    fs::remove_file(&profile_path).map_err(|e| format!("Cannot delete profile: {e}"))?;

    if profiles_dir.exists() {
        let remaining: Vec<_> = fs::read_dir(&profiles_dir)
            .map_err(|e| format!("Cannot read loadouts dir: {e}"))?
            .filter_map(|e| e.ok())
            .collect();
        if remaining.is_empty() {
            let _ = fs::remove_dir(&profiles_dir);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_mods_instance(tmp: &tempfile::TempDir) -> std::path::PathBuf {
        let dir = tmp.path().join("test-instance");
        let mods = dir.join("mods");
        fs::create_dir_all(&mods).unwrap();
        fs::write(mods.join("sodium.jar"), b"sodium").unwrap();
        fs::write(mods.join("lithium.jar"), b"lithium").unwrap();
        fs::write(mods.join("phosphor.jar"), b"phosphor").unwrap();
        dir
    }

    #[test]
    fn test_create_profile_records_enabled_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_instance(&tmp);

        let profile = create_profile(&dir, "test-profile").unwrap();
        assert_eq!(profile.name, "test-profile");
        assert_eq!(profile.enabled_mods.len(), 3);
        assert!(profile.enabled_mods.contains(&"sodium.jar".to_string()));
    }

    #[test]
    fn test_list_profiles_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empty");

        let profiles = list_profiles(&dir).unwrap();
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_apply_profile_disables_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_instance(&tmp);
        let mods = dir.join("mods");

        create_profile(&dir, "full").unwrap();

        let minimal = LoadoutProfile {
            name: "minimal".to_string(),
            enabled_mods: vec!["sodium.jar".to_string()],
            created_at: Utc::now().to_rfc3339(),
        };
        let profiles_dir = loadouts_dir(&dir);
        fs::create_dir_all(&profiles_dir).unwrap();
        fs::write(
            profiles_dir.join("minimal.json"),
            serde_json::to_string_pretty(&minimal).unwrap(),
        )
        .unwrap();

        apply_profile(&dir, "minimal").unwrap();

        let entries: Vec<String> = fs::read_dir(&mods)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert!(entries.contains(&"sodium.jar".to_string()));
        assert!(entries.contains(&"lithium.jar.disabled".to_string()));
        assert!(entries.contains(&"phosphor.jar.disabled".to_string()));
    }

    #[test]
    fn test_apply_profile_enables_disabled_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_instance(&tmp);
        let mods = dir.join("mods");

        fs::rename(mods.join("lithium.jar"), mods.join("lithium.jar.disabled")).unwrap();
        fs::rename(mods.join("phosphor.jar"), mods.join("phosphor.jar.disabled")).unwrap();

        let profile = LoadoutProfile {
            name: "all-on".to_string(),
            enabled_mods: vec![
                "sodium.jar".to_string(),
                "lithium.jar".to_string(),
                "phosphor.jar".to_string(),
            ],
            created_at: Utc::now().to_rfc3339(),
        };
        let profiles_dir = loadouts_dir(&dir);
        fs::create_dir_all(&profiles_dir).unwrap();
        fs::write(
            profiles_dir.join("all-on.json"),
            serde_json::to_string_pretty(&profile).unwrap(),
        )
        .unwrap();

        apply_profile(&dir, "all-on").unwrap();

        let entries: Vec<String> = fs::read_dir(&mods)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert!(entries.contains(&"sodium.jar".to_string()));
        assert!(entries.contains(&"lithium.jar".to_string()));
        assert!(entries.contains(&"phosphor.jar".to_string()));
        assert!(!entries.iter().any(|n| n.ends_with(".disabled")));
    }

    #[test]
    fn test_create_list_delete_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_instance(&tmp);

        create_profile(&dir, "alpha").unwrap();
        create_profile(&dir, "beta").unwrap();

        let profiles = list_profiles(&dir).unwrap();
        assert_eq!(profiles.len(), 2);

        delete_profile(&dir, "alpha").unwrap();
        let profiles = list_profiles(&dir).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "beta");

        delete_profile(&dir, "beta").unwrap();
        let profiles = list_profiles(&dir).unwrap();
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_delete_profile_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("no-profiles");
        let result = delete_profile(&dir, "nonexistent");
        assert!(result.is_err());
    }
}
