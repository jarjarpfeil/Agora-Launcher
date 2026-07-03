//! Hardware-adaptive JVM Garbage Collection architect (Phase 5).
//!
//! Replaces the raw "JVM args" string box with an intelligent GC tuning wizard.
//! The backend queries total system RAM + CPU threads via `sysinfo`, reads the
//! instance's target JRE version, and computes optimal GC flags.
//!
//! Three engines:
//! - **Low-Latency** (Java 21 Generational ZGC): `-XX:+UseZGC -XX:+ZGenerational`
//! - **High-Efficiency** (Aikar's G1GC derivation): dynamically sized from RAM
//! - **Manual**: advanced users edit raw flags (Phase 10 Advanced toggle)
//!
//! Heap allocation has safe OS-headroom guardrails (never >75% of detected RAM).

use serde::{Deserialize, Serialize};

/// GC profile selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GcProfile {
    /// Java 21 Generational ZGC — minimal stutter on high-RAM allocations.
    LowLatency,
    /// Aikar's G1GC derivation — high throughput for modded Minecraft.
    HighEfficiency,
    /// User-supplied raw flags (Advanced mode).
    Manual,
}

impl GcProfile {
    pub fn recommended_for_java_version(java_version: u32) -> Self {
        if java_version >= 21 {
            GcProfile::LowLatency
        } else {
            GcProfile::HighEfficiency
        }
    }
}

/// System hardware snapshot for GC tuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemResources {
    pub total_ram_mb: u64,
    pub cpu_threads: usize,
}

impl SystemResources {
    pub fn detect() -> Self {
        use sysinfo::System;
        let mut sys = System::new_all();
        sys.refresh_all();
        let total_ram_mb = sys.total_memory() / 1024; // sysinfo returns KB
        let cpu_threads = sys.cpus().len();
        SystemResources { total_ram_mb, cpu_threads }
    }
}

/// Safe heap allocation with OS-headroom guardrails.
///
/// Returns the recommended `-Xmx` value in MB. Never exceeds 75% of total RAM.
/// Defaults to 4096 MB if detection fails or RAM is very low.
pub fn safe_heap_mb(requested_mb: i64, total_ram_mb: u64) -> i64 {
    if total_ram_mb == 0 {
        return 4096;
    }
    let max_allowed = (total_ram_mb as f64 * 0.75) as i64;
    let min_recommended = 2048i64;

    requested_mb
        .max(min_recommended)
        .min(max_allowed)
        .min(32768) // hard cap at 32GB
}

/// Generate the full JVM argument string for a given GC profile + heap.
///
/// The output is space-separated flags ready for `java -Xmx... -XX...`.
pub fn generate_args(profile: GcProfile, heap_mb: i64, manual_args: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Memory
    parts.push(format!("-Xmx{}M", heap_mb));
    parts.push(format!("-Xms{}M", heap_mb));

    match profile {
        GcProfile::LowLatency => {
            // Java 21 Generational ZGC
            parts.push("-XX:+UseZGC".into());
            parts.push("-XX:+ZGenerational".into());
            parts.push("-XX:+AlwaysPreTouch".into());
        }
        GcProfile::HighEfficiency => {
            // Aikar's G1GC flags, dynamically sized
            parts.push("-XX:+UseG1GC".into());
            parts.push(format!("-XX:G1HeapRegionSize={}", g1_region_size(heap_mb)));
            parts.push("-XX:MaxGCPauseMillis=50".into());
            parts.push("-XX:G1ReservePercent=20".into());
            parts.push("-XX:InitiatingHeapOccupancyPercent=20".into());
            parts.push("-XX:SurvivorRatio=32".into());
            parts.push("-XX:+PerfDisableSharedMem".into());
            parts.push("-XX:MaxTenuringThreshold=1".into());
            parts.push("-XX:+AlwaysPreTouch".into());
            parts.push("-XX:+ParallelRefProcEnabled".into());
            parts.push("-XX:+UseStringDeduplication".into());
        }
        GcProfile::Manual => {
            if !manual_args.trim().is_empty() {
                parts.push(manual_args.trim().to_string());
            }
        }
    }

    parts.join(" ")
}

