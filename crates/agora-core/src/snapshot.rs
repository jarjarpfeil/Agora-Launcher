use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use sha2::Digest;
use zip::write::FileOptions;
use zip::CompressionMethod;

const RESTORE_MARKER: &str = ".agora_restore_in_progress";
const SNAPSHOT_SCHEMA_VERSION: u32 = 2;

const TRACKED_ENTRIES: &[&str] = &[
    "mods",
    "config",
    "resourcepacks",
    "shaderpacks",
    "datapacks",
    "saves",
    "options.txt",
    "instance_manifest.json",
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
    #[serde(default = "legacy_snapshot_schema_version")]
    schema_version: u32,
    snapshot: Snapshot,
    files: Vec<SnapshotFileEntry>,
}

#[derive(Serialize, Deserialize)]
struct SnapshotFileEntry {
    relative_path: String,
    size: u64,
    /// Snapshots written before schema v2 did not record hashes.  Keep the
    /// field optional so those recovery archives remain listable/restorable;
    /// restore computes the missing hash from the archive before mutation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

fn legacy_snapshot_schema_version() -> u32 {
    1
}

fn snapshots_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join(".agora_snapshots")
}

fn snapshot_zip_path(instance_dir: &Path, id: &str) -> PathBuf {
    snapshots_dir(instance_dir).join(format!("{id}.zip"))
}

fn pre_restore_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join(".agora_pre_restore")
}

