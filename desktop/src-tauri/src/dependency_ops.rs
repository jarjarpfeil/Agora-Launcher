//! Desktop shim for dependency resolution.
//!
//! Re-exports all types and functions from `agora_core::dependency_ops` and
//! bridges the desktop `crash_investigator::JarMetadata` to core's `JarDeps`.
//!
//! The three plan-building functions that callers in `commands.rs` use accept
//! the original desktop `JarMetadata` type so no caller code needs to change.

use crate::crash_investigator::JarMetadata;
use agora_core::dependency_ops::JarDeps;

// ---------------------------------------------------------------------------
// 1. Re-export all public types from core
// ---------------------------------------------------------------------------

pub use agora_core::dependency_ops::{
    AliasMap, DepCandidate, DepConflict, DepSource, DependentInfo, DisablePlan,
    InstallPlan, RemovalPlan, Requirement, ResolvedInstallDeps,
};

// ---------------------------------------------------------------------------
// 2. Bridge: crash_investigator::JarMetadata → core::JarDeps
// ---------------------------------------------------------------------------

impl From<JarMetadata> for JarDeps {
    fn from(jm: JarMetadata) -> Self {
        Self {
            java_packages: jm.java_packages,
            mod_jar_id: jm.mod_jar_id,
            depends_on: jm.depends_on,
            optional_deps: jm.optional_deps,
            incompatible_deps: jm.incompatible_deps,
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Re-export core functions that callers reference
// ---------------------------------------------------------------------------

pub use agora_core::dependency_ops::{
    build_install_plan_with_aliases, build_removal_plan_with_aliases,
    build_disable_plan_with_aliases, detect_source_disagreement,
    find_dependents, find_dependents_with_aliases, resolve_install_deps,
    resolve_install_deps_with_aliases,
};

// ---------------------------------------------------------------------------
// 4. Desktop-specific wrappers preserving original signatures
// ---------------------------------------------------------------------------

/// Build a disable plan for a target mod.
///
/// Preserves the original signature used by `commands::get_disable_plan`.
pub fn build_disable_plan(
    installed: &[crate::models::InstalledMod],
    target: &crate::models::InstalledMod,
) -> DisablePlan {
    agora_core::dependency_ops::build_disable_plan(installed, target)
}

/// Build a removal plan for a target mod.
///
/// Preserves the original signature used by `commands::get_removal_plan`.
pub fn build_removal_plan(
    installed: &[crate::models::InstalledMod],
    target: &crate::models::InstalledMod,
) -> RemovalPlan {
    agora_core::dependency_ops::build_removal_plan(installed, target)
}

/// Build an install plan for a target mod.
///
/// Accepts the desktop `JarMetadata` (from `crash_investigator::parse_jar_metadata`)
/// and converts it to core's `JarDeps` before delegating.
pub fn build_install_plan(
    target_manifest_deps: Option<crate::registry::ManifestDeps>,
    target_jar_deps: &JarMetadata,
    installed: &[crate::models::InstalledMod],
) -> InstallPlan {
    let jar_deps: JarDeps = target_jar_deps.clone().into();
    agora_core::dependency_ops::build_install_plan(target_manifest_deps, &jar_deps, installed)
}
