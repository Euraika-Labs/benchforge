use serde::{Deserialize, Serialize};

use crate::adapters;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessToolRequest {
    pub preset_id: String,
    #[serde(default = "default_action")]
    pub action: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessToolResultDto {
    pub preset_id: String,
    pub action: String,
    pub status: String,
    pub install_command: String,
    pub check_command: String,
    pub log: String,
}

#[derive(Debug, Clone, Copy)]
struct HarnessToolPreset {
    id: &'static str,
    label: &'static str,
    install: &'static [&'static str],
    check: &'static [&'static str],
    executable: &'static str,
}

const PRESETS: &[HarnessToolPreset] = &[
    HarnessToolPreset {
        id: "evalplus",
        label: "EvalPlus",
        install: &["python3", "-m", "pip", "install", "evalplus"],
        check: &["python3", "-m", "evalplus.evaluate", "--help"],
        executable: "python3",
    },
    HarnessToolPreset {
        id: "aider-polyglot",
        label: "Aider Polyglot",
        install: &["python3", "-m", "pip", "install", "aider-chat"],
        check: &["python3", "-m", "aider", "--version"],
        executable: "python3",
    },
    HarnessToolPreset {
        id: "terminal-bench",
        label: "Terminal-Bench",
        install: &["python3", "-m", "pip", "install", "terminal-bench"],
        check: &["tb", "--help"],
        executable: "tb",
    },
    HarnessToolPreset {
        id: "swebench",
        label: "SWE-bench Lite",
        install: &["python3", "-m", "pip", "install", "swebench"],
        check: &["python3", "-m", "swebench.harness.run_evaluation", "--help"],
        executable: "python3",
    },
];

fn default_action() -> String {
    "check".into()
}

pub fn run_harness_tool_action(
    request: HarnessToolRequest,
) -> Result<HarnessToolResultDto, String> {
    let preset = harness_tool_preset(&request.preset_id)
        .ok_or_else(|| format!("unsupported harness preset: {}", request.preset_id))?;
    let action = request.action.trim().to_ascii_lowercase();
    if action != "install" && action != "check" {
        return Err(format!(
            "unsupported harness tool action: {}. Use install or check.",
            request.action
        ));
    }

    let mut log = Vec::new();
    log.push(format!("Harness preset: {}", preset.label));
    log.push(format!(
        "Install command: {}",
        command_display(preset.install)
    ));
    log.push(format!("Check command: {}", command_display(preset.check)));

    let status = if action == "install" {
        run_allowlisted_command("install", preset.install, &mut log)?;
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
        match run_allowlisted_command("check", preset.check, &mut log) {
            Ok(()) => "ready",
            Err(err) => {
                log.push(format!("Readiness check failed: {}", err));
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

    Ok(HarnessToolResultDto {
        preset_id: preset.id.into(),
        action,
        status: status.into(),
        install_command: command_display(preset.install),
        check_command: command_display(preset.check),
        log: log.join("\n"),
    })
}

fn harness_tool_preset(id: &str) -> Option<HarnessToolPreset> {
    PRESETS.iter().copied().find(|preset| preset.id == id)
}

fn run_allowlisted_command(
    label: &str,
    args: &[&str],
    log: &mut Vec<String>,
) -> Result<(), String> {
    let Some((command, command_args)) = args.split_first() else {
        return Err("empty command".into());
    };
    log.push(format!("Running {}: {}", label, command_display(args)));
    let mut cmd = adapters::command_with_gui_path(command);
    cmd.args(command_args);
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

fn command_display(args: &[&str]) -> String {
    args.join(" ")
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
    fn rejects_unknown_harness_preset() {
        let err = run_harness_tool_action(HarnessToolRequest {
            preset_id: "custom".into(),
            action: "check".into(),
        })
        .expect_err("custom preset should not be executable");
        assert!(err.contains("unsupported harness preset"));
    }

    #[test]
    fn rejects_unknown_harness_action() {
        let err = run_harness_tool_action(HarnessToolRequest {
            preset_id: "evalplus".into(),
            action: "remove".into(),
        })
        .expect_err("unknown actions should be rejected");
        assert!(err.contains("unsupported harness tool action"));
    }

    #[test]
    fn renders_allowlisted_commands() {
        let preset = harness_tool_preset("terminal-bench").expect("preset");
        assert_eq!(
            command_display(preset.install),
            "python3 -m pip install terminal-bench"
        );
        assert_eq!(command_display(preset.check), "tb --help");
    }
}
