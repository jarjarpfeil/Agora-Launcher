use crate::paths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClonePrefs {
    pub copy_saves: bool,
    pub copy_mods: bool,
    pub copy_resource_packs: bool,
    pub copy_shader_packs: bool,
    pub copy_screenshots: bool,
    pub copy_config: bool,
    pub copy_servers: bool,
    pub copy_options: bool,
    pub use_hard_links: bool,
    pub use_sym_links: bool,
}

impl Default for ClonePrefs {
    fn default() -> Self {
        ClonePrefs {
            copy_saves: true,
            copy_mods: true,
            copy_resource_packs: true,
            copy_shader_packs: true,
            copy_screenshots: true,
            copy_config: true,
            copy_servers: true,
            copy_options: true,
            use_hard_links: false,
            use_sym_links: false,
        }
    }
}

struct DirMapping {
    dir: &'static str,
    pref: fn(&ClonePrefs) -> bool,
}

const DIR_MAPPINGS: &[DirMapping] = &[
    DirMapping {
        dir: "saves",
        pref: |p| p.copy_saves,
    },
    DirMapping {
        dir: "mods",
        pref: |p| p.copy_mods,
    },
    DirMapping {
        dir: "resourcepacks",
        pref: |p| p.copy_resource_packs,
    },
    DirMapping {
        dir: "shaderpacks",
        pref: |p| p.copy_shader_packs,
    },
    DirMapping {
        dir: "screenshots",
        pref: |p| p.copy_screenshots,
    },
    DirMapping {
        dir: "config",
        pref: |p| p.copy_config,
    },
    DirMapping {
        dir: "servers",
        pref: |p| p.copy_servers,
    },
    DirMapping {
        dir: "options",
        pref: |p| p.copy_options,
    },
];

/// Clone an instance directory with the given copy preferences.
/// Returns the new instance_id (a sanitized version of the name).
pub fn clone_instance(
    src_dir: &Path,
    dest_dir: &Path,
    prefs: &ClonePrefs,
) -> Result<String, String> {
    if !src_dir.is_dir() {
        return Err(format!("Source {:?} is not a directory", src_dir));
    }

    let name = src_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let instance_id = paths::sanitize_id(&name);

    if dest_dir.exists() {
        fs::remove_dir_all(dest_dir)
            .map_err(|e| format!("Cannot remove existing dest {dest_dir:?}: {e}"))?;
    }
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Cannot create dest dir {dest_dir:?}: {e}"))?;

    let manifest_src = src_dir.join("manifest.json");
    if manifest_src.exists() {
        fs::copy(&manifest_src, dest_dir.join("manifest.json"))
            .map_err(|e| format!("Cannot copy manifest.json: {e}"))?;
    }

    let manifest_json_src = src_dir.join("instance_manifest.json");
    if manifest_json_src.exists() {
        fs::copy(&manifest_json_src, dest_dir.join("instance_manifest.json"))
            .map_err(|e| format!("Cannot copy instance_manifest.json: {e}"))?;
    }

    for mapping in DIR_MAPPINGS {
        if !(mapping.pref)(prefs) {
            continue;
        }
        let src_child = src_dir.join(mapping.dir);
        if !src_child.exists() {
            continue;
        }
        let dst_child = dest_dir.join(mapping.dir);
        copy_entry(&src_child, &dst_child, prefs)?;
    }

    Ok(instance_id)
}

