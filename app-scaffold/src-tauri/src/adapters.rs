use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterSpec {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub adapter_version: u32,
    pub schema_version: u32,
    #[serde(default)]
    pub default_base_url: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub env: serde_json::Value,
    #[serde(default)]
    pub capabilities: serde_json::Value,
    #[serde(default)]
    pub security: serde_json::Value,
    #[serde(default)]
    pub validation: serde_json::Value,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(rename = "adapterVersion")]
    pub adapter_version: u32,
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "defaultBaseUrl")]
    pub default_base_url: Option<String>,
    pub command: Option<String>,
    pub path: String,
    #[serde(rename = "validationStatus")]
    pub validation_status: String,
    #[serde(rename = "validationDetail")]
    pub validation_detail: String,
    pub capabilities: serde_json::Value,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct LoadedAdapter {
    pub spec: AdapterSpec,
    pub path: PathBuf,
}

pub fn render_template(value: &str, vars: &std::collections::HashMap<String, String>) -> String {
    let mut out = value.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{}}}}}", k), v);
    }
    out
}

pub fn load_builtin_adapters() -> Result<Vec<LoadedAdapter>, String> {
    load_adapters_from(&paths::resource_root().join("adapters"))
}

pub fn load_adapters_from(root: &Path) -> Result<Vec<LoadedAdapter>, String> {
    let mut files = Vec::new();
    collect_yaml(root, &mut files).map_err(|err| format!("{}: {}", root.display(), err))?;
    files.sort();

    let mut adapters = Vec::new();
    let mut errors = Vec::new();
    for path in files {
        match load_adapter(&path) {
            Ok(spec) => adapters.push(LoadedAdapter { spec, path }),
            Err(err) => errors.push(err),
        }
    }

    if errors.is_empty() {
        Ok(adapters)
    } else {
        Err(errors.join("\n"))
    }
}

pub fn load_adapter(path: &Path) -> Result<AdapterSpec, String> {
    let raw = fs::read_to_string(path).map_err(|err| format!("{}: {}", path.display(), err))?;
    let value: serde_yaml::Value =
        serde_yaml::from_str(&raw).map_err(|err| format!("{}: {}", path.display(), err))?;
    for field in [
        "id",
        "name",
        "kind",
        "adapter_version",
        "schema_version",
        "capabilities",
    ] {
        if value.get(field).is_none() {
            return Err(format!("{}: $.{} is required", path.display(), field));
        }
    }
    serde_yaml::from_value(value).map_err(|err| format!("{}: {}", path.display(), err))
}

pub fn adapter_to_dto(adapter: &LoadedAdapter) -> AdapterDto {
    let (validation_status, validation_detail) = validate_adapter(&adapter.spec);
    AdapterDto {
        id: adapter.spec.id.clone(),
        name: adapter.spec.name.clone(),
        kind: adapter.spec.kind.clone(),
        adapter_version: adapter.spec.adapter_version,
        schema_version: adapter.spec.schema_version,
        default_base_url: adapter.spec.default_base_url.clone(),
        command: adapter.spec.command.clone(),
        path: adapter
            .path
            .strip_prefix(paths::resource_root())
            .or_else(|_| adapter.path.strip_prefix(paths::repo_root()))
            .unwrap_or(&adapter.path)
            .to_string_lossy()
            .to_string(),
        validation_status,
        validation_detail,
        capabilities: adapter.spec.capabilities.clone(),
        metadata: adapter.spec.metadata.clone(),
    }
}

pub fn validate_adapter(spec: &AdapterSpec) -> (String, String) {
    if spec.kind == "cli_agent" || spec.kind == "benchmark_harness" {
        let Some(command) = &spec.command else {
            return (
                "error".into(),
                format!("{} adapter has no command", spec.kind),
            );
        };
        return if adapter_command_exists(command) {
            ("ok".into(), format!("{} found", command))
        } else {
            (
                "warn".into(),
                format!(
                    "{} not found in PATH; install it or configure an absolute path",
                    command
                ),
            )
        };
    }

    if let Some(secret_env) = spec
        .validation
        .get("secret_env")
        .and_then(|value| value.as_str())
    {
        return if std::env::var(secret_env)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        {
            ("ok".into(), format!("{} is set", secret_env))
        } else {
            (
                "warn".into(),
                format!("{} is not set; provider calls will be skipped", secret_env),
            )
        };
    }

    if let Some(endpoint) = spec
        .validation
        .get("endpoint")
        .and_then(|value| value.as_str())
    {
        return validate_adapter_endpoint(spec, endpoint);
    }

    ("ok".into(), "no validation required".into())
}

