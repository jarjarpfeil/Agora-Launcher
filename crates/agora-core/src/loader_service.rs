//! Core-owned loader installation and repair.

use crate::ctx::Ctx;
use crate::error::{LauncherError, LauncherResult};
use crate::event_sink::{CoreEvent, EventStatus, ProgressEvent, ProgressPhase};
use crate::installed_profile::{self, InstallReceiptSummary, LoaderTuple};
use crate::java::JavaInstallation;
use crate::loader_manifests::{self, LoaderEntry};
use crate::network::{NetworkCategory, NetworkPolicy};
use crate::operation_manager::OpHandle;
use std::path::Path;
use std::time::Duration;

const BACKUP_SUFFIX: &str = ".bak-reinstall";
const INSTALLER_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_INSTALLER_OUTPUT_BYTES: u64 = 1024 * 1024;

/// Core-owned loader installation service.
#[derive(Clone)]
pub struct LoaderService {
    ctx: Ctx,
}

impl LoaderService {
    pub fn new(ctx: Ctx) -> Self {
        Self { ctx }
    }

    pub fn list_versions(
        &self,
        loader: &str,
        minecraft_version: &str,
    ) -> Vec<LoaderVersionSummary> {
        loader_manifests::list_versions(loader, minecraft_version)
            .into_iter()
            .map(|entry| LoaderVersionSummary {
                loader: loader.to_owned(),
                mc_version: entry.mc_version,
                loader_version: entry.loader_version,
                file_type: entry.file_type,
            })
            .collect()
    }

    pub async fn repair(&self, instance_id: &str) -> LauncherResult<InstallReceiptSummary> {
        let conn = crate::db::local_state_connection(&self.ctx.paths.local_state_db())
            .map_err(|_| LauncherError::LocalStateFailed)?;
        let row = crate::db::get_instance(&conn, instance_id)
            .map_err(|_| LauncherError::LocalStateFailed)?
            .ok_or_else(|| LauncherError::Generic {
                code: "ERR_INSTANCE_NOT_FOUND".into(),
                message: format!("Instance '{instance_id}' not found"),
            })?;
        self.ensure_installed(
            &row.loader,
            &row.minecraft_version,
            &row.loader_version,
            true,
        )
        .await
    }

    pub async fn ensure_installed(
        &self,
        loader: &str,
        minecraft_version: &str,
        loader_version: &str,
        force_reinstall: bool,
    ) -> LauncherResult<InstallReceiptSummary> {
        let entry = loader_manifests::find_entry(loader, minecraft_version, loader_version)
            .ok_or(LauncherError::UnsupportedLoader)?;
        self.ensure_entry(loader.to_owned(), entry, force_reinstall)
            .await
    }

    async fn ensure_entry(
        &self,
        loader: String,
        entry: LoaderEntry,
        force_reinstall: bool,
    ) -> LauncherResult<InstallReceiptSummary> {
        let tuple = LoaderTuple {
            loader: loader.clone(),
            minecraft_version: entry.mc_version.clone(),
            loader_version: entry.loader_version.clone(),
        };
        let label = format!("Install {} {}", tuple.loader, tuple.loader_version);
        let op = self
            .ctx
            .operation_manager
            .register_for_instance(&label, &tuple.loader);

        let minecraft_root = self.ctx.paths.minecraft_runtime_root();
        let receipts_root = self.ctx.paths.loader_receipts();
        let cache_dir = self
            .ctx
            .paths
            .loader_cache()
            .join(&tuple.loader)
            .join(&tuple.minecraft_version)
            .join(&tuple.loader_version);
        let operation_id = op.id().clone();
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            op.fail(e.to_string());
            return Err(LauncherError::InstanceCreateFailed);
        }
        self.ctx.progress_sink.report(ProgressEvent::new(
            operation_id.clone(),
            ProgressPhase::Installing,
            format!("Ensuring {} {}", tuple.loader, tuple.loader_version),
        ));
        let _lock = self.ctx.lock_manager.acquire(
            crate::lock_manager::LockResource::LoaderInstall,
            "loader-install",
        )?;

        let expected_sha = loader_manifests::strip_sha_prefix(&entry.sha256);
        if !force_reinstall {
            if let Ok(adopted) = installed_profile::adopt_installed_profile(
                &minecraft_root,
                &receipts_root,
                &tuple,
                expected_sha,
            ) {
                op.complete();
                return Ok(summary_from_adoption(tuple, adopted, true));
            }
        }

