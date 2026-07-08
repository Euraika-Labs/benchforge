# Codex prompt: build from blueprint

You are working in the BenchForge blueprint repository. Turn it into a functioning macOS desktop app incrementally.

Start by reading:

1. `README.md`
2. `docs/PRODUCT_REQUIREMENTS.md`
3. `docs/ARCHITECTURE.md`
4. `docs/SECURITY_SANDBOX.md`
5. `docs/IMPLEMENTATION_BACKLOG.md`

Rules:

- Keep the project buildable after each step.
- Make small commits/patches.
- Do not add real API keys.
- Do not run destructive host commands.
- Prefer tests and mock targets before real provider calls.
- Preserve the separation between raw model, harnessed model, CLI agent, and benchmark harness.

First milestone:

1. Make the `app-scaffold/` Tauri app compile and launch.
2. Make `workers/` installable with `pip install -e .`.
3. Add a top-level `justfile` or `Makefile` with `doctor`, `dev`, `test`, and `smoke` commands.
4. Add automated checks for JSON schema validity.
5. Update `README.md` with exact commands that passed.

Stop after the first milestone and report:

- files changed;
- commands run;
- tests passed;
- known failures;
- next recommended task.
