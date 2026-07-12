use crate::dependency_ops::{IncompatibilityDecl, IncompatibilitySource, JarDeps};
use std::io::Read;
use std::path::Path;

/// Loader/framework mod IDs that are part of the ecosystem but never present as
/// installable mod JARs. Declaring any of these as a dependency would produce a
/// false `MissingRequiredDependency` blocker, so they are filtered out.
const DEPENDENCY_IGNORE_LIST: &[&str] = &[
    "minecraft",
    "fabricloader",
    "quilt_loader",
    "java",
    "forge",
    "neoforge",
];

/// Parse a `.jar` file to extract Java packages, mod ID, and declared
/// dependencies from `fabric.mod.json` / `META-INF/mods.toml` /
/// `META-INF/neoforge.mods.toml`.
///
/// Returns [`JarDeps::default()`] on any error — never panics.
pub fn parse_jar_metadata(jar_path: &Path) -> JarDeps {
    let file = match std::fs::File::open(jar_path) {
        Ok(f) => f,
        Err(_) => return JarDeps::default(),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return JarDeps::default(),
    };

    let mut packages: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut mod_jar_id: Option<String> = None;
    let mut depends_on: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut optional_deps: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // Flat id summary (filled from decls).
    let mut incompatible_ids: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut incompatibility_decls: Vec<IncompatibilityDecl> = Vec::new();
    let mut forge_mod_id: Option<String> = None;
    let mut saw_neoforge_toml = false;

    for i in 0..archive.len() {
        let name = match archive.by_index(i) {
            Ok(e) => e.name().to_string(),
            Err(_) => continue,
        };
        if name.ends_with(".class") {
            let stem = match name.strip_suffix(".class") {
                Some(s) => s,
                None => continue,
            };
            let replaced = stem.replace('\\', "/");
            let segments: Vec<&str> = replaced.split('/').collect();
            if segments.len() < 3 {
                continue;
            }
            let dir_segments: Vec<&str> = segments[..segments.len() - 1].to_vec();
            packages.insert(dir_segments.join("."));
            continue;
        }
        if name == "fabric.mod.json" {
            if let Some(content_str) = read_entry_utf8(&mut archive, i) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content_str) {
                    if let Some(id_str) = value.get("id").and_then(|v| v.as_str()) {
                        if !id_str.is_empty() {
                            mod_jar_id = Some(id_str.to_string());
                        }
                    }
                    // Required deps.
                    if let Some(val) = value.get("depends") {
                        extract_fabric_deps(val, &mut depends_on, None);
                    }
                    // Optional deps (recommends + suggests both soft).
                    for key in ["recommends", "suggests"] {
                        if let Some(val) = value.get(key) {
                            extract_fabric_deps(val, &mut optional_deps, None);
                        }
                    }
                    // breaks -> hard incompat; conflicts -> soft incompat.
                    if let Some(val) = value.get("breaks") {
                        extract_fabric_deps(
                            val,
                            &mut incompatible_ids,
                            Some((
                                IncompatibilitySource::FabricBreaks,
                                &mut incompatibility_decls,
                            )),
                        );
                    }
                    if let Some(val) = value.get("conflicts") {
                        extract_fabric_deps(
                            val,
                            &mut incompatible_ids,
                            Some((
                                IncompatibilitySource::FabricConflicts,
                                &mut incompatibility_decls,
                            )),
                        );
                    }
                }
            }
            continue;
        }
        // NeoForge ships neoforge.mods.toml; Forge ships mods.toml. Same TOML
        // schema, so both go through the same parser. Prefer neoforge when both
        // are present (a NeoForge mod's mods.toml, if also shipped, is usually
        // a stub).
        if name == "META-INF/neoforge.mods.toml" {
            saw_neoforge_toml = true;
            if let Some(content) = read_entry_utf8(&mut archive, i) {
                extract_forge_deps(
                    &content,
                    &mut depends_on,
                    &mut optional_deps,
                    &mut incompatible_ids,
                    &mut incompatibility_decls,
                    &mut forge_mod_id,
                );
            }
            continue;
        }
        if name == "META-INF/mods.toml" && !saw_neoforge_toml {
            if let Some(content) = read_entry_utf8(&mut archive, i) {
                extract_forge_deps(
                    &content,
                    &mut depends_on,
                    &mut optional_deps,
                    &mut incompatible_ids,
                    &mut incompatibility_decls,
                    &mut forge_mod_id,
                );
            }
            continue;
        }
    }

    if mod_jar_id.is_none() {
        mod_jar_id = forge_mod_id;
    }

    depends_on.retain(|dep| !DEPENDENCY_IGNORE_LIST.contains(&dep.as_str()));
    optional_deps.retain(|dep| !DEPENDENCY_IGNORE_LIST.contains(&dep.as_str()));
    incompatible_ids.retain(|dep| !DEPENDENCY_IGNORE_LIST.contains(&dep.as_str()));
    incompatibility_decls.retain(|d| !DEPENDENCY_IGNORE_LIST.contains(&d.mod_id.as_str()));

    JarDeps {
        java_packages: packages.into_iter().collect(),
        mod_jar_id,
        depends_on: depends_on.into_iter().collect(),
        optional_deps: optional_deps.into_iter().collect(),
        incompatible_deps: incompatible_ids.into_iter().collect(),
        incompatibility_decls,
    }
}

