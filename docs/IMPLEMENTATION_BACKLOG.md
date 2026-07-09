# Implementation backlog

## Epic 1: App foundation

### BF-001 Create buildable Tauri shell

Acceptance:

- `npm run tauri:dev` opens a desktop window.
- Window displays Dashboard, Targets, Benchmarks, Runs, Results, Doctor, Settings nav.

### BF-002 Add Rust command boundary

Acceptance:

- UI can call `list_targets`, `list_benchmark_packs`, and `run_doctor`.
- Commands return typed JSON.

### BF-003 Add SQLite store

Acceptance:

- migrations run on app startup;
- targets and runs can be persisted;
- tests cover basic CRUD.

## Epic 2: Schemas and registry

### BF-010 Validate adapter schema

Acceptance:

- invalid adapter YAML shows exact path and reason;
- built-in adapter folder loads on startup.

### BF-011 Validate benchmark pack schema

Acceptance:

- pack metadata and tasks validate;
- checksum recorded for pack and task.
- status: schema validation covers adapters, benchmark packs, and task YAML, and now adds semantic validation for built-in prompt comparison packs so calibration metadata cannot omit review scope or required local/cloud quality gates.

### BF-012 Target CRUD

Acceptance:

- create/edit/delete/list targets in UI;
- export target with redacted secrets.

## Epic 3: Doctor checks

### BF-020 Runtime checks

Acceptance:

- detect git, docker, colima, python, node, rust;
- show versions when available.
- status: implemented in the in-app Doctor with Homebrew, npm, cargo, rustc, GUI PATH, Python 3.10+ validation, and shared GUI-safe PATH reuse for sanitized host scoring.

### BF-021 CLI checks

Acceptance:

- detect codex, claude, vibe, copilot;
- show install remediation when missing.
- status: implemented with optional severity and remediation text; local runtime and external harness presets now expose allowlisted check/install actions where safe, while app-specific/manual installers remain documented rather than executed blindly.

### BF-022 Endpoint checks

Acceptance:

- validate Ollama and LM Studio OpenAI-compatible endpoints;
- show model list when available.
- status: default endpoint probes now cover Ollama, LM Studio, llama.cpp, vLLM, MLX / mlx-lm, and oMLX; target creation remains on the Targets screen.

## Epic 4: Runner

### BF-030 Run planner

Acceptance:

- targets × tasks × repetitions expands into run queue.
- bounded concurrency is validated, persisted, and included in reproducibility metadata.

### BF-031 Workspace manager

Acceptance:

- per-run workspace created;
- fixture copied;
- git initialized;
- cleanup policy works.

### BF-032 Process supervisor

Acceptance:

- spawn subprocess;
- stream logs;
- enforce timeout;
- kill on cancel;
- capture exit code.

### BF-033 Artifact capture

Acceptance:

- stdout/stderr saved;
- git diff saved;
- scoring output saved.

## Epic 5: Scoring and sandbox

### BF-040 Scoring commands

Acceptance:

- scoring command runs after target execution;
- test pass/fail is parsed into normalized result.

### BF-041 Docker scoring

Acceptance:

- scoring can run in container;
- network can be disabled;
- workspace mounted read/write or read-only depending phase.

### BF-042 Safety scanner

Acceptance:

- suspicious commands are flagged;
- possible secret strings are redacted and warned.

## Epic 6: Direct model harness

### BF-050 OpenAI-compatible client

Acceptance:

- Ollama/LM Studio target can complete a simple prompt;
- token usage imported if provider returns it.

### BF-051 Anthropic client

Acceptance:

- Anthropic target can complete a simple prompt;
- cost metadata captured where usage is available.

### BF-052 Patch protocol

Acceptance:

- model receives task and fixture context;
- returns unified diff or file edits;
- edits are applied;
- tests run;
- one retry with test feedback works.

## Epic 7: CLI agents

### BF-060 Codex CLI adapter

Acceptance:

- validates `codex` exists;
- runs non-interactive task;
- captures diff/logs.

### BF-061 Claude Code adapter

Acceptance:

- validates `claude` exists;
- runs `claude -p` with JSON or stream-json where configured;
- captures cost/session metadata where available.

### BF-062 Mistral Vibe adapter

Acceptance:

- validates `vibe` exists;
- runs `vibe --prompt` with bounded max turns;
- captures JSON output where available.

### BF-063 GitHub Copilot CLI adapter

Acceptance:

- validates `copilot` exists;
- supports `copilot -p`;
- supports `--model` and allow-tool flags;
- supports BYOK env config as target option.

## Epic 8: Benchmark workers

### BF-070 Python worker CLI

Acceptance:

- `benchforge-worker run ...` emits JSONL events.

### BF-071 EvalPlus worker

Acceptance:

- runs a tiny EvalPlus subset or dry run;
- imports pass@1-style result.

### BF-072 Terminal-Bench worker

Acceptance:

- runs a selected task or imports existing result;
- maps to normalized result.

### BF-073 Aider worker

Acceptance:

- runs subset tasks where dependencies are available;
- imports pass/fail, cost/time when available.

## Epic 9: UI and export

### BF-080 Run builder UI

Acceptance:

- select targets and benchmark pack;
- set repetitions, warmups, and bounded concurrency;
- show warnings and estimated run count;
- start run.

### BF-081 Live run UI

Acceptance:

- logs stream;
- queue updates;
- cancel works.

### BF-082 Results UI

Acceptance:

- comparison table;
- filters;
- status badges;
- metric columns.

### BF-083 Artifact viewers

Acceptance:

- diff viewer;
- stdout/stderr viewer;
- test output viewer.

### BF-084 Export

Acceptance:

- JSONL export;
- CSV export;
- Markdown report export.

## Epic 10: Packaging

### BF-090 DMG packaging

Acceptance:

- `.dmg` produced on macOS;
- app has icon and bundle identifier;
- README documents signing/notarization path.
- status: unsigned local DMG packaging is wired through `make package-dmg`; `make release-preflight` verifies the root Apache-2.0 license, community files, GitHub templates, Tauri bundle metadata, bundled resource mapping, package lock, generated BenchForge icon, and packaging docs before release handoff; `make verify-dmg` verifies the built image checksum, mounts the DMG, inspects `BenchForge.app` metadata/executable/resource shape including worker source, and reports signature status; `make install-smoke-dmg` copies the app out of the DMG, runs first-run benchmark execution from bundled resources, executes the bundled worker through the installed app, and runs the built-in `security-defensive` worker-backed pack from bundled worker code and fixtures; the bundle has a maintained app icon, stable identifier, and explicit hardened runtime. README documents signing/notarization environment variables without committing credentials, `make release-signing-preflight` validates Developer ID or CI certificate inputs plus notarization credentials when `BENCHFORGE_RELEASE_DISTRIBUTION=1`, `make package-release-dmg` turns that gate on for public builds, and `make verify-distribution-dmg` requires Developer ID signing, Gatekeeper assessment, and stapled notarization ticket validation.

### BF-091 First-run onboarding

Acceptance:

- Doctor opens on first run;
- app explains missing dependencies clearly.
- status: Doctor opens on first launch and once for an empty benchmark workspace that has only the seeded mock target; Doctor groups dependency, endpoint, setup, evidence, remediation, and next-step checks with direct actions.
