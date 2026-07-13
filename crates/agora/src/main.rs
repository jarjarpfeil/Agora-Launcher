use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// A silent progress reporter for the CLI — no progress events are emitted.
struct SilentReporter;

impl agora_core::install_pipeline::ProgressReporter for SilentReporter {
    fn report(&self, _event: agora_core::install_pipeline::ProgressEvent) {}
}

#[derive(Parser)]
#[command(name = "agora", about = "Agora Minecraft Launcher CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true, help = "Path to Agora data directory")]
    data_dir: Option<PathBuf>,

    #[arg(long, global = true, help = "JSON output")]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    ListInstances,
    GetInstance {
        id: String,
    },
    Mods {
        #[command(subcommand)]
        action: ModsCmd,
    },
    Health {
        instance: String,
    },
    Registry {
        #[command(subcommand)]
        action: RegistryCmd,
    },
    Snapshots {
        #[command(subcommand)]
        action: SnapshotsCmd,
    },
    Import {
        path: PathBuf,
        #[arg(long, help = "Symlink saves instead of copying")]
        symlink_saves: bool,
    },
    Launch {
        instance: String,
        #[arg(long, help = "Skip health check confirmation")]
        yes: bool,
    },
    Auth {
        #[command(subcommand)]
        action: AuthCmd,
    },
    Serve {
        #[arg(long, default_value = "39741", help = "Port to listen on")]
        port: u16,
    },
    Sync,
}

#[derive(Subcommand)]
enum ModsCmd {
    List {
        instance: String,
    },
    Install {
        project: String,
        instance: String,
        #[arg(short, long)]
        version: Option<String>,
    },
    Remove {
        project: String,
        instance: String,
    },
}

#[derive(Subcommand)]
enum RegistryCmd {
    Status,
    Sync,
}

#[derive(Subcommand)]
enum SnapshotsCmd {
    List {
        instance: String,
    },
    Create {
        instance: String,
        #[arg(short, long)]
        label: Option<String>,
    },
    Restore {
        instance: String,
        snapshot_id: String,
    },
}

#[derive(Subcommand)]
enum AuthCmd {
    Login,
    Status,
    Logout,
}

