use serde::{Deserialize, Serialize};

use crate::adapters;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRuntimeToolRequest {
    pub runtime_id: String,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRuntimeToolResultDto {
    pub runtime_id: String,
    pub action: String,
    pub status: String,
    pub install_command: Option<String>,
    pub check_command: String,
    pub log: String,
}

#[derive(Debug, Clone, Copy)]
struct LocalRuntimeToolPreset {
    id: &'static str,
    label: &'static str,
    install: Option<&'static [&'static str]>,
    check: &'static [&'static str],
    executable: &'static str,
}

const PRESETS: &[LocalRuntimeToolPreset] = &[
    LocalRuntimeToolPreset {
        id: "ollama",
        label: "Ollama",
        install: Some(&["brew", "install", "ollama"]),
        check: &["ollama", "--version"],
        executable: "ollama",
    },
    LocalRuntimeToolPreset {
        id: "llama-cpp",
        label: "llama.cpp",
        install: Some(&["brew", "install", "llama.cpp"]),
        check: &["llama-server", "--version"],
        executable: "llama-server",
    },
    LocalRuntimeToolPreset {
        id: "vllm",
        label: "vLLM",
        install: Some(&["python3", "-m", "pip", "install", "vllm"]),
        check: &[
            "python3",
            "-m",
            "vllm.entrypoints.openai.api_server",
            "--help",
        ],
        executable: "python3",
    },
    LocalRuntimeToolPreset {
        id: "mlx-lm",
        label: "MLX / mlx-lm",
        install: Some(&["python3", "-m", "pip", "install", "mlx-lm"]),
        check: &["mlx_lm.server", "--help"],
        executable: "mlx_lm.server",
    },
];

fn default_action() -> String {
    "check".into()
}

pub fn run_local_runtime_tool_action(
    request: LocalRuntimeToolRequest,
) -> Result<LocalRuntimeToolResultDto, String> {
    let preset = local_runtime_tool_preset(&request.runtime_id)
        .ok_or_else(|| format!("unsupported local runtime setup: {}", request.runtime_id))?;
    let action = request.action.trim().to_ascii_lowercase();
    if action != "install" && action != "check" && action != "pull" {
        return Err(format!(
            "unsupported local runtime tool action: {}. Use install, check, or pull.",
            request.action
        ));
    }

    let mut log = Vec::new();
    log.push(format!("Local runtime: {}", preset.label));
    if let Some(install) = preset.install {
        log.push(format!("Install command: {}", command_display(install)));
    }
    log.push(format!("Check command: {}", command_display(preset.check)));

    let status = if action == "install" {
        let install = preset
            .install
            .ok_or_else(|| format!("{} requires manual installation", preset.label))?;
        run_allowlisted_command("install", install, &mut log)?;
        match run_allowlisted_command("check", preset.check, &mut log) {
            Ok(()) => "ready",
            Err(err) => {
                log.push(format!(
                    "Install finished, but the readiness check did not pass: {}",
                    err
                ));
                "partial"
            }
        }
    } else if adapters::command_exists(preset.executable) {
        match run_runtime_action(&action, preset, request.model.as_deref(), &mut log) {
            Ok(()) => "ready",
            Err(err) => {
                log.push(format!("{} action failed: {}", action, err));
                "missing"
            }
        }
    } else {
        log.push(format!(
            "{} executable was not found on the GUI PATH",
            preset.executable
        ));
        "missing"
    };

    Ok(LocalRuntimeToolResultDto {
        runtime_id: preset.id.into(),
        action,
        status: status.into(),
        install_command: preset.install.map(command_display),
        check_command: command_display(preset.check),
        log: log.join("\n"),
    })
}

fn run_runtime_action(
    action: &str,
    preset: LocalRuntimeToolPreset,
    model: Option<&str>,
    log: &mut Vec<String>,
) -> Result<(), String> {
    match action {
        "check" => run_allowlisted_command("check", preset.check, log),
        "pull" => {
            if preset.id != "ollama" {
                return Err("pull is currently supported only for Ollama".into());
            }
            let model = validated_ollama_model(model)?;
            let args = vec!["ollama".to_string(), "pull".to_string(), model];
            run_allowlisted_command("pull", &args, log)
        }
        _ => Err(format!("unsupported runtime action: {action}")),
    }
}