fn validate_adapter_endpoint(spec: &AdapterSpec, endpoint: &str) -> (String, String) {
    let Some(base_url) = spec.default_base_url.as_deref() else {
        return (
            "warn".into(),
            format!(
                "{} declares endpoint {} but has no default base URL; create a target to validate it",
                spec.name, endpoint
            ),
        );
    };
    if base_url.contains("YOUR-") || base_url.contains("example.com") {
        return (
            "warn".into(),
            format!(
                "{} needs a configured base URL before probing {}",
                spec.name, endpoint
            ),
        );
    }
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return (
            "error".into(),
            format!(
                "{} default base URL is not HTTP(S): {}",
                spec.name, base_url
            ),
        );
    }
    if !command_exists("curl") {
        return (
            "warn".into(),
            format!("curl is not available; cannot probe {}", base_url),
        );
    }

    let url = join_base_url_and_endpoint(base_url, endpoint);
    match curl_probe_endpoint(&url, 2) {
        Ok(probe) => match probe.status {
            Some(status) if (200..300).contains(&status) => {
                if endpoint_response_looks_usable(&probe.body) {
                    ("ok".into(), format!("{} responded HTTP {}", url, status))
                } else {
                    (
                        "warn".into(),
                        format!(
                            "{} responded HTTP {} but did not look like a model-list response",
                            url, status
                        ),
                    )
                }
            }
            Some(status) => (
                "warn".into(),
                format!(
                    "{} responded HTTP {}; check the runtime configuration",
                    url, status
                ),
            ),
            None => (
                "warn".into(),
                format!("{} responded without an HTTP status marker", url),
            ),
        },
        Err(err) => (
            "warn".into(),
            format!("{} is not reachable: {}", url, short_error(&err)),
        ),
    }
}

fn join_base_url_and_endpoint(base_url: &str, endpoint: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        endpoint.trim_start_matches('/')
    )
}

#[derive(Debug)]
struct EndpointProbe {
    body: String,
    status: Option<u16>,
}

fn curl_probe_endpoint(url: &str, max_time_seconds: u64) -> Result<EndpointProbe, String> {
    let mut cmd = command_with_gui_path("curl");
    let max_time = max_time_seconds.clamp(1, 10).to_string();
    cmd.args([
        "-sS",
        "--connect-timeout",
        "1",
        "--max-time",
        &max_time,
        "-w",
        "\n__BENCHFORGE_ADAPTER_HTTP_STATUS__:%{http_code}",
        url,
    ]);
    let output = cmd.output().map_err(|err| err.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let probe = parse_adapter_http_response(&stdout);
    if !output.status.success() {
        let detail = if stderr.trim().is_empty() {
            probe.body.clone()
        } else {
            stderr.trim().to_string()
        };
        return Err(detail);
    }
    Ok(probe)
}

fn parse_adapter_http_response(stdout: &str) -> EndpointProbe {
    const MARKER: &str = "\n__BENCHFORGE_ADAPTER_HTTP_STATUS__:";
    let Some(index) = stdout.rfind(MARKER) else {
        return EndpointProbe {
            body: stdout.to_string(),
            status: None,
        };
    };
    let body = stdout[..index].to_string();
    let status = stdout[index + MARKER.len()..].trim().parse::<u16>().ok();
    EndpointProbe { body, status }
}

fn endpoint_response_looks_usable(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    value.get("data").and_then(|data| data.as_array()).is_some()
        || value
            .get("models")
            .and_then(|models| models.as_array())
            .is_some()
        || value.as_array().is_some()
}

fn short_error(error: &str) -> String {
    error
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(error)
        .trim()
        .to_string()
}

fn adapter_command_exists(command: &str) -> bool {
    command_exists(command)
        || (command == "benchforge-worker"
            && (paths::worker_venv_launcher().exists()
                || paths::bundled_worker_launcher().exists()))
}

pub fn find_adapter(id: &str) -> Result<Option<LoadedAdapter>, String> {
    Ok(load_builtin_adapters()?
        .into_iter()
        .find(|adapter| adapter.spec.id == id))
}

pub fn command_exists(command: &str) -> bool {
    if command.contains('/') {
        return Path::new(command).exists();
    }
    let paths = gui_path_parts();
    paths
        .iter()
        .any(|dir| Path::new(dir).join(command).exists())
}

pub fn command_with_gui_path(command: &str) -> Command {
    let mut cmd = Command::new(command);
    cmd.env("PATH", gui_path());
    cmd
}

pub fn gui_path() -> String {
    gui_path_parts().join(":")
}

fn gui_path_parts() -> Vec<String> {
    let mut path_parts = vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
        "/usr/sbin".to_string(),
        "/sbin".to_string(),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        path_parts.push(home.join(".local/bin").to_string_lossy().to_string());
        path_parts.push(home.join(".cargo/bin").to_string_lossy().to_string());
    }
    if let Some(path) = std::env::var_os("PATH") {
        path_parts
            .extend(std::env::split_paths(&path).map(|path| path.to_string_lossy().to_string()));
    }
    path_parts
}

