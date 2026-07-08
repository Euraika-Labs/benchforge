# Product requirements

## Product name

Working name: **BenchForge**.

## Primary goal

Build a macOS GUI app that can run the same benchmark tasks against local LLMs, cloud LLMs, and coding-agent CLIs, then compare results side-by-side with reproducible metrics.

## Non-goals for v1

- No IDE autocomplete benchmarking.
- No browser automation against ChatGPT/Claude web UIs.
- No hosted leaderboard by default.
- No full SWE-bench run as the first release.
- No offensive cyber range in the default install.
- No silent auto-approval on the user's real filesystem.

## Target taxonomy

A **target** is anything BenchForge can benchmark.

```text
Target categories:
1. direct_model
2. harnessed_model
3. cli_agent
4. benchmark_harness
```

### direct_model

Raw model call through an API or local compatible endpoint. Examples:

- OpenAI Responses / Chat Completions compatible adapters
- Anthropic Messages API
- Mistral API
- Ollama OpenAI-compatible endpoint
- LM Studio local server
- vLLM server
- llama.cpp server
- mlx-lm server

### harnessed_model

A raw model wrapped by BenchForge's own tool loop. This creates a fairer comparison because every model gets the same file reading, editing, command execution, retry policy, and context strategy.

### cli_agent

A product CLI that brings its own agent loop, prompts, tools, context management, and patching behavior. Examples:

- `codex exec`
- `claude -p`
- `vibe --prompt`
- `copilot -p`

### benchmark_harness

An external benchmark system that BenchForge invokes and imports results from. Examples:

- EvalPlus
- Aider benchmark runner
- Terminal-Bench
- SWE-bench
- Inspect AI

## Core user flows

### Flow 1: first run smoke benchmark

1. User opens BenchForge.
2. App runs Doctor checks: Docker, Git, Python, Node, Rust, Ollama/LM Studio endpoint, CLI agent availability.
3. User selects targets.
4. User selects `quick-smoke` benchmark.
5. App creates isolated workspaces.
6. App runs each target.
7. App displays pass/fail, time, cost, logs, diff, and test output.

Acceptance:

- User can compare at least one local model, one cloud model, and one CLI agent in one table.
- Every run stores full reproducibility metadata.
- Failed runs show useful logs, not just “failed”.

### Flow 2: adapter setup

1. User opens Settings > Targets.
2. User adds a provider adapter from a template.
3. User supplies base URL, model name, and secret reference.
4. App validates connectivity with a tiny model call or CLI `--version` check.
5. Adapter appears as selectable target.

Acceptance:

- Secrets are never persisted in plaintext in project files.
- Adapter export redacts secrets.
- Adapter validation records endpoint, model, version, and capabilities when available.

### Flow 3: benchmark pack authoring

1. User creates a new benchmark pack from template.
2. User adds tasks with repo fixtures and scoring commands.
3. App validates schemas.
4. App runs one task locally.
5. App packages benchmark pack for sharing.

Acceptance:

- Invalid benchmark packs fail fast with clear schema errors.
- Scoring commands run after target execution in a clean evaluation phase.
- BenchForge can import/export packs as folders or zip files.

### Flow 4: CLI agent comparison

1. User selects `Codex CLI`, `Claude Code`, `Mistral Vibe`, and `GitHub Copilot CLI` targets.
2. User selects a repo-patch benchmark.
3. App clones fixture into isolated workspace per target.
4. App launches each CLI in non-interactive mode.
5. App captures stdout, stderr, transcript files, exit codes, git diff, command logs, and test results.

Acceptance:

- CLI process timeout is enforced.
- Hanging processes are killed, including child processes where possible.
- Result stores stdout/stderr, command line after secret redaction, and transcript location.

## Metrics

### Required v1 metrics

```text
pass_fail
score_numeric
wall_time_ms
setup_time_ms
target_time_ms
evaluation_time_ms
exit_code
stdout_bytes
stderr_bytes
files_changed
lines_added
lines_deleted
commands_observed_count
dangerous_command_hits
input_tokens
output_tokens
total_tokens
estimated_cost_usd
ttft_ms
decode_tokens_per_sec
peak_rss_mb
```

Metrics may be `null` when unsupported, but unsupported must be explicit.

### Implemented extension metrics

```text
cache_read_tokens
cache_write_tokens
```

Prompt-cache metrics are reported when providers expose them. Targets may also store optional cache read/write pricing so cost estimates can distinguish cached input from normal input.

### Optional later metrics

```text
energy_joules
power_watts_avg
cpu_percent_avg
gpu_percent_avg
thermal_pressure
context_tokens_used
retry_count
rate_limit_count
network_bytes
```

## Result views

The UI must include:

- run table;
- target comparison matrix;
- task detail view;
- diff viewer;
- stdout/stderr viewer;
- test output viewer;
- cost and latency chart;
- adapter configuration view;
- benchmark pack manager;
- Doctor diagnostics panel.

## Reproducibility requirements

Every run must record:

```text
BenchForge version
benchmark pack id/version/checksum
task id/version/checksum
target id/adapter version/model id
provider endpoint without secrets
prompt/system prompt/tool prompt hashes
temperature/top_p/max tokens
retry policy
timeout policy
sandbox policy
host OS and architecture
Docker image digest where used
workspace git commit/hash
all scoring command versions where feasible
```

## Failure taxonomy

```text
adapter_validation_failed
model_call_failed
cli_not_found
cli_auth_failed
cli_permission_blocked
sandbox_setup_failed
workspace_setup_failed
timeout
test_failed
scoring_failed
invalid_output_format
unsafe_command_detected
rate_limited
provider_overloaded
unknown_error
```

## v1 acceptance checklist

- App launches on macOS as Tauri dev app.
- Doctor detects Git, Docker/Colima, Python, Node, and configured CLIs.
- User can create/edit/list targets.
- User can run quick-smoke benchmark against at least one mock target and one real CLI target.
- Result persists to SQLite.
- UI shows diff/logs/test output.
- App can export results to JSONL and CSV.
- App can package as `.dmg` on macOS.