/// Compute G1 heap region size based on total heap (Aikar's derivation).
fn g1_region_size(heap_mb: i64) -> String {
    // Region sizes: 1, 2, 4, 8, 16, 32 MB
    // Aikar's rules: <4GB→2MB, 4-32GB→16MB+ based on size
    let region = if heap_mb >= 32768 {
        32
    } else if heap_mb >= 16384 {
        16
    } else if heap_mb >= 8192 {
        8
    } else if heap_mb >= 4096 {
        4
    } else {
        2
    };
    format!("{}M", region)
}

/// Full GC tuning result — what the Java & args tab renders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcResult {
    pub profile: GcProfile,
    pub jvm_args: String,
    pub heap_mb: i64,
    pub total_ram_mb: u64,
    pub cpu_threads: usize,
    pub recommended: bool,
}

/// Compute the full GC result for an instance.
///
/// `java_version` is the major version (8, 17, 21, etc.).
/// `requested_heap_mb` is the user's slider value (or 0 for auto).
/// `manual_args` is the raw string for Manual mode.
pub fn compute_gc(
    java_version: u32,
    requested_heap_mb: i64,
    manual_args: &str,
    override_profile: Option<GcProfile>,
) -> GcResult {
    let sys = SystemResources::detect();
    let heap = if requested_heap_mb > 0 {
        safe_heap_mb(requested_heap_mb, sys.total_ram_mb)
    } else {
        // Auto: 4GB default, capped at 75% of system RAM
        safe_heap_mb(4096, sys.total_ram_mb)
    };

    let profile = override_profile
        .unwrap_or_else(|| GcProfile::recommended_for_java_version(java_version));

    let jvm_args = generate_args(profile, heap, manual_args);

    GcResult {
        profile,
        jvm_args,
        heap_mb: heap,
        total_ram_mb: sys.total_ram_mb,
        cpu_threads: sys.cpu_threads,
        recommended: override_profile.is_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zgc_recommended_for_java_21() {
        assert_eq!(GcProfile::recommended_for_java_version(21), GcProfile::LowLatency);
        assert_eq!(GcProfile::recommended_for_java_version(22), GcProfile::LowLatency);
    }

    #[test]
    fn g1gc_recommended_for_java_17() {
        assert_eq!(GcProfile::recommended_for_java_version(17), GcProfile::HighEfficiency);
        assert_eq!(GcProfile::recommended_for_java_version(8), GcProfile::HighEfficiency);
    }

    #[test]
    fn safe_heap_never_exceeds_75_percent() {
        assert!(safe_heap_mb(16384, 8192) <= 6144); // 75% of 8GB
    }

    #[test]
    fn safe_heap_falls_back_on_zero_ram() {
        assert_eq!(safe_heap_mb(8192, 0), 4096);
    }

    #[test]
    fn safe_heap_floor_is_2048() {
        assert!(safe_heap_mb(256, 16384) >= 2048);
    }

    #[test]
    fn g1_region_size_scales_with_heap() {
        assert_eq!(g1_region_size(2048), "2M");
        assert_eq!(g1_region_size(4096), "4M");
        assert_eq!(g1_region_size(8192), "8M");
        assert_eq!(g1_region_size(16384), "16M");
        assert_eq!(g1_region_size(32768), "32M");
    }

    #[test]
    fn zgc_args_include_generational_flag() {
        let args = generate_args(GcProfile::LowLatency, 4096, "");
        assert!(args.contains("ZGenerational"));
        assert!(args.contains("AlwaysPreTouch"));
    }

    #[test]
    fn g1gc_args_include_aikar_flags() {
        let args = generate_args(GcProfile::HighEfficiency, 8192, "");
        assert!(args.contains("G1GC"));
        assert!(args.contains("MaxGCPauseMillis=50"));
        assert!(args.contains("ParallelRefProcEnabled"));
    }

    #[test]
    fn manual_args_passthrough() {
        let args = generate_args(GcProfile::Manual, 4096, "-XX:+UseShenandoahGC -Xss1M");
        assert!(args.contains("ShenandoahGC"));
        assert!(args.contains("-Xss1M"));
    }

    #[test]
    fn compute_gc_auto_selects_profile() {
        let result = compute_gc(21, 0, "", None);
        assert_eq!(result.profile, GcProfile::LowLatency);
        assert!(result.recommended);
    }
}
