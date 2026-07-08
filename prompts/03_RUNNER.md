# Codex prompt: runner and workspace

Implement the first real run path.

Scope:

- load `benchmark-packs/quick-smoke/pack.yaml`;
- create run matrix for selected targets/tasks/repetitions;
- create per-run workspace under app data dir;
- copy fixture into workspace;
- initialize git;
- run mock target command;
- capture stdout/stderr;
- capture git diff;
- run scoring command;
- store normalized result.

Acceptance:

- `scripts/run-smoke-local.sh` runs a mock target and produces a stored result;
- timeout is enforced;
- artifacts are saved.
