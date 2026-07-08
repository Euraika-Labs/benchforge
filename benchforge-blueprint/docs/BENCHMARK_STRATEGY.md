# Benchmark strategy

## Benchmark layers

BenchForge should support multiple layers because no single benchmark answers every question.

```text
Layer 1: performance and smoke checks
Layer 2: classic code generation
Layer 3: file-editing tasks
Layer 4: repo-level bugfixing
Layer 5: terminal-agent tasks
Layer 6: defensive security tasks
Layer 7: private/team benchmark packs
```

## Layer 1: performance and smoke checks

Goal: quickly determine whether a target is reachable, usable, and not absurdly slow.

Metrics:

- time to first token;
- tokens/sec;
- wall time;
- output validity;
- JSON/tool-call validity;
- peak memory where available;
- cost estimate;
- basic patch pass/fail.

Use custom fixtures in `fixtures/`.

## Layer 2: classic code generation

Recommended:

- EvalPlus HumanEval+;
- EvalPlus MBPP+;
- EvalPerf;
- MultiPL-E later.

What it measures:

```text
Can the model generate a function that passes tests?
```

What it does not measure:

```text
Can the model work in a repository, edit files, run tests, and recover from failures?
```

## Layer 3: file-editing tasks

Recommended:

- Aider Polyglot subset first;
- full Aider Polyglot later;
- custom Exercism-like tasks.

What it measures:

- editing ability;
- format adherence;
- test feedback recovery;
- multi-language correctness.

## Layer 4: repo-level bugfixing

Recommended:

- SWE-bench Lite subset;
- SWE-bench Verified subset;
- private GitHub issue benchmark packs.

What it measures:

- repository understanding;
- patch generation;
- issue-to-code reasoning;
- regression test success.

Caution: this is heavy on disk, time, and dependency setup. Do not make it part of the first-run experience.

## Layer 5: terminal-agent tasks

Recommended:

- Terminal-Bench subset;
- custom terminal tasks.

This is the best benchmark layer for product CLIs such as Codex CLI, Claude Code, Mistral Vibe, and GitHub Copilot CLI because they operate as terminal agents.

Measure:

- task pass/fail;
- commands run;
- wall time;
- filesystem diff;
- dangerous command attempts;
- transcript quality;
- tool permission failures;
- autonomous recovery.

## Layer 6: defensive security tasks

Recommended v1 defensive checks:

- Semgrep rules for generated patches;
- Bandit for Python fixtures;
- npm audit / osv-scanner where dependency tasks are included;
- secret scanning with gitleaks/trufflehog-like tools;
- custom secure-code tasks.

Optional research benchmarks:

- CyberSecEval defensive subsets;
- PrimeVul-style vulnerability detection tasks;
- SecCodePLT-style secure code generation/fixing tasks.

Avoid offensive exploitation packs in the default distribution.

## Layer 7: private benchmark packs

The highest-value benchmark is usually the user's own workload.

Support private packs with:

- repo fixture import;
- issue prompt import;
- scoring scripts;
- static analysis scripts;
- expected patch constraints;
- local-only results.

BenchForge loads user-owned packs from `.benchforge/benchmark-packs/` in addition to the built-in `benchmark-packs/` directory. The Benchmark Packs page can create a starter private prompt pack, append or edit prompt tasks with exact, contains-all, regex, valid-JSON, non-empty, JSON field equality/contains, exact/ordered arrays, exact object keys, numeric tolerance, or numeric bounds scoring, test scoring against a pasted sample response before running a local/cloud benchmark, preview task prompts/scoring/weights before running them against local and cloud targets, delete extra user-pack tasks while retaining at least one task, export packs as folders or zip archives, and import validated pack folders, `pack.yaml` files, or `.zip` archives into the user pack root. Run Builder loads each pack's task list and can run only selected task IDs while preserving pack order in estimates, queued jobs, run snapshots, retries, and duplicates. Prompt-task edits preserve task IDs and refuse non-prompt tasks. Advanced users can add more roots with `BENCHFORGE_BENCHMARK_PACK_DIRS`. Pack IDs must be unique, malformed structured scorer JSON is rejected before task files are created, imported symlinks and unsafe zip paths are rejected, task paths must stay inside the pack folder, and malformed custom packs are reported as diagnostics without hiding valid packs.

## Benchmark pack schema

A benchmark pack is a folder:

```text
.benchforge/benchmark-packs/my-pack/
  pack.yaml
  tasks/
    task-001.yaml
    task-002.yaml
  fixtures/
    task-001/
  scripts/
    score_task_001.sh
```

`pack.yaml` may include calibration metadata:

```yaml
calibration:
  status: pilot # uncalibrated, pilot, reviewed, or calibrated
  sample_size: 0
  baseline_models: []
  last_reviewed: "2026-07-07"
  notes: Reviewed prompt contracts; not empirical public benchmark calibration.
```

`evidence_profile` is derived from pack/task shape and says whether a pack is broad enough for first-pass comparison. `calibration.status` is author-provided provenance and says how much review or empirical baseline work supports the pack. Keep these separate: a `prompt_comparison` pack can still be only `pilot` evidence. The Benchmark Packs page can edit calibration metadata for user packs and can suggest sample size, baselines, review date, and notes from stored benchmark evidence. Suggestions also flag weak evidence composition: missing local/cloud baseline pairs, missing cloud cost metrics, prompt-cache pricing assumptions, configured-model fallback identities, mixed generation policies, too few targets/tasks/run groups, and missing target provenance. `calibrated` still requires a positive sample size, at least two baseline models, a `YYYY-MM-DD` review date, and review notes. Results and exports surface non-calibrated or missing calibration statuses beside the Decision Snapshot so shared reports do not overstate definitive calibration.

## Scoring model

Every task should produce:

```json
{
  "passed": true,
  "score": 1.0,
  "tests_total": 12,
  "tests_passed": 12,
  "security_warnings": [],
  "notes": []
}
```

## Repetitions and statistics

For quick checks, run once.

For real comparisons:

```text
n = 3 minimum
n = 5 useful
n = 10 for publishable internal reports
```

Show mean, median, min, max, and failure modes.

## Temperature and sampling

Default benchmark mode:

```text
temperature = 0 or provider's most deterministic setting
top_p = 1
pass@1 primary
```

Exploration mode:

```text
allow temperature > 0
run multiple samples
report pass@k separately
```

Do not mix deterministic and exploration runs in the same leaderboard without labeling them. BenchForge Results and exports now emit generation-setting warnings when a visible comparison mixes temperature, top-p, or seed policy, and keep that evidence below `comparison_ready`.

## Benchmark inclusion order

### v1

- quick-smoke custom pack;
- EvalPlus HumanEval+/MBPP+ integration;
- basic CLI agent repo-patch task;
- Aider Polyglot subset;
- Terminal-Bench subset.

### v1.5

- full Aider Polyglot;
- Terminal-Bench full/core version;
- SWE-bench Lite subset;
- defensive security pack.

### v2

- SWE-bench Verified;
- Inspect AI integration;
- custom private pack builder;
- remote runner support;
- team exports and report generation.
