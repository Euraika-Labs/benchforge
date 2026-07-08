# Architecture

## Architectural style

BenchForge is a desktop app with a local orchestration core and isolated benchmark workers.

```text
┌──────────────────────────────────────────────────────────────┐
│ BenchForge.app                                                │
│                                                              │
│  React/Svelte UI                                             │
│      │ invoke/events                                         │
│      ▼                                                       │
│  Tauri command boundary                                      │
│      │                                                       │
│      ▼                                                       │
│  Rust core                                                   │
│  ├─ Adapter registry                                         │
│  ├─ Run planner                                              │
│  ├─ Sandbox manager                                          │
│  ├─ Process supervisor                                       │
│  ├─ Metrics collector                                        │
│  ├─ Result importer/exporter                                 │
│  └─ SQLite store                                             │
│      │                                                       │
│      ├────────── direct model calls                          │
│      │        ┌────────────────────────────────────┐          │
│      │        │ OpenAI/Anthropic/Mistral/local API │          │
│      │        └────────────────────────────────────┘          │
│      │                                                       │
│      ├────────── CLI agents                                  │
│      │        ┌────────────────────────────┐                  │
│      │        │ codex/claude/vibe/copilot │                  │
│      │        └────────────────────────────┘                  │
│      │                                                       │
│      └────────── Python workers                              │
│               ┌──────────────────────────────────────┐       │
│               │ EvalPlus/Aider/T-Bench/SWE-bench     │       │
│               └──────────────────────────────────────┘       │
└──────────────────────────────────────────────────────────────┘
```

## Key design decision

The GUI never talks directly to model providers or CLIs. It talks to the Rust core. The Rust core creates a run plan, starts isolated workers/processes, streams events, and persists results.

This makes benchmarks observable, cancellable, reproducible, and safer.

## Component responsibilities

### UI

Responsibilities:

- target configuration;
- benchmark pack selection;
- run queue controls;
- live logs and progress;
- comparison tables;
- result drill-down;
- diff viewing;
- export controls;
- Doctor/diagnostics.

Non-responsibilities:

- no direct provider calls;
- no direct shell execution;
- no secret handling beyond invoking keychain-backed commands.

### Rust core

Responsibilities:

- validate benchmark packs and adapters;
- expand run matrix: targets × tasks × repetitions;
- create per-run workspace;
- supervise processes;
- enforce timeouts;
- stream logs/events to UI;
- collect git diffs;
- run evaluation commands;
- import worker results;
- store results.

### Adapter registry

Loads adapter YAML/JSON from:

```text
built-ins/adapters
user config directory
project .benchforge/adapters
```

Adapters are declarative where possible and executable only through approved adapter kinds.

### Run planner

Input:

```text
targets[]
benchmark_pack
tasks[]
run_config
sandbox_policy
```

Output:

```text
RunPlan
- run_id
- task_id
- target_id
- workspace_path
- command or model call plan
- scoring plan
- metrics plan
- timeout policy
- secret references
```

### Sandbox manager

Initial implementation:

- Docker/Colima for command execution and test evaluation;
- isolated workspace copy per target/task/repetition;
- network disabled by default for evaluation phase;
- explicit network allowlist when required.

Later:

- macOS VM isolation;
- Firecracker on Linux runners;
- remote runners.

### Process supervisor

Responsibilities:

- spawn CLI agents;
- spawn Python workers;
- stream stdout/stderr;
- track exit code;
- kill timed-out processes;
- redact secrets from logs;
- capture command metadata;
- optionally allocate PTY for CLIs that behave differently without TTY.

### Python worker layer

The worker layer exists to avoid rewriting large benchmark ecosystems in Rust.

Contract:

```bash
benchforge-worker run \
  --benchmark-pack ./benchmark-packs/evalplus \
  --target-config ./tmp/target.json \
  --run-config ./tmp/run.json \
  --output ./tmp/result.jsonl
```

The Rust core treats workers as black-box commands that emit structured JSONL events and final result objects.

### Storage

SQLite is enough for v1.

Tables:

- targets;
- benchmark_packs;
- tasks;
- runs;
- run_events;
- artifacts;
- metrics;
- costs;
- diagnostics;
- app_settings.

DuckDB can be added later for analytics across large result corpora.

## Run lifecycle

```text
1. Validate targets
2. Validate benchmark pack
3. Resolve tasks
4. Build run matrix
5. Create isolated workspace per run
6. Apply task fixture
7. Start target execution
8. Stream logs/events
9. Stop on success/timeout/error
10. Capture git diff and artifacts
11. Run scoring in clean evaluation phase
12. Store result
13. Update comparison table
14. Export if requested
```

## Direct model run flow

```text
UI → Rust core → Adapter registry → Direct model adapter
                      │
                      ▼
              BenchForge harness
                      │
                      ├─ read files
                      ├─ ask model for patch/tool call
                      ├─ apply edits
                      ├─ run tests
                      └─ retry according to policy
```

Direct model runs require BenchForge to provide its own harness. Without that, raw models cannot fairly compete against agent CLIs on repo-editing tasks.

## CLI agent run flow

```text
UI → Rust core → CLI adapter → Process supervisor
                      │
                      ▼
              sandboxed workspace
                      │
                      ├─ codex exec / claude -p / vibe --prompt / copilot -p
                      ├─ stdout/stderr/transcript capture
                      ├─ git diff capture
                      └─ scoring phase
```

The CLI agent owns the agent loop. BenchForge only controls the workspace, prompt, permissions, timeout, and scoring.

## Benchmark harness run flow

```text
UI → Rust core → Python worker → external harness
                      │
                      ▼
       EvalPlus / Aider / Terminal-Bench / SWE-bench
                      │
                      ▼
              normalized result import
```

## Event protocol

Workers and runners should emit JSONL events.

```json
{"type":"run_started","run_id":"...","timestamp":"..."}
{"type":"stdout","run_id":"...","data":"..."}
{"type":"metric","run_id":"...","name":"wall_time_ms","value":1234}
{"type":"artifact","run_id":"...","kind":"git_diff","path":"..."}
{"type":"run_finished","run_id":"...","status":"passed"}
```

## Concurrency model

v1:

- default concurrency = 1 for local models;
- default concurrency = 2 for cloud APIs;
- CLI agent concurrency default = 1;
- user can override with warnings.

Rationale: local Mac benchmarks become meaningless if too many local model runs contend for GPU/RAM.

## Adapter compatibility strategy

Every local runtime should be attempted through OpenAI-compatible API first. Native adapters are only used when OpenAI-compatible mode is missing key features.

```text
OpenAI-compatible:
- Ollama
- LM Studio
- llama.cpp server
- vLLM
- many gateways

Native/direct:
- Anthropic
- Mistral
- OpenAI Responses API
- CLI agents
```

## Security boundary

The Tauri app and Rust core are not considered sandboxed. The benchmark target is untrusted. The target must run inside an isolated workspace and ideally inside a container or VM.

## Packaging

Tauri builds the desktop application and can produce macOS `.app` and `.dmg` bundles on macOS.

## Future architecture extensions

- Remote runners for GPU Linux machines.
- Shared team result database.
- Cloud artifact storage.
- Plugin marketplace for benchmark packs and adapters.
- MCP/ACP server mode to let other agent tools control BenchForge.
- Deterministic replay of runs.
