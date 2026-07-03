//! Crash diagnostics shim — preserves all original public signatures while
//! delegating to `agora_core::crash_diagnostics` for the actual logic.
//!
//! Phase 3: triage works with ZERO `registry.db` dependency. The shim passes
//! `None` for the DB connection when `registry_connection` fails, ensuring
//! crash-triage succeeds even when the registry database is absent.

use crate::db;
use crate::error::{LauncherError, LauncherResult};
use crate::paths;
pub use agora_core::crash_diagnostics::{
    CrashReportInfo, CrashTriageResult, MAX_REGEX_LEN,
};
use agora_core::crash_diagnostics as core;

/// Check whether a fresh crash report appeared after the instance's
/// `last_launched_at`. Returns the newest qualifying file.
///
/// Reads `last_launched_at` from `local_state.db`, lists files in
/// `instances/<id>/crash-reports/`, and returns the newest file whose mtime is
/// strictly newer than `last_launched_at`. If the instance was never launched
/// or no newer crash report exists, returns `None`.
pub fn check_for_crash<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: &str,
) -> LauncherResult<Option<CrashReportInfo>> {
    let sanitized = paths::sanitize_id(instance_id);

    let conn = db::local_state_connection(app).map_err(|_| LauncherError::LocalStateFailed)?;
    let row = db::get_instance(&conn, &sanitized)
        .map_err(|_| LauncherError::LocalStateFailed)?;

    let last_launched_at = match row.and_then(|r| r.last_launched_at) {
        Some(ts) => ts,
        None => return Ok(None),
    };

    let dir = match paths::instance_dir(app, &sanitized) {
        Ok(d) => d.join("crash-reports"),
        Err(_) => return Ok(None),
    };

    core::check_for_crash_from_path(&dir, &last_launched_at)
}

/// Triage a crash log against curated signatures.
///
/// Phase 3: uses the embedded signature corpus by default. If `registry.db`
/// is present and contains the `crash_signatures` table, runtime-added
/// signatures are also checked. Triage succeeds even when `registry.db`
/// is absent.
pub fn triage_crash<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: &str,
    filename: &str,
) -> LauncherResult<CrashTriageResult> {
    let sanitized = paths::sanitize_id(instance_id);
    let reports_dir = match paths::instance_dir(app, &sanitized) {
        Ok(d) => d.join("crash-reports"),
        Err(_) => return Ok(CrashTriageResult::no_match()),
    };
    let safe_name = std::path::Path::new(filename)
        .file_name()
        .ok_or_else(|| LauncherError::Generic {
            code: "ERR_CRASH_LOG_PATH".to_string(),
            message: "Invalid crash log filename.".to_string(),
        })?
        .to_string_lossy()
        .to_string();
    let crash_path = reports_dir.join(&safe_name);

    let text = match std::fs::read_to_string(&crash_path) {
        Ok(t) => t,
        Err(_) => return Ok(CrashTriageResult::no_match()),
    };

    // Open registry connection optionally — if it fails, triage still works
    // against the embedded corpus (Phase 3 property).
    let conn_opt = crate::db::registry_connection(app).ok();
    Ok(core::triage_with_db(&text, conn_opt.as_ref()))
}

/// List all crash report files for an instance with modification times and sizes.
pub fn list_crash_reports<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: &str,
) -> LauncherResult<Vec<CrashReportInfo>> {
    let sanitized = paths::sanitize_id(instance_id);
    let dir = match paths::instance_dir(app, &sanitized) {
        Ok(d) => d.join("crash-reports"),
        Err(_) => return Ok(Vec::new()),
    };
    Ok(core::list_reports_from_dir(&dir))
}