fn copy_entry(src: &Path, dst: &Path, prefs: &ClonePrefs) -> Result<(), String> {
    if prefs.use_sym_links {
        if symlink_entry(src, dst) {
            return Ok(());
        }
    }

    if prefs.use_hard_links && src.is_file() {
        if hardlink_entry(src, dst) {
            return Ok(());
        }
    }

    if src.is_dir() {
        fs::create_dir_all(dst).map_err(|e| format!("Cannot create dir {:?}: {}", dst, e))?;
        let entries = fs::read_dir(src).map_err(|e| format!("Cannot read dir {:?}: {}", src, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("Cannot read entry: {}", e))?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_entry(&child_src, &child_dst, prefs)?;
        }
    } else if src.is_file() {
        fs::copy(src, dst).map_err(|e| format!("Cannot copy {:?} to {:?}: {}", src, dst, e))?;
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_entry(src: &Path, dst: &Path) -> bool {
    std::os::unix::fs::symlink(src, dst).is_ok()
}

#[cfg(windows)]
fn symlink_entry(src: &Path, dst: &Path) -> bool {
    let result = if src.is_dir() {
        std::os::windows::fs::symlink_dir(src, dst)
    } else {
        std::os::windows::fs::symlink_file(src, dst)
    };
    result.is_ok()
}

#[cfg(not(any(unix, windows)))]
fn symlink_entry(_src: &Path, _dst: &Path) -> bool {
    false
}

#[cfg(any(unix, windows))]
fn hardlink_entry(src: &Path, dst: &Path) -> bool {
    fs::hard_link(src, dst).is_ok()
}

#[cfg(not(any(unix, windows)))]
fn hardlink_entry(_src: &Path, _dst: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_all_dirs_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("original");

        for dir in ["mods", "saves", "config", "resourcepacks", "shaderpacks", "screenshots", "servers", "options"]
        {
            fs::create_dir_all(src.join(dir)).unwrap();
            fs::write(src.join(dir).join("placeholder.txt"), b"data").unwrap();
        }
        fs::write(src.join("instance_manifest.json"), b"{}").unwrap();

        let dst = tmp.path().join("clone");
        let prefs = ClonePrefs::default();
        let id = clone_instance(&src, &dst, &prefs).unwrap();
        assert!(!id.is_empty());

        for dir in ["mods", "saves", "config", "resourcepacks", "shaderpacks", "screenshots", "servers", "options"]
        {
            assert!(dst.join(dir).exists(), "missing {dir}");
        }
        assert!(dst.join("instance_manifest.json").exists());
    }

    #[test]
    fn test_clone_no_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("original");

        fs::create_dir_all(src.join("mods")).unwrap();
        fs::write(src.join("mods").join("some-mod.jar"), b"mod").unwrap();
        fs::create_dir_all(src.join("saves")).unwrap();
        fs::write(src.join("saves").join("world.dat"), b"world").unwrap();

        let dst = tmp.path().join("clone");
        let prefs = ClonePrefs {
            copy_mods: false,
            ..Default::default()
        };
        clone_instance(&src, &dst, &prefs).unwrap();

        assert!(!dst.join("mods").exists());
        assert!(dst.join("saves").exists());
    }

    #[test]
    fn test_clone_hardlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("original");

        fs::create_dir_all(src.join("mods")).unwrap();
        fs::write(src.join("mods").join("test.jar"), b"hardlink test").unwrap();

        let dst = tmp.path().join("clone");
        let prefs = ClonePrefs {
            use_hard_links: true,
            ..Default::default()
        };
        clone_instance(&src, &dst, &prefs).unwrap();

        assert!(dst.join("mods").join("test.jar").exists());
        assert_eq!(
            fs::read(src.join("mods").join("test.jar")).unwrap(),
            fs::read(dst.join("mods").join("test.jar")).unwrap()
        );
    }

    #[test]
    fn test_clone_source_not_a_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let not_dir = tmp.path().join("nonexistent");
        let dst = tmp.path().join("dest");
        let prefs = ClonePrefs::default();
        let result = clone_instance(&not_dir, &dst, &prefs);
        assert!(result.is_err());
    }

    #[test]
    fn test_clone_default_prefs_all_true() {
        let prefs = ClonePrefs::default();
        assert!(prefs.copy_saves);
        assert!(prefs.copy_mods);
        assert!(prefs.copy_resource_packs);
        assert!(prefs.copy_shader_packs);
        assert!(prefs.copy_screenshots);
        assert!(prefs.copy_config);
        assert!(prefs.copy_servers);
        assert!(prefs.copy_options);
        assert!(!prefs.use_hard_links);
        assert!(!prefs.use_sym_links);
    }
}
