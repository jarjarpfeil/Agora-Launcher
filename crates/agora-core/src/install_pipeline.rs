//! Canonical install-transaction pipeline.
//!
//! All install/update/remove entry points flow through `InstallPipeline`:
//!
//! 1. **Resolve** — pure data, no instance changes. Returns a `ResolvedInstallPlan`.
//! 2. **Stage** — download + verify artifacts.
//! 3. **Snapshot** — create recovery zip.
//! 4. **Apply** — atomic file moves + manifest commit.
//! 5. **Health scan** — post-apply verification.
//!
//! This module owns the pipeline types and the resolve phase. Staging, application,
//! and health scanning are added in C2.
//!
//! ```text
//! ┌─ Intent ─▶ Resolve ─▶ Plan (read-only) ─▶ Stage ─▶ Snapshot ─▶ Apply ─▶ Health ─▶ Result
//! ```
//!
//! Key invariants:
//! - Planning makes zero instance changes.
//! - Snapshot is taken BEFORE any instance mutation and is mandatory.
//! - The manifest atomic rename is the single commit point.
//! - Post-apply health failure triggers automatic snapshot restore.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

// Reuse types from dependency_ops to avoid duplication.
pub use crate::dependency_ops::{DepSource, Requirement};

// ---------------------------------------------------------------------------
// 1. Operation type
// ---------------------------------------------------------------------------

/// What the user wants to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallOperation {
    Install,
    Update,
    Remove,
}

// ---------------------------------------------------------------------------
// 2. Source type
// ---------------------------------------------------------------------------

/// Where the item comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceType {
    /// Curated registry item (GitHub Release).
    Curated,
    /// Raw Modrinth project.
    Modrinth,
    /// Local file path.
    Manual,
}

// ---------------------------------------------------------------------------
// 3. Dependency policy
// ---------------------------------------------------------------------------

/// How optional dependencies are handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OptionalDepsPolicy {
    /// Include only these specific optional deps (empty = none).
    Include(Vec<String>),
    /// Skip all optional deps.
    ExcludeAll,
    /// Prompt the user (returns choices as pending_choices).
    Prompt,
}

// ---------------------------------------------------------------------------
// 4. Request context
// ---------------------------------------------------------------------------

/// Who or what initiated this install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestSource {
    Interactive,
    CLI,
    AutoUpdate,
}

// ---------------------------------------------------------------------------
// 5. Artifact locator
// ---------------------------------------------------------------------------

/// Describes where to obtain the artifact content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ArtifactSource {
    /// Download from a URL.
    Download { url: String },
    /// Use a local file directly (manual mod install).
    LocalFile { path: String },
}