fn print_table(columns: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        for col in columns {
            print!("{col}  ");
        }
        println!();
        return;
    }
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }
    for (i, col) in columns.iter().enumerate() {
        print!("{col}");
        if i < widths.len() {
            for _ in 0..widths[i].saturating_sub(col.len()) + 2 {
                print!(" ");
            }
        }
    }
    println!();
    let total: usize =
        widths.iter().map(|w| w + 2).sum::<usize>() + columns.len().saturating_sub(1);
    for _ in 0..total {
        print!("-");
    }
    println!();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            print!("{cell}");
            if i < widths.len() {
                for _ in 0..widths[i].saturating_sub(cell.len()) + 2 {
                    print!(" ");
                }
            }
        }
        println!();
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let data_dir = cli.data_dir.clone().unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agora")
    });
    let client = reqwest::Client::new();

    let result = run_command(cli, &data_dir, &client).await;
    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run_command(cli: Cli, data_dir: &PathBuf, client: &reqwest::Client) -> anyhow::Result<()> {
    let json = cli.json;

    match cli.command {
        Commands::ListInstances => {
            let db_path = data_dir.join("local_state.db");
            if !db_path.exists() {
                println!("No local state database found at {}", db_path.display());
                return Ok(());
            }
            let conn = agora_core::db::local_state_connection(&db_path)?;
            let instances = agora_core::db::list_instances(&conn)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&instances)?);
            } else {
                let rows: Vec<Vec<String>> = instances
                    .iter()
                    .map(|i| {
                        vec![
                            i.instance_id.clone(),
                            i.name.clone(),
                            i.minecraft_version.clone(),
                            i.loader.clone(),
                            i.loader_version.clone(),
                            i.last_launched_at.clone().unwrap_or_default(),
                        ]
                    })
                    .collect();
                print_table(
                    &["ID", "Name", "MC", "Loader", "Version", "Launched"],
                    &rows,
                );
            }
        }
        Commands::GetInstance { id } => {
            let db_path = data_dir.join("local_state.db");
            if !db_path.exists() {
                eprintln!("No local state database found");
                std::process::exit(1);
            }
            let conn = agora_core::db::local_state_connection(&db_path)?;
            match agora_core::db::get_instance(&conn, &id)? {
                Some(instance) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&instance)?);
                    } else {
                        println!("ID:       {}", instance.instance_id);
                        println!("Name:     {}", instance.name);
                        println!("MC:       {}", instance.minecraft_version);
                        println!("Loader:   {} {}", instance.loader, instance.loader_version);
                        println!("Locked:   {}", instance.is_locked);
                        println!("Modpack:  {}", instance.is_modpack);
                        println!(
                            "Launched: {}",
                            instance.last_launched_at.unwrap_or_default()
                        );
                    }
                }
                None => {
                    eprintln!("Instance '{}' not found", id);
                    std::process::exit(1);
                }
            }
        }
        Commands::Mods { action } => match action {
            ModsCmd::List { instance } => {
                let manifest_path = agora_core::paths::instance_manifest_path(data_dir, &instance)?;
                if !manifest_path.exists() {
                    eprintln!("Instance '{}' not found", instance);
                    std::process::exit(1);
                }
                let text = std::fs::read_to_string(&manifest_path)?;
                let manifest: agora_core::models::InstanceManifest = serde_json::from_str(&text)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&manifest.mods)?);
                } else {
                    let rows: Vec<Vec<String>> = manifest
                        .mods
                        .iter()
                        .map(|m| {
                            vec![
                                m.filename.clone(),
                                m.source.clone(),
                                m.version.clone().unwrap_or_default(),
                                m.modrinth_id.clone().unwrap_or_default(),
                            ]
                        })
                        .collect();
                    print_table(&["Filename", "Source", "Version", "Modrinth ID"], &rows);
                }
            }
            ModsCmd::Install {
                project,
                instance,
                version,
            } => {
                let db_path = data_dir.join("local_state.db");
                if !db_path.exists() {
                    eprintln!("No local state database found. Run 'agora sync' first.");
                    std::process::exit(1);
                }
                let conn = agora_core::db::local_state_connection(&db_path)?;
                let instance_row = agora_core::db::get_instance(&conn, &instance)?;

                let instance_dir = agora_core::paths::instance_dir(data_dir, &instance)?;
                if !instance_dir.exists() {
                    eprintln!("Instance '{}' not found", instance);
                    std::process::exit(1);
                }

                // Resolve Modrinth version candidates
                let candidates = agora_core::modrinth::list_raw_modrinth_versions(
                    &conn,
                    instance_row.as_ref(),
                    &project,
                )
                .await?;

                let candidate = if let Some(ver) = version {
                    candidates
                        .into_iter()
                        .find(|v| v.version == ver || v.name == ver)
                        .ok_or_else(|| {
                            anyhow::anyhow!("Version '{}' not found for project {}", ver, project)
                        })?
                } else {
                    candidates
                        .into_iter()
                        .find(|v| v.primary)
                        .unwrap_or_else(|| panic!("no versions available for project {}", project))
                };

                // Build the hash spec from Modrinth's published hashes
                let hashes = match &candidate.sha1 {
                    Some(sha1) if !sha1.is_empty() => agora_core::install_pipeline::HashSpec {
                        values: vec![agora_core::install_pipeline::HashedValue {
                            algorithm: agora_core::install_pipeline::HashAlgorithm::Sha1,
                            value: sha1.clone(),
                        }],
                    },
                    _ => agora_core::install_pipeline::HashSpec { values: vec![] },
                };

                let artifact = agora_core::install_pipeline::ResolvedArtifact::Download(
                    agora_core::install_pipeline::ResolvedDownload {
                        item_id: project.clone(),
                        version_id: candidate.version_id.clone(),
                        source: agora_core::install_pipeline::ArtifactSource::Download {
                            url: candidate.download_url.clone(),
                        },
                        hashes,
                        size: 0,
                        filename: candidate.filename.clone(),
                        metadata: agora_core::install_pipeline::ArtifactMetadata {
                            source_type: agora_core::install_pipeline::SourceType::Modrinth,
                            registry_id: None,
                            modrinth_id: Some(project.clone()),
                            content_type: "mod".into(),
                        },
                    },
                );

                let registry_revision = agora_core::registry_sync::get_status(data_dir, &db_path)
                    .cached_tag
                    .unwrap_or_default();

                let prepared = agora_core::install_pipeline::PreparedPlan {
                    operation: agora_core::install_pipeline::ResolvedOperation::Install {
                        artifact: artifact.clone(),
                    },
                    dependencies: vec![],
                    conflicts: vec![],
                    registry_revision: registry_revision.clone(),
                };

                let intent = agora_core::install_pipeline::InstallIntent {
                    action: agora_core::install_pipeline::InstallAction::Install {
                        source_type: agora_core::install_pipeline::SourceType::Modrinth,
                        item_id: project.clone(),
                        candidate_version: Some(candidate.version.clone()),
                    },
                    target_instance: instance.clone(),
                    optional_deps: agora_core::install_pipeline::OptionalDepsPolicy::ExcludeAll,
                    requested_by: agora_core::install_pipeline::RequestSource::CLI,
                    overrides: agora_core::install_pipeline::PlanOverrides {
                        allow_replace: true,
                        skip_health_scan: true,
                        ..Default::default()
                    },
                };

                let pipeline = agora_core::install_pipeline::InstallPipeline;
                let reporter = SilentReporter;
                let cancel = agora_core::install_pipeline::CancellationToken::new();

                let plan = pipeline
                    .resolve_plan(intent, &instance_dir, prepared, &reporter)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to resolve install plan: {e}"))?;

                // Preview / error gate — fail closed on unresolved plans
                if !plan.is_fully_resolved() {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "status": "blocked",
                                "blockingErrors": plan.blocking_errors,
                                "pendingChoices": plan.pending_choices,
                                "conflicts": plan.conflicts,
                            }))?
                        );
                    } else {
                        for err in &plan.blocking_errors {
                            eprintln!("[BLOCK] {}: {}", err.code, err.message);
                        }
                        for conflict in &plan.conflicts {
                            if conflict.chosen.is_none() {
                                eprintln!("[CONFLICT] {}", conflict.message);
                            }
                        }
                        for choice in &plan.pending_choices {
                            let label = match choice {
                                agora_core::install_pipeline::PendingChoice::OptionalDependencies { .. } => "Optional dependencies",
                                agora_core::install_pipeline::PendingChoice::Conflict { .. } => "Conflict resolution",
                            };
                            eprintln!("[CHOICE] {} requires user input", label);
                        }
                    }
                    std::process::exit(1);
                }

                // Execute the plan with snapshot, verifiable staging, and health gate
                let outcome = pipeline
                    .execute_plan(&plan, &instance_dir, &registry_revision, &reporter, &cancel)
                    .await;

                match outcome {
                    agora_core::install_pipeline::InstallOutcome::Success {
                        warnings,
                        snapshot_id,
                        ..
                    } => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "status": "success",
                                    "filename": plan.files_to_add.first().map(|f| &f.target_filename),
                                    "version": candidate.version,
                                    "snapshotId": snapshot_id,
                                    "warnings": warnings,
                                }))?
                            );
                        } else {
                            let filename = plan
                                .files_to_add
                                .first()
                                .map(|f| &f.target_filename)
                                .unwrap_or(&candidate.filename);
                            println!("Installed {} ({})", filename, candidate.version);
                        }
                    }
                    agora_core::install_pipeline::InstallOutcome::HealthRollback {
                        health_report,
                        ..
                    } => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "status": "health_rollback",
                                    "blockers": health_report.blockers,
                                }))?
                            );
                        } else {
                            eprintln!(
                                "Install completed but post-install health check found blockers; rolled back."
                            );
                            for b in &health_report.blockers {
                                eprintln!("  [BLOCK] {}", b.message);
                            }
                        }
                        std::process::exit(1);
                    }
                    agora_core::install_pipeline::InstallOutcome::Cancelled { phase, .. } => {
                        eprintln!("Install was cancelled during {}.", phase);
                        std::process::exit(1);
                    }
                    agora_core::install_pipeline::InstallOutcome::Failed {
                        error,
                        rollback_performed,
                        ..
                    } => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "status": "failed",
                                    "error": error,
                                    "rollbackPerformed": rollback_performed,
                                }))?
                            );
                        } else {
                            eprintln!("Install failed: {}", error);
                        }
                        std::process::exit(1);
                    }
                }
            }
            ModsCmd::Remove { project, instance } => {
                let instance_dir = agora_core::paths::instance_dir(data_dir, &instance)?;
                let manifest_path = agora_core::paths::instance_manifest_path(data_dir, &instance)?;
                if !manifest_path.exists() {
                    eprintln!("Instance '{}' not found", instance);
                    std::process::exit(1);
                }
                let text = std::fs::read_to_string(&manifest_path)?;
                let manifest: agora_core::models::InstanceManifest = serde_json::from_str(&text)?;
                let idx = manifest
                    .mods
                    .iter()
                    .position(|m| {
                        m.filename == project
                            || m.modrinth_id.as_deref() == Some(&project)
                            || m.registry_id.as_deref() == Some(&project)
                    })
                    .ok_or_else(|| anyhow::anyhow!("Mod '{}' not found in instance", project))?;
                let removed = manifest.mods[idx].clone();
                let target_filename = removed.filename.clone();

                let db_path = data_dir.join("local_state.db");
                let registry_revision = agora_core::registry_sync::get_status(data_dir, &db_path)
                    .cached_tag
                    .unwrap_or_default();

                let prepared = agora_core::install_pipeline::PreparedPlan {
                    operation: agora_core::install_pipeline::ResolvedOperation::Remove {
                        target_filename: target_filename.clone(),
                        reverse_dependents: vec![],
                        content_type: None,
                    },
                    dependencies: vec![],
                    conflicts: vec![],
                    registry_revision: registry_revision.clone(),
                };

                let intent = agora_core::install_pipeline::InstallIntent {
                    action: agora_core::install_pipeline::InstallAction::Remove {
                        filename: target_filename.clone(),
                    },
                    target_instance: instance.clone(),
                    optional_deps: agora_core::install_pipeline::OptionalDepsPolicy::ExcludeAll,
                    requested_by: agora_core::install_pipeline::RequestSource::CLI,
                    overrides: agora_core::install_pipeline::PlanOverrides {
                        allow_replace: true,
                        skip_health_scan: true,
                        ..Default::default()
                    },
                };

                let pipeline = agora_core::install_pipeline::InstallPipeline;
                let reporter = SilentReporter;
                let cancel = agora_core::install_pipeline::CancellationToken::new();

                let plan = pipeline
                    .resolve_plan(intent, &instance_dir, prepared, &reporter)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to resolve remove plan: {e}"))?;

                // Preview / error gate — fail closed on unresolved plans
                if !plan.is_fully_resolved() {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "status": "blocked",
                                "blockingErrors": plan.blocking_errors,
                            }))?
                        );
                    } else {
                        for err in &plan.blocking_errors {
                            eprintln!("[BLOCK] {}: {}", err.code, err.message);
                        }
                    }
                    std::process::exit(1);
                }

                // Execute the remove plan with snapshot, file removal, and health gate
                let outcome = pipeline
                    .execute_plan(&plan, &instance_dir, &registry_revision, &reporter, &cancel)
                    .await;

                match outcome {
                    agora_core::install_pipeline::InstallOutcome::Success { .. } => {
                        if json {
                            println!("{}", serde_json::to_string_pretty(&removed)?);
                        } else {
                            println!("Removed {}", target_filename);
                        }
                    }
                    agora_core::install_pipeline::InstallOutcome::Failed { error, .. } => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "status": "failed",
                                    "error": error,
                                }))?
                            );
                        } else {
                            eprintln!("Remove failed: {}", error);
                        }
                        std::process::exit(1);
                    }
                    other => {
                        let err_msg = format!("Remove encountered unexpected state: {:?}", other);
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "status": "unexpected",
                                    "error": err_msg,
                                }))?
                            );
                        } else {
                            eprintln!("{}", err_msg);
                        }
                        std::process::exit(1);
                    }
                }
            }
        },
        Commands::Health { instance } => {
            let instance_dir = agora_core::paths::instance_dir(data_dir, &instance)?;
            if !instance_dir.exists() {
                eprintln!("Instance '{}' not found", instance);
                std::process::exit(1);
            }
            let manifest_path = agora_core::paths::instance_manifest_path(data_dir, &instance)?;
            if !manifest_path.exists() {
                eprintln!("Instance manifest not found for '{}'", instance);
                std::process::exit(1);
            }
            let text = std::fs::read_to_string(&manifest_path)?;
            let manifest: agora_core::models::InstanceManifest = serde_json::from_str(&text)?;
            let reg_path = data_dir.join("registry.db");
            let reg_opt = if reg_path.exists() {
                Some(reg_path)
            } else {
                None
            };
            let report = agora_core::health::health(&instance_dir, &manifest, reg_opt.as_deref());
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Health score: {:?}", report.score);
                for w in &report.warnings {
                    println!("  [WARN] {}", w.message);
                }
                for b in &report.blockers {
                    println!("  [BLOCK] {}", b.message);
                }
            }
            std::process::exit(match report.score {
                agora_core::health::HealthScore::Green => 0,
                agora_core::health::HealthScore::Yellow => 1,
                agora_core::health::HealthScore::Red => 2,
            });
        }
        Commands::Registry { action } => match action {
            RegistryCmd::Status => {
                let local_state = data_dir.join("local_state.db");
                let status = agora_core::registry_sync::get_status(data_dir, &local_state);
                if json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    println!("Cached DB:     {}", status.has_cached_db);
                    println!(
                        "Cached tag:    {}",
                        status.cached_tag.as_deref().unwrap_or("none")
                    );
                    println!(
                        "Schema:        {}",
                        status
                            .cached_schema_version
                            .map_or("N/A".into(), |v| v.to_string())
                    );
                    println!("Update avail:  {}", status.update_available);
                    println!("Message:       {}", status.message);
                }
            }
            RegistryCmd::Sync => {
                let local_state = data_dir.join("local_state.db");
                if !local_state.exists() {
                    agora_core::db::init_local_state_db(&local_state)?;
                }
                let report = agora_core::registry_sync::check_and_download_update(
                    data_dir,
                    &local_state,
                    true,
                    None,
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!("Registry sync: {}", report.message);
                }
            }
        },
        Commands::Snapshots { action } => match action {
            SnapshotsCmd::List { instance } => {
                let instance_dir = agora_core::paths::instance_dir(data_dir, &instance)?;
                if !instance_dir.exists() {
                    eprintln!("Instance '{}' not found", instance);
                    std::process::exit(1);
                }
                let snapshots = agora_core::snapshot::list_snapshots(&instance_dir)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&snapshots)?);
                } else {
                    let rows: Vec<Vec<String>> = snapshots
                        .iter()
                        .map(|s| {
                            vec![
                                s.id.clone(),
                                s.label.clone().unwrap_or_default(),
                                s.created_at.clone(),
                                s.file_count.to_string(),
                            ]
                        })
                        .collect();
                    print_table(&["ID", "Label", "Created", "Files"], &rows);
                }
            }
            SnapshotsCmd::Create { instance, label } => {
                let instance_dir = agora_core::paths::instance_dir(data_dir, &instance)?;
                if !instance_dir.exists() {
                    eprintln!("Instance '{}' not found", instance);
                    std::process::exit(1);
                }
                let snapshot =
                    agora_core::snapshot::create_snapshot(&instance_dir, label.as_deref())
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&snapshot)?);
                } else {
                    println!(
                        "Created snapshot {} ({})",
                        snapshot.id,
                        snapshot.label.as_deref().unwrap_or("unlabeled")
                    );
                }
            }
            SnapshotsCmd::Restore {
                instance,
                snapshot_id,
            } => {
                let instance_dir = agora_core::paths::instance_dir(data_dir, &instance)?;
                if !instance_dir.exists() {
                    eprintln!("Instance '{}' not found", instance);
                    std::process::exit(1);
                }
                agora_core::snapshot::restore_snapshot(&instance_dir, &snapshot_id)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                println!(
                    "Restored instance '{}' from snapshot {}",
                    instance, snapshot_id
                );
            }
        },
        Commands::Import {
            path,
            symlink_saves,
        } => {
            if !path.exists() {
                eprintln!("Path '{}' does not exist", path.display());
                std::process::exit(1);
            }
            let target = agora_core::paths::instances_dir(data_dir)?;
            let result = if path.is_dir() {
                agora_core::import::import_directory(&path, &target, symlink_saves)?
            } else {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                match ext {
                    "mrpack" => agora_core::import::import_mrpack(&path, &target, symlink_saves)?,
                    "zip" => agora_core::import::import_prism_zip(&path, &target, symlink_saves)?,
                    _ => anyhow::bail!(
                        "Unsupported file type '.{}'. Use .mrpack, .zip, or a directory",
                        ext
                    ),
                }
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Imported: {} ({} mods)", result.name, result.imported_mods);
            }
        }
        Commands::Launch { instance, yes: _ } => {
            let instance_dir = agora_core::paths::instance_dir(data_dir, &instance)?;
            if !instance_dir.exists() {
                eprintln!("Instance '{}' not found", instance);
                std::process::exit(1);
            }
            let manifest_path = agora_core::paths::instance_manifest_path(data_dir, &instance)?;
            if !manifest_path.exists() {
                eprintln!("Instance manifest not found for '{}'", instance);
                std::process::exit(1);
            }
            let text = std::fs::read_to_string(&manifest_path)?;
            let manifest: agora_core::models::InstanceManifest = serde_json::from_str(&text)?;

            let creds = agora_core::msa::load_credentials()?;
            let (username, uuid, access_token) = match creds {
                Some(c) if !c.is_expired() => {
                    println!("Authenticated as {}", c.username);
                    (c.username, c.uuid, c.access_token)
                }
                Some(_) => {
                    eprintln!("Credentials expired. Run 'agora auth login' to re-authenticate.");
                    std::process::exit(1);
                }
                None => {
                    eprintln!("Not authenticated. Run 'agora auth login' first.");
                    std::process::exit(1);
                }
            };

            let reg_path = data_dir.join("registry.db");
            let reg_opt = if reg_path.exists() {
                Some(reg_path)
            } else {
                None
            };
            let report = agora_core::health::health(&instance_dir, &manifest, reg_opt.as_deref());
            if report.score == agora_core::health::HealthScore::Red {
                eprintln!("Health check failed — blockers prevent launch.");
                for b in &report.blockers {
                    eprintln!("  [BLOCK] {}", b.message);
                }
                std::process::exit(2);
            }

            let java_home = find_java()?;
            let mc_dir = agora_core::paths::minecraft_dir()
                .ok_or_else(|| anyhow::anyhow!("Could not determine Minecraft directory"))?;

            let loader = agora_core::launch::LoaderInfo {
                loader_type: manifest.loader.clone(),
                version: manifest.loader_version.clone(),
                version_url: String::new(),
            };

            let opts = agora_core::launch::LaunchOptions {
                java_path: java_home,
                mc_version: manifest.minecraft_version.clone(),
                game_dir: instance_dir.clone(),
                assets_dir: mc_dir.join("assets"),
                username,
                access_token,
                uuid,
                user_type: "msa".into(),
                jvm_args: String::new(),
                mc_args_extra: Vec::new(),
                loader: Some(loader),
            };

            let snapshot_label = format!(
                "pre-launch-cli-{}",
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            );
            let snapshot_id = {
                let lkg = agora_core::lkg::read_lkg_state(&instance_dir)
                    .map_err(|error| anyhow::anyhow!(error))?;
                let reusable = lkg.current_lkg_snapshot_id.and_then(|snapshot_id| {
                    let reference =
                        agora_core::snapshot::snapshot_file_index(&instance_dir, &snapshot_id)
                            .ok()?;
                    let current = agora_core::snapshot::live_file_index(&instance_dir).ok()?;
                    (reference == current).then_some(snapshot_id)
                });
                match reusable {
                    Some(snapshot_id) => snapshot_id,
                    None => {
                        agora_core::snapshot::create_snapshot(&instance_dir, Some(&snapshot_label))
                            .map_err(|error| anyhow::anyhow!(error))?
                            .id
                    }
                }
            };
            if !json {
                println!(
                    "Launching '{}'; Agora will remain attached until the game exits.",
                    instance
                );
            }
            let (spawned, outcome) = agora_core::launch::spawn_java_and_wait(&opts).await?;
            agora_core::lkg::record_launch_outcome(
                &instance_dir,
                Some(&snapshot_id),
                &format!("cli-{}", spawned.pid),
                outcome.clone(),
            )
            .map_err(|error| anyhow::anyhow!(error))?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "pid": spawned.pid,
                        "outcome": outcome,
                        "snapshot_id": snapshot_id,
                    })
                );
            } else {
                println!("Launch finished with outcome {:?}.", outcome);
            }
        }
        Commands::Auth { action } => match action {
            AuthCmd::Login => {
                let db_path = data_dir.join("local_state.db");
                let flow = agora_core::msa::begin_login(client, &db_path).await?;
                println!("Open this URL in your browser:");
                println!("{}", flow.auth_uri);
                println!();
                println!("After authorizing, paste the full redirect URL here:");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                let input = input.trim();
                if input.is_empty() {
                    eprintln!("No input provided");
                    std::process::exit(1);
                }
                let (code, state) = extract_auth_redirect(input)?;
                let credentials =
                    agora_core::msa::finish_login(client, &code, &flow, Some(&state), &db_path).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "username": credentials.username,
                            "uuid": credentials.uuid,
                            "expires": credentials.expires,
                        })
                    );
                } else {
                    println!("Signed in as {}", credentials.username);
                }
            }
            AuthCmd::Status => match agora_core::msa::load_credentials()? {
                Some(creds) => {
                    if creds.is_expired() {
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({"status": "expired", "username": creds.username})
                            );
                        } else {
                            println!(
                                "Signed in as {} (expired — run 'agora auth login')",
                                creds.username
                            );
                        }
                    } else {
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({
                                    "status": "valid",
                                    "username": creds.username,
                                    "expires": creds.expires,
                                })
                            );
                        } else {
                            println!(
                                "Signed in as {} (expires {})",
                                creds.username, creds.expires
                            );
                        }
                    }
                }
                None => {
                    if json {
                        println!("{}", serde_json::json!({"status": "not_authenticated"}));
                    } else {
                        println!("Not authenticated. Run 'agora auth login'.");
                    }
                }
            },
            AuthCmd::Logout => {
                agora_core::msa::clear_credentials()?;
                if json {
                    println!("{}", serde_json::json!({"status": "logged_out"}));
                } else {
                    println!("Signed out.");
                }
            }
        },
        Commands::Serve { port } => {
            println!("Starting MCP server on 127.0.0.1:{}", port);
            println!("MCP server is not yet implemented in the CLI binary.");
        }
        Commands::Sync => {
            let local_state = data_dir.join("local_state.db");
            if !local_state.exists() {
                agora_core::db::init_local_state_db(&local_state)?;
            }
            let report = agora_core::registry_sync::check_and_download_update(
                data_dir,
                &local_state,
                true,
                None,
            )
            .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.message);
            }
        }
    }

    Ok(())
}

