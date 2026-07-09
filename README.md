# BenchForge

BenchForge is a macOS desktop workbench for benchmarking local and cloud LLMs with reproducible runs, inspectable artifacts, latency and token metrics, cost estimates, normalized errors, and shareable reports.

Use it to answer practical model-selection questions: which local or hosted model passes the same tasks, how fast it runs, what it costs, what failed, and whether the evidence is strong enough to trust.

This repository includes the desktop app, built-in benchmark packs, provider and runtime adapters, Python worker harnesses, packaging scripts, community files, and the implementation roadmap. Development commands run from the repository root unless noted otherwise.

## Contents

- [Status](#status)
- [What BenchForge Benchmarks](#what-benchforge-benchmarks)
- [Features](#features)
- [Capability Matrix](#capability-matrix)
- [Repository Layout](#repository-layout)
- [Architecture At A Glance](#architecture-at-a-glance)
- [Requirements](#requirements)
- [Quick Start](#quick-start)
- [Fresh Mac To First Benchmark](#fresh-mac-to-first-benchmark)
- [End-to-End Workflow](#end-to-end-workflow)
- [First Benchmark Recipes](#first-benchmark-recipes)
- [Configure Secrets](#configure-secrets)
- [Run A Local Hugging Face Model](#run-a-local-hugging-face-model)
- [Run Other Local Runtimes](#run-other-local-runtimes)
- [Run Cloud Benchmarks](#run-cloud-benchmarks)
- [Run CLI Agent Benchmarks](#run-cli-agent-benchmarks)
- [Benchmark Packs](#benchmark-packs)
- [External Harnesses And Imports](#external-harnesses-and-imports)
- [Results And Reports](#results-and-reports)
- [Data, Secrets, And Safety](#data-secrets-and-safety)
- [Development Commands](#development-commands)
- [Packaging](#packaging)
- [Troubleshooting](#troubleshooting)
- [Known Limits](#known-limits)
- [Documentation Map](#documentation-map)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [Community And Governance](#community-and-governance)
- [License](#license)

## Status

BenchForge is an active pre-1.0 implementation, not a static mockup. The Tauri desktop app, Rust backend, Python worker bridge, benchmark packs, target registry, Hugging Face GGUF workflow, cloud provider adapters, job queue, SQLite result storage, comparison views, report exports, and macOS DMG packaging checks are implemented.

Use it today as an engineering benchmark harness. Do not treat bundled packs as a definitive public leaderboard yet. The app surfaces evidence-grade, calibration, model-identity, cost, generation-setting, and coverage warnings so weak evidence is not mistaken for a final decision.

The Vite browser build is only a frontend preview. Keychain storage, downloads, provider calls, subprocesses, benchmark execution, and packaging require the Tauri desktop app.

Latest local release-readiness verification: `make benchmark-readiness-full` passed on 2026-07-09, covering clean first-run, local/cloud contracts, Hugging Face GGUF search/download/start, report export, unsigned DMG packaging, installed-app smoke, and local server cleanup. The default no-spend `make live-cloud-smoke` also skipped safely when no real provider keys were configured.

There is no stable release channel yet. For now, install from source with the commands below or build a local DMG from this checkout.

## What BenchForge Benchmarks

BenchForge can run prompt, repo/code, security, and external harness tasks against:

- Local model runtimes: Hugging Face GGUF through `llama.cpp`, Ollama, LM Studio, vLLM, MLX / `mlx-lm`, oMLX-style servers, and generic OpenAI-compatible endpoints.
- Cloud providers: OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, Google Gemini, and generic OpenAI-compatible APIs.
- Agent and harness targets: Codex CLI, Claude Code, GitHub Copilot CLI, Mistral Vibe CLI, BenchForge Python workers, EvalPlus, Aider Polyglot, Terminal-Bench, and SWE-bench-style commands.
- Worker-backed tasks: repo/code-edit tasks, defensive security checks, external harness commands, and imported benchmark output.
- Prompt benchmark packs: connectivity, instruction following, structured output, grounded context, decision making, practical model selection, and reliability checks.

## Features

- Secure target setup with macOS Keychain-backed API keys, editable input/output and prompt-cache pricing, visible pricing-assumption warnings, and redacted target configs.
- Target validation with tiny completion probes and normalized errors such as `auth`, `rate_limit`, `model_not_found`, `timeout`, and `malformed_response`.
- Hugging Face GGUF search, file selection, download planning, disk checks, token handling, `curl` fallback, `llama-server` startup, local target registration, and optional benchmark handoff.
- Cloud catalog search and validation probes for supported providers.
- Background jobs with progress, Dashboard stop/retry actions, duplicate/retry recovery, startup recovery, and grouped run history.
- Metrics for score, pass/fail, wall time, setup time, target time, evaluation time, p95 latency, process exit codes, stdout/stderr byte counts, files changed, lines added/deleted, observed command counts, dangerous command hits, peak RSS when observable, provider timing, time to first token when available, token usage, reasoning tokens, prompt-cache read/write tokens when providers report them, throughput, retry attempts, retry delay, HTTP status, finish reason, served model identity, estimated cost, and pricing assumptions.
- Stable v1 export aliases such as `pass_fail`, `score_numeric`, `input_tokens`, `output_tokens`, `estimated_cost_usd`, `ttft_ms`, and `decode_tokens_per_sec`; `result.json` artifacts include all required v1 metric keys and use explicit `null` values for unsupported metrics.
- Worker harness support for running configured external commands or importing CSV, JSON, JSONL, JUnit XML, text, log, and output result files, with structured summaries prioritized before large logs and read-file provenance recorded.
- Artifact inspection for prompts, responses, raw provider payloads, logs, diffs, scorer output, worker events, and redacted target configs.
- Exports to CSV, JSONL, Analysis JSON, Markdown, and full report folders with reproducibility metadata, Docker scoring image evidence when used, and review-before-sharing warnings.

## Capability Matrix

| Area | Current state |
| --- | --- |
| Desktop app | Implemented as a Tauri macOS app with React UI, Rust backend, SQLite storage, background jobs, and artifact browsing. |
| Local Hugging Face GGUF | Implemented for GGUF search, file planning, download/reuse, `llama-server` startup, target registration, validation, and optional benchmark queueing. |
| Other local runtimes | Implemented through OpenAI-compatible adapters for Ollama, LM Studio, `llama.cpp`, vLLM, MLX / `mlx-lm`, oMLX-style servers, and custom endpoints. |
| Cloud models | Implemented for OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, Google Gemini, and generic OpenAI-compatible APIs, with key storage and validation probes. |
| Agent and harness benchmarks | Implemented for CLI-agent adapters, worker-backed external commands, and import of existing CSV, JSON, JSONL, JUnit XML, text, log, or output files. |
| Evidence and reports | Implemented with result metrics, report folders, reproducibility manifests, copied artifacts, warnings, and analysis summaries. |
| Public releases | Source and local DMG builds are supported. A stable signed/notarized release channel is still planned. |

## Repository Layout

| Path | Purpose |
| --- | --- |
| `app-scaffold/` | Tauri desktop app. React/TypeScript UI is in `src/`; Rust backend is in `src-tauri/src/`. |
| `workers/` | Python worker package for external harnesses and defensive security checks. |
| `benchmark-packs/` | Built-in benchmark pack definitions and task YAML. |
| `adapters/` | YAML adapter definitions for local runtimes, cloud providers, CLI agents, and harness targets. |
| `fixtures/` | Small test projects used by repo/code and security smoke benchmarks. |
| `scripts/` | Bootstrap, doctor, readiness, smoke, schema validation, and packaging scripts. |
| `docs/` | Product requirements, architecture, data model, security model, benchmark strategy, UI spec, and roadmap. |
| `prompts/` | Ordered implementation prompts from the original blueprint. |
| `.benchforge/` | Local runtime data, models, runs, exports, and readiness logs. Do not commit this directory. |
| `.github/` | Issue templates and pull request template. |

## Architecture At A Glance

BenchForge is a Tauri 2 desktop app. The React/TypeScript UI calls Rust commands for target setup, validation, jobs, runs, result storage, reports, and local process control. The Rust backend stores durable state in SQLite under the app data directory, writes per-run artifacts to `.benchforge/runs/`, and reads benchmark/adapter YAML from the repository or user-provided pack roots.

Python workers handle external harness execution and imports when a benchmark needs a separate command boundary. Local and cloud models are normalized into target records so the same benchmark pack can compare a GGUF model served by `llama-server`, an Ollama or LM Studio endpoint, and hosted APIs through one results surface.

## Requirements

Required for development:

- macOS for the desktop app and Keychain integration.
- Node.js and npm.
- Rust and Cargo.
- Python 3.10 or newer.
- Git.

Optional but useful:

- Homebrew for installing local tools.
- Hugging Face CLI (`hf`) for resumable Hub downloads.
- `llama.cpp` / `llama-server` for GGUF local serving.
- Ollama, LM Studio, vLLM, or MLX for local runtime discovery.
- Docker Desktop or Colima for sandboxed repo/code scoring.
- Provider API keys for live cloud benchmarks.
- Apple signing/notarization credentials for release DMGs.

## Quick Start

Install host tools:

```bash
brew install node python git llama.cpp
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Bootstrap and launch:

```bash
./scripts/bootstrap.sh
make doctor
make test
make dev
```

What those commands do:

- `./scripts/bootstrap.sh` installs app dependencies and creates an editable Python worker virtual environment under `workers/.venv/`. It selects Python 3.10+ from `BENCHFORGE_PYTHON`, `python3`, or versioned `python3.10+` commands and stops with a repair hint if only older Python is available.
- `make doctor` checks required tools and reports optional helpers such as Docker, Colima, Hugging Face CLI, and `llama-server`.
- `make test` runs schema validation, the TypeScript/Vite production build, Rust tests, Python worker tests, and the worker CLI help check.
- `make dev` starts the real Tauri desktop app.

Use `cd app-scaffold && npm run dev` only for the browser preview with sample data.

Choose the fastest path for the thing you want to prove:

| Goal | Start with |
| --- | --- |
| Test the app and built-in fixtures | `make test && make benchmark-readiness` |
| Benchmark a GGUF model from Hugging Face | `make dev`, then use the Dashboard local setup action or Settings -> Hugging Face Local Model |
| Compare a local server to a cloud model | Let the Dashboard route ready local/cloud setup, then run an `llm-*` pack with at least 3 repetitions |
| Verify report quality | `make report-smoke` or export a report folder from Results |
| Build a macOS installer | `make package-dmg && make verify-dmg && make install-smoke-dmg` |

## Fresh Mac To First Benchmark

1. Install the host tools and run `./scripts/bootstrap.sh`.
2. Run `make doctor` and fix required setup issues. The Hugging Face GGUF path needs Python 3.10+ and `llama.cpp`; `hf` is recommended, but BenchForge can use `curl` for public downloads.
3. Start the desktop app with `make dev`. On a clean workspace, BenchForge opens a ready local runtime or keyed cloud provider setup path when Doctor can detect one; otherwise it opens Doctor.
4. In Settings, save `HF_TOKEN` if the model is gated or private. You must accept any gated model license on Hugging Face first.
5. Open Hugging Face Local Model, search for a GGUF repository, choose a `.gguf` file, and leave Start after download enabled.
6. BenchForge creates a download job, verifies or reuses the model file, starts `llama-server`, registers and validates the local target, then queues the selected benchmark pack when auto-run is enabled.
7. Open Results, compare score, pass rate, latency, throughput, cost, served model identity, and evidence warnings, then export a report folder.

## End-to-End Workflow

1. Run `make doctor` and fix required tools before adding targets.
2. Add at least one target: a local runtime such as Hugging Face GGUF, Ollama, LM Studio, or `llama.cpp`, or a cloud provider such as OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, or Gemini.
3. Validate the target. Validation runs a tiny probe and stores the latest health result so full benchmark runs fail less mysteriously.
4. Choose a benchmark pack. All pack tasks are selected by default; expand Tasks only when you want a subset. Local/cloud prompt comparisons use 3 repetitions, 1 warmup, and a default cost cap unless you intentionally customize them. Advanced run settings keep warmups, concurrency, Docker scoring, and paid-provider cost caps available when needed.
5. Run the benchmark from the app. BenchForge stores runs in SQLite and writes artifacts under `.benchforge/runs/`.
6. Compare targets on the Results page by pass rate, weighted score, latency, cost, throughput, model identity, coverage, and evidence grade.
7. Export a report folder when you need a portable review package with CSV, JSONL, Analysis JSON, copied artifacts, and reproducibility metadata.

## First Benchmark Recipes

Use these paths to prove the tool works before investing in larger benchmark runs.

### Offline App And Pack Check

```bash
make test
make benchmark-readiness
```

This validates schemas, builds the frontend, runs Rust and Python tests, checks benchmark pack contracts, exercises worker imports, and writes readiness logs under `.benchforge/readiness/`.

### Local Hugging Face GGUF Run

```bash
make dev
```

In the desktop app, open Settings, save `HF_TOKEN` if the model is gated, then use Hugging Face Local Model to search for a GGUF repository, pick a quantized file, download it, start `llama-server`, register the target, and run `llm-basics`. If a priced cloud target already exists, the guided action defaults to a capped local/cloud comparison and lets you choose the cloud counterpart.

### Local Vs Cloud Comparison

1. Add and validate a local OpenAI-compatible target, such as Ollama, LM Studio, or `llama.cpp`. If Doctor already sees a reachable local endpoint, opening Targets auto-detects it; otherwise use Local Runtimes -> Detect.
2. Add and validate a cloud target, such as OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, or Gemini. If a provider key is already available, BenchForge preselects that provider and a priced preset so the target form can go straight to **Add + run** or **Add + compare**. Dashboard, Doctor, and Settings setup buttons preserve the already-configured local or cloud counterpart for the automatic handoff.
3. On the Dashboard, use **Local setup** when you need a local model. It detects an already-reachable local runtime when Doctor found one; otherwise it opens the managed Hugging Face GGUF flow.
4. On the Dashboard, use the primary comparison action. For two to four comparable local/priced cloud targets it labels the exact scope, such as **Compare 3 models**, and runs the same capped default pack across all of them. Larger ready sets show **Compare recommended pair** first and keep **Compare all** as the explicit all-target action. In Run Builder, the local/cloud shortcut uses the same count-based label and applies 3 repetitions, 1 warmup, bounded concurrency, and the default cap. Shortcuts prefer cloud targets with input/output pricing so capped runs can estimate spend.
5. Compare pass rate, score, p95 latency, throughput, cost, served model identity, and evidence warnings in Results.

## Configure Secrets

Use the app's Settings and Targets screens whenever possible. BenchForge stores provider keys in macOS Keychain and keeps raw secret values out of target JSON and exports. Environment variables are also supported when launching from a shell.

| Service | Environment variable | Notes |
| --- | --- | --- |
| Hugging Face | `HF_TOKEN` | Required for gated/private models; public downloads may work without it. Accept gated model licenses on Hugging Face before downloading. |
| OpenAI | `OPENAI_API_KEY` | Used by OpenAI targets and generic compatible targets when configured. |
| Anthropic | `ANTHROPIC_API_KEY` | Used for Anthropic validation and benchmark calls. |
| Mistral | `MISTRAL_API_KEY` | Used for Mistral catalog and run calls. |
| OpenRouter | `OPENROUTER_API_KEY` | Useful for broad model comparisons through one provider. |
| Azure OpenAI | `AZURE_OPENAI_API_KEY` | Use the Azure `/openai/v1` base URL and put the deployment name in the Model field. |
| Google Gemini | `GEMINI_API_KEY` | Uses Gemini's OpenAI-compatible endpoint. |

Example:

```bash
OPENAI_API_KEY=sk-... HF_TOKEN=hf_... make dev
```

Do not commit `.env` files, API keys, downloaded models, exported reports, or `.benchforge/` runtime data.

## Run A Local Hugging Face Model

1. Start the desktop app with `make dev`.
2. Open Doctor and fix required setup issues. For GGUF workflows, install `llama.cpp`; the Hugging Face CLI is recommended but BenchForge can fall back to `curl`.
3. In Settings, save a Hugging Face token if you need gated or private model access.
4. Use Hugging Face Local Model to browse popular GGUF repositories, run a query search, inspect concrete `.gguf` files, and choose a quantized file.
5. Leave Start after download enabled to let BenchForge download the model, start `llama-server`, register a local target, and optionally queue a benchmark pack.
6. Open Results to inspect metrics and artifacts, then export a report folder.

Automatic here means BenchForge owns the app-side job flow after you choose a model file and confirm the action. It does not silently accept gated model licenses, spend cloud credits, install broad system dependencies, or benchmark a provider without the required key, pricing/cost policy, and target validation.

Useful smoke checks:

```bash
make hf-local-smoke
make hf-local-cloud-basics-smoke
```

## Run Other Local Runtimes

BenchForge works best when local runtimes expose an OpenAI-compatible API.

| Runtime | Typical setup | Target notes |
| --- | --- | --- |
| Ollama | Start Ollama and pull a model, for example `ollama pull llama3.1:8b`. | Use `http://localhost:11434/v1` and the Ollama model tag. |
| LM Studio | Start the local server from LM Studio. | Use its local `/v1` base URL, often `http://localhost:1234/v1`. |
| llama.cpp | Run `llama-server -m /path/model.gguf --port 8080`. | Use `http://127.0.0.1:8080/v1`. |
| vLLM / MLX / custom server | Start the server with an OpenAI-compatible endpoint. | Use the matching adapter or generic OpenAI-compatible adapter. |

After adding a target, run Validate before benchmarking. Validation records health details and catches missing endpoints, model names, keys, or malformed responses before a full run.

## Run Cloud Benchmarks

1. Open Targets and choose a cloud adapter such as OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, or Google Gemini.
2. Paste the API key. BenchForge stores it in macOS Keychain.
3. Search the provider catalog when available, or enter a model manually.
4. Use the built-in provider URL unless you are configuring Azure OpenAI, a proxy, or a generic OpenAI-compatible endpoint.
5. Validate the target with a tiny completion probe.
6. Choose a benchmark pack and repetitions. Advanced run settings include warmups, concurrency, Docker scoring, and a max-cost cap. BenchForge blocks capped cloud runs when pricing is missing.
7. Run the benchmark and compare results by target, provider, model, pack, task, run group, and status.

The Dashboard and Doctor also have **Validate cloud** actions. Use them after saving cloud keys or targets to run live provider probes and make product-readiness evidence visible before you start a paid comparison.

Live provider probes are outside the default readiness gate and skip cleanly when no provider key is configured.

```bash
BENCHFORGE_LIVE_CLOUD_PROVIDERS=openai,anthropic,openrouter,gemini make live-cloud-smoke
make live-cloud-run
make live-cloud-run-basics
```

By default `make live-cloud-smoke` validates configured providers and prints a `setup.nextAction` checklist without starting a paid benchmark run. `make live-cloud-run` adds `BENCHFORGE_LIVE_CLOUD_RUN=1` and runs `llm-connectivity` with a low token budget. `make live-cloud-run-basics` runs the same live providers on `llm-basics`. Set `BENCHFORGE_LIVE_CLOUD_PACK` to another prompt-only pack such as `llm-core` or `llm-reliability` when you want real providers to run the same comparison pack you use in the app.

## Run CLI Agent Benchmarks

CLI agent targets run product agents such as Codex CLI, Claude Code, GitHub Copilot CLI, and Mistral Vibe against repo/code tasks. BenchForge creates an isolated Git workspace, renders the adapter prompt, applies configured adapter environment variables, runs the CLI with a timeout, captures `cli-stdout.txt`, `cli-stderr.txt`, `cli-agent-command.json`, `diff.patch`, scorer output, and the final `result.json`.

Built-in CLI adapters live under `adapters/agents/`. Advanced users can also create a custom CLI target by setting `command`, `args`, optional `working_dir`, `env`, and `validation.command_args` in target config. Template variables include `{{prompt}}`, `{{workspace}}`, `{{model}}`, and `{{max_turns}}`.

CLI command metadata in artifacts and reports redacts secrets and replaces the raw task prompt with `<task_prompt>` while preserving prompt SHA-256 hashes for reproducibility.

## Benchmark Packs

Built-in pack IDs include:

| Pack | Use |
| --- | --- |
| `llm-connectivity` | Fast sanity check that a target can answer. |
| `llm-basics` | Small instruction-following, JSON, and summarization checks. |
| `llm-core` | Classification, extraction, arithmetic, tool-call shape, and boundary tasks. |
| `llm-structured-output` | Strict JSON, arrays, nested fields, and numeric conversion tasks. |
| `llm-grounded-context` | Citation, distractor filtering, contradiction, and synthesis tasks. |
| `llm-practical` | Model-selection, routing, budget, privacy, and triage tasks. |
| `llm-decision-suite` | Constraint ranking, abstention, multilingual extraction, and normalization tasks. |
| `llm-reliability` | Ambiguity, instruction hierarchy, context recall, confidence, SLO, and retry-discipline tasks. |
| `code-edit-core` | Deterministic repo patch tasks. |
| `security-defensive` | Worker-backed defensive Semgrep/Bandit/dependency/secret checks with fallbacks. |
| `evalplus`, `aider-polyglot-subset`, `terminal-bench-subset`, `swebench-lite-subset` | External harness-style benchmark entry points. |

BenchForge loads built-in packs from `benchmark-packs/` and user packs from `.benchforge/benchmark-packs/`. Private prompt packs can be created in the app, imported from folders or zip archives, and exported for sharing. Advanced users can add more roots with `BENCHFORGE_BENCHMARK_PACK_DIRS`.

Pack calibration metadata is captured in run reproducibility and affects whether results can become comparison-ready. Built-in LLM comparison packs declare calibration quality gates for local and cloud baseline evidence, provider-confirmed served model IDs, full pack/task coverage, at least three repetitions per task/target, cloud cost metrics, and one generation policy.

## External Harnesses And Imports

Worker-backed harness targets can run an external benchmark command or import a result file that was produced elsewhere. Use `harness.command` when BenchForge should execute the suite:

```json
{
  "harness": {
    "command": ["python", "-m", "evalplus.evaluate", "--samples", "{workspace}/samples.jsonl"],
    "timeout_seconds": 3600
  }
}
```

Use `harness.import_path` when you already have benchmark output to normalize into BenchForge results:

```json
{
  "harness": {
    "import_path": "import-results.jsonl"
  }
}
```

Import paths must resolve inside the per-run worker workspace or output directory. Supported inputs are CSV, JSON, JSONL, JUnit XML, text, log, and output files or directories containing those files. Symlinked result files in import directories are rejected so imported evidence cannot escape the worker boundary. BenchForge preserves raw imported output as an artifact, records import provenance, counts unsupported files ignored during directory imports, and extracts common `total`, `passed`, `failed`, `score`, `pass@1`, and accuracy fields.

## Results And Reports

The Results page ranks targets by weighted pass rate, pass rate, average score, score spread, p95 latency, cost, throughput, and sample size. It also shows normalized-error recovery hints with target-repair shortcuts that preload editable target forms when possible, plus per-pack scoped Run Builder reruns that skip unavailable historical targets, and warns about incomplete pack/task coverage, low repetitions, missing pricing, prompt-cache pricing assumptions, missing provider-confirmed model identity, mixed generation settings, and uncalibrated packs. Dashboard mirrors the evidence grade and can launch the recommended evidence follow-up directly when another local/cloud run is needed. Run Builder also surfaces pre-run validation blockers with the same target-repair flow, and its Comparison Readiness panel can switch one-sided local/cloud selections into a paired comparison or route missing local/cloud setup directly.

Report-folder exports include:

- `README.md` with a human-readable run summary, including worker-import provenance when external benchmark results were imported.
- `results.csv` and `results.jsonl`.
- `analysis.json` with rankings, evidence grade, risks, task drilldowns, normalized-error recovery hints, worker-import summaries, and recommended next run.
- `reproducibility.json` with run config, generation settings, prompt/system-prompt hashes, redacted targets, benchmark metadata, host profile, pricing snapshots, scoring command versions, CLI-agent command/transcript checksums, workspace Git baseline/diff hashes for repo/code runs, and Docker scoring image ID/digest plus Dockerfile checksum when Docker scoring is used.
- `artifacts.json` with artifact copy status, sensitivity flags, and review summaries.
- Copied artifact files when safe and available.

Review exported prompts, responses, raw payloads, diffs, and logs before sharing them outside your environment.

## Data, Secrets, And Safety

- Source/dev runs store local app data under `.benchforge/`; the installed macOS app uses `~/Library/Application Support/BenchForge`. Use `BENCHFORGE_DATA_DIR=/path/to/data` for an isolated store.
- API keys are stored in macOS Keychain or read from configured environment variables. They should not be committed or placed in target JSON.
- Target configs and exports redact secret-shaped fields such as `authorization`, `access_token`, `client_secret`, `token`, and `password`.
- Repo/code tasks run in disposable Git workspaces. BenchForge records the baseline commit/tree, captures `diff.patch` including untracked source files, excludes generated caches such as `.benchforge-venv` and `node_modules`, and records sandbox level plus permission mode in artifacts and reports.
- Docker-backed scoring applies to eligible Python repo/code tasks, runs scorer containers with network disabled, and records the runner image identity plus Dockerfile checksum in artifacts and reports.
- Worker harness child processes inherit a minimal environment by default; secret-bearing variables require explicit `env_passthrough`.
- CLI-agent command evidence redacts secrets and elides raw task prompts in metadata; review copied CLI stdout/stderr before sharing exports.
- Managed `llama-server` processes are marked so BenchForge can clean up only servers it owns.

## Development Commands

Run these from the repository root.

| Command | Description |
| --- | --- |
| `make dev` | Start the Tauri desktop app in development mode. |
| `make test` | Run schema validation, web build, Rust tests, Python worker tests, and worker CLI help. |
| `make benchmark-readiness` | Offline readiness gate for local/cloud benchmarking, worker harness imports, and report evidence. |
| `make dependency-audit` | Verify target-specific dependency advisory exceptions against the supported macOS build graph. |
| `make benchmark-readiness-full` | Extended gate before packaging or handoff. |
| `make product-readiness` | Summarize local readiness evidence, check that readiness evidence matches the current commit, and report external live-cloud/signing blockers without spending credits or requiring Apple credentials. |
| `make live-cloud-smoke` | Optional real-provider validation with setup guidance; does not run a benchmark unless `BENCHFORGE_LIVE_CLOUD_RUN=1` is set. |
| `make live-cloud-run` | Validate configured real providers and run the low-token `llm-connectivity` live benchmark. |
| `make live-cloud-run-basics` | Validate configured real providers and run the same `llm-basics` pack against them. |
| `make first-run-smoke` | Verify a clean app store using a temporary `BENCHFORGE_DATA_DIR`. |
| `make local-runtime-discovery-smoke` | Verify local runtime discovery, validation, and handoff contracts. |
| `make cloud-provider-job-smoke` | Verify provider-style queued jobs, snapshots, metrics, retry evidence, and exports. |
| `make cloud-catalog-smoke` | Verify provider catalog parsing, pricing/context metadata, and catalog-to-target redaction. |
| `make report-smoke` | Verify report export content. |
| `make worker-harness-contract-smoke` | Verify external harness command execution and CSV/JSON/JSONL/JUnit import contracts. |
| `make smoke-docker` | Verify Docker/Colima scoring, network-off container execution, and Docker image reproducibility metadata. |
| `make release-preflight` | Verify release hygiene, dependency advisory scope, bundled resources, lockfiles, icons, and packaging docs. |
| `make package-dmg` | Build a local macOS DMG. |
| `make verify-dmg` | Verify the latest built DMG checksum, mount it, inspect `BenchForge.app`, and report signature status. |
| `make install-smoke-dmg` | Copy the app out of the DMG and run first-run, worker, and security-pack smokes. |
| `make release-signing-plan` | Print the public-release signing/notarization checklist without requiring credentials. |
| `make package-release-dmg` | Build a public macOS DMG with signing and notarization checks enabled. |
| `make verify-distribution-dmg` | Require Developer ID signing, Gatekeeper assessment, and notarization ticket validation. |

Use focused `make *-smoke` targets when changing a specific workflow. Use `make benchmark-readiness` before handing benchmark-critical changes to another user.
Readiness targets are time-bounded by default: 15 minutes per target for quick mode and 30 minutes for full mode. Set `BENCHFORGE_READINESS_TARGET_TIMEOUT_SECONDS=0` to disable that bound, or set a custom number of seconds when debugging slow hardware or downloads.
Use `make product-readiness` before a release handoff to see which local gates are proven, whether the latest readiness summary belongs to the current commit, and which external live-provider or Apple distribution checks still need credentials.
`make product-readiness` detects provider keys saved by the app in macOS Keychain as well as shell environment variables, but it still will not contact live providers unless `BENCHFORGE_PRODUCT_READINESS_RUN_LIVE=1` is set.
The in-app Doctor and Dashboard surface the same product-readiness split: validated remote cloud targets count as live-provider evidence, while signed/notarized public distribution stays explicit until Apple release credentials are used.

## Packaging

Build a local DMG:

```bash
make release-preflight
make package-dmg
make verify-dmg
make install-smoke-dmg
```

For a public release DMG, enable distribution checks and provide Apple signing plus notarization credentials:

```bash
make release-signing-plan
BENCHFORGE_RELEASE_DISTRIBUTION=1 \
APPLE_SIGNING_IDENTITY="Developer ID Application: Example" \
APPLE_ID="developer@example.com" \
APPLE_TEAM_ID="TEAMID1234" \
APPLE_PASSWORD="app-specific-password" \
make package-release-dmg
```

Use `APPLE_CERTIFICATE` and `APPLE_CERTIFICATE_PASSWORD` instead of a local keychain identity in CI. For App Store Connect API notarization, use `APPLE_API_KEY`, `APPLE_API_ISSUER`, and `APPLE_API_KEY_PATH` instead of `APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID`. After a release build, run `make verify-distribution-dmg` to require Developer ID signing, Gatekeeper assessment, and notarization ticket validation.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| Hugging Face CLI install says Python 3.10+ is required. | Install a newer Python with `brew install python`, rerun `./scripts/bootstrap.sh`, or use `BENCHFORGE_PYTHON=/opt/homebrew/bin/python3 ./scripts/bootstrap.sh` if `python3` still points to an older system interpreter. BenchForge can still use `curl` fallback for public downloads. |
| The browser preview opens but benchmarks do not run. | Use `make dev` from the repository root; the Vite preview cannot access Keychain, subprocesses, provider calls, or local downloads. |
| Hugging Face search works but download does not start. | Check that the model is public or your `HF_TOKEN` is saved, the gated model license has been accepted, the selected file is a real `.gguf`, and the model cache has enough disk space. Retry from the Hugging Face job row after fixing the cause. |
| A GGUF downloads but no benchmark is queued. | Confirm Start after download and an automatic benchmark pack were selected. Check the Jobs and Targets views for a failed `llama-server` start, occupied port, target validation error, or missing runtime tool. |
| Cloud target validation says no key is configured. | Save the provider key in the app or launch with the matching environment variable, such as `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`. |
| A capped cloud run is blocked. | Add input/output pricing metadata for the target/model or remove the max-cost cap. Add cache read/write pricing too when you want provider-reported prompt-cache tokens priced separately. BenchForge blocks capped runs when it cannot estimate cost. |
| Results mention pricing assumptions. | Add cache read/write token prices for the affected target, then rerun or re-export before treating cost ranking as decisive. Without those prices, cache tokens are visibly priced with the normal input-token rate. |
| Local target validation fails. | Confirm the server is running, the base URL ends in `/v1` when required, and the model name matches the runtime's model list. |
| Readiness fails or times out. | Open the summary path printed by `make benchmark-readiness`; detailed logs are under `.benchforge/readiness/`. If a target timed out on a slow machine, rerun it directly or raise `BENCHFORGE_READINESS_TARGET_TIMEOUT_SECONDS`. |

## Known Limits

- Desktop support is macOS-first because the app uses macOS Keychain and DMG packaging.
- Built-in prompt packs are useful for engineering comparison but are not a definitive leaderboard.
- Live cloud smokes are opt-in so default readiness gates do not spend API credits.
- Some external benchmark harnesses require their own dependencies, datasets, licenses, and runtime setup.
- Public release distribution requires valid Apple Developer ID signing and notarization credentials.

## Documentation Map

The README is the operator entry point. Use the deeper docs when changing internals:

| Document | Use |
| --- | --- |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Tauri, Rust backend, storage, worker, and adapter architecture. |
| [`docs/DATA_MODEL.md`](docs/DATA_MODEL.md) | SQLite entities, run records, artifacts, jobs, and reproducibility data. |
| [`docs/BENCHMARK_STRATEGY.md`](docs/BENCHMARK_STRATEGY.md) | Benchmark pack design, evidence grading, calibration, and scoring philosophy. |
| [`docs/SECURITY_SANDBOX.md`](docs/SECURITY_SANDBOX.md) | Sandbox, redaction, worker, Docker, and artifact-sharing rules. |
| [`docs/UI_UX_SPEC.md`](docs/UI_UX_SPEC.md) | Product flows and expected desktop app behavior. |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Current implementation status and remaining milestone work. |

## Roadmap

Near-term priorities:

- Empirically calibrate public and private benchmark suites.
- Deepen local runtime lifecycle management beyond the managed `llama.cpp` path.
- Expand live provider coverage for streaming, pricing, retries, and provider-specific edge cases.
- Harden worker imports and Docker/Colima sandboxing for code and agent benchmarks.
- Keep clean-machine onboarding and packaging verified while adding signed/notarized distribution coverage.

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the full milestone plan.

## Contributing

Read [`AGENTS.md`](AGENTS.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md) before making changes. Keep changes scoped, preserve user data under `.benchforge/`, and include verification commands in every pull request. For UI changes, include screenshots or a short screen recording. For benchmark, runner, export, or security changes, run the relevant smoke target plus `make benchmark-readiness` when practical.

Security issues should be reported privately using [`SECURITY.md`](SECURITY.md). Project participation is covered by [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Community And Governance

- [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) defines expected community behavior.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) describes setup, pull request expectations, coding style, and safety rules.
- [`SECURITY.md`](SECURITY.md) explains supported versions, vulnerability reporting, scope, and hardening expectations.
- [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md) captures verification, screenshots, and risk notes for reviews.
- [`LICENSE`](LICENSE) contains the Apache License 2.0 terms.

## License

BenchForge is licensed under the Apache License 2.0. See [`LICENSE`](LICENSE).
