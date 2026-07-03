use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const RESTORE_MARKER: &str = ".agora_restore_in_progress";

const TRACKED_ENTRIES: &[&str] = &[
    "mods",
    "config",
    "resourcepacks",
    "shaderpacks",
    "saves",
    "options.txt",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub label: Option<String>,
    pub created_at: String,
    pub file_count: usize,
    pub size_estimate: u64,
}

#[derive(Serialize, Deserialize)]
struct SnapshotManifest {
    snapshot: Snapshot,
    files: Vec<SnapshotFileEntry>,
}

#[derive(Serialize, Deserialize)]
struct SnapshotFileEntry {
    relative_path: String,
    size: u64,
}

fn snapshot_dir(instance_dir: &Path, id: &str) -> PathBuf {
    instance_dir.join(".agora_snapshots").join(id)
}

fn pre_restore_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join(".agora_pre_restore")
}

/// Create a snapshot of an instance directory. Snapshots are stored in
/// `<instance_dir>/.agora_snapshots/<id>/` as hardlinks (cheap, no duplication).
pub fn create_snapshot(instance_dir: &Path, label: Option<&str>) -> Result<Snapshot, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let snap_dir = snapshot_dir(instance_dir, &id);

    fs::create_dir_all(&snap_dir)
        .map_err(|e| format!("failed to create snapshot dir: {}", e))?;

    let mut files: Vec<SnapshotFileEntry> = Vec::new();
    let mut total_size: u64 = 0;

    for entry_name in TRACKED_ENTRIES {
        let src = instance_dir.join(entry_name);
        if !src.exists() {
            continue;
        }
        let dst = snap_dir.join(entry_name);

        if src.is_file() {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create parent dir: {}", e))?;
            }
            hardlink_or_copy(&src, &dst)?;
            let size = fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
            files.push(SnapshotFileEntry {
                relative_path: entry_name.to_string(),
                size,
            });
            total_size += size;
        } else if src.is_dir() {
            walk_and_link(&src, &dst, entry_name, &mut files, &mut total_size)?;
        }
    }

    let snapshot = Snapshot {
        id: id.clone(),
        label: label.map(String::from),
        created_at: chrono::Utc::now().to_rfc3339(),
        file_count: files.len(),
        size_estimate: total_size,
    };

    let manifest = SnapshotManifest {
        snapshot: snapshot.clone(),
        files,
    };

    let manifest_path = snap_dir.join("manifest.json");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .map_err(|e| format!("failed to write manifest: {}", e))?;

    Ok(snapshot)
}

fn walk_and_link(
    src: &Path,
    dst: &Path,
    prefix: &str,
    files: &mut Vec<SnapshotFileEntry>,
    total_size: &mut u64,
) -> Result<(), String> {
    fs::create_dir_all(dst)
        .map_err(|e| format!("failed to create dir {}: {}", dst.display(), e))?;

    let entries =
        fs::read_dir(src).map_err(|e| format!("failed to read dir {}: {}", src.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
        let entry_type = entry
            .file_type()
            .map_err(|e| format!("file type error: {}", e))?;
        let entry_name = entry.file_name().to_string_lossy().to_string();
        let src_path = entry.path();
        let dst_path = dst.join(&entry_name);
        let relative = format!("{}/{}", prefix, entry_name);

        if entry_type.is_dir() {
            walk_and_link(&src_path, &dst_path, &relative, files, total_size)?;
        } else if entry_type.is_file() {
            hardlink_or_copy(&src_path, &dst_path)?;
            let size = fs::metadata(&dst_path).map(|m| m.len()).unwrap_or(0);
            files.push(SnapshotFileEntry {
                relative_path: relative,
                size,
            });
            *total_size += size;
        }
    }

    Ok(())
}

fn hardlink_or_copy(src: &Path, dst: &Path) -> Result<(), String> {
    if fs::hard_link(src, dst).is_err() {
        fs::copy(src, dst).map_err(|e| format!("copy fallback failed: {}", e))?;
    }
    Ok(())
}

/// Restore an instance to a snapshot. CURRENT files are moved to `.agora_pre_restore/`
/// (as a safety net), then snapshot files are linked back.
pub fn restore_snapshot(instance_dir: &Path, snapshot_id: &str) -> Result<(), String> {
    let snap_dir = snapshot_dir(instance_dir, snapshot_id);
    if !snap_dir.exists() {
        return Err(format!("snapshot {} not found", snapshot_id));
    }

    let manifest_path = snap_dir.join("manifest.json");
    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("failed to read manifest: {}", e))?;
    let manifest: SnapshotManifest =
        serde_json::from_str(&content).map_err(|e| format!("failed to parse manifest: {}", e))?;

    let pre_dir = pre_restore_dir(instance_dir);
    if pre_dir.exists() {
        fs::remove_dir_all(&pre_dir)
            .map_err(|e| format!("failed to remove pre-restore dir: {}", e))?;
    }
    fs::create_dir_all(&pre_dir)
        .map_err(|e| format!("failed to create pre-restore dir: {}", e))?;

    // Write marker BEFORE moving files to detect interruption
    let marker_path = instance_dir.join(RESTORE_MARKER);
    fs::write(&marker_path, b"restore in progress")
        .map_err(|e| format!("failed to write restore marker: {}", e))?;

    for entry_name in TRACKED_ENTRIES {
        let src = instance_dir.join(entry_name);
        if src.exists() {
            let dst = pre_dir.join(entry_name);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create parent: {}", e))?;
            }
            fs::rename(&src, &dst)
                .map_err(|e| format!("failed to move {}: {}", entry_name, e))?;
        }
    }

    for file_entry in &manifest.files {
        let src = snap_dir.join(&file_entry.relative_path);
        let dst = instance_dir.join(&file_entry.relative_path);

        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create dir: {}", e))?;
        }

        hardlink_or_copy(&src, &dst)?;
    }

    // Remove marker — restore completed successfully
    if marker_path.exists() {
        fs::remove_file(&marker_path)
            .map_err(|e| format!("failed to remove restore marker: {}", e))?;
    }

    Ok(())
}

