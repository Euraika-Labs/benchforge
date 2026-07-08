# BenchForge

BenchForge is a macOS desktop workbench for benchmarking local and cloud LLMs with reproducible runs, inspectable artifacts, latency and token metrics, cost estimates, normalized errors, and shareable reports.

It is built for practical model selection: add local or cloud targets, run the same benchmark pack across them, compare quality beside speed and cost, then export enough evidence for another engineer to reproduce or challenge the result.

Run development commands from this directory unless a command below says otherwise.

This directory contains the working application, built-in benchmark packs, adapter definitions, Python worker harnesses, packaging scripts, and implementation docs. The repository root contains community files and the public-facing README.

## Contents

- [Status](#status)
- [What BenchForge Can Benchmark](#what-benchforge-can-benchmark)
- [Key Features](#key-features)
- [Repository Layout](#repository-layout)
- [Requirements](#requirements)
- [Install From Source](#install-from-source)
- [End-to-End Workflow](#end-to-end-workflow)
- [First Benchmark Recipes](#first-benchmark-recipes)
- [Configure Secrets](#configure-secrets)
- [Run A Local Hugging Face Model](#run-a-local-hugging-face-model)
- [Run Another Local Runtime](#run-another-local-runtime)
- [Run A Cloud Benchmark](#run-a-cloud-benchmark)
- [Run CLI Agent Benchmarks](#run-cli-agent-benchmarks)
- [Benchmark Packs](#benchmark-packs)
- [External Harnesses And Imports](#external-harnesses-and-imports)
- [Reports And Evidence](#reports-and-evidence)
- [Data, Secrets, And Safety](#data-secrets-and-safety)
- [Development And Verification Commands](#development-and-verification-commands)
- [Packaging](#packaging)
- [Troubleshooting](#troubleshooting)
- [Known Limits](#known-limits)
- [Documentation Map](#documentation-map)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [Community And Governance](#community-and-governance)
- [License](#license)

## Status

BenchForge is an active pre-1.0 implementation, not a static mockup. The Tauri desktop app, Rust backend, Python worker bridge, benchmark packs, target registry, Hugging Face GGUF workflow, cloud provider adapters, job queue, result storage, comparison views, report-folder exports, and macOS DMG packaging checks are implemented.

Use it today as an engineering benchmark harness. Do not treat bundled packs as a definitive public leaderboard yet. Several packs are useful for directional model selection, while calibrated public and private suites remain a roadmap priority. The app surfaces evidence-grade, calibration, model-identity, cost, and generation-setting warnings so weak evidence is not mistaken for a final decision.

The Vite browser build is only a frontend preview. Keychain storage, local downloads, provider calls, subprocesses, benchmark execution, and packaging require the Tauri desktop app.

There is no stable release channel yet. For now, install from source with the commands below or build a local DMG from this checkout.

## What BenchForge Can Benchmark

- Local model runtimes: Hugging Face GGUF through `llama.cpp`, Ollama, LM Studio, vLLM, MLX / `mlx-lm`, oMLX-style servers, and generic OpenAI-compatible endpoints.
- Cloud providers: OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, Google Gemini, and generic compatible APIs.
- Agent and harness targets: Codex CLI, Claude Code, GitHub Copilot CLI, Mistral Vibe CLI, BenchForge Python workers, EvalPlus, Aider Polyglot, Terminal-Bench, and SWE-bench-style commands.
- Worker-backed tasks: repo/code-edit tasks, defensive security checks, external harness commands, and imported benchmark output.
- Prompt benchmark packs: connectivity, instruction following, structured output, grounded context, decision making, practical model selection, and reliability checks.

## Key Features

- Secure target setup with macOS Keychain-backed API keys, editable input/output and prompt-cache pricing, visible pricing-assumption warnings, and redacted target configs.
- Target validation with tiny completion probes and actionable error codes such as `auth`, `rate_limit`, `model_not_found`, `timeout`, and `malformed_response`.
- Hugging Face GGUF search, file selection, download planning, disk checks, token handling, `curl` fallback, `llama-server` startup, target registration, and optional benchmark handoff.
- Cloud catalog search and validation probes for supported providers.
- Background jobs with progress, cancellation, retry, duplicate, startup recovery, and grouped run history.
- Metrics for score, pass/fail, wall time, setup time, target time, evaluation time, p95 latency, process exit codes, stdout/stderr byte counts, files changed, lines added/deleted, observed command counts, dangerous command hits, peak RSS when observable, provider timing, time to first token when available, token usage, reasoning tokens, prompt-cache read/write tokens when providers report them, throughput, retry attempts, retry delay, HTTP status, finish reason, served model identity, estimated cost, and pricing assumptions, including retry evidence for terminal provider failures.
- Stable v1 export aliases such as `pass_fail`, `score_numeric`, `input_tokens`, `output_tokens`, `estimated_cost_usd`, `ttft_ms`, and `decode_tokens_per_sec`; `result.json` artifacts include all required v1 metric keys and use explicit `null` values for unsupported metrics.
- Worker harness support for running configured EvalPlus, Aider, Terminal-Bench, SWE-bench-style commands or importing existing benchmark result files from the worker workspace, with structured summaries prioritized before large logs and read-file provenance recorded.
- Artifact inspection for prompts, responses, raw provider payloads, logs, diffs, scorer output, worker events, and redacted target configs.
- Exports to CSV, JSONL, Analysis JSON, Markdown, and full report folders with reproducibility metadata, Docker scoring image evidence when used, and review-before-sharing warnings.

## Repository Layout

| Path | Purpose |
| --- | --- |
| `app-scaffold/` | Tauri desktop app. React/TypeScript UI is in `src/`; Rust backend is in `src-tauri/src/`. |
| `workers/` | Python worker package for external harnesses and security checks. |
| `benchmark-packs/` | Built-in benchmark pack definitions and task YAML. |
| `adapters/` | YAML adapter definitions for local runtimes, cloud providers, CLI agents, and harness targets. |
| `fixtures/` | Small test projects used by repo/code and security smoke benchmarks. |
| `scripts/` | Bootstrap, doctor, readiness, smoke, schema validation, and packaging scripts. |
| `docs/` | Product requirements, architecture, security model, data model, benchmark strategy, roadmap, and UI specs. |
| `prompts/` | Ordered implementation prompts from the original blueprint. |
| `.benchforge/` | Local runtime data, models, runs, exports, and readiness logs. This is user data and should not be committed. |

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

## Install From Source

Install the host toolchain first. On a fresh macOS machine, a typical setup is:

```bash
brew install node python git llama.cpp
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then bootstrap BenchForge:

```bash
./scripts/bootstrap.sh
make doctor
make test
make dev
```

`./scripts/bootstrap.sh` installs app dependencies and creates an editable Python worker virtual environment under `workers/.venv/`. It selects Python 3.10+ from `BENCHFORGE_PYTHON`, `python3`, or versioned `python3.10+` commands and stops with a repair hint if only older Python is available.

`make doctor` checks required host tools (`git`, Node, npm, Cargo, and Python 3.10+) and reports optional tools such as Docker, Colima, Hugging Face CLI, and `llama-server` as warnings.

`make test` runs schema validation, the TypeScript/Vite production build, Rust tests, Python worker tests, and the worker CLI help check.

`make dev` starts the Tauri desktop app. Use `cd app-scaffold && npm run dev` only when you want the browser preview with sample data.

Choose the fastest path for the thing you want to prove:

| Goal | Start with |
| --- | --- |
| Test the app and built-in fixtures | `make test && make benchmark-readiness` |
| Benchmark a GGUF model from Hugging Face | `make dev`, then use Settings -> Hugging Face Local Model |
| Compare a local server to a cloud model | Add/validate both targets, then run an `llm-*` pack with at least 3 repetitions |
| Verify report quality | `make report-smoke` or export a report folder from Results |
| Build a macOS installer | `make package-dmg && make verify-dmg && make install-smoke-dmg` |

## End-to-End Workflow

1. Run `make doctor` and fix required tools before adding targets.
2. Add at least one target: a local runtime such as Hugging Face GGUF, Ollama, LM Studio, or `llama.cpp`, or a cloud provider such as OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, or Gemini.
3. Validate the target. Validation runs a tiny probe and stores the latest health result so full benchmark runs fail less mysteriously.
4. Choose a benchmark pack, repetitions, warmups, concurrency, and a max-cost cap for paid providers.
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

In the desktop app, open Settings, save `HF_TOKEN` if the model is gated, then use Hugging Face Local Model to search for a GGUF repository, pick a quantized file, download it, start `llama-server`, register the target, and run `llm-connectivity` or `llm-basics`. If a priced cloud target already exists, enable **Compare with cloud target** before starting to queue a capped local/cloud benchmark after the local target validates.

### Local Vs Cloud Comparison

1. Add and validate a local OpenAI-compatible target, such as Ollama, LM Studio, or `llama.cpp`.
2. Add and validate a cloud target, such as OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, or Gemini.
3. In Run Builder, choose an `llm-*` pack, set at least 3 repetitions, and set a max-cost cap for paid providers.
4. Compare pass rate, score, p95 latency, throughput, cost, served model identity, and evidence warnings in Results.

## Configure Secrets

Use the app's Settings and Targets screens whenever possible. BenchForge stores provider keys in macOS Keychain and keeps raw secret values out of target JSON and exports. Environment variables are also supported when launching from a shell.

| Service | Environment variable | Notes |
| --- | --- | --- |
| Hugging Face | `HF_TOKEN` | Required for gated/private models; public downloads may work without it. Accept gated model licenses on Hugging Face before downloading. |
| OpenAI | `OPENAI_API_KEY` | Used by OpenAI targets and generic OpenAI-compatible targets when configured. |
| Anthropic | `ANTHROPIC_API_KEY` | Used for validation and live cloud probes. |
| Mistral | `MISTRAL_API_KEY` | Used for Mistral catalog and run calls. |
| OpenRouter | `OPENROUTER_API_KEY` | Useful for broad model comparisons through one provider. |
| Azure OpenAI | `AZURE_OPENAI_API_KEY` | Use the Azure `/openai/v1` base URL and put the deployment name in the Model field. |
| Google Gemini | `GEMINI_API_KEY` | Uses Gemini's OpenAI-compatible endpoint. |

Example shell launch with keys:

```bash
OPENAI_API_KEY=sk-... HF_TOKEN=hf_... make dev
```

Do not commit `.env` files, API keys, downloaded models, exported reports, or `.benchforge/` runtime data.

## Run A Local Hugging Face Model

1. Start the app with `make dev`.
2. Open Doctor and fix required setup issues. For GGUF workflows, install `llama.cpp`; the Hugging Face CLI is recommended but BenchForge can fall back to `curl`.
3. In Settings, save a Hugging Face token if you need gated or private model access.
4. Use Hugging Face Local Model to search popular GGUF repositories, optionally pin a branch, tag, or commit in Revision, inspect concrete `.gguf` files, and choose a quantized file.
5. Leave Start after download enabled to let BenchForge download the model, start `llama-server`, register a local target, and optionally queue a benchmark pack. Enable Compare with cloud target when you want the automatic benchmark to include an existing priced cloud target.
6. Open Results to inspect metrics and artifacts, then export a report folder.

Useful smoke checks:

```bash
make hf-local-smoke
make hf-local-cloud-basics-smoke
```

## Run Another Local Runtime

BenchForge works best when local runtimes expose an OpenAI-compatible API.

| Runtime | Typical setup | Target notes |
| --- | --- | --- |
| Ollama | Start Ollama and pull a model, for example `ollama pull llama3.1:8b`. | Use the Ollama adapter with `http://localhost:11434/v1` and the Ollama model tag. |
| LM Studio | Start the local server from LM Studio. | Use the LM Studio adapter with its local `/v1` base URL, often `http://localhost:1234/v1`. |
| llama.cpp | Run `llama-server -m /path/model.gguf --port 8080`. | Use the llama.cpp adapter with `http://127.0.0.1:8080/v1`. |
| vLLM / MLX / custom server | Start the server with an OpenAI-compatible endpoint. | Use the matching adapter or the generic OpenAI-compatible adapter. |

After adding a target, run Validate before benchmarking. Validation records the latest health result and catches missing endpoints, model names, or malformed responses before a full run.

## Run A Cloud Benchmark

1. Open Targets and choose a cloud adapter such as OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, or Google Gemini.
2. Paste the API key. BenchForge stores it in macOS Keychain and keeps raw secret values out of target JSON and exports.
3. Search the provider catalog when available, or enter a model manually.
4. Validate the target. Validation performs a tiny completion probe and stores the latest health result.
5. Choose a benchmark pack, repetitions, warmups, concurrency, and a max-cost cap. BenchForge blocks capped cloud runs when pricing is missing.
6. Run the benchmark and compare results by target, provider, model, pack, task, run group, and status.

Live provider probes are outside the default readiness gate and skip cleanly when no provider key is configured.

```bash
BENCHFORGE_LIVE_CLOUD_PROVIDERS=openai,anthropic,openrouter,gemini make live-cloud-smoke
BENCHFORGE_LIVE_CLOUD_RUN=1 make live-cloud-smoke
```

## Run CLI Agent Benchmarks

CLI agent targets run product agents such as Codex CLI, Claude Code, GitHub Copilot CLI, and Mistral Vibe against repo/code tasks. BenchForge creates an isolated Git workspace, renders the adapter prompt, applies configured adapter environment variables, runs the CLI with a timeout, captures `cli-stdout.txt`, `cli-stderr.txt`, `cli-agent-command.json`, `diff.patch`, scorer output, and the final `result.json`.

Built-in CLI adapters live under `adapters/agents/`. Advanced users can also create a custom CLI target by setting `command`, `args`, optional `working_dir`, `env`, and `validation.command_args` in target config. Template variables include `{{prompt}}`, `{{workspace}}`, `{{model}}`, and `{{max_turns}}`.

CLI command metadata in artifacts and reports redacts secrets and replaces the raw task prompt with `<task_prompt>` while preserving prompt SHA-256 hashes for reproducibility.

## Benchmark Packs

Built-in pack IDs include:

| Pack | Use |
| --- | --- |
| `llm-connectivity` | Fast sanity check that a target can answer. |
| `llm-basics` | Small instruction-following and JSON/summarization checks. |
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

Pack calibration metadata is captured in run reproducibility and affects whether results can become comparison-ready. Built-in LLM comparison packs declare calibration quality gates for local+cloud baseline evidence, provider-confirmed served model IDs, full pack/task coverage, at least 3 repetitions per task/target, cloud cost metrics, and one generation policy.

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

Import paths must resolve inside the per-run worker workspace or output directory. Supported inputs are CSV, JSON, JSONL, JUnit XML, text, log, and output files or directories containing those files. Symlinked result files in import directories are rejected so imported evidence cannot escape the worker boundary. Directory imports are bounded by file and byte limits; if BenchForge truncates input or omits supported files, the final result records that as import provenance. BenchForge preserves the raw imported output as an artifact, records import provenance such as format, source type, file count, per-file SHA-256 fingerprints for consumed evidence, and summary parser, and extracts common `total`, `passed`, `failed`, `score`, `pass@1`, and accuracy fields. It also normalizes CSV or JSONL streams of per-case `passed`/`resolved`/`status` records, JUnit `testsuite`/`testcase` reports, and nested `results`/`instances` maps used by external code and agent suites.

## Reports And Evidence

The Results page ranks targets by weighted pass rate, pass rate, average score, score spread, p95 latency, cost, throughput, and sample size. It also shows normalized-error recovery hints with target-repair shortcuts that preload editable target forms when possible, plus per-pack scoped Run Builder reruns that skip unavailable historical targets, and warnings for incomplete pack/task coverage, low repetitions, missing pricing, prompt-cache pricing assumptions, missing provider-confirmed model identity, mixed generation settings, and uncalibrated packs. Run Builder also surfaces pre-run validation blockers with the same target-repair flow.

Report-folder exports include:

- `README.md` with a human-readable run summary, including worker-import provenance when external benchmark results were imported.
- `results.csv` and `results.jsonl`.
- `analysis.json` with rankings, evidence grade, risks, task drilldowns, normalized-error recovery hints, worker-import summaries with file fingerprints, and recommended next run.
- `reproducibility.json` with run config, generation settings, prompt/system-prompt hashes, redacted targets, benchmark metadata, host profile, pricing snapshots, scoring command versions, CLI-agent command/transcript checksums, worker-import file fingerprints, workspace Git baseline/diff hashes for repo/code runs, and Docker scoring image ID/digest plus Dockerfile checksum when Docker scoring is used.
- `artifacts.json` with artifact copy status, sensitivity flags, and review summaries.
- Copied artifact files when safe and available.

Review exported prompts, responses, raw payloads, diffs, and logs before sharing them outside your environment.

## Data, Secrets, And Safety

- Source/dev runs store local app data under `.benchforge/`; the installed macOS app uses `~/Library/Application Support/BenchForge`. Use `BENCHFORGE_DATA_DIR=/path/to/data` to run with an isolated store.
- API keys are stored in macOS Keychain or read from configured environment variables. They should not be committed or placed in target JSON.
- Target configs and exports redact secret-shaped fields such as `authorization`, `access_token`, `client_secret`, `token`, and `password`.
- Repo/code tasks run in disposable Git workspaces. BenchForge records the baseline commit/tree, captures `diff.patch` including untracked source files, excludes generated caches such as `.benchforge-venv` and `node_modules`, and records sandbox level plus permission mode in artifacts and reports.
- Docker-backed scoring applies to eligible Python repo/code tasks, runs scorer containers with network disabled, and records the runner image identity plus Dockerfile checksum in artifacts and reports.
- Worker harness child processes inherit a minimal environment by default; secret-bearing variables require explicit `env_passthrough`.
- CLI-agent command evidence redacts secrets and elides raw task prompts in metadata; review copied CLI stdout/stderr before sharing exports.
- Managed `llama-server` processes are marked so BenchForge can clean up only servers it owns.

## Development And Verification Commands

| Command | Description |
| --- | --- |
| `make dev` | Start the Tauri desktop app in development mode. |
| `make test` | Run schema validation, web build, Rust tests, Python worker tests, and worker CLI help. |
| `make benchmark-readiness` | Default offline readiness gate for local/cloud benchmark functionality, worker harness imports, and report evidence. Writes logs under `.benchforge/readiness/`. |
| `make benchmark-readiness-full` | Extended gate before packaging or handoff. Adds release preflight, provider-error contracts, reliability, DMG, and installed-app smoke checks. |
| `make live-cloud-smoke` | Optional real-provider probe. Skips without configured provider keys unless `BENCHFORGE_LIVE_CLOUD_RUN=1` requests a real benchmark run. |
| `make first-run-smoke` | Verify a clean app store using a temporary `BENCHFORGE_DATA_DIR`. |
| `make local-runtime-discovery-smoke` | Verify local runtime discovery, validation, and handoff contracts. |
| `make cloud-provider-job-smoke` | Verify provider-style queued jobs, snapshots, metrics, retry evidence, and exports. |
| `make cloud-catalog-smoke` | Verify provider catalog parsing, pricing/context metadata, and catalog-to-target redaction. |
| `make report-smoke` | Verify report export content. |
| `make worker-harness-contract-smoke` | Verify external harness command execution and JSON/JSONL/CSV/JUnit import contracts. |
| `make smoke-docker` | Verify Docker/Colima scoring, network-off container execution, and Docker image reproducibility metadata. |
| `make release-preflight` | Verify release hygiene: license/community files, GitHub templates, Tauri bundle metadata, package lock, icon, and packaging docs. |
| `make package-dmg` | Build a local macOS DMG through `scripts/package-dmg-macos.sh`. |
| `make verify-dmg` | Verify the latest built DMG checksum, mount it, inspect `BenchForge.app`, and report signature status. |
| `make install-smoke-dmg` | Copy the app out of the latest DMG and run first-run, worker, and security-pack smokes from bundled resources. |
| `make package-release-dmg` | Build a public macOS DMG with signing and notarization checks enabled. |
| `make verify-distribution-dmg` | Require Developer ID signing, Gatekeeper assessment, and notarization ticket validation for the latest DMG. |

Use focused `make *-smoke` targets when changing a specific workflow. Use `make benchmark-readiness` before handing benchmark-critical changes to another user.

## Packaging

Build a local DMG with:

```bash
make release-preflight
make package-dmg
make verify-dmg
make install-smoke-dmg
```

For a public release DMG, enable distribution checks and provide Apple signing plus notarization credentials:

```bash
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
| The browser preview opens but benchmarks do not run. | Use `make dev`; the Vite preview cannot access Keychain, subprocesses, provider calls, or local downloads. |
| Cloud target validation says no key is configured. | Save the provider key in the app or launch with the matching environment variable, such as `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`. |
| A capped cloud run is blocked. | Add input/output pricing metadata for the target/model or remove the max-cost cap. Add cache read/write pricing too when you want provider-reported prompt-cache tokens priced separately. BenchForge blocks capped runs when it cannot estimate cost. |
| Results mention pricing assumptions. | Add cache read/write token prices for the affected target, then rerun or re-export before treating cost ranking as decisive. Without those prices, cache tokens are visibly priced with the normal input-token rate. |
| Local target validation fails. | Confirm the server is running, the base URL ends in `/v1` when required, and the model name matches the runtime's model list. |
| Readiness fails. | Open the summary path printed by `make benchmark-readiness`; detailed logs are under `.benchforge/readiness/`. |

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
| [`../README.md`](../README.md) | Public repository landing page. |
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
- Improve clean-machine onboarding, packaging, and distribution signing.

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the full milestone plan and current implementation status.

## Contributing

Read [`../AGENTS.md`](../AGENTS.md), [`AGENTS.md`](AGENTS.md), and [`../CONTRIBUTING.md`](../CONTRIBUTING.md) before making changes. Keep changes scoped, preserve user data under `.benchforge/`, and include verification commands in every pull request. For UI changes, include screenshots or a short screen recording. For benchmark, runner, export, or security changes, run the relevant smoke target plus `make benchmark-readiness` when practical.

Security issues should be reported privately using [`../SECURITY.md`](../SECURITY.md). Project participation is covered by [`../CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md).

## Community And Governance

- [`../CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md) defines expected community behavior.
- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) describes setup, pull request expectations, coding style, and safety rules.
- [`../SECURITY.md`](../SECURITY.md) explains supported versions, vulnerability reporting, scope, and hardening expectations.
- [`../.github/PULL_REQUEST_TEMPLATE.md`](../.github/PULL_REQUEST_TEMPLATE.md) captures verification, screenshots, and risk notes for reviews.
- [`../LICENSE`](../LICENSE) contains the Apache License 2.0 terms.

## License

BenchForge is licensed under the Apache License 2.0. See [`../LICENSE`](../LICENSE).
