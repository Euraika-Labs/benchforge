# Adapter architecture

## Adapter philosophy

Adapters are the heart of BenchForge. Everything benchmarkable is a target, and every target is backed by an adapter.

Adapters must be:

- declarative where possible;
- explicit about capabilities;
- explicit about security requirements;
- versioned;
- testable through Doctor checks;
- exportable without secrets.

## Adapter kinds

```text
openai_compatible
openai_responses
anthropic_messages
mistral_api
cli_agent
python_worker
benchmark_harness
mock
```

## Capability flags

```yaml
capabilities:
  text_generation: true
  streaming: true
  tool_calling: true
  json_mode: true
  file_editing: false
  shell_execution: false
  repo_agent: false
  cost_reporting: true
  token_usage_reporting: true
  ttft_reporting: true
```

## Target vs adapter

An adapter is a reusable integration template.

A target is a configured instance.

Example:

```text
adapter: ollama-openai-compatible
target: qwen3-local-ollama
model: qwen3.6:35b
base_url: http://localhost:11434/v1
```

## Direct model adapters

Direct model adapters expose a normalized chat/completion interface to the BenchForge harness.

Required functions:

```text
validate()
list_models() optional
complete(request)
stream(request) optional
estimate_cost(usage) optional
```

Normalized request:

```json
{
  "model": "qwen3.6",
  "messages": [
    {"role":"system","content":"..."},
    {"role":"user","content":"..."}
  ],
  "temperature": 0,
  "max_output_tokens": 4096,
  "tools": [],
  "response_format": null
}
```

Normalized response:

```json
{
  "text": "...",
  "tool_calls": [],
  "usage": {
    "input_tokens": 100,
    "output_tokens": 200
  },
  "raw": {}
}
```

## CLI agent adapters

CLI agents are launched as child processes in a workspace.

Required properties:

```yaml
command: codex
args:
  - exec
  - --cd
  - "{{workspace}}"
  - "{{prompt}}"
working_dir: "{{workspace}}"
timeout_seconds: 900
```

Required behavior:

- validate command exists;
- validate auth where possible;
- launch with safe environment;
- apply adapter `env` values after template rendering;
- capture stdout/stderr transcripts;
- capture command line, version/help probe, exit code, timeout, and transcript paths;
- capture git diff;
- run scoring after process exits.

Targets can also provide an inline custom CLI adapter through target config:

```json
{
  "command": "my-agent",
  "args": ["run", "--workspace", "{{workspace}}", "{{prompt}}"],
  "working_dir": "{{workspace}}",
  "env": {"MY_AGENT_MODEL": "{{model}}"},
  "validation": {"command_args": ["--version"]}
}
```

BenchForge writes `cli-stdout.txt`, `cli-stderr.txt`, and `cli-agent-command.json` for CLI runs. Command metadata redacts secrets and replaces the raw task prompt with `<task_prompt>`; reproducibility records preserve prompt hashes separately.

## CLI adapter variables

```text
{{prompt}}
{{workspace}}
{{model}}
{{max_turns}}
```

## Environment variable policy

Adapter specs can reference env vars by name.

```yaml
env:
  ANTHROPIC_API_KEY:
    from_secret: anthropic.default
```

Generated commands must never print secret values.

## Built-in adapter matrix

| Adapter | Kind | v1 priority | Notes |
|---|---|---:|---|
| Ollama | openai_compatible | P0 | local Mac standard |
| LM Studio | openai_compatible | P0 | default port 1234 |
| llama.cpp server | openai_compatible | P1 | common local runtime |
| vLLM | openai_compatible | P1 | mostly remote GPU/Linux but useful |
| MLX/mlx-lm | local/openai_compatible | P1 | Apple Silicon native path |
| Generic OpenAI-compatible | openai_compatible | P0 | manual local or cloud endpoint |
| OpenAI | openai_responses/openai_compatible | P0 | cloud model baseline |
| Anthropic | anthropic_messages | P0 | Claude model baseline |
| Mistral | mistral_api/openai_compatible | P1 | cloud/local options |
| OpenRouter | openai_compatible | P1 | cloud aggregator + model matrix |
| Azure OpenAI | azure_openai | P1 | resource/deployment URL shapes |
| Google Gemini | openai_compatible | P1 | Gemini OpenAI-compatible endpoint |
| Codex CLI | cli_agent | P0 | product agent |
| Claude Code | cli_agent | P0 | product agent |
| Mistral Vibe | cli_agent | P0 | product agent |
| GitHub Copilot CLI | cli_agent | P0 | product agent + BYOK |
| Promptfoo | benchmark_harness | P2 | optional integration |
| Inspect AI | benchmark_harness | P2 | optional integration |

## Adapter validation levels

```text
level 1: file/schema valid
level 2: command exists or endpoint reachable
level 3: auth valid
level 4: tiny completion succeeds
level 5: benchmark smoke task passes
```

Current adapter listing performs level 2 checks for built-ins: CLI and harness adapters verify the command is available on the GUI-safe PATH, cloud adapters report whether their configured environment key is present, and local OpenAI-compatible adapters with a default base URL probe their declared endpoint with a short `/models` check. Target validation still owns level 3-5 checks because it has the concrete model, endpoint, key, and benchmark pack.

## Adapter errors

Use structured errors:

```json
{
  "code": "cli_not_found",
  "message": "codex was not found in PATH",
  "remediation": "Install Codex CLI or configure the absolute command path."
}
```

## Versioning

Adapter specs include:

```yaml
adapter_version: 1
schema_version: 1
last_verified: "2026-07-02"
```

The app should warn when a built-in adapter has not been verified against the installed CLI version.