/// List all snapshots for an instance.
pub fn list_snapshots(instance_dir: &Path) -> Result<Vec<Snapshot>, String> {
    let marker = instance_dir.join(RESTORE_MARKER);
    if marker.exists() {
        return Err(
            "Previous restore was interrupted. Check .agora_pre_restore/ for backed-up files.".into(),
        );
    }

    let snapshots_dir = instance_dir.join(".agora_snapshots");
    if !snapshots_dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();

    let entries = fs::read_dir(&snapshots_dir)
        .map_err(|e| format!("failed to read snapshots dir: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }

        let content = fs::read_to_string(&manifest_path)
            .map_err(|e| format!("failed to read manifest: {}", e))?;
        let manifest: SnapshotManifest = serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse manifest: {}", e))?;

        snapshots.push(manifest.snapshot);
    }

    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(snapshots)
}

/// Delete a snapshot.
pub fn delete_snapshot(instance_dir: &Path, snapshot_id: &str) -> Result<(), String> {
    let snap_dir = snapshot_dir(instance_dir, snapshot_id);
    if !snap_dir.exists() {
        return Err(format!("snapshot {} not found", snapshot_id));
    }

    fs::remove_dir_all(&snap_dir)
        .map_err(|e| format!("failed to delete snapshot: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_instance(tmp: &TempDir) -> PathBuf {
        let dir = tmp.path().join("instance");
        fs::create_dir_all(dir.join("mods")).unwrap();
        fs::create_dir_all(dir.join("config")).unwrap();
        fs::create_dir_all(dir.join("resourcepacks")).unwrap();
        fs::create_dir_all(dir.join("shaderpacks")).unwrap();
        fs::write(dir.join("mods").join("test.jar"), b"mod content").unwrap();
        fs::write(dir.join("config").join("settings.toml"), b"key=value").unwrap();
        fs::write(dir.join("options.txt"), b"render_distance=12").unwrap();
        dir
    }

    #[test]
    fn create_and_list_snapshot() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);

        let snap = create_snapshot(&inst, Some("before-update")).unwrap();
        assert_eq!(snap.label.as_deref(), Some("before-update"));
        assert!(snap.file_count > 0);
        assert!(snap.size_estimate > 0);

        let snaps = list_snapshots(&inst).unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].id, snap.id);
    }

    #[test]
    fn restore_snapshot_preserves_content() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);

        let snap = create_snapshot(&inst, None).unwrap();

        // Modify original files
        fs::write(inst.join("mods").join("test.jar"), b"modified").unwrap();
        fs::write(inst.join("options.txt"), b"modified").unwrap();

        // Restore
        restore_snapshot(&inst, &snap.id).unwrap();

        assert_eq!(
            fs::read(inst.join("mods").join("test.jar")).unwrap(),
            b"mod content"
        );
        assert_eq!(
            fs::read(inst.join("options.txt")).unwrap(),
            b"render_distance=12"
        );

        // Pre-restore safety net exists
        assert!(inst.join(".agora_pre_restore").exists());
    }

    #[test]
    fn snapshot_is_immutable() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);

        let _snap = create_snapshot(&inst, None).unwrap();

        // Modify original
        fs::write(inst.join("mods").join("test.jar"), b"changed").unwrap();

        // Snapshot files should still have original content
        let snap_dir = snapshot_dir(&inst, &_snap.id);
        let content = fs::read(snap_dir.join("mods").join("test.jar")).unwrap();
        assert_eq!(content, b"mod content");
    }

    #[test]
    fn delete_snapshot_removes_dir() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);

        let snap = create_snapshot(&inst, None).unwrap();
        let snap_dir = snapshot_dir(&inst, &snap.id);
        assert!(snap_dir.exists());

        delete_snapshot(&inst, &snap.id).unwrap();
        assert!(!snap_dir.exists());
    }

    #[test]
    fn list_snapshots_empty_when_none() {
        let tmp = TempDir::new().unwrap();
        let snaps = list_snapshots(tmp.path()).unwrap();
        assert!(snaps.is_empty());
    }
}
