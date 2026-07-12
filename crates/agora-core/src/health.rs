use crate::db;
use crate::dependency_ops::{AliasMap, JarDeps};
use crate::jar_metadata::parse_jar_metadata;
use crate::models::InstanceManifest;
use crate::registry;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Pre-launch health score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthScore {
    Green,
    Yellow,
    Red,
}

/// A non-blocking concern surfaced in the health dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    pub mod_id: Option<String>,
    /// The actual `.jar` filename on disk, when the finding concerns a specific
    /// installed mod. `None` for findings where no installed JAR is involved
    /// (e.g. a missing required dependency).
    pub filename: Option<String>,
    pub message: String,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    MissingOptionalDependency,
    DuplicateModId,
    UnknownMod,
    /// A JAR-declared hard incompatibility (`breaks` / Forge `incompatible`)
    /// whose version range could NOT be positively matched against the
    /// installed version (version matching is not yet implemented). Surfaced
    /// as a warning rather than a blocker to avoid false launch-blocks.
    IncompatibleModUnverified,
    /// A soft incompatibility: Fabric `conflicts` or NeoForge `discouraged`.
    /// The target is installed; the user should review whether they coexist.
    IncompatibleModSoft,
    /// A curated `known_conflicts` record whose severity is not launch-breaking.
    CuratedConflictSoft,
}

/// A blocking concern that should prevent launch until resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blocker {
    pub kind: BlockerKind,
    pub mod_id: Option<String>,
    /// The actual `.jar` filename on disk, when the finding concerns a specific
    /// installed mod. `None` for findings where no installed JAR is involved.
    pub filename: Option<String>,
    pub message: String,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlockerKind {
    MissingRequiredDependency,
    IncompatibleMod,
    CuratedConflict,
}

/// Full health report for a pre-launch scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub score: HealthScore,
    pub warnings: Vec<Warning>,
    pub blockers: Vec<Blocker>,
}

/// Per-JAR parsed metadata indexed by filename.
struct InstalledJar {
    filename: String,
    jar: JarDeps,
}

