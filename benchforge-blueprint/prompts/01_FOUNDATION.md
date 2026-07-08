# Codex prompt: foundation

Implement the foundation milestone.

Scope:

- make `app-scaffold/` a buildable Tauri + React + Rust app;
- implement basic navigation pages;
- implement Tauri commands:
  - `list_targets`
  - `list_benchmark_packs`
  - `run_doctor`
  - `get_app_version`
- add SQLite migration runner;
- add unit tests for store initialization;
- add `scripts/doctor.sh` that checks local dependencies.

Acceptance:

```bash
cd app-scaffold
npm install
npm run build:web
cargo test --manifest-path src-tauri/Cargo.toml
```

If `npm run tauri:dev` cannot be run in the environment, explain why and keep the code ready for local macOS execution.
