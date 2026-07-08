# Repository Guidelines

## Project Structure & Module Organization

BenchForge is a macOS desktop app for benchmarking local and cloud LLMs.

- `app-scaffold/`: Tauri desktop app. React/TypeScript UI lives in `src/`; Rust backend lives in `src-tauri/src/`.
- `workers/`: Python worker package (`benchforge_worker`) for external harnesses, imports, and security checks.
- `benchmark-packs/`: built-in benchmark pack metadata and task YAML.
- `adapters/`: YAML target definitions for local runtimes, cloud providers, CLI agents, and harness targets.
- `fixtures/`: small Python and JavaScript projects used by smoke and regression tests.
- `scripts/`: bootstrap, doctor, readiness, schema validation, smoke, and packaging helpers.
- `docs/` and `prompts/`: architecture/product notes and ordered implementation prompts.
- `.benchforge/`: local models, runs, exports, and readiness logs. Do not commit this data.

## Build, Test, and Development Commands

Run commands from the repository root unless noted.

- `./scripts/bootstrap.sh`: install app dependencies and create `workers/.venv/`.
- `make doctor`: check required tools and report optional runtime helpers.
- `make test`: validate schemas, build the web UI, run Rust tests, run worker tests, and check worker CLI help.
- `make benchmark-readiness`: run the full local readiness gate, including Hugging Face and local/cloud smokes.
- `make dev`: start the Tauri desktop app.
- `cd app-scaffold && npm run dev`: start the browser-only Vite preview.
- `cd app-scaffold && npm run tauri:build:dmg`: build a macOS DMG.

## Coding Style & Naming Conventions

Follow nearby code. TypeScript uses 2-space indentation, single quotes, React function components, and PascalCase component names. Rust follows `rustfmt`, snake_case functions/modules, and explicit error strings for user-facing failures. Python uses 4-space indentation, type hints where helpful, snake_case functions, and JSON-line worker events. Keep YAML IDs lowercase and hyphenated, for example `llm-core-json-001`.

## Testing Guidelines

Prefer `make test` before handoff. Use focused checks while developing: `cargo test --manifest-path app-scaffold/src-tauri/Cargo.toml`, `workers/.venv/bin/python -m unittest discover -s workers/tests`, or specific smoke targets such as `make worker-harness-contract-smoke` and `make hf-local-cloud-basics-smoke`. Add regression tests when changing result persistence, exports, benchmark scoring, or worker contracts.

## Commit & Pull Request Guidelines

Use concise imperative commits with scope, such as `app: surface import bounds` or `workers: reject unsafe imports`. Pull requests should explain user-visible behavior, list verification commands, link related issues or roadmap items, and include screenshots for UI changes. Never commit secrets, API keys, downloaded models, restricted datasets, `.benchforge/`, or host-specific paths.
