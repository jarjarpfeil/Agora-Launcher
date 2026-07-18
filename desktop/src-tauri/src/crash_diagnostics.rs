//! Crash diagnostics shim â€” preserves all original public signatures while
//! delegating to `agora_core::crash_service::CrashService` for the actual
//! logic. Direct `local_state.db` / `registry.db` opens have been migrated
//! into `CrashService`; this module only adapts the Tauri `AppHandle` to
//! the core `Ctx`.

use crate::error::LauncherResult;
use agora_core::crash_diagnostics as core;
pub use agora_core::crash_diagnostics::{CrashReportInfo, CrashTriageResult, MAX_REGEX_LEN};
use agora_core::crash_service::CrashService;

/// Check whether a fresh crash report appeared after the instance's
/// `last_launched_at`. Returns the newest qualifying file.
pub fn check_for_crash<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: &str,
) -> LauncherResult<Option<CrashReportInfo>> {
    let ctx = crate::core_context(app)?;
    CrashService::new(ctx).check_for_crash(instance_id)
}

/// Triage a crash log against curated signatures.
pub fn triage_crash<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: &str,
    filename: &str,
) -> LauncherResult<CrashTriageResult> {
    let ctx = crate::core_context(app)?;
    CrashService::new(ctx).triage_crash(instance_id, filename)
}

/// List all crash report files for an instance with modification times and sizes.
pub fn list_crash_reports<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: &str,
) -> LauncherResult<Vec<CrashReportInfo>> {
    let ctx = crate::core_context(app)?;
    CrashService::new(ctx).list_reports(instance_id)
}

/// Read the content of a specific crash report file.
pub fn read_crash_log<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: &str,
    filename: &str,
) -> LauncherResult<String> {
    let ctx = crate::core_context(app)?;
    CrashService::new(ctx).read_crash_log(instance_id, filename)
}

/// Pure regex matching helper â€” compiles a pattern and checks if it matches
/// the given text. Returns `false` for invalid patterns or non-matches.
pub fn match_signature(pattern: &str, crash_text: &str) -> bool {
    core::match_signature(pattern, crash_text)
}

/// Check whether a regex pattern exceeds the MAX_REGEX_LEN guard.
pub fn is_regex_too_long(pattern: &str) -> bool {
    core::is_regex_too_long(pattern)
}

/// List crash report `.txt` files from a directory path, returning sorted
/// (newest first) `[CrashReportInfo]`. Returns an empty vec when the
/// directory does not exist or cannot be read.
pub fn list_crash_reports_from_dir(dir: &std::path::Path) -> Vec<CrashReportInfo> {
    core::list_reports_from_dir(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Regex matching ---

    #[test]
    fn test_regex_matches_known_crash() {
        let pattern = "Mixin apply failed";
        let crash_text =
            "[06:12:33] [Worker-3/FABRIC]: Mixin apply failed mixins.fabric.json:debug.mixins.json:DebugMixin -> org.example.Mod: java/lang/RuntimeException";
        assert!(match_signature(pattern, crash_text));
    }

    #[test]
    fn test_regex_no_match_unrelated() {
        let pattern = "Mixin apply failed";
        let unrelated = "Game loaded successfully with 42 mods active.";
        assert!(!match_signature(pattern, unrelated));
    }

    #[test]
    fn test_regex_no_match_empty() {
        let pattern = "java\\.lang\\.OutOfMemoryError";
        assert!(!match_signature(pattern, ""));
    }

    #[test]
    fn test_regex_no_match_malformed() {
        let pattern = "java\\.lang\\.OutOfMemoryError";
        let garbage = "x\x00y\x01z\x02garbage";
        assert!(!match_signature(pattern, garbage));
    }

    // --- Crash report discovery ---

    #[test]
    fn test_list_crash_reports_finds_txt() {
        let tmp =
            std::env::temp_dir().join(format!("agora_test_crash_reports_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("crash-1.txt"), "crash data 1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(tmp.join("crash-2.txt"), "crash data 2").unwrap();

        let reports = list_crash_reports_from_dir(&tmp);
        assert_eq!(reports.len(), 2);
        let names: Vec<&str> = reports.iter().map(|r| r.filename.as_str()).collect();
        assert!(names.contains(&"crash-2.txt"));
        assert!(names.contains(&"crash-1.txt"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_list_crash_reports_empty_dir() {
        let tmp =
            std::env::temp_dir().join(format!("agora_test_crash_empty_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        let reports = list_crash_reports_from_dir(&tmp);
        assert!(reports.is_empty());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_list_crash_reports_nonexistent_dir() {
        let tmp = std::env::temp_dir().join(format!(
            "agora_test_crash_missing_{}_nonexistent",
            std::process::id()
        ));
        let reports = list_crash_reports_from_dir(&tmp);
        assert!(reports.is_empty());
    }

    // --- MAX_REGEX_LEN guard ---

    #[test]
    fn test_max_regex_len_rejects_long() {
        let long_pattern = "a".repeat(257);
        assert!(is_regex_too_long(&long_pattern));
    }

    #[test]
    fn test_max_regex_len_accepts_short() {
        let short_pattern = "java\\.lang\\.OutOfMemoryError";
        assert!(!is_regex_too_long(short_pattern));
    }

    // --- Struct serialization ---

    #[test]
    fn test_crash_report_info_serializes() {
        let info = CrashReportInfo {
            filename: "crash-1.txt".to_string(),
            modified_at: "2024-01-15T10:30:00Z".to_string(),
            size_bytes: 4096,
        };
        let json = serde_json::to_string(&info).expect("should serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");
        assert_eq!(parsed["filename"], "crash-1.txt");
        assert_eq!(parsed["modified_at"], "2024-01-15T10:30:00Z");
        assert_eq!(parsed["size_bytes"], 4096);
    }
}
