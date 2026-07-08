use std::process::Command;

const SERVICE_PREFIX: &str = "benchforge";
const ACCOUNT: &str = "api_key";

pub fn save_cloud_api_key(provider: &str, api_key: &str) -> Result<(), String> {
    let provider = normalize_provider(provider)?;
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("API key is empty".into());
    }
    save_secret(&format!("cloud/{}", provider), ACCOUNT, api_key)
}

pub fn read_cloud_api_key(provider: &str) -> Option<String> {
    let provider = normalize_provider(provider).ok()?;
    read_secret(&format!("cloud/{}", provider), ACCOUNT)
}

pub fn cloud_api_key_available(provider: &str) -> bool {
    let Ok(provider) = normalize_provider(provider) else {
        return false;
    };
    secret_available(&format!("cloud/{}", provider), ACCOUNT)
}

fn save_secret(name: &str, account: &str, value: &str) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("persistent secret storage currently uses macOS Keychain".into());
    }
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            account,
            "-s",
            &service_name(name),
            "-w",
            value,
            "-U",
        ])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

fn read_secret(name: &str, account: &str) -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            account,
            "-s",
            &service_name(name),
            "-w",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn secret_available(name: &str, account: &str) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            account,
            "-s",
            &service_name(name),
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn service_name(name: &str) -> String {
    format!("{}/{}", SERVICE_PREFIX, name)
}

fn normalize_provider(provider: &str) -> Result<String, String> {
    let provider = provider.trim();
    if provider.is_empty()
        || provider.contains("..")
        || provider.contains('/')
        || !provider
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("invalid provider id".into());
    }
    Ok(provider.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_are_restricted() {
        assert_eq!(normalize_provider("openai").unwrap(), "openai");
        assert!(normalize_provider("../openai").is_err());
        assert!(normalize_provider("cloud/openai").is_err());
    }
}