/// Read the content of a specific crash report file.
pub fn read_crash_log<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: &str,
    filename: &str,
) -> LauncherResult<String> {
    let sanitized = paths::sanitize_id(instance_id);
    let reports_dir = match paths::instance_dir(app, &sanitized) {
        Ok(d) => d.join("crash-reports"),
        Err(_) => return Err(LauncherError::Generic {
            code: "ERR_CRASH_LOG_READ".to_string(),
            message: "Could not read the crash log file.".to_string(),
        }),
    };
    let safe_name = std::path::Path::new(filename)
        .file_name()
        .ok_or_else(|| LauncherError::Generic {
            code: "ERR_CRASH_LOG_PATH".to_string(),
            message: "Invalid crash log filename.".to_string(),
        })?
        .to_string_lossy()
        .to_string();
    let path = reports_dir.join(&safe_name);
    core::read_crash_log_from_path(&path)
}

/// Pure regex matching helper — compiles a pattern and checks if it matches
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
        let tmp = std::env::temp_dir().join(format!(
            "agora_test_crash_reports_{}",
            std::process::id()
        ));
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
        let tmp = std::env::temp_dir().join(format!(
            "agora_test_crash_empty_{}",
            std::process::id()
        ));
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

/// Pure regex matching: compile a single crash-signature pattern and check
/// whether it matches the given text. Mirrors the core logic inside
/// `triage_crash` without requiring AppHandle or a database connection.
pub fn match_signature(pattern: &str, text: &str) -> bool {
    if pattern.chars().count() > MAX_REGEX_LEN {
        return false;
    }
    regex::Regex::new(pattern).map(|re| re.is_match(text)).unwrap_or(false)
}

/// Pure file-scanning: list `.txt` files in a directory with metadata,
/// sorted by modification time descending. Mirrors the core loop inside
/// `list_crash_reports` and `check_for_crash` without AppHandle.
pub fn list_crash_report_files(dir: &std::path::Path) -> Vec<CrashReportInfo> {
    if !dir.exists() {
        return Vec::new();
    }
    let mut out: Vec<(CrashReportInfo, std::time::SystemTime)> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    for entry in entries.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let mtime = match meta.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let filename = entry.file_name().to_string_lossy().to_string();
        out.push((
            CrashReportInfo {
                filename,
                modified_at: system_time_to_rfc3339(mtime),
                size_bytes: meta.len(),
            },
            mtime,
        ));
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out.into_iter().map(|(info, _)| info).collect()
}