fn collect_yaml(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_yaml(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
        {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn test_endpoint_adapter(base_url: Option<String>) -> AdapterSpec {
        AdapterSpec {
            id: "test-endpoint".into(),
            name: "Test Endpoint".into(),
            kind: "openai_compatible".into(),
            adapter_version: 1,
            schema_version: 1,
            default_base_url: base_url,
            command: None,
            args: vec![],
            working_dir: None,
            timeout_seconds: None,
            env: serde_json::json!({}),
            capabilities: serde_json::json!({"text_generation": true}),
            security: serde_json::json!({}),
            validation: serde_json::json!({"endpoint": "/models"}),
            metadata: serde_json::json!({}),
        }
    }

    fn spawn_probe_server(status: u16, body: &'static str) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener.local_addr().expect("listener address should read");
        let handle = thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let _ = handle_probe_connection(stream, status, body);
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn handle_probe_connection(
        mut stream: TcpStream,
        status: u16,
        body: &str,
    ) -> Result<(), String> {
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer).map_err(|err| err.to_string())?;
        let reason = if status == 200 { "OK" } else { "Error" };
        let response = format!(
            "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            status,
            reason,
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .map_err(|err| err.to_string())
    }

    #[test]
    fn builtin_adapters_load() {
        let adapters = load_builtin_adapters().expect("adapters should load");
        assert!(adapters
            .iter()
            .any(|adapter| adapter.spec.id == "codex-cli"));
        assert!(adapters
            .iter()
            .any(|adapter| adapter.spec.id == "ollama-openai"));
        assert!(adapters
            .iter()
            .any(|adapter| adapter.spec.id == "openrouter"));
        assert!(adapters
            .iter()
            .any(|adapter| adapter.spec.id == "azure-openai"));
        assert!(adapters.iter().any(|adapter| adapter.spec.id == "gemini"));
        let openai = adapters
            .iter()
            .find(|adapter| adapter.spec.id == "openai")
            .expect("OpenAI adapter should load");
        assert!(openai
            .spec
            .metadata
            .get("model_presets")
            .and_then(|value| value.as_array())
            .is_some_and(|items| !items.is_empty()));
        let gemini = adapters
            .iter()
            .find(|adapter| adapter.spec.id == "gemini")
            .expect("Gemini adapter should load");
        assert_eq!(gemini.spec.kind, "openai_compatible");
        assert!(gemini
            .spec
            .metadata
            .get("model_presets")
            .and_then(|value| value.as_array())
            .is_some_and(|items| !items.is_empty()));
    }

    #[test]
    fn invalid_adapter_has_clear_path() {
        let dir =
            std::env::temp_dir().join(format!("benchforge-adapter-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.yaml");
        fs::write(&path, "name: Missing ID\nkind: mock\n").unwrap();
        let err = load_adapters_from(&dir).unwrap_err();
        assert!(err.contains("$.id is required"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn adapter_endpoint_validation_probes_default_base_url() {
        let (base_url, handle) = spawn_probe_server(200, r#"{"data":[{"id":"local-model"}]}"#);
        let spec = test_endpoint_adapter(Some(base_url));

        let (status, detail) = validate_adapter(&spec);

        handle.join().expect("probe server should finish");
        assert_eq!(status, "ok");
        assert!(detail.contains("responded HTTP 200"));
        assert!(!detail.contains("stubbed"));
    }

    #[test]
    fn adapter_endpoint_validation_warns_for_non_model_response() {
        let (base_url, handle) = spawn_probe_server(200, r#"{"ok":true}"#);
        let spec = test_endpoint_adapter(Some(base_url));

        let (status, detail) = validate_adapter(&spec);

        handle.join().expect("probe server should finish");
        assert_eq!(status, "warn");
        assert!(detail.contains("did not look like a model-list response"));
    }

    #[test]
    fn adapter_endpoint_validation_warns_without_default_base_url() {
        let spec = test_endpoint_adapter(None);

        let (status, detail) = validate_adapter(&spec);

        assert_eq!(status, "warn");
        assert!(detail.contains("has no default base URL"));
        assert!(!detail.contains("stubbed"));
    }

    #[test]
    fn adapter_secret_env_validation_requires_non_empty_value() {
        let name = format!("BENCHFORGE_TEST_SECRET_{}", uuid::Uuid::new_v4()).replace('-', "_");
        let mut spec = test_endpoint_adapter(None);
        spec.validation = serde_json::json!({"secret_env": name});

        std::env::remove_var(&name);
        let (missing_status, missing_detail) = validate_adapter(&spec);
        assert_eq!(missing_status, "warn");
        assert!(missing_detail.contains("is not set"));

        std::env::set_var(&name, "secret-value");
        let (present_status, present_detail) = validate_adapter(&spec);
        std::env::remove_var(&name);
        assert_eq!(present_status, "ok");
        assert!(present_detail.contains("is set"));
    }

    #[test]
    fn gui_path_includes_common_desktop_tool_locations() {
        let path = gui_path();

        assert!(path.contains("/opt/homebrew/bin"));
        assert!(path.contains("/usr/local/bin"));
        assert!(path.contains("/usr/bin"));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            assert!(path.contains(&home.join(".local/bin").to_string_lossy().to_string()));
            assert!(path.contains(&home.join(".cargo/bin").to_string_lossy().to_string()));
        }
    }
}