/// Extract Fabric dependency ids (and, for `breaks`/`conflicts`, structured
/// `IncompatibilityDecl`s carrying version-range predicates + severity).
///
/// - `out` receives the dep id strings (shared across depends/optional/incompat
///   flat lists depending on the caller).
/// - `incompat`: when `Some((severity, decls))`, also emit structured decls
///   capturing each version predicate. `None` for depends/recommends/suggests.
///
/// Fabric semantics:
/// - Object form `{"modid": "<2.0"}` → single AND predicate string.
/// - Object form `{"modid": ["<2.0", ">=3.0"]}` → OR array of predicate strings.
/// - Object form `{"modid": "*"}` → unconditional (any version).
/// - Array form `[{"id":..,"version":..}, {"identifier":..,"version":..}]` →
///   each object's `version` (may be absent) becomes a single-element range.
fn extract_fabric_deps(
    depends: &serde_json::Value,
    out: &mut std::collections::BTreeSet<String>,
    mut incompat: Option<(IncompatibilitySource, &mut Vec<IncompatibilityDecl>)>,
) {
    match depends {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                let ranges = fabric_version_ranges(val);
                out.insert(key.clone());
                if let Some((sev, decls)) = incompat.as_mut() {
                    decls.push(IncompatibilityDecl {
                        mod_id: key.clone(),
                        version_ranges: ranges,
                        source: *sev,
                    });
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for elem in arr {
                let id = elem
                    .get("id")
                    .and_then(|v| v.as_str())
                    .or_else(|| elem.get("identifier").and_then(|v| v.as_str()));
                if let Some(id) = id {
                    out.insert(id.to_string());
                    if let Some((sev, decls)) = incompat.as_mut() {
                        let ranges = match elem.get("version") {
                            Some(v) => fabric_version_ranges(v),
                            None => Vec::new(),
                        };
                        decls.push(IncompatibilityDecl {
                            mod_id: id.to_string(),
                            version_ranges: ranges,
                            source: *sev,
                        });
                    }
                }
            }
        }
        _ => {}
    }
}