        let conn = match crate::db::local_state_connection(&self.ctx.paths.local_state_db()) {
            Ok(c) => c,
            Err(_) => {
                op.fail("local state failed");
                return Err(LauncherError::LocalStateFailed);
            }
        };
        let policy = NetworkPolicy::from_db(&conn);
        if let Err(e) = policy.check(NetworkCategory::LoaderMetadataAndContent) {
            op.fail(e.to_string());
            return Err(e);
        }
        let file_path = cache_dir.join(&entry.file_name);
        let data = match verified_cache_hit(&file_path, &loader, &entry) {
            Some(bytes) => bytes,
            None => {
                self.ctx.progress_sink.report(ProgressEvent::new(
                    operation_id.clone(),
                    ProgressPhase::Downloading,
                    format!("Downloading {}", entry.file_name),
                ));
                match crate::download::download_verified_with_clients(
                    &self.ctx.http_clients,
                    &loader,
                    &entry.file_name,
                    &entry.file_type,
                    &entry.source_url,
                    &entry.sha256,
                )
                .await
                {
                    Ok(bytes) => {
                        if let Err(e) = atomic_write(&file_path, &bytes) {
                            op.fail(e.to_string());
                            return Err(e);
                        }
                        bytes
                    }
                    Err(e) => {
                        op.fail(e.to_string());
                        return Err(e);
                    }
                }
            }
        };

