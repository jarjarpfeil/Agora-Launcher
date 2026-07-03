use regex::Regex;
use std::sync::OnceLock;

fn windows_user_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"C:\\Users\\([^\\]+)").unwrap())
}

fn linux_home_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/home/([^/]+)").unwrap())
}

fn macos_home_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/Users/([^/]+)").unwrap())
}

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b[a-f0-9]{32,}\b").unwrap())
}

pub fn sanitize_log(input: &str) -> String {
    let s = windows_user_re().replace_all(input, r"C:\Users\<user>");
    let s = linux_home_re().replace_all(&s, r"/home/<user>");
    let s = macos_home_re().replace_all(&s, r"/Users/<user>");
    let s = token_re().replace_all(&s, "<redacted>");
    s.to_string()
}

pub fn sanitize_log_lines(input: &str) -> String {
    input
        .lines()
        .map(sanitize_log)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_windows_user() {
        let input = r"Loading mod from C:\Users\JohnDoe\.minecraft\mods";
        let result = sanitize_log(input);
        assert!(!result.contains("JohnDoe"));
        assert!(result.contains(r"C:\Users\<user>"));
    }

    #[test]
    fn test_sanitize_linux_home() {
        let input = "Loading mod from /home/john/.minecraft/mods";
        let result = sanitize_log(input);
        assert!(!result.contains("/home/john"));
        assert!(result.contains("/home/<user>"));
    }

    #[test]
    fn test_sanitize_macos_home() {
        let input = "Loading mod from /Users/john/.minecraft/mods";
        let result = sanitize_log(input);
        assert!(!result.contains("/Users/john"));
        assert!(result.contains("/Users/<user>"));
    }

    #[test]
    fn test_sanitize_token() {
        let input = "token: abc123def456789012345678901234567890";
        let result = sanitize_log(input);
        assert!(result.contains("<redacted>"));
    }

    #[test]
    fn test_sanitize_multiple_lines() {
        let input = "line1\nline2";
        let result = sanitize_log_lines(input);
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn test_sanitize_does_not_alter_normal_text() {
        let input = "This is a normal log line with no PII";
        let result = sanitize_log(input);
        assert_eq!(result, input);
    }
}
