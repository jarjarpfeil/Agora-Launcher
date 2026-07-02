use crate::dependency_ops::JarDeps;
use std::io::Read;
use std::path::Path;

const DEPENDENCY_IGNORE_LIST: &[&str] = &["minecraft", "fabricloader", "quilt_loader", "java"];

/// Parse a `.jar` file to extract Java packages, mod ID, and declared
/// dependencies from `fabric.mod.json` / `META-INF/mods.toml`.
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
    let mut incompatible_deps: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut forge_mod_id: Option<String> = None;

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
                    for key in ["depends", "recommends", "suggests", "breaks", "conflicts"] {
                        if let Some(val) = value.get(key) {
                            let out = match key {
                                "depends" => &mut depends_on,
                                "recommends" | "suggests" => &mut optional_deps,
                                "breaks" | "conflicts" => &mut incompatible_deps,
                                _ => unreachable!(),
                            };
                            extract_fabric_deps(val, out);
                        }
                    }
                }
            }
            continue;
        }
        if name == "META-INF/mods.toml" {
            if let Some(content) = read_entry_utf8(&mut archive, i) {
                extract_forge_deps(
                    &content,
                    &mut depends_on,
                    &mut optional_deps,
                    &mut incompatible_deps,
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
    incompatible_deps.retain(|dep| !DEPENDENCY_IGNORE_LIST.contains(&dep.as_str()));

    JarDeps {
        java_packages: packages.into_iter().collect(),
        mod_jar_id,
        depends_on: depends_on.into_iter().collect(),
        optional_deps: optional_deps.into_iter().collect(),
        incompatible_deps: incompatible_deps.into_iter().collect(),
    }
}

fn extract_fabric_deps(depends: &serde_json::Value, out: &mut std::collections::BTreeSet<String>) {
    match depends {
        serde_json::Value::Object(map) => {
            for key in map.keys() {
                out.insert(key.clone());
            }
        }
        serde_json::Value::Array(arr) => {
            for elem in arr {
                if let Some(id) = elem.get("id").and_then(|v| v.as_str()) {
                    out.insert(id.to_string());
                } else if let Some(id) = elem.get("identifier").and_then(|v| v.as_str()) {
                    out.insert(id.to_string());
                }
            }
        }
        _ => {}
    }
}

fn extract_forge_deps(
    content: &str,
    required_out: &mut std::collections::BTreeSet<String>,
    optional_out: &mut std::collections::BTreeSet<String>,
    incompatible_out: &mut std::collections::BTreeSet<String>,
    mod_id_out: &mut Option<String>,
) {
    let mut current_dep_id: Option<String> = None;
    let mut current_type: Option<String> = None;
    let mut in_dep_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(dep_id) = trimmed.strip_prefix("[[dependencies.") {
            flush_forge(
                current_dep_id.take(),
                current_type.as_deref(),
                required_out,
                optional_out,
                incompatible_out,
            );
            current_type = None;
            in_dep_block = false;
            if let Some(end) = dep_id.find(']') {
                let block_key = &dep_id[..end];
                if !block_key.is_empty() {
                    in_dep_block = true;
                    current_dep_id = Some(block_key.to_string());
                }
            }
            continue;
        }
        if trimmed.starts_with("[[") {
            flush_forge(
                current_dep_id.take(),
                current_type.as_deref(),
                required_out,
                optional_out,
                incompatible_out,
            );
            current_type = None;
            in_dep_block = false;
            continue;
        }
        if in_dep_block {
            if let Some(rest) = trimmed.strip_prefix("type") {
                let rest = rest.trim_start();
                if rest.starts_with('=') {
                    let rest = rest[1..].trim();
                    if let Some(val) = rest
                        .strip_prefix(['"', '\''])
                        .and_then(|s| s.split(['"', '\'']).next())
                    {
                        current_type = Some(val.to_string());
                    }
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix("modId") {
            let rest = rest.trim_start();
            if rest.starts_with('=') {
                let rest = rest[1..].trim();
                if let Some(id) = rest
                    .strip_prefix(['"', '\''])
                    .and_then(|s| s.split(['"', '\'']).next())
                {
                    if !in_dep_block && mod_id_out.is_none() {
                        *mod_id_out = Some(id.to_string());
                    }
                }
            }
        }
    }
    flush_forge(
        current_dep_id,
        current_type.as_deref(),
        required_out,
        optional_out,
        incompatible_out,
    );
}

fn flush_forge(
    dep_id: Option<String>,
    type_str: Option<&str>,
    required_out: &mut std::collections::BTreeSet<String>,
    optional_out: &mut std::collections::BTreeSet<String>,
    incompatible_out: &mut std::collections::BTreeSet<String>,
) {
    if let Some(dep_id_val) = dep_id {
        match type_str.unwrap_or("required") {
            "required" => { required_out.insert(dep_id_val); },
            "optional" => { optional_out.insert(dep_id_val); },
            "incompatible" => { incompatible_out.insert(dep_id_val); },
            _ => { optional_out.insert(dep_id_val); },
        };
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

    #[test]
    fn parse_jar_metadata_missing_file_returns_default() {
        let meta = parse_jar_metadata(std::path::Path::new("/nonexistent/jar.jar"));
        assert!(meta.java_packages.is_empty());
        assert!(meta.mod_jar_id.is_none());
        assert!(meta.depends_on.is_empty());
    }

    #[test]
    fn extract_fabric_deps_object_form() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"fabric-api": ">=0.40.0", "minecraft": ">=1.20"}"#).unwrap();
        let mut out = std::collections::BTreeSet::new();
        extract_fabric_deps(&v, &mut out);
        assert!(out.contains("fabric-api"));
        assert!(out.contains("minecraft"));
    }

    #[test]
    fn extract_fabric_deps_array_form() {
        let v: serde_json::Value =
            serde_json::from_str(r#"[{"id": "sodium"}, {"identifier": "lithium"}]"#).unwrap();
        let mut out = std::collections::BTreeSet::new();
        extract_fabric_deps(&v, &mut out);
        assert!(out.contains("sodium"));
        assert!(out.contains("lithium"));
    }
}