        let result = match entry.file_type.as_str() {
            "profile_json" => {
                install_profile_json(&minecraft_root, &receipts_root, &tuple, &entry, &data)
            }
            "installer_jar" => {
                self.install_forge_profile(
                    &minecraft_root,
                    &receipts_root,
                    &tuple,
                    &entry,
                    &data,
                    &policy,
                    force_reinstall,
                    &op,
                )
                .await
            }
            _ => {
                op.fail("unsupported loader");
                return Err(LauncherError::UnsupportedLoader);
            }
        };
        match result {
            Ok(summary) => {
                op.complete();
                self.ctx.event_sink.emit(CoreEvent::ModOperation {
                    operation_id,
                    instance_id: tuple.minecraft_version.clone(),
                    action: crate::event_sink::ModAction::Install,
                    status: EventStatus::Completed,
                    message: format!("Installed {} {}", tuple.loader, tuple.loader_version),
                });
                Ok(summary)
            }
            Err(e) => {
                op.fail(e.to_string());
                Err(e)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn install_forge_profile(
        &self,
        minecraft_root: &Path,
        receipts_root: &Path,
        tuple: &LoaderTuple,
        entry: &LoaderEntry,
        data: &[u8],
        policy: &NetworkPolicy,
        force_reinstall: bool,
        op: &OpHandle,
    ) -> LauncherResult<InstallReceiptSummary> {
        let backup = if force_reinstall {
            Some(backup_profile(minecraft_root, receipts_root, tuple)?)
        } else {
            None
        };
        let result = async {
            policy.check(NetworkCategory::LoaderMetadataAndContent)?;
            let java_path = self
                .resolve_installer_java(minecraft_root, &tuple.minecraft_version, policy, op)
                .await?;
            let staged = self
                .ctx
                .paths
                .staging_dir(&format!("loader-installer-{}", uuid::Uuid::new_v4()))?;
            std::fs::create_dir_all(&staged).map_err(|_| LauncherError::InstanceCreateFailed)?;
            let installer = staged.join(&entry.file_name);
            atomic_write(&installer, data)?;
            let status =
                run_installer_process(&java_path.path, &installer, &tuple.loader, minecraft_root)
                    .await?;
            if status != 0 {
                return Err(LauncherError::Generic {
                    code: "ERR_INSTALLER_FAILED".into(),
                    message: format!("{} installer exited with status {status}", tuple.loader),
                });
            }
            let receipt = installed_profile::create_receipt_for_installed_profile(
                minecraft_root,
                receipts_root,
                tuple,
                loader_manifests::strip_sha_prefix(&entry.sha256),
                &entry.source_url,
                status,
            )
            .map_err(|issue| LauncherError::Generic {
                code: "ERR_PROFILE_CORRUPT".into(),
                message: issue.reasons.join("; "),
            })?;
            let _ = std::fs::remove_dir_all(&staged);
            Ok(InstallReceiptSummary {
                tuple: tuple.clone(),
                profile_id: receipt.profile_id,
                cache_hit: false,
                profile_stable_hash: receipt.profile_stable_hash,
                receipt_schema_version: receipt.schema_version,
                installer_exit_status: receipt.installer_exit_status,
            })
        }
        .await;
        match result {
            Ok(summary) => {
                if let Some(backup) = backup {
                    delete_backup(minecraft_root, &backup.profile_id);
                }
                Ok(summary)
            }
            Err(error) => {
                if let Some(backup) = backup {
                    restore_backup(minecraft_root, receipts_root, &backup);
                }
                Err(error)
            }
        }
    }

    async fn resolve_installer_java(
        &self,
        minecraft_root: &Path,
        minecraft_version: &str,
        policy: &NetworkPolicy,
        _op: &OpHandle,
    ) -> LauncherResult<JavaInstallation> {
        let version = crate::minecraft_metadata::ensure_base_version_metadata(
            minecraft_root,
            minecraft_version,
            policy,
        )
        .await?;
        let required = crate::java::java_requirement_from_version(&version).major;
        let runtimes_root = self.ctx.paths.java_runtimes_root();
        let candidates = tokio::task::spawn_blocking(move || {
            crate::java::detect_java_candidates(Some(&runtimes_root), None)
        })
        .await
        .map_err(|error| LauncherError::Generic {
            code: "ERR_JAVA_DISCOVERY".into(),
            message: error.to_string(),
        })?;
        if let Some(candidate) = candidates
            .into_iter()
            .find(|candidate| candidate.version == required)
        {
            return Ok(candidate);
        }
        let runtime_root = self.ctx.paths.java_runtimes_root();
        let catalog = self.ctx.runtime_catalog.snapshot();
        let policy = policy.clone();
        let lock_manager = self.ctx.lock_manager().clone();
        tokio::task::spawn_blocking(move || {
            crate::runtime_manager::ensure_runtime(
                &runtime_root,
                required,
                &catalog,
                &policy,
                None,
                Some(&lock_manager),
            )
        })
        .await
        .map_err(|error| LauncherError::Generic {
            code: "ERR_JAVA_PROVISION".into(),
            message: error.to_string(),
        })?
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LoaderVersionSummary {
    pub loader: String,
    pub mc_version: String,
    pub loader_version: String,
    pub file_type: String,
}

fn summary_from_adoption(
    tuple: LoaderTuple,
    adopted: installed_profile::AdoptedProfile,
    cache_hit: bool,
) -> InstallReceiptSummary {
    InstallReceiptSummary {
        tuple,
        profile_id: adopted.profile_id,
        cache_hit,
        profile_stable_hash: adopted.profile_stable_hash,
        receipt_schema_version: adopted
            .receipt
            .as_ref()
            .map(|receipt| receipt.schema_version)
            .unwrap_or_default(),
        installer_exit_status: 0,
    }
}

fn verified_cache_hit(path: &Path, loader: &str, entry: &LoaderEntry) -> Option<Vec<u8>> {
    let data = std::fs::read(path).ok()?;
    let actual =
        crate::download::compute_loader_hash(loader, &entry.file_name, &entry.file_type, &data);
    (actual == loader_manifests::strip_sha_prefix(&entry.sha256)).then_some(data)
}

fn install_profile_json(
    minecraft_root: &Path,
    receipts_root: &Path,
    tuple: &LoaderTuple,
    entry: &LoaderEntry,
    data: &[u8],
) -> LauncherResult<InstallReceiptSummary> {
    let version_id = entry.file_name.trim_end_matches(".json");
    let target = minecraft_root
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));
    atomic_write(&target, data)?;
    installed_profile::create_receipt_for_profile_json(
        minecraft_root,
        receipts_root,
        tuple,
        loader_manifests::strip_sha_prefix(&entry.sha256),
        &entry.source_url,
        std::collections::BTreeMap::new(),
    )
    .map_err(|issue| LauncherError::Generic {
        code: "ERR_PROFILE_CORRUPT".into(),
        message: issue.reasons.join("; "),
    })?;
    let profile: serde_json::Value =
        serde_json::from_slice(data).map_err(|error| LauncherError::Generic {
            code: "ERR_PROFILE_CORRUPT".into(),
            message: error.to_string(),
        })?;
    Ok(InstallReceiptSummary {
        tuple: tuple.clone(),
        profile_id: installed_profile::derive_profile_id(tuple),
        cache_hit: false,
        profile_stable_hash: installed_profile::stable_profile_hash(&profile),
        receipt_schema_version: installed_profile::RECEIPT_SCHEMA_VERSION,
        installer_exit_status: 0,
    })
}

fn atomic_write(path: &Path, bytes: &[u8]) -> LauncherResult<()> {
    let parent = path.parent().ok_or(LauncherError::InstanceCreateFailed)?;
    std::fs::create_dir_all(parent).map_err(|_| LauncherError::InstanceCreateFailed)?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temp, bytes).map_err(|_| LauncherError::InstanceCreateFailed)?;
    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        LauncherError::Generic {
            code: "ERR_ATOMIC_WRITE".into(),
            message: error.to_string(),
        }
    })
}

