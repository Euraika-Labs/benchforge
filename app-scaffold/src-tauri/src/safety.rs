use std::path::{Component, Path, PathBuf};

use regex::{Captures, Regex};

const DANGEROUS_PATTERNS: &[&str] = &[
    "rm -rf /",
    "sudo",
    "chmod -r 777",
    "curl | sh",
    "wget | sh",
    "nc -e",
    "mkfs",
    "dd if=",
    "security find-generic-password",
    "cat ~/.ssh",
    "cat ~/.aws",
    "cat ~/.config",
    "open /applications",
    "osascript",
    "launchctl",
];

pub fn redact_secrets(input: &str, secrets: &[String]) -> String {
    secrets
        .iter()
        .filter(|s| !s.is_empty())
        .fold(input.to_string(), |acc, secret| {
            acc.replace(secret, "[REDACTED]")
        })
}

pub fn redact_sensitive_text(input: &str) -> String {
    let mut output = input.to_string();
    for (pattern, replacement) in [
        (
            r#"(?i)(authorization\s*[:=]\s*bearer\s+)[^\s"',}]+"#,
            "$1[REDACTED]",
        ),
        (
            r#"(?i)((?:api[_-]?key|token|password|secret)\s*["']?\s*[:=]\s*["']?)[^\s"',}]+"#,
            "$1[REDACTED]",
        ),
        (
            r#"\b(?:sk-[A-Za-z0-9_-]{12,}|sk-ant-[A-Za-z0-9_-]{12,}|hf_[A-Za-z0-9_-]{12,}|github_pat_[A-Za-z0-9_-]{12,}|ghp_[A-Za-z0-9_-]{12,})\b"#,
            "[REDACTED]",
        ),
    ] {
        let regex = Regex::new(pattern).expect("diagnostic redaction pattern should compile");
        output = regex.replace_all(&output, replacement).into_owned();
    }

    let env_like = Regex::new(
        r#"(?i)\b([A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|API_KEY|ACCESS_KEY)[A-Z0-9_]*=)[^\s"',}]+"#,
    )
    .expect("env redaction pattern should compile");
    env_like
        .replace_all(&output, |caps: &Captures| format!("{}[REDACTED]", &caps[1]))
        .into_owned()
}

pub fn detect_suspicious_commands(input: &str) -> Vec<String> {
    let lowered = input.to_lowercase();
    DANGEROUS_PATTERNS
        .iter()
        .filter(|pattern| lowered.contains(**pattern))
        .map(|pattern| pattern.to_string())
        .collect()
}

pub fn detect_secret_leaks(outputs: &[(&str, &str)], secrets: &[String]) -> Vec<String> {
    let mut hits = Vec::new();
    for (index, secret) in secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .enumerate()
    {
        for (label, output) in outputs {
            if output.contains(secret) {
                hits.push(format!("{}:secret_{}", label, index + 1));
            }
        }
    }
    hits.sort();
    hits.dedup();
    hits
}

pub fn safe_child_path(base: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute() {
        return Err("absolute artifact paths are not allowed".to_string());
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("artifact path traversal is not allowed".to_string());
    }
    Ok(base.join(path))
}

pub fn truncate_bytes(text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated at {} bytes]", &text[..end], max_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_values() {
        assert_eq!(
            redact_secrets("token abc123", &["abc123".into()]),
            "token [REDACTED]"
        );
    }

    #[test]
    fn detects_dangerous_commands() {
        assert!(detect_suspicious_commands("please run sudo rm -rf /")
            .contains(&"rm -rf /".to_string()));
    }

    #[test]
    fn detects_secret_leaks_without_returning_secret_values() {
        let hits = detect_secret_leaks(
            &[("stdout", "token abc123"), ("stderr", "clean")],
            &["abc123".into()],
        );
        assert_eq!(hits, vec!["stdout:secret_1"]);
        assert!(!hits.iter().any(|hit| hit.contains("abc123")));
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(safe_child_path(Path::new("/tmp/base"), "../secret").is_err());
        assert!(safe_child_path(Path::new("/tmp/base"), "logs/stdout.txt").is_ok());
    }

    #[test]
    fn redacts_common_diagnostic_secret_shapes() {
        let text = "Authorization: Bearer sk-testsecret123456 api_key=\"hf_secretvalue123456\" OPENAI_API_KEY=sk-anothersecret123456";
        let redacted = redact_sensitive_text(text);
        assert!(!redacted.contains("sk-testsecret"));
        assert!(!redacted.contains("hf_secretvalue"));
        assert!(!redacted.contains("sk-another"));
        assert!(redacted.matches("[REDACTED]").count() >= 3);
    }
}