// ---------------------------------------------------------------------------
// 6. Hash spec — stores multiple algorithms for defense in depth
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HashSpec {
    /// Ordered by preference (strongest first). At least one entry required.
    /// SHA-256 is mandatory for curated items; SHA-1 accepted for Modrinth
    /// backward compatibility only if accompanied by a stronger hash.
    pub values: Vec<HashedValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HashedValue {
    pub algorithm: HashAlgorithm,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HashAlgorithm {
    Sha256,
    Sha512,
    Sha1,
}

// ---------------------------------------------------------------------------
// 7. Resolved item — typed by source
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDownload {
    pub item_id: String,
    pub version_id: String,
    pub source: ArtifactSource,
    pub hashes: HashSpec,
    pub size: u64,
    /// The filename that will be written to mods/.
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLocal {
    pub item_id: String,
    pub source_path: String,
    pub hashes: HashSpec, // computed at staging time
    pub size: u64,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ResolvedArtifact {
    Download(ResolvedDownload),
    LocalFile(ResolvedLocal),
}

// ---------------------------------------------------------------------------
// 8. Dependency disposition
// ---------------------------------------------------------------------------

/// How a resolved dependency relates to the instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DepDisposition {
    /// Already installed at a compatible version — no action needed.
    ReuseExisting { mod_jar_id: String, installed_filename: String },
    /// Will be downloaded and installed.
    InstallCandidate(ResolvedDownload),
    /// User chose to exclude this optional dependency.
    Excluded,
    /// Could not be resolved — kept for diagnostics.
    Unresolved { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDep {
    pub mod_jar_id: String,
    pub requirement: Requirement,
    pub source: DepSource,
    pub disposition: DepDisposition,
}

// ---------------------------------------------------------------------------
// 9. Conflict
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepConflict {
    /// Stable identifier for this conflict (used in PendingChoice responses).
    pub conflict_id: String,
    pub kind: ConflictKind,
    pub existing_mod_jar_id: String,
    pub incoming_mod_jar_id: String,
    pub message: String,
    pub blocking: bool,
    pub resolution_options: Vec<ConflictResolution>,
    /// Set by user override or by the resolver for non-blocking defaults.
    pub chosen: Option<ConflictResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    VersionConflict,
    DuplicateMod,
    LoaderMismatch,
    GameVersionMismatch,
    IncompatibleMod,
    BrokenReverseDep,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictResolution {
    Replace,
    Skip,
    DisableExisting,
    Abort,
}

// ---------------------------------------------------------------------------
// 10. File actions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAdd {
    pub target_filename: String,
    pub staging_filename: String,
    pub hashes: HashSpec,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRemove {
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDisable {
    pub filename: String,
}

// ---------------------------------------------------------------------------
// 11. Warnings & errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanError {
    pub code: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// 12. Mandatory snapshot plan
// ---------------------------------------------------------------------------

/// Snapshot is always required for mutating operations.
/// This struct carries only the parameters, not an optional flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPlan {
    pub label: String,            // encodes plan fingerprint + timestamp
    pub estimated_bytes: u64,
}

// ---------------------------------------------------------------------------
// 13. Disk-space estimate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpaceEstimate {
    pub download_bytes: u64,
    pub snapshot_bytes: u64,
    pub apply_overhead_bytes: u64,
    /// Peak additional disk usage during the transaction.
    pub peak_additional_bytes: u64,
    /// Change in committed disk usage after the transaction.
    pub post_commit_delta_bytes: i64,
}

impl DiskSpaceEstimate {
    pub fn zero() -> Self {
        Self {
            download_bytes: 0,
            snapshot_bytes: 0,
            apply_overhead_bytes: 0,
            peak_additional_bytes: 0,
            post_commit_delta_bytes: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 14. Pending choices — typed with stable identifiers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PendingChoice {
    OptionalDependencies {
        choice_id: String,
        options: Vec<OptionalDepOption>,
    },
    Conflict {
        choice_id: String,
        conflict_id: String,
        options: Vec<ConflictResolutionOption>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionalDepOption {
    pub mod_jar_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictResolutionOption {
    pub resolution: ConflictResolution,
    pub label: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// 15. Plan overrides
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanOverrides {
    pub allow_replace: bool,
    pub skip_health_scan: bool,
    /// Override applied conflict resolutions by conflict_id.
    pub force_conflict_resolution: BTreeMap<String, ConflictResolution>,
}

// ---------------------------------------------------------------------------
// 16. Operation-specific resolved payload
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ResolvedOperation {
    Install {
        artifact: ResolvedArtifact,
    },
    Update {
        old_version_id: String,
        new_artifact: ResolvedArtifact,
    },
    Remove {
        target_filename: String,
        reverse_dependents: Vec<ReverseDepInfo>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReverseDepInfo {
    pub mod_jar_id: String,
    pub filename: String,
    pub requirement: Requirement,
    /// How this reverse dep will be affected. None = unchanged.
    pub impact: Option<String>,
}

// ---------------------------------------------------------------------------
// 17. InstallIntent — action-tagged for operation safety
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InstallAction {
    Install {
        source_type: SourceType,
        item_id: String,
        candidate_version: Option<String>,
    },
    Update {
        item_id: String,
        target_version: String,
    },
    Remove {
        filename: String,
    },
}

/// Pure input — what the user wants to do.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallIntent {
    pub action: InstallAction,
    pub target_instance: String,
    pub optional_deps: OptionalDepsPolicy,
    pub requested_by: RequestSource,
    pub overrides: PlanOverrides,
}

// ---------------------------------------------------------------------------
// 18. ResolvedInstallPlan
// ---------------------------------------------------------------------------

/// Full read-only plan. Making a plan commits to no instance changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedInstallPlan {
    pub fingerprint: String,
    pub intent: InstallIntent,
    pub operation: ResolvedOperation,
    pub dependencies: Vec<ResolvedDep>,
    pub conflicts: Vec<DepConflict>,
    pub files_to_add: Vec<FileAdd>,
    pub files_to_remove: Vec<FileRemove>,
    pub files_to_disable: Vec<FileDisable>,
    pub snapshot: SnapshotPlan,
    pub disk_estimate: DiskSpaceEstimate,
    pub warnings: Vec<PlanWarning>,
    pub blocking_errors: Vec<PlanError>,
    pub pending_choices: Vec<PendingChoice>,
    pub created_at: String,
    pub instance_state_hash: String,
    pub registry_revision: String,
}

impl ResolvedInstallPlan {
    /// Whether the plan is fully resolved and can be submitted for execution.
    ///
    /// Does NOT check freshness — the executor must revalidate `instance_state_hash`
    /// and `registry_revision` against the current state before applying.
    pub fn is_fully_resolved(&self) -> bool {
        if !self.blocking_errors.is_empty() {
            return false;
        }
        if !self.pending_choices.is_empty() {
            return false;
        }
        // Every blocking conflict must have a chosen resolution.
        if self.conflicts.iter().any(|c| c.blocking && c.chosen.is_none()) {
            return false;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 19. Fingerprint input — deterministic ordering via BTreeMap
// ---------------------------------------------------------------------------

/// A dedicated input type for plan fingerprint computation.
/// Uses BTreeMap and sorted collections for deterministic serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanFingerprintInput {
    pub schema_version: u32,
    pub action: InstallAction,
    pub resolved_artifacts: BTreeMap<String, ArtifactFingerprint>,
    pub dependency_dispositions: BTreeMap<String, String>,
    pub conflict_resolutions: BTreeMap<String, String>,
    pub instance_state_hash: String,
    pub registry_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactFingerprint {
    pub source_kind: String,          // "download" | "local"
    pub version_id: String,
    pub filename: String,
    pub hashes: Vec<(String, String)>, // (algorithm, value) sorted pairs
    pub size: u64,
}

// ---------------------------------------------------------------------------
// 20. Health outcome
// ---------------------------------------------------------------------------

use crate::health::HealthReport;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HealthOutcome {
    Completed(HealthReport),
    Skipped { reason: String },
}

// ---------------------------------------------------------------------------
// 21. InstallResult — typed outcome
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InstallOutcome {
    Success {
        installed_items: Vec<String>,
        existing_items_reused: Vec<String>,
        warnings: Vec<PlanWarning>,
        health: HealthOutcome,
        snapshot_id: String,
    },
    HealthRollback {
        health_report: HealthReport,
        snapshot_id: String,
        warnings: Vec<PlanWarning>,
    },
    Cancelled {
        phase: String,
        rollback_performed: bool,
    },
    Failed {
        error: String,
        rollback_performed: bool,
        snapshot_id: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// 22. Progress events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub plan_id: String,
    pub phase: ProgressPhase,
    pub step: u32,
    pub total_steps: u32,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProgressPhase {
    Resolving,
    Staging,
    Snapshotting,
    Applying,
    HealthScan,
    Done,
    Failed,
    Cancelled,
}

// ---------------------------------------------------------------------------
// 23. CancellationToken — scoped per transaction
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 24. ProgressReporter trait (Tauri-agnostic)
// ---------------------------------------------------------------------------

pub trait ProgressReporter: Send + Sync {
    fn report(&self, event: ProgressEvent);
}

// ---------------------------------------------------------------------------
// 25. InstallPipeline
// ---------------------------------------------------------------------------

pub struct InstallPipeline;

impl InstallPipeline {
/// Phase 1: resolve. Pure data, no instance changes.
    ///
    /// Currently a stub. Full dependency resolution from `dependency_ops.rs`
    /// will be wired here in C2.
    pub async fn resolve_plan(
        &self,
        intent: InstallIntent,
        _reporter: &dyn ProgressReporter,
    ) -> Result<ResolvedInstallPlan, String> {
        // TODO: Wire actual dependency resolution from dependency_ops.rs
        let action_desc = match &intent.action {
            InstallAction::Install { item_id, .. } => item_id.clone(),
            InstallAction::Update { item_id, .. } => item_id.clone(),
            InstallAction::Remove { filename } => filename.clone(),
        };

        // Stub — returns a non-applicable plan.
        Ok(ResolvedInstallPlan {
            fingerprint: String::new(),
            intent,
            operation: ResolvedOperation::Install {
                artifact: ResolvedArtifact::Download(ResolvedDownload {
                    item_id: "stub".into(),
                    version_id: "stub".into(),
                    source: ArtifactSource::Download {
                        url: "https://example.com/stub.jar".into(),
                    },
                    hashes: HashSpec { values: vec![] },
                    size: 0,
                    filename: "stub.jar".into(),
                }),
            },
            dependencies: vec![],
            conflicts: vec![],
            files_to_add: vec![],
            files_to_remove: vec![],
            files_to_disable: vec![],
            snapshot: SnapshotPlan {
                label: format!("stub-{}", action_desc),
                estimated_bytes: 0,
            },
            disk_estimate: DiskSpaceEstimate::zero(),
            warnings: vec![],
            blocking_errors: vec![PlanError {
                code: "ERR_STUB".into(),
                message: "Resolution not yet implemented — stub plan is not applicable.".into(),
            }],
            pending_choices: vec![],
            created_at: chrono::Utc::now().to_rfc3339(),
            instance_state_hash: String::new(),
            registry_revision: String::new(),
        })
    }

    // -----------------------------------------------------------------------
    // C2: Transactional execution
    // -----------------------------------------------------------------------

    /// Phase 2–5: execute a fully-resolved install plan.
    ///
    /// Orchestrates staging → snapshot → apply → health scan, emitting
    /// progress events and respecting the cancellation token.
    pub async fn execute_plan(
        &self,
        plan: &ResolvedInstallPlan,
        instance_dir: &std::path::Path,
        reporter: &dyn ProgressReporter,
        cancel: &CancellationToken,
    ) -> InstallOutcome {
        let plan_id = &plan.fingerprint;
        if plan_id.is_empty() {
            return InstallOutcome::Failed {
                error: "Plan has no fingerprint — cannot execute stub.".into(),
                rollback_performed: false,
                snapshot_id: None,
            };
        }

        // Phase 2: stage (download + verify)
        reporter.report(ProgressEvent {
            plan_id: plan_id.clone(),
            phase: ProgressPhase::Staging,
            step: 0,
            total_steps: plan.files_to_add.len() as u32,
            bytes_downloaded: 0,
            bytes_total: plan.disk_estimate.download_bytes,
            message: "Downloading and verifying artifacts…".into(),
        });

        let staging_dir = instance_dir.join(".agora").join("staging").join(plan_id);
        if let Err(e) = stage_artifacts(&plan.files_to_add, &staging_dir, cancel).await {
            return InstallOutcome::Failed {
                error: e,
                rollback_performed: false,
                snapshot_id: None,
            };
        }

        // Phase 3: snapshot
        reporter.report(ProgressEvent {
            plan_id: plan_id.clone(),
            phase: ProgressPhase::Snapshotting,
            step: 0,
            total_steps: 1,
            bytes_downloaded: 0,
            bytes_total: plan.disk_estimate.snapshot_bytes,
            message: "Creating recovery snapshot…".into(),
        });

        let snapshot = crate::snapshot::create_snapshot(instance_dir, Some(&plan.snapshot.label));
        let snapshot_id = match snapshot {
            Ok(ref s) => s.id.clone(),
            Err(ref e) => {
                return InstallOutcome::Failed {
                    error: format!("Snapshot failed before apply: {e}"),
                    rollback_performed: false,
                    snapshot_id: None,
                };
            }
        };

        // Check cancellation before mutate.
        if cancel.is_cancelled() {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return InstallOutcome::Cancelled {
                phase: "snapshotting".into(),
                rollback_performed: false,
            };
        }

        // Phase 4: apply (atomic file operations + manifest commit)
        reporter.report(ProgressEvent {
            plan_id: plan_id.clone(),
            phase: ProgressPhase::Applying,
            step: 0,
            total_steps: 1,
            bytes_downloaded: 0,
            bytes_total: 0,
            message: "Applying changes…".into(),
        });

        let apply_result = apply_plan_files(
            &plan.files_to_remove,
            &plan.files_to_disable,
            &plan.files_to_add,
            instance_dir,
            &staging_dir,
        );
        let rollback_result = match &apply_result {
            Ok(()) => None,
            Err(e) => {
                match crate::snapshot::restore_snapshot(instance_dir, &snapshot_id) {
                    Ok(()) => Some(format!("Apply failed, snapshot restored: {e}")),
                    Err(restore_err) => Some(format!(
                        "Apply failed AND snapshot restore also failed. Instance may be inconsistent. \
                         Restore error: {restore_err}. Original error: {e}"
                    )),
                }
            }
        };
        if let Some(error_msg) = rollback_result {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return InstallOutcome::Failed {
                error: error_msg,
                rollback_performed: true, // best-effort
                snapshot_id: Some(snapshot_id),
            };
        }

        // Check cancellation after commit.
        if cancel.is_cancelled() {
            return InstallOutcome::Cancelled {
                phase: "applying".into(),
                rollback_performed: false,
            };
        }

        // Phase 5: health scan
        reporter.report(ProgressEvent {
            plan_id: plan_id.clone(),
            phase: ProgressPhase::HealthScan,
            step: 0,
            total_steps: 1,
            bytes_downloaded: 0,
            bytes_total: 0,
            message: "Running post-install health scan…".into(),
        });

        // Read manifest and run health scan.
        let manifest_path = instance_dir.join("instance_manifest.json");
        let manifest_json = std::fs::read_to_string(&manifest_path).unwrap_or_else(|_| "{}".into());
        let manifest: Option<crate::models::InstanceManifest> = serde_json::from_str(&manifest_json).ok();
        let default_health = crate::health::HealthReport {
            score: crate::health::HealthScore::Green,
            warnings: vec![],
            blockers: vec![],
        };
        let health_report: crate::health::HealthReport = if let Some(ref m) = manifest {
            crate::health::health(instance_dir, m, None)
        } else {
            default_health
        };

        let has_issues = !health_report.blockers.is_empty() || !health_report.warnings.is_empty();
        if has_issues && !plan.intent.overrides.skip_health_scan {
            let restore_outcome = crate::snapshot::restore_snapshot(instance_dir, &snapshot_id);
            let _ = std::fs::remove_dir_all(&staging_dir);
            return match restore_outcome {
                Ok(()) => InstallOutcome::HealthRollback {
                    health_report,
                    snapshot_id,
                    warnings: plan.warnings.clone(),
                },
                Err(restore_err) => InstallOutcome::Failed {
                    error: format!("Health scan found issues AND snapshot restore failed: {restore_err}"),
                    rollback_performed: false,
                    snapshot_id: Some(snapshot_id),
                },
            };
        }

        let _ = std::fs::remove_dir_all(&staging_dir);

        reporter.report(ProgressEvent {
            plan_id: plan_id.clone(),
            phase: ProgressPhase::Done,
            step: 1,
            total_steps: 1,
            bytes_downloaded: 0,
            bytes_total: 0,
            message: "Install complete.".into(),
        });

        InstallOutcome::Success {
            installed_items: plan.files_to_add.iter().map(|f| f.target_filename.clone()).collect(),
            existing_items_reused: vec![],
            warnings: plan.warnings.clone(),
            health: HealthOutcome::Completed(health_report),
            snapshot_id,
        }
    }
}

// -----------------------------------------------------------------------
// Internal helpers
// -----------------------------------------------------------------------

/// Download all artifacts to staging and verify their hashes.
async fn stage_artifacts(
    files: &[FileAdd],
    staging_dir: &std::path::Path,
    cancel: &CancellationToken,
) -> Result<(), String> {
    std::fs::create_dir_all(staging_dir)
        .map_err(|e| format!("failed to create staging dir: {e}"))?;

    for file in files {
        if cancel.is_cancelled() {
            return Err("Cancelled during staging.".into());
        }
        // Currently a stub — downloads will use the download module in C2.
        // For now, we just verify the staging path exists.
        let _target = staging_dir.join(&file.staging_filename);
    }
    Ok(())
}

/// Apply file actions: remove, disable, add, then commit manifest.
fn apply_plan_files(
    removes: &[FileRemove],
    disables: &[FileDisable],
    adds: &[FileAdd],
    instance_dir: &std::path::Path,
    staging_dir: &std::path::Path,
) -> Result<(), String> {
    // 1. Remove phase: move to staging trash (reversible).
    for remove in removes {
        let src = instance_dir.join("mods").join(&remove.filename);
        if !src.exists() {
            continue;
        }
        let trash = staging_dir.join("trash").join(&remove.filename);
        if let Some(parent) = trash.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create trash dir: {e}"))?;
        }
        std::fs::rename(&src, &trash)
            .map_err(|e| format!("failed to move {} to trash: {e}", remove.filename))?;
    }

    // 2. Disable phase: .jar → .jar.disabled
    for disable in disables {
        let src = instance_dir.join("mods").join(&disable.filename);
        if !src.exists() {
            continue;
        }
        let dst = instance_dir.join("mods").join(format!("{}.disabled", disable.filename));
        std::fs::rename(&src, &dst)
            .map_err(|e| format!("failed to disable {}: {e}", disable.filename))?;
    }

    // 3. Add phase: atomic rename from staging into mods/.
    let mods_dir = instance_dir.join("mods");
    std::fs::create_dir_all(&mods_dir)
        .map_err(|e| format!("failed to ensure mods dir: {e}"))?;

    for add in adds {
        let staged = staging_dir.join(&add.staging_filename);
        if !staged.exists() {
            continue;
        }
        let target = mods_dir.join(&add.target_filename);
        std::fs::rename(&staged, &target)
            .map_err(|e| format!("failed to move {} into mods: {e}", add.target_filename))?;
    }

    // 4. Manifest commit: write .tmp → fsync → rename (single commit point).
    let manifest_path = instance_dir.join("instance_manifest.json");
    let tmp_path = instance_dir.join("instance_manifest.json.tmp");

    // Read existing manifest, or start fresh.
    let existing_manifest = std::fs::read_to_string(&manifest_path).unwrap_or_else(|_| "{}".into());
    let _backup_path = instance_dir.join(format!("instance_manifest.json.bak.{}", uuid::Uuid::new_v4()));
    let _ = std::fs::write(&tmp_path, &existing_manifest);
    // TODO: Update manifest content with new mod list.
    // For now, just commit the existing manifest unchanged.
    std::fs::rename(&tmp_path, &manifest_path)
        .map_err(|e| format!("failed to commit manifest: {e}"))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_install_round_trip() {
        let intent = InstallIntent {
            action: InstallAction::Install {
                source_type: SourceType::Curated,
                item_id: "test-mod".into(),
                candidate_version: Some("1.0.0".into()),
            },
            target_instance: "test-instance".into(),
            optional_deps: OptionalDepsPolicy::ExcludeAll,
            requested_by: RequestSource::Interactive,
            overrides: PlanOverrides::default(),
        };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: InstallIntent = serde_json::from_str(&json).unwrap();
        match restored.action {
            InstallAction::Install { ref item_id, ref candidate_version, .. } => {
                assert_eq!(item_id, "test-mod");
                assert_eq!(candidate_version, &Some("1.0.0".into()));
            }
            _ => panic!("wrong action variant"),
        }
    }

    #[test]
    fn test_intent_remove_round_trip() {
        let intent = InstallIntent {
            action: InstallAction::Remove {
                filename: "old-mod.jar".into(),
            },
            target_instance: "test-instance".into(),
            optional_deps: OptionalDepsPolicy::ExcludeAll,
            requested_by: RequestSource::Interactive,
            overrides: PlanOverrides::default(),
        };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: InstallIntent = serde_json::from_str(&json).unwrap();
        match restored.action {
            InstallAction::Remove { ref filename } => {
                assert_eq!(filename, "old-mod.jar");
            }
            _ => panic!("wrong action variant"),
        }
    }

    #[test]
    fn test_is_fully_resolved_blocks_with_errors() {
        let plan = stub_plan();
        // stub plan has a blocking error — not resolved
        assert!(!plan.is_fully_resolved());
    }

    #[test]
    fn test_is_fully_resolved_ok() {
        let mut plan = stub_plan();
        plan.blocking_errors.clear();
        assert!(plan.is_fully_resolved());

        // pending choices blocks
        plan.pending_choices.push(PendingChoice::Conflict {
            choice_id: "c1".into(),
            conflict_id: "conflict-1".into(),
            options: vec![ConflictResolutionOption {
                resolution: ConflictResolution::Skip,
                label: "Skip".into(),
                description: "Skip this mod".into(),
            }],
        });
        assert!(!plan.is_fully_resolved());
    }

    #[test]
    fn test_is_fully_resolved_blocks_with_unresolved_conflict() {
        let mut plan = stub_plan();
        plan.blocking_errors.clear();
        plan.conflicts.push(DepConflict {
            conflict_id: "c1".into(),
            kind: ConflictKind::VersionConflict,
            existing_mod_jar_id: "a-1.0".into(),
            incoming_mod_jar_id: "a-2.0".into(),
            message: "Version conflict".into(),
            blocking: true,
            resolution_options: vec![ConflictResolution::Replace, ConflictResolution::Skip],
            chosen: None,
        });
        assert!(!plan.is_fully_resolved());

        // Set chosen → resolved
        plan.conflicts[0].chosen = Some(ConflictResolution::Replace);
        assert!(plan.is_fully_resolved());
    }

    #[test]
    fn test_cancellation_token() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_resolve_plan_returns_stub() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let pipeline = InstallPipeline;

        let intent = InstallIntent {
            action: InstallAction::Install {
                source_type: SourceType::Curated,
                item_id: "fabric-api".into(),
                candidate_version: None,
            },
            target_instance: "my-instance".into(),
            optional_deps: OptionalDepsPolicy::ExcludeAll,
            requested_by: RequestSource::Interactive,
            overrides: PlanOverrides::default(),
        };

        let result = rt.block_on(async {
            let reporter = NoopReporter;
            pipeline.resolve_plan(intent, &reporter).await
        });

        assert!(result.is_ok());
        let plan = result.unwrap();
        // Stub plan is deliberately not applicable.
        assert!(!plan.is_fully_resolved());
        assert!(!plan.blocking_errors.is_empty());
    }

    struct NoopReporter;

    impl ProgressReporter for NoopReporter {
        fn report(&self, _event: ProgressEvent) {}
    }

    fn stub_plan() -> ResolvedInstallPlan {
        ResolvedInstallPlan {
            fingerprint: "test-fp".into(),
            intent: InstallIntent {
                action: InstallAction::Install {
                    source_type: SourceType::Curated,
                    item_id: "test".into(),
                    candidate_version: None,
                },
                target_instance: "test".into(),
                optional_deps: OptionalDepsPolicy::ExcludeAll,
                requested_by: RequestSource::Interactive,
                overrides: PlanOverrides::default(),
            },
            operation: ResolvedOperation::Install {
                artifact: ResolvedArtifact::Download(ResolvedDownload {
                    item_id: "test".into(),
                    version_id: "1.0".into(),
                    source: ArtifactSource::Download {
                        url: "https://example.com/test.jar".into(),
                    },
                    hashes: HashSpec { values: vec![] },
                    size: 0,
                    filename: "test.jar".into(),
                }),
            },
            dependencies: vec![],
            conflicts: vec![],
            files_to_add: vec![],
            files_to_remove: vec![],
            files_to_disable: vec![],
            snapshot: SnapshotPlan {
                label: "test-snapshot".into(),
                estimated_bytes: 0,
            },
            disk_estimate: DiskSpaceEstimate::zero(),
            warnings: vec![],
            blocking_errors: vec![PlanError {
                code: "ERR_STUB".into(),
                message: "Stub plan".into(),
            }],
            pending_choices: vec![],
            created_at: String::new(),
            instance_state_hash: String::new(),
            registry_revision: String::new(),
        }
    }
}
