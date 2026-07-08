# UI/UX specification

## UX principle

BenchForge is not a chat app. It is a lab bench. The interface should make it obvious what was tested, how it was tested, and why a score changed.

## Navigation

```text
Dashboard
Targets
Benchmarks
Runs
Results
Artifacts
Doctor
Settings
```

## Dashboard

Purpose: show current health and recent benchmark outcomes.

Widgets:

- configured targets count;
- last run status;
- best target by selected benchmark;
- cost this week;
- local runtime status;
- Docker/Colima status;
- pending warnings.

## Targets screen

Columns:

```text
Name | Kind | Adapter | Model | Endpoint/CLI | Status | Last validated | Actions
```

Actions:

- Add target;
- Validate;
- Duplicate;
- Disable;
- Export redacted config;
- Delete.

Target creation wizard:

1. Choose type: Local model, Cloud model, CLI agent, Harness.
2. Choose adapter template.
3. Fill config.
4. Validate.
5. Save.

## Benchmarks screen

Columns:

```text
Pack | Version | Type | Tasks | Estimated runtime | Sandbox required | Actions
```

Benchmark detail:

- task list;
- languages;
- scoring methods;
- required tools;
- network requirements;
- estimated cost;
- warnings.

## Run builder

The run builder is the most important screen.

Layout:

```text
┌────────────────────────┬───────────────────────────┐
│ Targets                │ Benchmark pack             │
│ [x] Claude Code        │ [quick-smoke]              │
│ [x] Codex CLI          │ Tasks: 3 selected          │
│ [x] Qwen Ollama        │                           │
│ [ ] GPT cloud          │ Run config                 │
│                        │ repetitions: 1             │
│                        │ timeout: 900s              │
│                        │ sandbox: Docker            │
│                        │ network: provider-only     │
└────────────────────────┴───────────────────────────┘
```

The task selector defaults to every task in the pack, but users can narrow a run to specific task IDs. Run estimates and the Comparison Readiness panel must use the selected task count; tiny subsets are useful for diagnostics but should not be labeled comparison-grade evidence. Run Builder also shows the selected pack's evidence profile and warnings, and prompt packs must be `prompt_comparison` before the panel can label a local/cloud run comparison-grade.

Before starting, show:

- number of runs;
- estimated time;
- estimated cost;
- sandbox warnings;
- missing validation warnings.

## Live run screen

Display:

- run queue;
- active target/task;
- live stdout/stderr;
- progress events;
- cancel button;
- resource metrics where available.

Each run row:

```text
Target | Task | Status | Wall time | Tokens | Cost | Tests | Warnings
```

## Results comparison

Default matrix:

```text
Target | Pass % | Score | Cost | Time | Tokens | TTFT | tok/s | Files changed | Warnings
```

Filters:

- benchmark pack;
- task;
- target type;
- date range;
- status;
- language.

Charts:

- pass rate by target;
- cost vs pass rate;
- time vs pass rate;
- token usage by target;
- failure taxonomy.

## Run detail

Tabs:

```text
Summary
Prompt
Logs
Diff
Tests
Metrics
Artifacts
Reproducibility
```

### Summary

- status;
- score;
- target;
- task;
- duration;
- cost;
- key warnings.

### Prompt

- system prompt;
- user prompt;
- task instructions;
- prompt hash;
- redacted secrets notice.

### Logs

- stdout;
- stderr;
- transcript;
- stream events;
- search/filter.

### Diff

- file tree;
- unified diff;
- changed line count;
- copy patch button.

### Tests

- scoring command;
- exit code;
- test summary;
- raw output.

### Reproducibility

Show exact:

- BenchForge version;
- adapter version;
- model id;
- CLI version;
- benchmark pack checksum;
- fixture checksum;
- sandbox image digest;
- run config.

## Doctor screen

Checks:

```text
Git
Docker
Colima
Python
Node
Rust
Ollama endpoint
LM Studio endpoint
Codex CLI
Claude Code
Mistral Vibe
GitHub Copilot CLI
API keys
Disk space
Memory
```

Each check has:

- status;
- detected version;
- remediation.

## Settings

Sections:

- General;
- Workspaces;
- Secrets;
- Sandbox;
- Cost models;
- Export;
- Advanced.

## UX copy tone

Clear, direct, not cute.

Examples:

```text
This run will execute an agent with file-write and shell permissions inside a Docker sandbox.
```

```text
This target has not been validated. The run may fail before scoring.
```

```text
Do not compare this score with a raw model score. This is a product-agent run.
```

## Accessibility

- keyboard navigation;
- readable monospace logs;
- high contrast diff view;
- no color-only pass/fail indicators;
- exportable data.