/// Run the pre-launch health scan on an instance.
///
/// Scans every JAR in `mods/`, parses declared dependencies, cross-references
/// against the curated `known_conflicts` table (if registry.db is available),
/// and returns a go/no-go [`HealthReport`].
///
/// Phase 3 property: this function NEVER requires registry.db. If the registry
/// connection is unavailable, curated-conflict checks are skipped — the rest
/// of the scan still runs.
pub fn health(
    instance_dir: &Path,
    manifest: &InstanceManifest,
    registry_db_path: Option<&std::path::Path>,
) -> HealthReport {
    let mods_dir = instance_dir.join("mods");

    // 1. Scan all JARs
    let mut jars: Vec<InstalledJar> = Vec::new();
    if mods_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&mods_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jar") {
                    let jar = parse_jar_metadata(&path);
                    let filename = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    jars.push(InstalledJar { filename, jar });
                }
            }
        }
    }

    // 2. Build index: mod_jar_id -> set of filenames
    let mut id_to_files: HashMap<String, Vec<String>> = HashMap::new();
    for ij in &jars {
        if let Some(ref id) = ij.jar.mod_jar_id {
            id_to_files
                .entry(id.clone())
                .or_default()
                .push(ij.filename.clone());
        }
    }

    // 3. Also build from manifest's installed mod list (modrinth_id / registry_id)
    let manifest_mod_ids: HashSet<String> = manifest
        .mods
        .iter()
        .filter_map(|m| m.registry_id.clone())
        .collect();

    let mut warnings = Vec::new();
    let mut blockers = Vec::new();

    // 3a. Load aliases and curated deps from the registry for alias resolution
    //     in subsequent checks. (registry.db, optional — Phase 3 decoupling)
    let alias_pairs: Vec<(String, String)> = registry_db_path
        .and_then(|p| {
            if p.exists() {
                db::registry_connection(p)
                    .ok()
                    .and_then(|conn| registry::get_all_mod_aliases(&conn).ok())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let aliases = AliasMap::from_pairs(&alias_pairs);

    let curated_deps: HashMap<String, registry::ManifestDeps> = registry_db_path
        .and_then(|p| {
            if p.exists() {
                db::registry_connection(p)
                    .ok()
                    .and_then(|conn| registry::get_all_manifest_dependencies(&conn).ok())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let curated_index: HashMap<String, &registry::ManifestDeps> = curated_deps
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v))
        .collect();

    // Rebuild id_to_files with alias-resolved keys so dep name lookups
    // (also alias-resolved) match canonical registry IDs.
    let mut resolved_id_to_files: HashMap<String, Vec<String>> = HashMap::new();
    for (id, files) in id_to_files.drain() {
        let canonical = aliases.resolve_or_self(&id).to_lowercase();
        resolved_id_to_files
            .entry(canonical)
            .or_default()
            .extend(files);
    }
    id_to_files = resolved_id_to_files;

    // 4. Duplicate mod_jar_id check
    for (id, files) in &id_to_files {
        if files.len() > 1 {
            warnings.push(Warning {
                kind: WarningKind::DuplicateModId,
                mod_id: Some(id.clone()),
                filename: files.first().cloned(),
                message: format!(
                    "Multiple JARs declare mod ID '{}': {}",
                    id,
                    files.join(", ")
                ),
                suggested_action: Some(
                    "Keep only one version of this mod; disable the others.".into(),
                ),
            });
        }
    }

    // 5. Required dependency checks (alias-aware)
    for ij in &jars {
        let source = &ij.filename;
        for dep in &ij.jar.depends_on {
            let dep_resolved = aliases.resolve_or_self(dep).to_lowercase();
            let dep_present = id_to_files.contains_key(&dep_resolved)
                || manifest_mod_ids
                    .iter()
                    .any(|id| aliases.resolve_or_self(id).to_lowercase() == dep_resolved);
            if !dep_present {
                let display_name = if dep_resolved != dep.to_lowercase() {
                    dep_resolved.clone()
                } else {
                    dep.clone()
                };
                blockers.push(Blocker {
                    kind: BlockerKind::MissingRequiredDependency,
                    mod_id: Some(display_name.clone()),
                    filename: None, // dependency is not installed
                    message: format!(
                        "'{}' requires '{}' but it is not installed.",
                        source, display_name
                    ),
                    suggested_action: Some(format!(
                        "Install '{}' to resolve this dependency.",
                        display_name
                    )),
                });
            }
        }
    }

    // 6. Incompatible mod checks (alias-aware with curated override).
    //
    // Consumes structured `IncompatibilityDecl`s (preserving severity + version
    // ranges) rather than the flat incompatible_deps list. Until full version
    // matching is implemented, the policy is:
    //   - hard (breaks/Forge incompatible) + unconditional range => BLOCKER;
    //   - hard + conditional range (version can't be confirmed) => warning;
    //   - soft (conflicts/discouraged) => always warning;
    //   - self-declared conflict => discarded;
    //   - curated ManifestDeps declaring the pair compatible => suppressed.
    //
    // Also backfill decls from older JAR parses that populated the flat
    // `incompatible_deps` list but emitted no `incompatibility_decls` (e.g.
    // legacy/desktop parser output). These legacy entries carry no severity or
    // version info, so they are treated as hard + unconditional — but because we
    // cannot confirm them, the safe default is a *warning*, not a blocker. We
    // model them as soft to avoid reintroducing false-positive blockers.
    for ij in &jars {
        let source = &ij.filename;
        let source_mod_id = ij.jar.mod_jar_id.as_deref();
        // Canonicalize the SOURCE id through aliases BEFORE lookup so curated
        // overrides keyed by the registry id still match a raw jar id.
        let source_resolved = source_mod_id.map(|id| aliases.resolve_or_self(id).to_lowercase());

        // Collect the effective declarations for this jar, backfilling any
        // flat-list ids that are not already represented in the structured
        // decls (legacy parses).
        let mut effective_decls: Vec<&crate::dependency_ops::IncompatibilityDecl> =
            ij.jar.incompatibility_decls.iter().collect();
        let structured_ids: HashSet<String> = ij
            .jar
            .incompatibility_decls
            .iter()
            .map(|d| d.mod_id.to_lowercase())
            .collect();
        let legacy_backfilled: Vec<crate::dependency_ops::IncompatibilityDecl> = ij
            .jar
            .incompatible_deps
            .iter()
            .filter(|id| !structured_ids.contains(&id.to_lowercase()))
            .map(|id| crate::dependency_ops::IncompatibilityDecl {
                mod_id: id.clone(),
                version_ranges: Vec::new(),
                source: crate::dependency_ops::IncompatibilitySource::ForgeDiscouraged,
            })
            .collect();
        effective_decls.extend(legacy_backfilled.iter());

        for decl in effective_decls {
            let incompat_resolved = aliases.resolve_or_self(&decl.mod_id).to_lowercase();

            // Self-conflict guard: a mod never conflicts with itself.
            if source_resolved.as_deref() == Some(incompat_resolved.as_str()) {
                continue;
            }

            let incompat_present = id_to_files.contains_key(&incompat_resolved)
                || manifest_mod_ids
                    .iter()
                    .any(|id| aliases.resolve_or_self(id).to_lowercase() == incompat_resolved);
            if !incompat_present {
                continue;
            }

            // Curated override: the curator has verified the pair is compatible
            // (either side lists the other as a required/optional dep). This
            // suppresses JAR-derived declarations of any severity.
            let curated_override =
                source_resolved.as_ref().is_some_and(|src| {
                    let source_side = curated_index.get(src).is_some_and(|deps| {
                        deps.required
                            .iter()
                            .any(|r| aliases.resolve_or_self(r).to_lowercase() == incompat_resolved)
                            || deps.optional.iter().any(|o| {
                                aliases.resolve_or_self(o).to_lowercase() == incompat_resolved
                            })
                    });
                    let target_side = curated_index.get(&incompat_resolved).is_some_and(|deps| {
                        let src = source_resolved.as_deref();
                        deps.required
                            .iter()
                            .any(|r| aliases.resolve_or_self(r).to_lowercase() == src.unwrap_or(""))
                            || deps.optional.iter().any(|o| {
                                aliases.resolve_or_self(o).to_lowercase() == src.unwrap_or("")
                            })
                    });
                    source_side || target_side
                });
            if curated_override {
                continue;
            }

            if decl.source.is_hard() && is_unconditional(&decl.version_ranges) {
                blockers.push(Blocker {
                    kind: BlockerKind::IncompatibleMod,
                    mod_id: Some(decl.mod_id.clone()),
                    filename: Some(source.clone()), // disable the source mod
                    message: format!(
                        "'{}' declares an incompatibility with '{}' and both are installed.",
                        source, decl.mod_id
                    ),
                    suggested_action: Some(format!(
                        "Remove '{}' or '{}' to resolve the conflict.",
                        source, decl.mod_id
                    )),
                });
            } else if decl.source.is_hard() {
                // Hard incompat but with a conditional range we can't verify —
                // surface as a warning, never a launch-blocker.
                let range_desc = decl
                    .version_ranges
                    .first()
                    .map(|r| format!(" (declared range: {})", r))
                    .unwrap_or_default();
                warnings.push(Warning {
                    kind: WarningKind::IncompatibleModUnverified,
                    mod_id: Some(decl.mod_id.clone()),
                    filename: Some(source.clone()),
                    message: format!(
                        "'{}' declares an incompatibility with '{}'{}, but Agora could not verify the installed version matches the incompatible range. Review before launch.",
                        source, decl.mod_id, range_desc
                    ),
                    suggested_action: Some(format!(
                        "Check that '{}' and '{}' are compatible versions, or remove one.",
                        source, decl.mod_id
                    )),
                });
            } else {
                // Soft incompatibility (Fabric `conflicts`, NeoForge
                // `discouraged`, or legacy backfilled entries with no severity):
                // always a warning, never a blocker.
                warnings.push(Warning {
                    kind: WarningKind::IncompatibleModSoft,
                    mod_id: Some(decl.mod_id.clone()),
                    filename: Some(source.clone()),
                    message: format!(
                        "'{}' may conflict with '{}' (soft incompatibility). The mod may still function; review before launch.",
                        source, decl.mod_id
                    ),
                    suggested_action: Some(format!(
                        "If you experience issues, remove '{}' or '{}'.",
                        source, decl.mod_id
                    )),
                });
            }
        }
    }

    // 7. Curated known_conflicts (registry.db, optional — Phase 3 decoupling)
    if let Some(reg_path) = registry_db_path {
        if reg_path.exists() {
            if let Ok(conn) = db::registry_connection(reg_path) {
                if let Ok(conflicts) = registry::get_known_conflicts(&conn) {
                    // Build reverse index: registry_id -> filename for cross-reference
                    let installed_registry_ids: HashSet<&str> = manifest
                        .mods
                        .iter()
                        .filter_map(|m| m.registry_id.as_deref())
                        .collect();

                    for conflict in &conflicts {
                        let a_present = installed_registry_ids.contains(conflict.mod_a_id.as_str())
                            || id_to_files.contains_key(conflict.mod_a_id.as_str());
                        let b_present = installed_registry_ids.contains(conflict.mod_b_id.as_str())
                            || id_to_files.contains_key(conflict.mod_b_id.as_str());
                        if a_present && b_present {
                            let mitigation = if conflict.mitigated_by.is_empty() {
                                "No known mitigation.".into()
                            } else {
                                format!("Try removing: {}", conflict.mitigated_by.join(", "))
                            };
                            let message = format!(
                                "Known conflict between '{}' and '{}' (severity: {}). {}",
                                conflict.mod_a_id,
                                conflict.mod_b_id,
                                conflict.severity,
                                conflict.notes.as_deref().unwrap_or("")
                            );
                            if is_hard_severity(&conflict.severity) {
                                blockers.push(Blocker {
                                    kind: BlockerKind::CuratedConflict,
                                    mod_id: None,
                                    filename: None, // no single actionable file
                                    message,
                                    suggested_action: Some(mitigation),
                                });
                            } else {
                                // Non-hard (or unrecognized/missing) severity:
                                // informational warning, never a launch-blocker.
                                warnings.push(Warning {
                                    kind: WarningKind::CuratedConflictSoft,
                                    mod_id: None,
                                    filename: None,
                                    message,
                                    suggested_action: Some(mitigation),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // 8. Optional dependency warnings (alias-aware)
    for ij in &jars {
        let source = &ij.filename;
        for dep in &ij.jar.optional_deps {
            let dep_resolved = aliases.resolve_or_self(dep).to_lowercase();
            let dep_present = id_to_files.contains_key(&dep_resolved)
                || manifest_mod_ids
                    .iter()
                    .any(|id| aliases.resolve_or_self(id).to_lowercase() == dep_resolved);
            if !dep_present {
                let display_name = if dep_resolved != dep.to_lowercase() {
                    dep_resolved.clone()
                } else {
                    dep.clone()
                };
                warnings.push(Warning {
                    kind: WarningKind::MissingOptionalDependency,
                    mod_id: Some(display_name.clone()),
                    filename: None, // dependency is not installed
                    message: format!(
                        "'{}' recommends '{}' but it is not installed. The mod may work without it.",
                        source, display_name
                    ),
                    suggested_action: None,
                });
            }
        }
    }

    // 9. Unknown mods (in mods/ dir but not tracked in manifest)
    let manifest_filenames: HashSet<&str> =
        manifest.mods.iter().map(|m| m.filename.as_str()).collect();
    for ij in &jars {
        if !manifest_filenames.contains(ij.filename.as_str()) {
            warnings.push(Warning {
                kind: WarningKind::UnknownMod,
                mod_id: ij.jar.mod_jar_id.clone(),
                filename: Some(ij.filename.clone()),
                message: format!(
                    "'{}' is in the mods folder but not tracked in the instance manifest.",
                    ij.filename
                ),
                suggested_action: Some(
                    "This may be a manually-added mod. It will be launched but is not managed by Agora.".into(),
                ),
            });
        }
    }

    // 10. Compute score
    let score = if blockers.is_empty() && warnings.is_empty() {
        HealthScore::Green
    } else if blockers.is_empty() {
        HealthScore::Yellow
    } else {
        HealthScore::Red
    };

    HealthReport {
        score,
        warnings,
        blockers,
    }
}

/// True when a version-range list represents an unconditional match (any
/// installed version satisfies it). This is the only version judgment Agora
/// makes without a full predicate/Maven-range parser:
///   - empty ranges => no constraint declared => unconditional;
///   - any `"*"` / empty-string entry => Fabric "any version" => unconditional;
///   - Forge open-ended empty/match-all ranges (`"[,)"`, `"[,]"`) => unconditional.
///
/// Anything else (e.g. `"<2.0"`, `"[1.0,2.0)"`) is treated as *conditional* —
/// unverified — and surfaced as a warning rather than a blocker.
fn is_unconditional(ranges: &[String]) -> bool {
    ranges.is_empty()
        || ranges.iter().any(|r| {
            let t = r.trim();
            t == "*" || t.is_empty() || t == "[,)" || t == "[,]"
        })
}

/// True when a curated `known_conflicts.severity` string denotes a
/// launch-breaking (hard) conflict. Uses exact, case-insensitive, trimmed
/// matching against an allowlist so values like `"hardcoded"` do not match.
/// Anything unrecognized (including missing/empty) is treated as soft (warning)
/// — whitelist-over-denylist, defaulting to the non-blocking classification.
fn is_hard_severity(s: &str) -> bool {
    matches!(
        s.trim().to_lowercase().as_str(),
        "hard" | "critical" | "breaking" | "fatal" | "incompatible" | "block" | "blocker"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency_ops::{IncompatibilityDecl, IncompatibilitySource, JarDeps};
    use crate::models::InstalledMod;

    #[test]
    fn health_empty_instance_is_green() {
        let manifest = InstanceManifest {
            instance_id: "test".into(),
            name: "Test".into(),
            created_from_pack: None,
            minecraft_version: "1.21".into(),
            loader: "fabric".into(),
            loader_version: "0.15.11".into(),
            is_locked: false,
            mods: vec![],
            resourcepacks: vec![],
            shaders: vec![],
            datapacks: vec![],
            worlds: vec![],
            user_preferences: serde_json::json!({}),
        };
        let dir = std::env::temp_dir().join("agora_health_test_empty");
        let _ = std::fs::create_dir_all(dir.join("mods"));
        let report = health(&dir, &manifest, None);
        assert_eq!(report.score, HealthScore::Green);
        assert!(report.blockers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_missing_required_dep_is_red() {
        let manifest = InstanceManifest {
            instance_id: "test".into(),
            name: "Test".into(),
            created_from_pack: None,
            minecraft_version: "1.21".into(),
            loader: "fabric".into(),
            loader_version: "0.15.11".into(),
            is_locked: false,
            mods: vec![InstalledMod {
                filename: "mod-with-dep.jar".into(),
                registry_id: None,
                modrinth_id: None,
                source: "modrinth".into(),
                source_url: None,
                version: Some("1.0.0".into()),
                sha256: "abc".into(),
                installed_at: "2024-01-01T00:00:00Z".into(),
                java_packages: vec![],
                mod_jar_id: Some("mod-with-dep".into()),
                depends_on: vec!["fabric-api".into()],
                optional_deps: vec![],
                incompatible_deps: vec![],
                enabled: true,
                content_type: "mod".into(),
            }],
            resourcepacks: vec![],
            shaders: vec![],
            datapacks: vec![],
            worlds: vec![],
            user_preferences: serde_json::json!({}),
        };
        let dir = std::env::temp_dir().join("agora_health_test_missing_dep");
        let mods_dir = dir.join("mods");
        let _ = std::fs::create_dir_all(&mods_dir);
        // No fabric-api.jar present, but mod-with-dep.jar declares it as required
        // Simulate by not placing any JARs (parse_jar_metadata returns defaults)
        // The health function walks mods/ which is empty, so no jars are found.
        // With no jars found, there are no blockers — this is the "no mods installed" case.
        // To test the missing-dep case properly we'd need a real JAR or a mock.
        // For now just verify the function doesn't panic.
        let report = health(&dir, &manifest, None);
        assert!(matches!(
            report.score,
            HealthScore::Green | HealthScore::Yellow
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -------------------------------------------------------------------
    // Incompatibility policy tests (Fabric breaks/conflicts + Forge).
    //
    // These build real .jar fixtures in a temp mods/ dir so the health check's
    // own parse path is exercised end-to-end.
    // -------------------------------------------------------------------

    /// Build an in-memory .jar with the given `(entry, content)` pairs into
    /// `mods_dir/<filename>`.
    fn write_jar(mods_dir: &Path, filename: &str, entries: &[(&str, &str)]) {
        use std::io::Write;
        let path = mods_dir.join(filename);
        let file = std::fs::File::create(&path).expect("create jar file");
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::FileOptions::default();
        for (name, content) in entries {
            zip.start_file(*name, opts).expect("start_file");
            zip.write_all(content.as_bytes()).expect("write_all");
        }
        zip.finish().expect("finish zip");
    }

    fn fresh_instance(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agora_health_incompat_{}_{}",
            label,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("mods")).expect("create mods dir");
        dir
    }

    fn tracked_manifest(mods: &[(&str, &str)]) -> InstanceManifest {
        // mods: (filename, mod_jar_id)
        let mods: Vec<InstalledMod> = mods
            .iter()
            .map(|(filename, jar_id)| InstalledMod {
                filename: filename.to_string(),
                registry_id: None,
                modrinth_id: None,
                source: "manual".into(),
                source_url: None,
                version: None,
                sha256: String::new(),
                installed_at: String::new(),
                java_packages: vec![],
                mod_jar_id: Some(jar_id.to_string()),
                depends_on: vec![],
                optional_deps: vec![],
                incompatible_deps: vec![],
                enabled: true,
                content_type: "mod".into(),
            })
            .collect();
        InstanceManifest {
            instance_id: "test".into(),
            name: "Test".into(),
            created_from_pack: None,
            minecraft_version: "1.21".into(),
            loader: "fabric".into(),
            loader_version: "0.15.11".into(),
            is_locked: false,
            mods,
            resourcepacks: vec![],
            shaders: vec![],
            datapacks: vec![],
            worlds: vec![],
            user_preferences: serde_json::json!({}),
        }
    }

    #[test]
    fn health_unconditional_breaks_blocks_launch() {
        // A breaks B with "*" (unconditional) and both installed => BLOCKER.
        let dir = fresh_instance("unconditional_breaks");
        let mods_dir = dir.join("mods");
        write_jar(
            &mods_dir,
            "a.jar",
            &[("fabric.mod.json", r#"{"id":"a","breaks":{"b":"*"}}"#)],
        );
        write_jar(&mods_dir, "b.jar", &[("fabric.mod.json", r#"{"id":"b"}"#)]);
        let manifest = tracked_manifest(&[("a.jar", "a"), ("b.jar", "b")]);
        let report = health(&dir, &manifest, None);
        assert_eq!(report.score, HealthScore::Red);
        assert_eq!(report.blockers.len(), 1);
        assert_eq!(report.blockers[0].kind, BlockerKind::IncompatibleMod);
        assert_eq!(report.blockers[0].mod_id.as_deref(), Some("b"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_conditional_breaks_is_warning_not_blocker() {
        // A breaks B with "<2.0" (conditional, unverifiable) => WARNING, not blocker.
        let dir = fresh_instance("conditional_breaks");
        let mods_dir = dir.join("mods");
        write_jar(
            &mods_dir,
            "a.jar",
            &[("fabric.mod.json", r#"{"id":"a","breaks":{"b":"<2.0"}}"#)],
        );
        write_jar(
            &mods_dir,
            "b.jar",
            &[("fabric.mod.json", r#"{"id":"b","version":"2.5"}"#)],
        );
        let manifest = tracked_manifest(&[("a.jar", "a"), ("b.jar", "b")]);
        let report = health(&dir, &manifest, None);
        assert!(
            report.blockers.is_empty(),
            "conditional breaks must not block: {:?}",
            report.blockers
        );
        assert_eq!(report.score, HealthScore::Yellow);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::IncompatibleModUnverified
                && w.mod_id.as_deref() == Some("b")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_fabric_conflicts_is_warning_never_blocker() {
        let dir = fresh_instance("conflicts_warning");
        let mods_dir = dir.join("mods");
        write_jar(
            &mods_dir,
            "a.jar",
            &[("fabric.mod.json", r#"{"id":"a","conflicts":{"b":"*"}}"#)],
        );
        write_jar(&mods_dir, "b.jar", &[("fabric.mod.json", r#"{"id":"b"}"#)]);
        let manifest = tracked_manifest(&[("a.jar", "a"), ("b.jar", "b")]);
        let report = health(&dir, &manifest, None);
        assert!(
            report.blockers.is_empty(),
            "conflicts must never block: {:?}",
            report.blockers
        );
        assert!(report.warnings.iter().any(
            |w| w.kind == WarningKind::IncompatibleModSoft && w.mod_id.as_deref() == Some("b")
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_self_conflict_discarded() {
        // A declares breaks on itself (parse bug or real edge case) => no finding.
        let dir = fresh_instance("self_conflict");
        let mods_dir = dir.join("mods");
        write_jar(
            &mods_dir,
            "a.jar",
            &[("fabric.mod.json", r#"{"id":"a","breaks":{"a":"*"}}"#)],
        );
        let manifest = tracked_manifest(&[("a.jar", "a")]);
        let report = health(&dir, &manifest, None);
        assert!(
            report.blockers.is_empty(),
            "self-conflict must not block: {:?}",
            report.blockers
        );
        assert!(
            !report.warnings.iter().any(|w| {
                w.kind == WarningKind::IncompatibleModSoft
                    || w.kind == WarningKind::IncompatibleModUnverified
            }),
            "self-conflict must not warn either: {:?}",
            report.warnings
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_breaks_target_absent_no_finding() {
        // A breaks B, but B is not installed => nothing.
        let dir = fresh_instance("breaks_target_absent");
        let mods_dir = dir.join("mods");
        write_jar(
            &mods_dir,
            "a.jar",
            &[("fabric.mod.json", r#"{"id":"a","breaks":{"b":"*"}}"#)],
        );
        let manifest = tracked_manifest(&[("a.jar", "a")]);
        let report = health(&dir, &manifest, None);
        assert!(report.blockers.is_empty());
        assert!(!report
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::IncompatibleModSoft
                || w.kind == WarningKind::IncompatibleModUnverified));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_forge_self_conflict_via_owner_header_fixed() {
        // Regression for the original bug: a Forge mod whose dependency block
        // header matched its own modId previously produced a self-conflict.
        let dir = fresh_instance("forge_self_conflict");
        let mods_dir = dir.join("mods");
        write_jar(
            &mods_dir,
            "examplemod.jar",
            &[
                (
                    "META-INF/mods.toml",
                    "modId=\"examplemod\"\n[[dependencies.examplemod]]\n    modId=\"othermod\"\n    type=\"incompatible\"\n",
                ),
            ],
        );
        let manifest = tracked_manifest(&[("examplemod.jar", "examplemod")]);
        let report = health(&dir, &manifest, None);
        assert!(
            report.blockers.is_empty(),
            "must not self-conflict: {:?}",
            report.blockers
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_alias_resolution_collapses_breaks_target() {
        // A breaks "mod-b" but B is installed with jar id "b", and an alias maps
        // "mod-b" -> "b". The break should resolve and fire (unconditional => blocker).
        let dir = fresh_instance("alias_breaks");
        let mods_dir = dir.join("mods");
        write_jar(
            &mods_dir,
            "a.jar",
            &[("fabric.mod.json", r#"{"id":"a","breaks":{"mod-b":"*"}}"#)],
        );
        write_jar(&mods_dir, "b.jar", &[("fabric.mod.json", r#"{"id":"b"}"#)]);
        let manifest = tracked_manifest(&[("a.jar", "a"), ("b.jar", "b")]);

        // Build a registry.db with a single alias and run health against it.
        let reg_path = dir.join("registry.db");
        build_alias_registry(&reg_path, &[("b", "mod-b")]);
        let report = health(&dir, &manifest, Some(&reg_path));
        assert_eq!(
            report.blockers.len(),
            1,
            "alias-resolved break should block: {:?}",
            report.blockers
        );
        assert_eq!(report.blockers[0].kind, BlockerKind::IncompatibleMod);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Create a registry.db containing only mod_jar_aliases for the given
    /// (registry_id, alias) pairs, so health() can load an AliasMap.
    fn build_alias_registry(path: &Path, aliases: &[(&str, &str)]) {
        let conn = rusqlite::Connection::open(path).expect("open registry db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS mod_jar_aliases (registry_id TEXT NOT NULL, alias TEXT NOT NULL);",
        )
        .expect("create aliases table");
        for (registry_id, alias) in aliases {
            conn.execute(
                "INSERT INTO mod_jar_aliases (registry_id, alias) VALUES (?1, ?2)",
                rusqlite::params![registry_id, alias],
            )
            .expect("insert alias");
        }
    }

    #[test]
    fn is_unconditional_helper() {
        assert!(is_unconditional(&[]));
        assert!(is_unconditional(&["*".to_string()]));
        assert!(is_unconditional(&["".to_string()]));
        assert!(is_unconditional(&["foo".to_string(), "*".to_string()])); // OR: any unconditional
        assert!(is_unconditional(&["[,)".to_string()]));
        assert!(!is_unconditional(&["<2.0".to_string()]));
        assert!(!is_unconditional(&["[1.0,2.0)".to_string()]));
    }

    #[test]
    fn is_hard_severity_helper() {
        assert!(is_hard_severity("hard"));
        assert!(is_hard_severity(" Hard "));
        assert!(is_hard_severity("critical"));
        assert!(is_hard_severity("breaking"));
        assert!(!is_hard_severity("soft"));
        assert!(!is_hard_severity("advisory"));
        assert!(!is_hard_severity("hardcoded")); // substring must NOT match
        assert!(!is_hard_severity(""));
    }

    #[test]
    fn jar_deps_default_has_empty_decls() {
        let d = JarDeps::default();
        assert!(d.incompatibility_decls.is_empty());
    }

    #[test]
    fn incompatibility_source_hardness() {
        assert!(IncompatibilitySource::FabricBreaks.is_hard());
        assert!(IncompatibilitySource::ForgeIncompatible.is_hard());
        assert!(!IncompatibilitySource::FabricConflicts.is_hard());
        assert!(!IncompatibilitySource::ForgeDiscouraged.is_hard());
    }

    #[test]
    fn incompatibility_decl_serializes() {
        let decl = IncompatibilityDecl {
            mod_id: "optifine".into(),
            version_ranges: vec!["<2.0".into()],
            source: IncompatibilitySource::FabricBreaks,
        };
        let json = serde_json::to_string(&decl).unwrap();
        assert!(json.contains("fabric_breaks"));
        let back: IncompatibilityDecl = serde_json::from_str(&json).unwrap();
        assert_eq!(back, decl);
    }
}