/// Return the verified file index stored in a snapshot.  Legacy entries with
/// no recorded hash are hashed from their archive bytes.  This is also the
/// canonical input for LKG/drift comparisons, ensuring both sides use the same
/// `mods/foo.jar` path format and the same tracked-entry set.
pub fn snapshot_file_index(
    instance_dir: &Path,
    snapshot_id: &str,
) -> Result<Vec<crate::lkg::FileEntry>, String> {
    let zip_path = snapshot_zip_path(instance_dir, snapshot_id);
    let file = fs::File::open(&zip_path)
        .map_err(|e| format!("failed to open snapshot {snapshot_id}: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("failed to read snapshot zip: {e}"))?;
    let manifest = read_manifest(&mut archive, snapshot_id)?;

    let mut result = Vec::with_capacity(manifest.files.len());
    let mut seen = HashSet::new();
    for indexed in &manifest.files {
        validate_relative_path(&indexed.relative_path)?;
        if !seen.insert(indexed.relative_path.clone()) {
            return Err(format!(
                "snapshot manifest contains duplicate path {}",
                indexed.relative_path
            ));
        }
        let mut entry = archive.by_name(&indexed.relative_path).map_err(|e| {
            format!(
                "snapshot file {} is missing from archive: {e}",
                indexed.relative_path
            )
        })?;
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .map_err(|e| format!("failed to read {}: {e}", indexed.relative_path))?;
        if contents.len() as u64 != indexed.size {
            return Err(format!(
                "snapshot size mismatch for {}",
                indexed.relative_path
            ));
        }
        let actual = sha256_hex(&contents);
        if let Some(expected) = &indexed.sha256 {
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(format!(
                    "snapshot hash mismatch for {}",
                    indexed.relative_path
                ));
            }
        }
        result.push(crate::lkg::FileEntry {
            path: indexed.relative_path.clone(),
            sha256: actual,
            size: indexed.size,
        });
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

/// Scan the current live state using the exact same tracked paths as snapshot
/// creation.  Paths always use forward slashes, even on Windows.
pub fn live_file_index(instance_dir: &Path) -> Result<Vec<crate::lkg::FileEntry>, String> {
    let mut result = Vec::new();
    for entry_name in TRACKED_ENTRIES {
        let path = instance_dir.join(entry_name);
        if path.is_file() {
            let contents =
                fs::read(&path).map_err(|e| format!("failed to read live {entry_name}: {e}"))?;
            result.push(crate::lkg::FileEntry {
                path: (*entry_name).to_string(),
                sha256: sha256_hex(&contents),
                size: contents.len() as u64,
            });
        } else if path.is_dir() {
            walk_and_hash(&path, entry_name, &mut result)?;
        }
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

fn walk_and_hash(
    directory: &Path,
    prefix: &str,
    result: &mut Vec<crate::lkg::FileEntry>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|e| format!("failed to scan {}: {e}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to scan {}: {e}", directory.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let relative = format!("{prefix}/{name}");
        let file_type = entry
            .file_type()
            .map_err(|e| format!("failed to inspect {}: {e}", entry.path().display()))?;
        if file_type.is_dir() {
            walk_and_hash(&entry.path(), &relative, result)?;
        } else if file_type.is_file() {
            let contents = fs::read(entry.path())
                .map_err(|e| format!("failed to read live {relative}: {e}"))?;
            result.push(crate::lkg::FileEntry {
                path: relative,
                sha256: sha256_hex(&contents),
                size: contents.len() as u64,
            });
        }
    }
    Ok(())
}

/// Create a snapshot of an instance directory, stored as a single compressed
/// `.zip` under `<instance_dir>/.agora_snapshots/<id>.zip`.
pub fn create_snapshot(instance_dir: &Path, label: Option<&str>) -> Result<Snapshot, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let zip_path = snapshot_zip_path(instance_dir, &id);

    fs::create_dir_all(snapshots_dir(instance_dir))
        .map_err(|e| format!("failed to create snapshots dir: {e}"))?;

    let file =
        fs::File::create(&zip_path).map_err(|e| format!("failed to create snapshot zip: {e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut files: Vec<SnapshotFileEntry> = Vec::new();
    let mut total_size: u64 = 0;

    for entry_name in TRACKED_ENTRIES {
        let src = instance_dir.join(entry_name);
        if !src.exists() {
            continue;
        }

        if src.is_file() {
            let contents =
                fs::read(&src).map_err(|e| format!("failed to read {entry_name}: {e}"))?;
            zip.start_file(*entry_name, options)
                .map_err(|e| format!("failed to start zip entry {entry_name}: {e}"))?;
            zip.write_all(&contents)
                .map_err(|e| format!("failed to write {entry_name}: {e}"))?;
            let sha256 = {
                let mut hasher = sha2::Sha256::new();
                hasher.update(&contents);
                format!("{:x}", hasher.finalize())
            };
            files.push(SnapshotFileEntry {
                relative_path: entry_name.to_string(),
                size: contents.len() as u64,
                sha256: Some(sha256),
            });
            total_size += contents.len() as u64;
        } else if src.is_dir() {
            walk_and_zip(
                &src,
                entry_name,
                &mut zip,
                options.clone(),
                &mut files,
                &mut total_size,
            )?;
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
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot: snapshot.clone(),
        files,
    };

    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("failed to serialize manifest: {e}"))?;
    zip.start_file("manifest.json", options)
        .map_err(|e| format!("failed to start manifest entry: {e}"))?;
    zip.write_all(manifest_json.as_bytes())
        .map_err(|e| format!("failed to write manifest: {e}"))?;

    zip.finish()
        .map_err(|e| format!("failed to finalize snapshot zip: {e}"))?;

    Ok(snapshot)
}

fn walk_and_zip(
    src: &Path,
    prefix: &str,
    zip: &mut zip::ZipWriter<fs::File>,
    options: FileOptions,
    files: &mut Vec<SnapshotFileEntry>,
    total_size: &mut u64,
) -> Result<(), String> {
    let entries =
        fs::read_dir(src).map_err(|e| format!("failed to read dir {}: {}", src.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read entry: {e}"))?;
        let entry_type = entry
            .file_type()
            .map_err(|e| format!("file type error: {e}"))?;
        let entry_name = entry.file_name().to_string_lossy().to_string();
        let src_path = entry.path();
        let relative = format!("{prefix}/{entry_name}");

        if entry_type.is_dir() {
            walk_and_zip(
                &src_path,
                &relative,
                zip,
                options.clone(),
                files,
                total_size,
            )?;
        } else if entry_type.is_file() {
            let contents = fs::read(&src_path)
                .map_err(|e| format!("failed to read {}: {e}", src_path.display()))?;
            zip.start_file(relative.clone(), options)
                .map_err(|e| format!("failed to start zip entry {relative}: {e}"))?;
            zip.write_all(&contents)
                .map_err(|e| format!("failed to write {relative}: {e}"))?;
            let sha256 = {
                let mut hasher = sha2::Sha256::new();
                hasher.update(&contents);
                format!("{:x}", hasher.finalize())
            };
            files.push(SnapshotFileEntry {
                relative_path: relative,
                size: contents.len() as u64,
                sha256: Some(sha256),
            });
            *total_size += contents.len() as u64;
        }
    }

    Ok(())
}

/// Restore an instance to a snapshot.
///
/// The archive is completely extracted and hash-verified before any live file
/// is moved.  Tracked top-level entries are then exchanged with same-volume
/// renames.  If any exchange fails, every partially promoted snapshot entry is
/// displaced before the pre-restore entries are moved back, so rollback never
/// depends on renaming over a non-empty destination.
pub fn restore_snapshot(instance_dir: &Path, snapshot_id: &str) -> Result<(), String> {
    restore_snapshot_impl(instance_dir, snapshot_id, None)
}

fn restore_snapshot_impl(
    instance_dir: &Path,
    snapshot_id: &str,
    fail_after_promotions: Option<usize>,
) -> Result<(), String> {
    recover_interrupted_restore(instance_dir)?;
    let zip_path = snapshot_zip_path(instance_dir, snapshot_id);
    if !zip_path.exists() {
        return Err(format!("snapshot {snapshot_id} not found"));
    }

    let restore_id = uuid::Uuid::new_v4().to_string();
    let extract_dir = instance_dir.join(format!(".agora_restore_extract_{restore_id}"));
    fs::create_dir_all(&extract_dir).map_err(|e| format!("failed to create extract dir: {e}"))?;

    let manifest = match extract_and_verify(&zip_path, snapshot_id, &extract_dir) {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = fs::remove_dir_all(&extract_dir);
            return Err(error);
        }
    };

    let pre_dir = pre_restore_dir(instance_dir);
    if pre_dir.exists() {
        fs::remove_dir_all(&pre_dir)
            .map_err(|e| format!("failed to remove pre-restore dir: {e}"))?;
    }
    fs::create_dir_all(&pre_dir).map_err(|e| format!("failed to create pre-restore dir: {e}"))?;

    let marker_path = instance_dir.join(RESTORE_MARKER);
    fs::write(&marker_path, b"restore in progress")
        .map_err(|e| format!("failed to write restore marker: {e}"))?;

    let mut moved_current = Vec::new();
    for entry_name in TRACKED_ENTRIES {
        let src = instance_dir.join(entry_name);
        if src.exists() {
            let dst = pre_dir.join(entry_name);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("failed to create parent: {e}"))?;
            }
            if let Err(error) = fs::rename(&src, &dst) {
                let rollback =
                    rollback_restore(instance_dir, &pre_dir, &[], &moved_current, &restore_id);
                let _ = fs::remove_dir_all(&extract_dir);
                return Err(combine_restore_error(
                    format!("failed to move current {entry_name} into backup: {error}"),
                    rollback,
                ));
            }
            moved_current.push((*entry_name).to_string());
        }
    }

    let staged_roots = snapshot_roots(&manifest);
    let mut promoted = Vec::new();
    for entry_name in TRACKED_ENTRIES {
        if !staged_roots.contains(*entry_name) {
            continue;
        }

        let src = extract_dir.join(entry_name);
        let dst = instance_dir.join(entry_name);
        let promote_result = if fail_after_promotions == Some(promoted.len()) {
            Err("injected restore promotion failure".to_string())
        } else {
            fs::rename(&src, &dst).map_err(|e| e.to_string())
        };
        if let Err(error) = promote_result {
            let rollback = rollback_restore(
                instance_dir,
                &pre_dir,
                &promoted,
                &moved_current,
                &restore_id,
            );
            let _ = fs::remove_dir_all(&extract_dir);
            return Err(combine_restore_error(
                format!("failed to promote restored {entry_name}: {error}"),
                rollback,
            ));
        }
        promoted.push((*entry_name).to_string());
    }

    if marker_path.exists() {
        fs::remove_file(&marker_path)
            .map_err(|e| format!("failed to remove restore marker: {e}"))?;
    }

    if pre_dir.exists() {
        fs::remove_dir_all(&pre_dir)
            .map_err(|e| format!("restore succeeded but backup cleanup failed: {e}"))?;
    }

    let _ = fs::remove_dir_all(&extract_dir);

    Ok(())
}

/// Complete rollback from a process interruption before starting any new
/// restore. Only roots with an actual backup are displaced, so roots that had
/// not yet moved when the process stopped remain untouched.
fn recover_interrupted_restore(instance_dir: &Path) -> Result<(), String> {
    let marker = instance_dir.join(RESTORE_MARKER);
    let pre_dir = pre_restore_dir(instance_dir);
    if !marker.exists() {
        if pre_dir.exists() {
            fs::remove_dir_all(&pre_dir)
                .map_err(|e| format!("failed to remove stale restore backup: {e}"))?;
        }
        return Ok(());
    }
    if !pre_dir.is_dir() {
        return Err(
            "Previous restore was interrupted without a recovery backup; live state was left untouched."
                .into(),
        );
    }
    let backed_up = TRACKED_ENTRIES
        .iter()
        .filter(|entry| pre_dir.join(entry).exists())
        .map(|entry| (*entry).to_string())
        .collect::<Vec<_>>();
    if backed_up.is_empty() {
        fs::remove_file(&marker)
            .map_err(|e| format!("failed to clear empty restore marker: {e}"))?;
        fs::remove_dir_all(&pre_dir)
            .map_err(|e| format!("failed to clear empty restore backup: {e}"))?;
        return Ok(());
    }
    rollback_restore(
        instance_dir,
        &pre_dir,
        &backed_up,
        &backed_up,
        &format!("interrupted-{}", uuid::Uuid::new_v4()),
    )?;
    if pre_dir.exists() {
        fs::remove_dir_all(&pre_dir)
            .map_err(|e| format!("failed to clean recovered restore backup: {e}"))?;
    }
    Ok(())
}

fn extract_and_verify(
    zip_path: &Path,
    snapshot_id: &str,
    extract_dir: &Path,
) -> Result<SnapshotManifest, String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("failed to open snapshot zip: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("failed to read snapshot zip: {e}"))?;

    let manifest = read_manifest(&mut archive, snapshot_id)?;

    let mut seen = HashSet::new();
    for file_entry in &manifest.files {
        validate_relative_path(&file_entry.relative_path)?;
        if !seen.insert(file_entry.relative_path.clone()) {
            return Err(format!(
                "snapshot manifest contains duplicate path {}",
                file_entry.relative_path
            ));
        }

        let mut entry = archive.by_name(&file_entry.relative_path).map_err(|e| {
            format!(
                "snapshot file {} is missing from archive: {e}",
                file_entry.relative_path
            )
        })?;
        if entry.is_dir() {
            return Err(format!(
                "snapshot path {} is a directory, expected a file",
                file_entry.relative_path
            ));
        }

        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .map_err(|e| format!("failed to read {}: {e}", file_entry.relative_path))?;
        if contents.len() as u64 != file_entry.size {
            return Err(format!(
                "snapshot size mismatch for {}: expected {}, got {}",
                file_entry.relative_path,
                file_entry.size,
                contents.len()
            ));
        }

        let actual_hash = sha256_hex(&contents);
        if let Some(expected_hash) = &file_entry.sha256 {
            if !actual_hash.eq_ignore_ascii_case(expected_hash) {
                return Err(format!(
                    "snapshot hash mismatch for {}: archive is corrupted or modified",
                    file_entry.relative_path
                ));
            }
        }

        let output = extract_dir.join(Path::new(&file_entry.relative_path));
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create restore staging directory: {e}"))?;
        }
        let mut output_file = fs::File::create(&output)
            .map_err(|e| format!("failed to stage {}: {e}", file_entry.relative_path))?;
        output_file
            .write_all(&contents)
            .map_err(|e| format!("failed to stage {}: {e}", file_entry.relative_path))?;
        output_file
            .sync_all()
            .map_err(|e| format!("failed to sync {}: {e}", file_entry.relative_path))?;
    }

    Ok(manifest)
}

fn read_manifest(
    archive: &mut zip::ZipArchive<fs::File>,
    snapshot_id: &str,
) -> Result<SnapshotManifest, String> {
    let manifest: SnapshotManifest = {
        let mut entry = archive
            .by_name("manifest.json")
            .map_err(|e| format!("snapshot manifest is missing: {e}"))?;
        let mut content = String::new();
        entry
            .read_to_string(&mut content)
            .map_err(|e| format!("failed to read snapshot manifest: {e}"))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse snapshot manifest: {e}"))?
    };

    if manifest.schema_version == 0 || manifest.schema_version > SNAPSHOT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported snapshot schema version {} (maximum supported is {})",
            manifest.schema_version, SNAPSHOT_SCHEMA_VERSION
        ));
    }
    if manifest.snapshot.id != snapshot_id {
        return Err(format!(
            "snapshot identity mismatch: requested {snapshot_id}, archive contains {}",
            manifest.snapshot.id
        ));
    }

    Ok(manifest)
}

