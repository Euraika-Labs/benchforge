# Contributing to BenchForge

Thanks for helping make BenchForge a reliable local and cloud LLM benchmark tool. Contributions should improve reproducibility, safety, provider compatibility, or the user workflow.

## Development Setup

Work from the app source directory:

```bash
cd benchforge-blueprint
./scripts/bootstrap.sh
make doctor
make test
```

Use `make dev` for the Tauri desktop app. The browser-only Vite preview is useful for UI work, but it does not exercise Keychain, subprocesses, downloads, provider calls, or benchmark execution.

## Pull Request Expectations

- Keep changes scoped to one workflow or subsystem.
- Include the commands you ran, such as `make test`, `make cloud-catalog-smoke`, or `make benchmark-readiness`.
- Include screenshots or a short recording for UI changes.
- Link related issues, roadmap items, or design notes.
- Explain any benchmark, scoring, security, or export behavior changes in plain language.

## Coding Guidelines

- Follow the style in nearby files.
- TypeScript uses 2-space indentation and React function components.
- Rust follows `rustfmt` defaults and snake_case modules/functions.
- Python uses 4-space indentation, type hints where useful, and JSON-line worker events.
- YAML identifiers should be stable, lowercase, and hyphenated.

## Safety Rules

- Never commit API keys, tokens, credentials, downloaded models, report exports, or `.benchforge/` runtime data.
- Keep target secrets in macOS Keychain or environment references, not target JSON.
- Treat model outputs, logs, benchmark artifacts, and imported reports as untrusted.
- Run code and agent benchmarks in isolated workspaces; do not point benchmark tasks at a real user repository.

## Useful Verification Commands

```bash
make test
make benchmark-readiness
make cloud-provider-job-smoke
make local-runtime-discovery-smoke
make hf-local-cloud-basics-smoke
make report-smoke
make security-smoke
```