/// Pure mtime comparison: returns true if `file_time` is strictly after
/// `reference_time`. Used by `check_for_crash` to detect new crash reports.
pub fn is_newer_than(file_time: std::time::SystemTime, reference_time: std::time::SystemTime) -> bool {
    file_time > reference_time
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Regex signature matching (pure) ──────────────────────────────

    /// Test that the OOM signature regex matches a real crash-log snippet.
    #[test]
    fn test_regex_matches_known_crash() {
        let pattern = "java\\.lang\\.OutOfMemoryError";
        let crash_log = "Caused by: java.lang.OutOfMemoryError: Java heap space\n\tat java.util.Arrays.copyOf(Arrays.java:3210)";
        assert!(match_signature(pattern, crash_log));
    }

    /// Test that the same regex does NOT match an unrelated log.
    #[test]
    fn test_regex_no_match_unrelated() {
        let pattern = "java\\.lang\\.OutOfMemoryError";
        let unrelated = "Loading 42 mods fabric-api-0.92.0\nInitialization complete.";
        assert!(!match_signature(pattern, unrelated));
    }

    /// Test that an empty string never triggers a match (no panic).
    #[test]
    fn test_regex_no_match_empty_string() {
        let pattern = "java\\.lang\\.OutOfMemoryError";
        assert!(!match_signature(pattern, ""));
    }

    /// Test that garbage / non-UTF8-like input does not panic.
    #[test]
    fn test_regex_no_match_malformed() {
        let pattern = "java\\.lang\\.OutOfMemoryError";
        // Build a string with embedded null bytes — realistic for corrupted logs.
        let malformed = "garbage\x00\x01\x02 data";
        assert!(!match_signature(pattern, malformed));
    }

    /// Test multiple signatures: the correct one matches among several.
    #[test]
    fn test_multiple_signatures_correct_match() {
        let signatures = vec![
            ("java\\.lang\\.OutOfMemoryError", "Out of Memory"),
            ("Mixin apply failed", "Mixin Injection Conflict"),
            ("ModResolutionException|incompatible mod set", "Mod Resolution Failure"),
            ("requires \\{fabric @", "Missing Fabric API"),
        ];

        let oom_log = "java.lang.OutOfMemoryError: GC overhead limit exceeded";
        let mixin_log = "[12:00:00] [main/FATAL]: Mixin apply failed mixin.fabric.json:client.mixins.json — you need MixinExtras";
        let resolution_log = "net.fabricmc.loader.impl.FormattedException: ModResolutionException: Mod 'foo' requires bar >= 1.0";

        for (pattern, name) in &signatures {
            for (log_text, expected_name) in [
                (oom_log, "Out of Memory"),
                (mixin_log, "Mixin Injection Conflict"),
                (resolution_log, "Mod Resolution Failure"),
            ] {
                let matched = match_signature(pattern, log_text);
                if *expected_name == **name {
                    assert!(matched, "Pattern '{}' (name={}) should match log for '{}'", pattern, name, expected_name);
                } else {
                    // A well-formed pattern should not falsely match unrelated logs.
                    // (Some patterns like "ModResolutionException|incompatible mod set"
                    //  are intentionally broad; we only assert non-match for clearly
                    // unrelated pairs.)
                    if *name == "Missing Fabric API" {
                        assert!(!matched, "Pattern '{}' should not match unrelated log", pattern);
                    }
                }
            }
        }
    }

    // ── Crash report file discovery (filesystem, temp dirs) ────────

    /// Create a temp dir with crash-report .txt files and verify listing.
    #[test]
    fn test_list_crash_reports_finds_txt() {
        let base = std::env::temp_dir().join("agora_test_crash_reports");
        let reports = base.join("crash-reports");
        let _ = std::fs::create_dir_all(&reports);
        std::fs::write(reports.join("crash-1.txt"), "first crash").unwrap();
        std::fs::write(reports.join("crash-2.txt"), "second crash").unwrap();
        // Also write a non-.txt file to ensure it is still listed (the impl
        // does not filter by extension — it lists all files).
        std::fs::write(reports.join("notes.md"), "notes").unwrap();

        let results = list_crash_report_files(&reports);
        assert_eq!(results.len(), 3);

        let names: Vec<&str> = results.iter().map(|r| r.filename.as_str()).collect();
        assert!(names.contains(&"crash-1.txt"));
        assert!(names.contains(&"crash-2.txt"));
        assert!(names.contains(&"notes.md"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Empty crash-reports directory returns an empty list.
    #[test]
    fn test_list_crash_reports_empty_dir() {
        let base = std::env::temp_dir().join("agora_test_crash_empty");
        let reports = base.join("crash-reports");
        let _ = std::fs::create_dir_all(&reports);

        let results = list_crash_report_files(&reports);
        assert!(results.is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Non-existent directory returns empty list without panic.
    #[test]
    fn test_list_crash_reports_nonexistent_dir() {
        let base = std::env::temp_dir().join("agora_test_crash_nonexistent");
        let results = list_crash_report_files(&base);
        assert!(results.is_empty());
    }

    // ── check_for_crash mtime logic (pure) ───────────────────────────

    /// A crash report with mtime AFTER the reference timestamp is detected as new.
    #[test]
    fn test_check_for_crash_detects_new() {
        let now = std::time::SystemTime::now();
        let future = now + std::time::Duration::from_secs(60);
        assert!(is_newer_than(future, now));
    }

    /// A crash report with mtime BEFORE the reference timestamp is NOT new.
    #[test]
    fn test_check_for_crash_no_new() {
        let now = std::time::SystemTime::now();
        let past = now - std::time::Duration::from_secs(60);
        assert!(!is_newer_than(past, now));
    }

    /// A crash report with mtime EXACTLY equal to the reference is NOT new
    /// (the real code uses strict `>` via `mtime <= last` skip).
    #[test]
    fn test_check_for_crash_exact_same_time_not_new() {
        let now = std::time::SystemTime::now();
        assert!(!is_newer_than(now, now));
    }

    // ── MAX_REGEX_LEN guard ──────────────────────────────────────────

    /// Patterns exceeding MAX_REGEX_LEN are rejected (return false).
    #[test]
    fn test_max_regex_len_rejects_long_patterns() {
        let long_pattern = "a".repeat(MAX_REGEX_LEN + 1);
        let crash_log = "a";
        assert!(!match_signature(&long_pattern, crash_log));
    }

    /// Patterns at exactly MAX_REGEX_LEN are accepted (not rejected).
    #[test]
    fn test_max_regex_len_accepts_boundary() {
        let boundary_pattern = "a".repeat(MAX_REGEX_LEN);
        let crash_log = "a".repeat(MAX_REGEX_LEN);
        // This compiles and matches because the pattern is within the limit.
        assert!(match_signature(&boundary_pattern, &crash_log));
    }

    // ── CrashSignatureRow struct fields ──────────────────────────────

    /// CrashSignatureRow can be constructed with all fields.
    #[test]
    fn test_crash_signature_row_fields() {
        let row = CrashSignatureRow {
            name: "Test".to_string(),
            regex_pattern: "foo".to_string(),
            solution_markdown: Some("**Fix**".to_string()),
            action_button_json: Some(r#"{"label":"Fix"}"#.to_string()),
        };
        assert_eq!(row.name, "Test");
        assert_eq!(row.regex_pattern, "foo");
        assert_eq!(row.solution_markdown, Some("**Fix**".to_string()));
        assert_eq!(row.action_button_json, Some(r#"{"label":"Fix"}"#.to_string()));
    }

    // ── CrashTriageResult ────────────────────────────────────────────

    /// no_match() returns a result with all fields None.
    #[test]
    fn test_triage_result_no_match() {
        let result = CrashTriageResult::no_match();
        assert!(!result.matched);
        assert!(result.signature_name.is_none());
        assert!(result.solution_markdown.is_none());
        assert!(result.action_button_json.is_none());
    }

    /// CrashReportInfo serializes with all expected fields.
    #[test]
    fn test_crash_report_info_serialization() {
        let info = CrashReportInfo {
            filename: "crash-1.txt".to_string(),
            modified_at: "2025-01-01T00:00:00Z".to_string(),
            size_bytes: 1024,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json.get("filename").unwrap().as_str().unwrap(), "crash-1.txt");
        assert_eq!(json.get("modified_at").unwrap().as_str().unwrap(), "2025-01-01T00:00:00Z");
        assert_eq!(json.get("size_bytes").unwrap().as_u64().unwrap(), 1024);
    }

    // ── parse_rfc3339 helper ─────────────────────────────────────────

    /// Valid RFC3339 timestamp parses successfully.
    #[test]
    fn test_parse_rfc3339_valid() {
        let result = parse_rfc3339("2025-01-01T12:00:00Z");
        assert!(result.is_some());
    }

    /// Invalid timestamp returns None.
    #[test]
    fn test_parse_rfc3339_invalid() {
        let result = parse_rfc3339("not-a-date");
        assert!(result.is_none());
    }

    /// Empty string returns None.
    #[test]
    fn test_parse_rfc3339_empty() {
        let result = parse_rfc3339("");
        assert!(result.is_none());
    }

    // ── system_time_to_rfc3339 helper ────────────────────────────────

    /// Converting a known system time and parsing it back yields a valid RFC3339 string.
    #[test]
    fn test_system_time_to_rfc3339_roundtrip() {
        let now = std::time::SystemTime::now();
        let s = system_time_to_rfc3339(now);
        // The round-trip parse should succeed.
        let parsed = parse_rfc3339(&s);
        assert!(parsed.is_some());
    }
}