fn validate_relative_path(relative_path: &str) -> Result<(), String> {
    if relative_path.is_empty() || relative_path.contains('\\') {
        return Err(format!("invalid snapshot path {relative_path:?}"));
    }
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("unsafe snapshot path {relative_path:?}"));
    }
    let root = path
        .components()
        .next()
        .and_then(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .ok_or_else(|| format!("invalid snapshot path {relative_path:?}"))?;
    if !TRACKED_ENTRIES.contains(&root) {
        return Err(format!(
            "snapshot path is outside tracked entries: {relative_path}"
        ));
    }
    if TRACKED_ENTRIES
        .iter()
        .any(|file| *file == root && Path::new(file).extension().is_some())
        && path.components().count() != 1
    {
        return Err(format!(
            "snapshot file path cannot contain children: {relative_path}"
        ));
    }
    Ok(())
}

fn snapshot_roots(manifest: &SnapshotManifest) -> HashSet<&str> {
    manifest
        .files
        .iter()
        .filter_map(|entry| entry.relative_path.split('/').next())
        .collect()
}

fn sha256_hex(contents: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

/// Reverse a failed restore without renaming over live destinations.  Any
/// partially promoted snapshot entries are first moved aside.  If rollback
/// itself fails, both the backup and displaced paths are retained and named in
/// the returned error so recovery never silently loses the protected state.
fn rollback_restore(
    instance_dir: &Path,
    pre_dir: &Path,
    promoted: &[String],
    moved_current: &[String],
    restore_id: &str,
) -> Result<(), String> {
    let failed_dir = instance_dir.join(format!(".agora_failed_restore_{restore_id}"));
    fs::create_dir_all(&failed_dir)
        .map_err(|e| format!("failed to create rollback displacement directory: {e}"))?;

    let mut errors = Vec::new();
    for entry_name in promoted.iter().rev() {
        let live = instance_dir.join(entry_name);
        if live.exists() {
            let displaced = failed_dir.join(entry_name);
            if let Some(parent) = displaced.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    errors.push(format!(
                        "could not prepare displacement for {entry_name}: {e}"
                    ));
                    continue;
                }
            }
            if let Err(e) = fs::rename(&live, &displaced) {
                errors.push(format!(
                    "could not displace partial restore {entry_name}: {e}"
                ));
            }
        }
    }

    for entry_name in moved_current.iter().rev() {
        let backup = pre_dir.join(entry_name);
        let live = instance_dir.join(entry_name);
        if !backup.exists() {
            errors.push(format!("rollback backup is missing for {entry_name}"));
            continue;
        }
        if live.exists() {
            errors.push(format!(
                "rollback destination still exists for {entry_name}"
            ));
            continue;
        }
        if let Err(e) = fs::rename(&backup, &live) {
            errors.push(format!("could not restore original {entry_name}: {e}"));
        }
    }

    if errors.is_empty() {
        let _ = fs::remove_dir_all(&failed_dir);
        let marker = instance_dir.join(RESTORE_MARKER);
        let _ = fs::remove_file(marker);
        Ok(())
    } else {
        Err(format!(
            "rollback incomplete; original data remains in {} and partial data in {}: {}",
            pre_dir.display(),
            failed_dir.display(),
            errors.join("; ")
        ))
    }
}