/// Extract the authorization code and CSRF state from the complete browser
/// redirect URL. The direct-launch OAuth flow always creates a state value, so
/// accepting a bare code would make CLI login fail closed and hide the cause.
fn extract_auth_redirect(input: &str) -> anyhow::Result<(String, String)> {
    let url = reqwest::Url::parse(input).map_err(|_| {
        anyhow::anyhow!(
            "Paste the complete redirect URL so Agora can verify the OAuth state parameter."
        )
    })?;
    let mut code = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            _ => {}
        }
    }

    let code =
        code.ok_or_else(|| anyhow::anyhow!("Redirect URL did not include an OAuth code."))?;
    let state =
        state.ok_or_else(|| anyhow::anyhow!("Redirect URL did not include an OAuth state."))?;
    Ok((code, state))
}

fn find_java() -> anyhow::Result<PathBuf> {
    #[cfg(windows)]
    {
        let candidates = vec![
            PathBuf::from(r"C:\Program Files\Java\jdk-21\bin\java.exe"),
            PathBuf::from(r"C:\Program Files\Java\jdk-17\bin\java.exe"),
            PathBuf::from(r"C:\Program Files\Eclipse Adoptium\jdk-21\bin\java.exe"),
            PathBuf::from(r"C:\Program Files\Eclipse Adoptium\jdk-17\bin\java.exe"),
        ];
        for c in &candidates {
            if c.exists() {
                return Ok(c.clone());
            }
        }
        if let Ok(paths) = std::env::var("PATH") {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join("java.exe");
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(paths) = std::env::var("PATH") {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join("java");
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
    }
    anyhow::bail!("Could not find Java. Install JDK 17+ and ensure java is on your PATH.");
}

#[cfg(test)]
mod tests {
    use super::extract_auth_redirect;
    use super::SilentReporter;
    use agora_core::install_pipeline::{
        CancellationToken, ProgressEvent, ProgressPhase, ProgressReporter,
    };

    #[test]
    fn parses_code_and_state_from_redirect_url() {
        let (code, state) = extract_auth_redirect(
            "https://login.live.com/oauth20_desktop.srf?code=abc%20123&state=csrf-token",
        )
        .unwrap();
        assert_eq!(code, "abc 123");
        assert_eq!(state, "csrf-token");
    }

    #[test]
    fn rejects_redirect_without_state() {
        assert!(extract_auth_redirect("https://example.invalid/?code=abc").is_err());
    }

    #[test]
    fn silent_reporter_accepts_progress_events() {
        let reporter = SilentReporter;
        // Should not panic
        reporter.report(ProgressEvent {
            plan_id: "test".into(),
            phase: ProgressPhase::Resolving,
            step: 0,
            total_steps: 1,
            bytes_downloaded: 0,
            bytes_total: 0,
            message: "test".into(),
        });
    }

    #[test]
    fn silent_reporter_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SilentReporter>();
    }

    #[test]
    fn cancellation_token_default_is_not_cancelled() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancellation_token_cancel_works() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }
}