/// Normalize a Fabric version value into a list of OR-joined predicate strings.
/// - String → single predicate (may contain space-separated AND predicates).
/// - Array of strings → OR list.
/// - Anything else → empty (unconditional).
fn fabric_version_ranges(val: &serde_json::Value) -> Vec<String> {
    match val {
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t == "*" || t.is_empty() {
                // "*" / empty = any version → represent as unconditional (empty).
                Vec::new()
            } else {
                vec![s.clone()]
            }
        }
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|e| match e {
                serde_json::Value::String(s) => {
                    let t = s.trim();
                    if t == "*" || t.is_empty() {
                        None
                    } else {
                        Some(s.clone())
                    }
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// In-flight Forge dependency block state.
#[derive(Default)]
struct PendingForgeDep {
    /// The TARGET mod id (the dependency), read from the inner `modId` line.
    mod_id: Option<String>,
    /// NeoForge `type`.
    dep_type: Option<String>,
    /// Traditional Forge `mandatory` (true=required, false=optional).
    mandatory: Option<bool>,
    /// `versionRange` (Maven range; empty string = any version).
    version_range: Option<String>,
}

/// Parse a Forge/NeoForge `mods.toml`/`neoforge.mods.toml` manifest.
///
/// Key fix: the section header `[[dependencies.<owner>]]` names the OWNER mod,
/// NOT the dependency. The dependency id is the `modId` line INSIDE the block.
/// Previously the parser stored the owner id as the dependency, which caused a
/// mod to appear to depend on / conflict with itself.
fn extract_forge_deps(
    content: &str,
    required_out: &mut std::collections::BTreeSet<String>,
    optional_out: &mut std::collections::BTreeSet<String>,
    incompatible_ids_out: &mut std::collections::BTreeSet<String>,
    incompatibility_decls_out: &mut Vec<IncompatibilityDecl>,
    mod_id_out: &mut Option<String>,
) {
    let mut pending = PendingForgeDep::default();
    let mut in_dep_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("[[dependencies.") {
            // Flush previous block, then open a new one.
            flush_forge_dep(
                &pending,
                required_out,
                optional_out,
                incompatible_ids_out,
                incompatibility_decls_out,
            );
            pending = PendingForgeDep::default();
            in_dep_block = false;
            if let Some(end) = rest.find(']') {
                let block_key = &rest[..end];
                if !block_key.is_empty() {
                    in_dep_block = true;
                    // block_key is the OWNER; intentionally NOT stored as the
                    // dependency id. The dependency id comes from the inner
                    // `modId` line.
                }
            }
            continue;
        }
        if trimmed.starts_with("[[") {
            flush_forge_dep(
                &pending,
                required_out,
                optional_out,
                incompatible_ids_out,
                incompatibility_decls_out,
            );
            pending = PendingForgeDep::default();
            in_dep_block = false;
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // `key = value` pairs. Capture inside dep blocks; also capture the
        // top-level (file-level) `modId` for the jar's own mod id.
        if let Some((key, value)) = parse_toml_kv(trimmed) {
            match key.as_str() {
                "modid" => {
                    if in_dep_block {
                        pending.mod_id = Some(value);
                    } else if mod_id_out.is_none() {
                        *mod_id_out = Some(value);
                    }
                }
                "type" => {
                    if in_dep_block {
                        pending.dep_type = Some(value);
                    }
                }
                "mandatory" => {
                    if in_dep_block {
                        pending.mandatory = parse_toml_bool(&value);
                    }
                }
                "versionrange" => {
                    if in_dep_block {
                        // Empty quoted string = any version (unconditional).
                        pending.version_range = Some(value);
                    }
                }
                _ => {}
            }
        }
    }
    flush_forge_dep(
        &pending,
        required_out,
        optional_out,
        incompatible_ids_out,
        incompatibility_decls_out,
    );
}

/// Finalize a pending Forge dependency block, routing it to the right buckets.
fn flush_forge_dep(
    pending: &PendingForgeDep,
    required_out: &mut std::collections::BTreeSet<String>,
    optional_out: &mut std::collections::BTreeSet<String>,
    incompatible_ids_out: &mut std::collections::BTreeSet<String>,
    incompatibility_decls_out: &mut Vec<IncompatibilityDecl>,
) {
    let dep_id = match &pending.mod_id {
        Some(id) if !id.is_empty() => id.clone(),
        _ => return, // No usable dependency id (e.g. block had no modId line).
    };

    // version_range: None / empty / "*" → unconditional (empty ranges).
    let ranges = match &pending.version_range {
        Some(r) => {
            let t = r.trim();
            if t.is_empty() || t == "*" {
                Vec::new()
            } else {
                vec![r.clone()]
            }
        }
        None => Vec::new(),
    };

    // Type takes precedence when present (NeoForge). When absent, fall back to
    // `mandatory` (traditional Forge); default required.
    let effective = pending.dep_type.as_deref();
    match effective {
        Some("incompatible") => {
            incompatible_ids_out.insert(dep_id.clone());
            incompatibility_decls_out.push(IncompatibilityDecl {
                mod_id: dep_id,
                version_ranges: ranges,
                source: IncompatibilitySource::ForgeIncompatible,
            });
        }
        Some("discouraged") => {
            incompatible_ids_out.insert(dep_id.clone());
            incompatibility_decls_out.push(IncompatibilityDecl {
                mod_id: dep_id,
                version_ranges: ranges,
                source: IncompatibilitySource::ForgeDiscouraged,
            });
        }
        Some("optional") => {
            optional_out.insert(dep_id);
        }
        Some("required") | Some(_) => {
            // Unknown type defaults to required (safe).
            required_out.insert(dep_id);
        }
        None => match pending.mandatory {
            Some(false) => {
                optional_out.insert(dep_id);
            }
            _ => {
                required_out.insert(dep_id);
            }
        },
    }
}

/// Parse a `key = "value"` / `key = value` / `key = true` line into (key, value).
/// Returns the value as a raw string (unquoted for quoted strings). Returns None
/// if not a simple `key = value` assignment.
fn parse_toml_kv(trimmed: &str) -> Option<(String, String)> {
    let eq = trimmed.find('=')?;
    let key = trimmed[..eq].trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    let raw = trimmed[eq + 1..].trim();
    let value = if let Some(rest) = raw.strip_prefix(['"', '\'']) {
        // Consume up to the matching quote.
        match rest.split(['"', '\'']).next() {
            Some(v) => v.to_string(),
            None => rest.to_string(),
        }
    } else {
        // Bare value (bool/number); strip trailing inline comments defensively.
        let v = raw.split_whitespace().next().unwrap_or("");
        v.to_string()
    };
    Some((key, value))
}

/// Parse a TOML boolean value string.
fn parse_toml_bool(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn read_entry_utf8(archive: &mut zip::ZipArchive<std::fs::File>, index: usize) -> Option<String> {
    let mut file = archive.by_index(index).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an in-memory `.jar` (zip) with the given `(entry_name, content)`
    /// pairs and write it to a unique temp file, returning the path.
    fn build_test_jar(entries: &[(&str, &str)]) -> std::path::PathBuf {
        use std::io::{Seek, Write};
        let mut file = tempfile::NamedTempFile::new().expect("create temp file");
        // ZipWriter needs the file seekable; write then rewind.
        {
            let mut zip = zip::ZipWriter::new(&file);
            let opts = zip::write::FileOptions::default();
            for (name, content) in entries {
                zip.start_file(*name, opts).expect("start_file");
                zip.write_all(content.as_bytes()).expect("write_all");
            }
            zip.finish().expect("finish zip");
        }
        file.seek(std::io::SeekFrom::Start(0)).expect("rewind");
        let (_file, path) = file.keep().expect("keep temp file");
        path
    }

    #[test]
    fn parse_jar_metadata_missing_file_returns_default() {
        let meta = parse_jar_metadata(std::path::Path::new("/nonexistent/jar.jar"));
        assert!(meta.java_packages.is_empty());
        assert!(meta.mod_jar_id.is_none());
        assert!(meta.depends_on.is_empty());
        assert!(meta.incompatibility_decls.is_empty());
    }

    #[test]
    fn extract_fabric_deps_object_form() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"fabric-api": ">=0.40.0", "minecraft": ">=1.20"}"#).unwrap();
        let mut out = std::collections::BTreeSet::new();
        extract_fabric_deps(&v, &mut out, None);
        assert!(out.contains("fabric-api"));
        assert!(out.contains("minecraft"));
    }

    #[test]
    fn extract_fabric_deps_array_form() {
        let v: serde_json::Value =
            serde_json::from_str(r#"[{"id": "sodium"}, {"identifier": "lithium"}]"#).unwrap();
        let mut out = std::collections::BTreeSet::new();
        extract_fabric_deps(&v, &mut out, None);
        assert!(out.contains("sodium"));
        assert!(out.contains("lithium"));
    }

    // -------------------------------------------------------------------
    // Fabric breaks/conflicts: severity + version capture
    // -------------------------------------------------------------------

    #[test]
    fn fabric_breaks_captured_as_hard_with_predicate() {
        let v: serde_json::Value = serde_json::from_str(r#"{"optifine": "<2.0"}"#).unwrap();
        let mut ids = std::collections::BTreeSet::new();
        let mut decls = Vec::new();
        extract_fabric_deps(
            &v,
            &mut ids,
            Some((IncompatibilitySource::FabricBreaks, &mut decls)),
        );
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].mod_id, "optifine");
        assert_eq!(decls[0].version_ranges, vec!["<2.0".to_string()]);
        assert_eq!(decls[0].source, IncompatibilitySource::FabricBreaks);
        assert!(decls[0].source.is_hard());
    }

    #[test]
    fn fabric_conflicts_is_soft() {
        let v: serde_json::Value = serde_json::from_str(r#"{"foo": "*"}"#).unwrap();
        let mut decls = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        extract_fabric_deps(
            &v,
            &mut ids,
            Some((IncompatibilitySource::FabricConflicts, &mut decls)),
        );
        assert_eq!(decls.len(), 1);
        assert!(!decls[0].source.is_hard());
        // "*" normalizes to unconditional (empty ranges).
        assert!(decls[0].version_ranges.is_empty());
    }

    #[test]
    fn fabric_array_predicates_captured_as_or() {
        let v: serde_json::Value = serde_json::from_str(r#"{"foo": ["<2.0", ">=3.0"]}"#).unwrap();
        let mut decls = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        extract_fabric_deps(
            &v,
            &mut ids,
            Some((IncompatibilitySource::FabricBreaks, &mut decls)),
        );
        assert_eq!(decls[0].version_ranges, vec!["<2.0", ">=3.0"]);
    }

    #[test]
    fn fabric_array_of_objects_form() {
        let v: serde_json::Value =
            serde_json::from_str(r#"[{"id":"foo","version":"<2.0"},{"id":"bar"}]"#).unwrap();
        let mut decls = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        extract_fabric_deps(
            &v,
            &mut ids,
            Some((IncompatibilitySource::FabricBreaks, &mut decls)),
        );
        let foo = decls.iter().find(|d| d.mod_id == "foo").unwrap();
        assert_eq!(foo.version_ranges, vec!["<2.0"]);
        let bar = decls.iter().find(|d| d.mod_id == "bar").unwrap();
        assert!(bar.version_ranges.is_empty()); // unconditional
    }

    // -------------------------------------------------------------------
    // Forge/NeoForge parsing fixes
    // -------------------------------------------------------------------

    #[test]
    fn forge_dep_block_reads_inner_modid_not_owner() {
        // Section header names the OWNER ("mymod"), but the dependency is
        // declared by the inner modId ("fabric-api").
        let toml = r#"modId="mymod"
version="1.0"

[[dependencies.mymod]]
    modId="fabric-api"
    type="required"

[[dependencies.mymod]]
    modId="sodium"
    type="optional"
"#;
        let mut required = std::collections::BTreeSet::new();
        let mut optional = std::collections::BTreeSet::new();
        let mut incompat_ids = std::collections::BTreeSet::new();
        let mut decls = Vec::new();
        let mut mod_id = None;
        extract_forge_deps(
            &toml,
            &mut required,
            &mut optional,
            &mut incompat_ids,
            &mut decls,
            &mut mod_id,
        );
        assert_eq!(mod_id, Some("mymod".to_string()));
        assert!(required.contains("fabric-api"));
        assert!(!required.contains("mymod"), "owner must NOT be its own dep");
        assert!(optional.contains("sodium"));
    }

    #[test]
    fn forge_mandatory_false_is_optional() {
        let toml = r#"[[dependencies.foo]]
    modId="bar"
    mandatory=false
"#;
        let mut required = std::collections::BTreeSet::new();
        let mut optional = std::collections::BTreeSet::new();
        let mut incompat_ids = std::collections::BTreeSet::new();
        let mut decls = Vec::new();
        let mut mod_id = None;
        extract_forge_deps(
            &toml,
            &mut required,
            &mut optional,
            &mut incompat_ids,
            &mut decls,
            &mut mod_id,
        );
        assert!(optional.contains("bar"));
        assert!(!required.contains("bar"));
    }

    #[test]
    fn forge_mandatory_true_is_required() {
        let toml = r#"[[dependencies.foo]]
    modId="bar"
    mandatory=true
"#;
        let mut required = std::collections::BTreeSet::new();
        let mut optional = std::collections::BTreeSet::new();
        let mut incompat_ids = std::collections::BTreeSet::new();
        let mut decls = Vec::new();
        let mut mod_id = None;
        extract_forge_deps(
            &toml,
            &mut required,
            &mut optional,
            &mut incompat_ids,
            &mut decls,
            &mut mod_id,
        );
        assert!(required.contains("bar"));
    }

    #[test]
    fn forge_incompatible_uses_target_id_and_captures_version_range() {
        let toml = r#"[[dependencies.mymod]]
    modId="optifine"
    type="incompatible"
    versionRange="[1.0,2.0)"
"#;
        let mut required = std::collections::BTreeSet::new();
        let mut optional = std::collections::BTreeSet::new();
        let mut incompat_ids = std::collections::BTreeSet::new();
        let mut decls = Vec::new();
        let mut mod_id = None;
        extract_forge_deps(
            &toml,
            &mut required,
            &mut optional,
            &mut incompat_ids,
            &mut decls,
            &mut mod_id,
        );
        assert!(incompat_ids.contains("optifine"));
        assert!(
            !incompat_ids.contains("mymod"),
            "owner must not self-conflict"
        );
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].mod_id, "optifine");
        assert_eq!(decls[0].version_ranges, vec!["[1.0,2.0)".to_string()]);
        assert_eq!(decls[0].source, IncompatibilitySource::ForgeIncompatible);
    }

    #[test]
    fn forge_discouraged_is_soft() {
        let toml = r#"[[dependencies.mymod]]
    modId="bar"
    type="discouraged"
"#;
        let mut required = std::collections::BTreeSet::new();
        let mut optional = std::collections::BTreeSet::new();
        let mut incompat_ids = std::collections::BTreeSet::new();
        let mut decls = Vec::new();
        let mut mod_id = None;
        extract_forge_deps(
            &toml,
            &mut required,
            &mut optional,
            &mut incompat_ids,
            &mut decls,
            &mut mod_id,
        );
        assert_eq!(decls.len(), 1);
        assert!(!decls[0].source.is_hard());
    }

    #[test]
    fn forge_empty_version_range_is_unconditional() {
        let toml = r#"[[dependencies.mymod]]
    modId="bar"
    type="incompatible"
    versionRange=""
"#;
        let mut required = std::collections::BTreeSet::new();
        let mut optional = std::collections::BTreeSet::new();
        let mut incompat_ids = std::collections::BTreeSet::new();
        let mut decls = Vec::new();
        let mut mod_id = None;
        extract_forge_deps(
            &toml,
            &mut required,
            &mut optional,
            &mut incompat_ids,
            &mut decls,
            &mut mod_id,
        );
        assert!(
            decls[0].version_ranges.is_empty(),
            "empty range = unconditional"
        );
    }

    #[test]
    fn forge_dep_block_without_modid_is_skipped() {
        // A block with no inner modId yields NO dependency entry (previously the
        // owner id from the header was used — the core bug).
        let toml = r#"[[dependencies.someowner]]
    type="required"
"#;
        let mut required = std::collections::BTreeSet::new();
        let mut optional = std::collections::BTreeSet::new();
        let mut incompat_ids = std::collections::BTreeSet::new();
        let mut decls = Vec::new();
        let mut mod_id = None;
        extract_forge_deps(
            &toml,
            &mut required,
            &mut optional,
            &mut incompat_ids,
            &mut decls,
            &mut mod_id,
        );
        assert!(required.is_empty());
        assert!(incompat_ids.is_empty());
        assert!(decls.is_empty());
    }

    // -------------------------------------------------------------------
    // Full JAR parse (zip fixture) end-to-end
    // -------------------------------------------------------------------

    #[test]
    fn parse_jar_fabric_breaks_and_conflicts() {
        let jar = build_test_jar(&[(
            "fabric.mod.json",
            r#"{"id":"mod_a","breaks":{"bad":"<2.0"},"conflicts":{"iffy":"*"}, "depends":{"fabric-api":">=0.40"}}"#,
        )]);
        let meta = parse_jar_metadata(&jar);
        let _ = std::fs::remove_file(&jar);
        assert_eq!(meta.mod_jar_id.as_deref(), Some("mod_a"));
        assert!(meta.depends_on.contains(&"fabric-api".to_string()));
        // Both breaks+conflicts targets in the flat id list.
        assert!(meta.incompatible_deps.contains(&"bad".to_string()));
        assert!(meta.incompatible_deps.contains(&"iffy".to_string()));
        let bad = meta
            .incompatibility_decls
            .iter()
            .find(|d| d.mod_id == "bad")
            .unwrap();
        assert_eq!(bad.source, IncompatibilitySource::FabricBreaks);
        assert_eq!(bad.version_ranges, vec!["<2.0".to_string()]);
        let iffy = meta
            .incompatibility_decls
            .iter()
            .find(|d| d.mod_id == "iffy")
            .unwrap();
        assert_eq!(iffy.source, IncompatibilitySource::FabricConflicts);
        assert!(iffy.version_ranges.is_empty()); // "*" → unconditional
    }

    #[test]
    fn parse_jar_neoforge_mods_toml_parsed_and_inner_modid_read() {
        let jar = build_test_jar(&[
            (
                "META-INF/neoforge.mods.toml",
                "modId=\"neomod\"\n\n[[dependencies.neomod]]\n    modId=\"optifine\"\n    type=\"incompatible\"\n",
            ),
        ]);
        let meta = parse_jar_metadata(&jar);
        let _ = std::fs::remove_file(&jar);
        assert_eq!(meta.mod_jar_id.as_deref(), Some("neomod"));
        assert!(meta.incompatible_deps.contains(&"optifine".to_string()));
        assert!(
            !meta.incompatible_deps.contains(&"neomod".to_string()),
            "owner must not self-conflict"
        );
        let optifine = meta
            .incompatibility_decls
            .iter()
            .find(|d| d.mod_id == "optifine")
            .expect("optifine decl");
        assert_eq!(optifine.source, IncompatibilitySource::ForgeIncompatible);
    }

    #[test]
    fn parse_jar_forge_self_conflict_bug_fixed() {
        // Real-world shape that previously produced a self-conflict blocker:
        // a mod declaring an incompatible dependency used to land the OWNER id.
        let jar = build_test_jar(&[
            (
                "META-INF/mods.toml",
                "modId=\"examplemod\"\n[[dependencies.examplemod]]\n    modId=\"othermod\"\n    type=\"incompatible\"\n",
            ),
        ]);
        let meta = parse_jar_metadata(&jar);
        let _ = std::fs::remove_file(&jar);
        assert!(meta.incompatible_deps.contains(&"othermod".to_string()));
        assert!(
            !meta.incompatible_deps.contains(&"examplemod".to_string()),
            "examplemod must not appear incompatible with itself"
        );
    }

    #[test]
    fn parse_jar_forge_neoforge_loader_deps_ignored() {
        // A mod declaring a required dep on the "neoforge" loader must NOT
        // produce a missing-required-dependency blocker.
        let jar = build_test_jar(&[
            (
                "META-INF/neoforge.mods.toml",
                "modId=\"m\"\n[[dependencies.m]]\n    modId=\"neoforge\"\n    type=\"required\"\n[[dependencies.m]]\n    modId=\"realdep\"\n    type=\"required\"\n",
            ),
        ]);
        let meta = parse_jar_metadata(&jar);
        let _ = std::fs::remove_file(&jar);
        assert!(!meta.depends_on.contains(&"neoforge".to_string()));
        assert!(meta.depends_on.contains(&"realdep".to_string()));
    }
}