fn local_runtime_tool_preset(id: &str) -> Option<LocalRuntimeToolPreset> {
    PRESETS.iter().copied().find(|preset| preset.id == id)
}

fn run_allowlisted_command<S: AsRef<str>>(
    label: &str,
    args: &[S],
    log: &mut Vec<String>,
) -> Result<(), String> {
    let Some((command, command_args)) = args.split_first() else {
        return Err("empty command".into());
    };
    log.push(format!("Running {}: {}", label, command_display(args)));
    let mut cmd = adapters::command_with_gui_path(command.as_ref());
    cmd.args(command_args.iter().map(|arg| arg.as_ref()));
    let output = cmd
        .output()
        .map_err(|err| format!("failed to start {} command: {}", label, err))?;
    append_stream(log, label, "stdout", &output.stdout);
    append_stream(log, label, "stderr", &output.stderr);
    if !output.status.success() {
        return Err(format!(
            "{} command failed with exit code {:?}",
            label,
            output.status.code()
        ));
    }
    log.push(format!("{} command completed", label));
    Ok(())
}

fn command_display<S: AsRef<str>>(args: &[S]) -> String {
    args.iter()
        .map(|arg| arg.as_ref())
        .collect::<Vec<_>>()
        .join(" ")
}

fn validated_ollama_model(model: Option<&str>) -> Result<String, String> {
    let model = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "Ollama pull requires a model id, for example qwen2.5-coder:7b".to_string()
        })?;
    if model.len() > 200 {
        return Err("Ollama model id is too long".into());
    }
    if model.starts_with('-') {
        return Err("Ollama model id must not start with '-'".into());
    }
    if !model
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':' | '/'))
    {
        return Err(
            "Ollama model id may contain only letters, numbers, '.', '_', '-', ':', and '/'".into(),
        );
    }
    Ok(model.to_string())
}

fn append_stream(log: &mut Vec<String>, label: &str, stream: &str, bytes: &[u8]) {
    let text = strip_ansi_codes(String::from_utf8_lossy(bytes).trim());
    if text.is_empty() {
        return;
    }
    let clipped: String = text.chars().take(6000).collect();
    let suffix = if text.chars().count() > clipped.chars().count() {
        "\n... truncated ..."
    } else {
        ""
    };
    log.push(format!("{} {}:\n{}{}", label, stream, clipped, suffix));
}

fn strip_ansi_codes(input: impl AsRef<str>) -> String {
    let mut output = String::new();
    let mut chars = input.as_ref().chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        output.push(ch);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_runtime_setup() {
        let err = run_local_runtime_tool_action(LocalRuntimeToolRequest {
            runtime_id: "lm-studio".into(),
            action: "check".into(),
            model: None,
        })
        .expect_err("manual runtimes should not be executable");
        assert!(err.contains("unsupported local runtime setup"));
    }

    #[test]
    fn rejects_unknown_runtime_action() {
        let err = run_local_runtime_tool_action(LocalRuntimeToolRequest {
            runtime_id: "ollama".into(),
            action: "start".into(),
            model: None,
        })
        .expect_err("unknown actions should be rejected");
        assert!(err.contains("unsupported local runtime tool action"));
    }

    #[test]
    fn renders_allowlisted_runtime_commands() {
        let ollama = local_runtime_tool_preset("ollama").expect("ollama preset");
        assert_eq!(
            command_display(ollama.install.expect("install")),
            "brew install ollama"
        );
        let mlx = local_runtime_tool_preset("mlx-lm").expect("mlx preset");
        assert_eq!(command_display(mlx.check), "mlx_lm.server --help");
    }

    #[test]
    fn validates_ollama_pull_model_ids() {
        assert_eq!(
            validated_ollama_model(Some("qwen2.5-coder:7b")).expect("model should validate"),
            "qwen2.5-coder:7b"
        );
        assert_eq!(
            command_display(&[
                "ollama".to_string(),
                "pull".to_string(),
                "library/llama3.2:3b".to_string()
            ]),
            "ollama pull library/llama3.2:3b"
        );
        for model in ["", "--help", "qwen;rm", "qwen 7b"] {
            assert!(
                validated_ollama_model(Some(model)).is_err(),
                "{model:?} should be rejected"
            );
        }
    }
}