async fn run_installer_process(
    java_path: &Path,
    installer_path: &Path,
    loader: &str,
    minecraft_root: &Path,
) -> LauncherResult<i32> {
    let mut child = tokio::process::Command::new(java_path)
        .args([
            std::ffi::OsString::from("-jar"),
            installer_path.as_os_str().to_owned(),
            std::ffi::OsString::from("--installClient"),
            minecraft_root.as_os_str().to_owned(),
        ])
        .current_dir(installer_path.parent().unwrap_or(minecraft_root))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| LauncherError::Generic {
            code: "ERR_INSTALLER_FAILED".into(),
            message: format!("Failed to spawn {loader} installer: {error}"),
        })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_task = tokio::spawn(read_pipe_bounded(stdout));
    let stderr_task = tokio::spawn(read_pipe_bounded(stderr));
    let status = tokio::time::timeout(INSTALLER_TIMEOUT, child.wait()).await;
    let _ = stdout_task.await;
    let _ = stderr_task.await;
    match status {
        Ok(Ok(status)) => Ok(status.code().unwrap_or(1)),
        Ok(Err(error)) => Err(LauncherError::Generic {
            code: "ERR_INSTALLER_FAILED".into(),
            message: error.to_string(),
        }),
        Err(_) => Err(LauncherError::Generic {
            code: "ERR_INSTALLER_TIMEOUT".into(),
            message: format!("{loader} installer timed out"),
        }),
    }
}

async fn read_pipe_bounded<R>(pipe: Option<R>) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    use tokio::io::AsyncReadExt;
    let Some(mut pipe) = pipe else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    while let Ok(count) = pipe.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        let remaining = MAX_INSTALLER_OUTPUT_BYTES.saturating_sub(output.len() as u64);
        output.extend_from_slice(&buffer[..(count as u64).min(remaining) as usize]);
    }
    output
}

struct BackupState {
    tuple: LoaderTuple,
    profile_id: String,
    old_receipt_json: Option<String>,
}

fn backup_profile(
    minecraft_root: &Path,
    receipts_root: &Path,
    tuple: &LoaderTuple,
) -> LauncherResult<BackupState> {
    let profile_id = installed_profile::derive_profile_id(tuple);
    let version_dir = minecraft_root.join("versions").join(&profile_id);
    let backup_dir = minecraft_root
        .join("versions")
        .join(format!("{profile_id}{BACKUP_SUFFIX}"));
    if version_dir.exists() {
        if backup_dir.exists() {
            std::fs::remove_dir_all(&backup_dir)
                .map_err(|_| LauncherError::InstanceCreateFailed)?;
        }
        std::fs::rename(&version_dir, &backup_dir)
            .map_err(|_| LauncherError::InstanceCreateFailed)?;
    }
    let receipt_path = installed_profile::receipt_path(receipts_root, tuple);
    let old_receipt_json = if receipt_path.exists() {
        let content = std::fs::read_to_string(&receipt_path).ok();
        let _ = installed_profile::remove_receipt(receipts_root, tuple);
        content
    } else {
        None
    };
    Ok(BackupState {
        tuple: tuple.clone(),
        profile_id,
        old_receipt_json,
    })
}

fn restore_backup(minecraft_root: &Path, receipts_root: &Path, state: &BackupState) {
    let version_dir = minecraft_root.join("versions").join(&state.profile_id);
    let backup_dir = minecraft_root
        .join("versions")
        .join(format!("{}{BACKUP_SUFFIX}", state.profile_id));
    if backup_dir.exists() {
        let _ = std::fs::remove_dir_all(&version_dir);
        let _ = std::fs::rename(&backup_dir, &version_dir);
    }
    if let Some(json) = &state.old_receipt_json {
        let path = installed_profile::receipt_path(receipts_root, &state.tuple);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, json);
    }
}

fn delete_backup(minecraft_root: &Path, profile_id: &str) {
    let path = minecraft_root
        .join("versions")
        .join(format!("{profile_id}{BACKUP_SUFFIX}"));
    let _ = std::fs::remove_dir_all(path);
}
