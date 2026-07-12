use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaInstallation {
    pub path: PathBuf,
    pub version: u32,
    pub version_string: String,
}

pub fn detect_installed_jres() -> Vec<JavaInstallation> {
    let mut results = Vec::new();

    // Windows paths
    #[cfg(target_os = "windows")]
    {
        let windows_roots = [
            r"C:\Program Files\Java",
            r"C:\Program Files (x86)\Java",
            r"C:\Program Files\Eclipse Adoptium",
            r"C:\Program Files\Microsoft\jdk",
            r"C:\Program Files\Zulu",
        ];
        for root in &windows_roots {
            let dir = PathBuf::from(root);
            if dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let javadir = entry.path().join("bin");
                        for bin in ["java.exe", "javaw.exe"] {
                            let path = javadir.join(bin);
                            if path.is_file() {
                                if let Some(inst) = check_java_at(&path) {
                                    results.push(inst);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // macOS paths
    #[cfg(target_os = "macos")]
    {
        let base = PathBuf::from("/Library/Java/JavaVirtualMachines");
        if base.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&base) {
                for entry in entries.flatten() {
                    let path = entry.path().join("Contents/Home/bin/java");
                    if path.is_file() {
                        if let Some(inst) = check_java_at(&path) {
                            results.push(inst);
                        }
                    }
                }
            }
        }
    }

    // Linux paths
    #[cfg(target_os = "linux")]
    {
        let linux_roots = ["/usr/lib/jvm", "/opt/jdk"];
        for root in &linux_roots {
            let dir = PathBuf::from(root);
            if dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path().join("bin/java");
                        if path.is_file() {
                            if let Some(inst) = check_java_at(&path) {
                                results.push(inst);
                            }
                        }
                    }
                }
            }
        }
        let global = PathBuf::from("/usr/bin/java");
        if global.is_file() {
            if let Some(inst) = check_java_at(&global) {
                results.push(inst);
            }
        }
    }

    // PATH scan (all platforms)
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for bin in ["java", "javaw"] {
                let path = dir.join(bin);
                if path.is_file() {
                    if let Some(inst) = check_java_at(&path) {
                        results.push(inst);
                    }
                }
            }
        }
    }

    results
}

fn check_java_at(path: &PathBuf) -> Option<JavaInstallation> {
    let cloned = path.clone();
    let path_for_result = path.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = std::process::Command::new(&cloned).arg("-version").output();
        let _ = tx.send(result);
    });
    let output = rx.recv_timeout(std::time::Duration::from_secs(5)).ok()?;
    let output = output.ok()?;
    let stderr = String::from_utf8(output.stderr).ok()?;
    let version_str = parse_version_string(&stderr)?;
    let major = extract_major_version(version_str)?;
    Some(JavaInstallation {
        path: path_for_result,
        version: major,
        version_string: version_str.to_string(),
    })
}

fn parse_version_string(stderr: &str) -> Option<&str> {
    // Match: openjdk version "17.0.9"  or  java version "1.8.0_352"
    for line in stderr.lines() {
        let line = line.trim();
        if let Some(start) = line.find("version \"") {
            let rest = &line[start + "version \"".len()..];
            if let Some(end) = rest.find('"') {
                return Some(&rest[..end]);
            }
        }
    }
    None
}

fn extract_major_version(version: &str) -> Option<u32> {
    // Java 8 and earlier: "1.8.0_352" -> 8
    if let Some(v) = version.strip_prefix("1.") {
        if let Some(dot) = v.find('.') {
            return v[..dot].parse::<u32>().ok();
        }
        return v.parse::<u32>().ok();
    }
    // Java 9+: "17.0.9" or "21" -> take the first component
    if let Some(dot) = version.find('.') {
        return version[..dot].parse::<u32>().ok();
    }
    if let Some(underscore) = version.find('_') {
        return version[..underscore].parse::<u32>().ok();
    }
    version.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_major_version_18() {
        assert_eq!(extract_major_version("1.8.0_352"), Some(8));
    }

    #[test]
    fn test_extract_major_version_17() {
        assert_eq!(extract_major_version("17.0.1"), Some(17));
    }

    #[test]
    fn test_extract_major_version_21() {
        assert_eq!(extract_major_version("21"), Some(21));
    }

    #[test]
    fn test_extract_major_version_invalid() {
        assert_eq!(extract_major_version("invalid"), None);
    }

    #[test]
    fn test_detect_no_panic() {
        let _ = detect_installed_jres();
    }

    #[test]
    fn test_parse_version_string_java8() {
        let input = "java version \"1.8.0_352\"\nJava(TM) SE Runtime Environment (build 1.8.0_352-b08)\nJava HotSpot(TM) 64-Bit Server VM (build 25.352-b08, mixed mode)";
        assert_eq!(parse_version_string(input), Some("1.8.0_352"));
    }

    #[test]
    fn test_parse_version_string_java17() {
        let input = "openjdk version \"17.0.9\" 2023-10-17\nOpenJDK Runtime Environment (build 17.0.9+9)\nOpenJDK 64-Bit Server VM (build 17.0.9+9, mixed mode)";
        assert_eq!(parse_version_string(input), Some("17.0.9"));
    }
}
