# Codex Desktop runbook

This document tells Codex how to turn this blueprint into the actual app.

## Build strategy

Do not try to build everything in one thread. Use small, verifiable milestones.

Recommended Codex workflow:

```text
Thread 1: foundation + build fixes
Thread 2: adapter registry + schemas
Thread 3: run planner + process supervisor
Thread 4: sandbox + scoring
Thread 5: direct model harness
Thread 6: CLI agent adapters
Thread 7: Python workers
Thread 8: UI polish + exports
```

If using Codex app worktrees, keep each thread scoped to one milestone.

## First prompt to use

Open `prompts/00_BUILD_FROM_BLUEPRINT.md` and paste it into Codex.

## Implementation order

1. Make the scaffold compile.
2. Add SQLite migrations and basic store layer.
3. Implement schema validation.
4. Implement adapter registry.
5. Implement target CRUD.
6. Implement Doctor checks.
7. Implement run planner.
8. Implement process supervisor.
9. Implement workspace manager.
10. Implement scoring command execution.
11. Implement mock target and quick-smoke benchmark.
12. Implement OpenAI-compatible direct model target.
13. Implement Anthropic target.
14. Implement CLI agent targets.
15. Implement Python worker bridge.
16. Implement result table and diff/log viewer.
17. Implement export.
18. Implement DMG packaging.

## Rules for Codex

- Keep the app buildable after every milestone.
- Prefer small PR-sized changes.
- Do not introduce real API keys into tests.
- Do not use host-destructive commands.
- Use mocks for unit tests.
- Treat every subprocess output as untrusted.
- Add regression tests for every bug fixed.
- Update docs when changing schemas.

## Minimum working v1 definition

The app is minimally useful when this works:

```text
1. Add target: mock-agent
2. Add target: local OpenAI-compatible endpoint
3. Add target: Codex CLI if installed
4. Select quick-smoke benchmark
5. Run all selected targets
6. See pass/fail, time, logs, diff, test output
7. Export JSONL and CSV
```

## Local dev commands

From `app-scaffold/`:

```bash
npm install
npm run tauri:dev
```

From `workers/`:

```bash
"${BENCHFORGE_PYTHON:-python3}" -m venv .venv
source .venv/bin/activate
python -m pip install -e .
benchforge-worker --help
```

Use Python 3.10 or newer. From the repo root, `./scripts/bootstrap.sh` performs the same version check and accepts `BENCHFORGE_PYTHON=/path/to/python3.10+`.

From repo root:

```bash
./scripts/doctor.sh
./scripts/run-smoke-local.sh
```

## Acceptance test prompt for Codex

Use this after each milestone:

```text
Run the relevant test/build commands. Fix failures. Then summarize exactly:
- what changed
- which tests passed
- which tests failed
- what remains risky
```

## Do not skip these hard parts

- Cancellation and timeout.
- Secret redaction.
- Workspace isolation.
- Schema validation.
- Reproducibility metadata.
- UI display of raw logs and diffs.

Those are the difference between a toy and a product.
