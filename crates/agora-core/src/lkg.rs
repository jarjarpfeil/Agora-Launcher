//! Last-known-good (LKG) and launch classification.
//!
//! Pure logic — no filesystem or process dependencies.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Launch outcome
// ---------------------------------------------------------------------------

/// Result of classifying a single launch session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchOutcome {
    /// Exit code 0, runtime >= threshold, no crash report or log signature.
    Success,
    /// Non-zero exit, crash report found, or signal termination.
    Crash,
    /// User initiated stop via the app (not a game crash).
    Cancelled,
    /// Process vanished without notification — conservative non-promotion.
    Unknown,
    /// Exit code 0 but runtime < threshold (opened and closed instantly).
    Abandoned,
}

// ---------------------------------------------------------------------------
// LKG state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LkgState {
    pub current_lkg_snapshot_id: Option<String>,
    pub last_promoted_at: Option<String>,
    pub last_launch_session_id: Option<String>,
    pub last_launch_outcome: Option<LaunchOutcome>,
    pub schema_version: u32,
}

impl Default for LkgState {
    fn default() -> Self {
        Self {
            current_lkg_snapshot_id: None,
            last_promoted_at: None,
            last_launch_session_id: None,
            last_launch_outcome: None,
            schema_version: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Input to the classification function.
#[derive(Debug, Clone)]
pub struct LaunchEvents {
    pub exit_code: Option<i32>,
    pub runtime_ms: u64,
    pub was_user_cancelled: bool,
    pub crash_report_found: bool,
    pub log_crash_signature_matched: bool,
}

/// Threshold below which a clean exit is considered abandoned (in ms).
pub const DEFAULT_ABANDON_THRESHOLD_MS: u64 = 60_000; // 60 seconds

/// Classify a launch session based on observed events.
///
/// Pure function — deterministic for identical inputs.
pub fn classify_launch(events: &LaunchEvents) -> LaunchOutcome {
    if events.was_user_cancelled {
        return LaunchOutcome::Cancelled;
    }

    match events.exit_code {
        Some(0) => {
            if events.runtime_ms < DEFAULT_ABANDON_THRESHOLD_MS {
                LaunchOutcome::Abandoned
            } else if events.crash_report_found || events.log_crash_signature_matched {
                LaunchOutcome::Crash
            } else {
                LaunchOutcome::Success
            }
        }
        Some(_) => LaunchOutcome::Crash,
        None => {
            if events.crash_report_found || events.log_crash_signature_matched {
                LaunchOutcome::Crash
            } else {
                LaunchOutcome::Unknown
            }
        }
    }
}

/// Whether an outcome promotes a snapshot to LKG.
pub fn promotes_to_lkg(outcome: &LaunchOutcome) -> bool {
    matches!(outcome, LaunchOutcome::Success)
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    pub path: String,
    pub old_sha256: Option<String>,
    pub new_sha256: Option<String>,
    pub old_size: Option<u64>,
    pub new_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub from_snapshot_id: Option<String>,
    pub to_snapshot_id: Option<String>,
    pub added: Vec<DiffEntry>,
    pub removed: Vec<DiffEntry>,
    pub modified: Vec<DiffEntry>,
    pub unchanged_count: usize,
    pub total_files_a: usize,
    pub total_files_b: usize,
}

/// A simplified file index for diff computation.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// Compute a diff between two file indexes.
pub fn compute_diff(
    from: &[FileEntry],
    to: &[FileEntry],
    from_id: Option<String>,
    to_id: Option<String>,
) -> Diff {
    use std::collections::HashMap;

    let from_map: HashMap<&str, &FileEntry> = from.iter().map(|f| (f.path.as_str(), f)).collect();
    let to_map: HashMap<&str, &FileEntry> = to.iter().map(|f| (f.path.as_str(), f)).collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();
    let mut unchanged_count = 0;

    for (path, from_entry) in &from_map {
        match to_map.get(path) {
            None => {
                removed.push(DiffEntry {
                    path: path.to_string(),
                    old_sha256: Some(from_entry.sha256.clone()),
                    new_sha256: None,
                    old_size: Some(from_entry.size),
                    new_size: None,
                });
            }
            Some(to_entry) if from_entry.sha256 != to_entry.sha256 => {
                modified.push(DiffEntry {
                    path: path.to_string(),
                    old_sha256: Some(from_entry.sha256.clone()),
                    new_sha256: Some(to_entry.sha256.clone()),
                    old_size: Some(from_entry.size),
                    new_size: Some(to_entry.size),
                });
            }
            Some(_) => {
                unchanged_count += 1;
            }
        }
    }

    for (path, to_entry) in &to_map {
        if !from_map.contains_key(path) {
            added.push(DiffEntry {
                path: path.to_string(),
                old_sha256: None,
                new_sha256: Some(to_entry.sha256.clone()),
                old_size: None,
                new_size: Some(to_entry.size),
            });
        }
    }

    Diff {
        from_snapshot_id: from_id,
        to_snapshot_id: to_id,
        added,
        removed,
        modified,
        unchanged_count,
        total_files_a: from.len(),
        total_files_b: to.len(),
    }
}

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicy {
    /// Number of LKG snapshots to keep (including current).
    pub keep_lkg_count: u32,
    /// Number of non-LKG snapshots to keep (most recent).
    pub keep_non_lkg_count: u32,
    /// Number of pre-restore snapshots to keep.
    pub keep_pre_restore_count: u32,
    /// Maximum total snapshot storage per instance in bytes.
    pub size_cap_bytes: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            keep_lkg_count: 3,
            keep_non_lkg_count: 1,
            keep_pre_restore_count: 1,
            size_cap_bytes: 2_000_000_000, // 2 GB
        }
    }
}

/// Compute which snapshot ids should be evicted under a given policy.
pub fn retention_plan(
    snapshot_ids: &[String],
    lkg_ids: &[String],
    pre_restore_ids: &[String],
    policy: &RetentionPolicy,
) -> Vec<String> {
    let mut keep = Vec::new();
    let mut evict = Vec::new();

    // Always keep current LKG.
    if let Some(current_lkg) = lkg_ids.first() {
        keep.push(current_lkg.clone());
    }

    // Keep N most recent LKG snapshots.
    for id in lkg_ids.iter().take(policy.keep_lkg_count as usize) {
        if !keep.contains(id) {
            keep.push(id.clone());
        }
    }

    // Keep most recent non-LKG snapshot.
    for id in snapshot_ids.iter().rev() {
        if !keep.contains(id) && !lkg_ids.contains(id) {
            keep.push(id.clone());
            break;
        }
    }

    // Keep most recent pre-restore.
    if let Some(pr) = pre_restore_ids.first() {
        if !keep.contains(pr) {
            keep.push(pr.clone());
        }
    }

    for id in snapshot_ids {
        if !keep.contains(id) {
            evict.push(id.clone());
        }
    }

    evict
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_success() {
        assert_eq!(
            classify_launch(&LaunchEvents {
                exit_code: Some(0),
                runtime_ms: 120_000,
                was_user_cancelled: false,
                crash_report_found: false,
                log_crash_signature_matched: false,
            }),
            LaunchOutcome::Success,
        );
    }

    #[test]
    fn test_classify_crash_nonzero_exit() {
        assert_eq!(
            classify_launch(&LaunchEvents {
                exit_code: Some(1),
                runtime_ms: 300_000,
                was_user_cancelled: false,
                crash_report_found: false,
                log_crash_signature_matched: false,
            }),
            LaunchOutcome::Crash,
        );
    }

    #[test]
    fn test_classify_crash_report() {
        assert_eq!(
            classify_launch(&LaunchEvents {
                exit_code: Some(0),
                runtime_ms: 300_000,
                was_user_cancelled: false,
                crash_report_found: true,
                log_crash_signature_matched: false,
            }),
            LaunchOutcome::Crash,
        );
    }

    #[test]
    fn test_classify_abandoned() {
        assert_eq!(
            classify_launch(&LaunchEvents {
                exit_code: Some(0),
                runtime_ms: 5_000,
                was_user_cancelled: false,
                crash_report_found: false,
                log_crash_signature_matched: false,
            }),
            LaunchOutcome::Abandoned,
        );
    }

    #[test]
    fn test_classify_cancelled() {
        assert_eq!(
            classify_launch(&LaunchEvents {
                exit_code: None,
                runtime_ms: 60_000,
                was_user_cancelled: true,
                crash_report_found: false,
                log_crash_signature_matched: false,
            }),
            LaunchOutcome::Cancelled,
        );
    }

    #[test]
    fn test_classify_unknown() {
        assert_eq!(
            classify_launch(&LaunchEvents {
                exit_code: None,
                runtime_ms: 120_000,
                was_user_cancelled: false,
                crash_report_found: false,
                log_crash_signature_matched: false,
            }),
            LaunchOutcome::Unknown,
        );
    }

    #[test]
    fn test_promotes_to_lkg() {
        assert!(promotes_to_lkg(&LaunchOutcome::Success));
        assert!(!promotes_to_lkg(&LaunchOutcome::Crash));
        assert!(!promotes_to_lkg(&LaunchOutcome::Cancelled));
        assert!(!promotes_to_lkg(&LaunchOutcome::Unknown));
        assert!(!promotes_to_lkg(&LaunchOutcome::Abandoned));
    }

    #[test]
    fn test_compute_diff_empty() {
        let diff = compute_diff(&[], &[], None, None);
        assert_eq!(diff.added.len(), 0);
        assert_eq!(diff.removed.len(), 0);
        assert_eq!(diff.modified.len(), 0);
    }

    #[test]
    fn test_compute_diff_added_removed_modified() {
        let from = vec![
            FileEntry { path: "mod-a.jar".into(), sha256: "aaa".into(), size: 100 },
            FileEntry { path: "mod-b.jar".into(), sha256: "bbb".into(), size: 200 },
        ];
        let to = vec![
            FileEntry { path: "mod-b.jar".into(), sha256: "bbb-changed".into(), size: 210 },
            FileEntry { path: "mod-c.jar".into(), sha256: "ccc".into(), size: 300 },
        ];
        let diff = compute_diff(&from, &to, None, None);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].path, "mod-c.jar");
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0].path, "mod-a.jar");
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.modified[0].path, "mod-b.jar");
        assert_eq!(diff.unchanged_count, 0);
    }

    #[test]
    fn test_retention_plan_keeps_current_lkg() {
        let ids: Vec<String> = (0..5).map(|i| format!("snap-{i}")).collect();
        let lkg: Vec<String> = vec!["snap-4".into(), "snap-2".into()];
        let policy = RetentionPolicy::default();
        let evict = retention_plan(&ids, &lkg, &[], &policy);
        assert!(!evict.contains(&"snap-4".to_string()));
    }

    #[test]
    fn test_retention_plan_evicts_oldest() {
        let ids: Vec<String> = (0..10).map(|i| format!("snap-{i}")).collect();
        let lkg: Vec<String> = vec!["snap-9".into(), "snap-8".into(), "snap-7".into()];
        let policy = RetentionPolicy {
            keep_lkg_count: 2,
            keep_non_lkg_count: 1,
            keep_pre_restore_count: 1,
            size_cap_bytes: 2_000_000_000,
        };
        let evict = retention_plan(&ids, &lkg, &[], &policy);
        // snap-9 and snap-8 kept (current LKG + 1 more), snap-7 evicted
        assert!(evict.contains(&"snap-7".to_string()));
        assert!(!evict.contains(&"snap-9".to_string()));
        assert!(!evict.contains(&"snap-8".to_string()));
    }
}