fn combine_restore_error(primary: String, rollback: Result<(), String>) -> String {
    match rollback {
        Ok(()) => format!("{primary}; original instance state was restored"),
        Err(rollback_error) => format!("{primary}; {rollback_error}"),
    }
}

/// List all snapshots for an instance.
pub fn list_snapshots(instance_dir: &Path) -> Result<Vec<Snapshot>, String> {
    let marker = instance_dir.join(RESTORE_MARKER);
    if marker.exists() {
        return Err(
            "Previous restore was interrupted. Check .agora_pre_restore/ for backed-up files."
                .into(),
        );
    }

    let dir = snapshots_dir(instance_dir);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();

    let entries = fs::read_dir(&dir).map_err(|e| format!("failed to read snapshots dir: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("zip") {
            continue;
        }

        let file = match fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(_) => continue,
        };

        let mut manifest_entry = match archive.by_name("manifest.json") {
            Ok(e) => e,
            Err(_) => continue,
        };

        let mut content = String::new();
        if manifest_entry.read_to_string(&mut content).is_err() {
            continue;
        }

        if let Ok(manifest) = serde_json::from_str::<SnapshotManifest>(&content) {
            snapshots.push(manifest.snapshot);
        }
    }

    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(snapshots)
}

/// Delete a snapshot.
pub fn delete_snapshot(instance_dir: &Path, snapshot_id: &str) -> Result<(), String> {
    let zip_path = snapshot_zip_path(instance_dir, snapshot_id);
    if !zip_path.exists() {
        return Err(format!("snapshot {snapshot_id} not found"));
    }

    fs::remove_file(&zip_path).map_err(|e| format!("failed to delete snapshot zip: {e}"))?;

    let dir = snapshots_dir(instance_dir);
    if dir.exists()
        && dir
            .read_dir()
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
    {
        let _ = fs::remove_dir(&dir);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use zip::write::FileOptions;

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

        fs::write(inst.join("mods").join("test.jar"), b"modified").unwrap();
        fs::write(inst.join("options.txt"), b"modified").unwrap();

        restore_snapshot(&inst, &snap.id).unwrap();

        assert_eq!(
            fs::read(inst.join("mods").join("test.jar")).unwrap(),
            b"mod content"
        );
        assert_eq!(
            fs::read(inst.join("options.txt")).unwrap(),
            b"render_distance=12"
        );

        assert!(!inst.join(".agora_pre_restore").exists());
    }

    #[test]
    fn snapshot_is_immutable() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);

        let snap = create_snapshot(&inst, None).unwrap();

        fs::write(inst.join("mods").join("test.jar"), b"changed").unwrap();

        let zip_path = snapshot_zip_path(&inst, &snap.id);
        let file = fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut entry = archive.by_name("mods/test.jar").unwrap();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        assert_eq!(content, b"mod content");
    }

    #[test]
    fn delete_snapshot_removes_zip() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);

        let snap = create_snapshot(&inst, None).unwrap();
        let zip_path = snapshot_zip_path(&inst, &snap.id);
        assert!(zip_path.exists());

        delete_snapshot(&inst, &snap.id).unwrap();
        assert!(!zip_path.exists());
    }

    #[test]
    fn list_snapshots_empty_when_none() {
        let tmp = TempDir::new().unwrap();
        let snaps = list_snapshots(tmp.path()).unwrap();
        assert!(snaps.is_empty());
    }

    #[test]
    fn legacy_snapshot_without_hashes_remains_listable_and_restorable() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);
        let id = "legacy-snapshot";
        fs::create_dir_all(snapshots_dir(&inst)).unwrap();

        let contents = b"legacy mod content";
        let snapshot = Snapshot {
            id: id.into(),
            label: Some("from-v1".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
            file_count: 1,
            size_estimate: contents.len() as u64,
        };
        let legacy_manifest = serde_json::json!({
            "snapshot": snapshot,
            "files": [{
                "relative_path": "mods/legacy.jar",
                "size": contents.len()
            }]
        });
        write_test_archive(
            &snapshot_zip_path(&inst, id),
            &[
                ("mods/legacy.jar", contents.as_slice()),
                (
                    "manifest.json",
                    serde_json::to_vec_pretty(&legacy_manifest)
                        .unwrap()
                        .as_slice(),
                ),
            ],
        );

        let listed = list_snapshots(&inst).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);

        fs::write(inst.join("mods").join("test.jar"), b"current state").unwrap();
        restore_snapshot(&inst, id).unwrap();
        assert_eq!(
            fs::read(inst.join("mods").join("legacy.jar")).unwrap(),
            contents
        );
        assert!(!inst.join("mods").join("test.jar").exists());
    }

    #[test]
    fn restore_rejects_hash_tampering_before_live_mutation() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);
        let snapshot = create_snapshot(&inst, None).unwrap();
        fs::write(inst.join("mods").join("test.jar"), b"current safe state").unwrap();

        rewrite_archive_entry(
            &snapshot_zip_path(&inst, &snapshot.id),
            "mods/test.jar",
            b"bad content",
        );

        let error = restore_snapshot(&inst, &snapshot.id).unwrap_err();
        assert!(error.contains("hash mismatch"));
        assert_eq!(
            fs::read(inst.join("mods").join("test.jar")).unwrap(),
            b"current safe state"
        );
        assert!(!inst.join(RESTORE_MARKER).exists());
    }

    #[test]
    fn partial_promotion_failure_restores_the_entire_current_state() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);
        let snapshot = create_snapshot(&inst, None).unwrap();

        fs::write(inst.join("mods").join("test.jar"), b"new mod state").unwrap();
        fs::write(
            inst.join("config").join("settings.toml"),
            b"new config state",
        )
        .unwrap();
        fs::write(inst.join("options.txt"), b"new options state").unwrap();

        let error = restore_snapshot_impl(&inst, &snapshot.id, Some(1)).unwrap_err();
        assert!(error.contains("original instance state was restored"));
        assert_eq!(
            fs::read(inst.join("mods").join("test.jar")).unwrap(),
            b"new mod state"
        );
        assert_eq!(
            fs::read(inst.join("config").join("settings.toml")).unwrap(),
            b"new config state"
        );
        assert_eq!(
            fs::read(inst.join("options.txt")).unwrap(),
            b"new options state"
        );
        assert!(!inst.join(RESTORE_MARKER).exists());
    }

    #[test]
    fn interrupted_restore_is_recovered_before_a_new_attempt() {
        let tmp = TempDir::new().unwrap();
        let inst = make_instance(&tmp);
        fs::write(
            inst.join("mods").join("test.jar"),
            b"protected current state",
        )
        .unwrap();

        let pre = pre_restore_dir(&inst);
        fs::create_dir_all(&pre).unwrap();
        fs::rename(inst.join("mods"), pre.join("mods")).unwrap();
        fs::create_dir_all(inst.join("mods")).unwrap();
        fs::write(
            inst.join("mods").join("test.jar"),
            b"partial snapshot state",
        )
        .unwrap();
        fs::write(inst.join(RESTORE_MARKER), b"restore in progress").unwrap();

        let error = restore_snapshot(&inst, "missing-snapshot").unwrap_err();
        assert!(error.contains("not found"));
        assert_eq!(
            fs::read(inst.join("mods").join("test.jar")).unwrap(),
            b"protected current state"
        );
        assert!(!inst.join(RESTORE_MARKER).exists());
        assert!(!pre.exists());
    }

    fn write_test_archive(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, contents) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(contents).unwrap();
        }
        zip.finish().unwrap();
    }

    fn rewrite_archive_entry(path: &Path, target: &str, replacement: &[u8]) {
        let mut entries = Vec::new();
        {
            let file = fs::File::open(path).unwrap();
            let mut archive = zip::ZipArchive::new(file).unwrap();
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index).unwrap();
                if entry.is_dir() {
                    continue;
                }
                let name = entry.name().to_string();
                let mut contents = Vec::new();
                entry.read_to_end(&mut contents).unwrap();
                if name == target {
                    contents = replacement.to_vec();
                }
                entries.push((name, contents));
            }
        }

        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, contents) in entries {
            zip.start_file(name, options).unwrap();
            zip.write_all(&contents).unwrap();
        }
        zip.finish().unwrap();
    }
}
