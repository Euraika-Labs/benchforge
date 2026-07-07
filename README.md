# BenchForge

BenchForge is a desktop benchmark tool for comparing local and cloud LLMs with reproducible runs, metrics, artifacts, and reports.

It is designed for engineers who want to answer practical model-selection questions without manually comparing chat transcripts: which model passed the task, how fast it was, what it cost, what it actually returned, and whether the evidence is strong enough to trust.

## Status

BenchForge is under active development. The current implementation includes a Tauri desktop scaffold, local/cloud target setup, Hugging Face GGUF search/download/start support, persisted benchmark jobs, result storage, comparison views, artifact inspection, and exportable reports.

It should be treated as an evolving benchmark workbench, not yet a definitive public evaluator. Calibrated public/private benchmark suites and broader runtime management are still part of the roadmap.

## What BenchForge Benchmarks

BenchForge supports LLM comparison across:

- Local models served through llama.cpp, Ollama, LM Studio, vLLM, MLX / mlx-lm, oMLX-style runtimes, or generic OpenAI-compatible servers.
- Hugging Face GGUF models downloaded locally and launched with `llama-server`.
- Cloud models from OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, and compatible APIs.
- Prompt-only benchmark packs for instruction following, JSON validity, summarization, reasoning, grounding, reliability, and structured output.
- Code/edit and worker-backed benchmark paths for deeper harness integration.

## Core Features

- Secure API-key storage through the desktop backend and system keychain.
- Hugging Face model browsing, search, GGUF file inspection, download planning, resume/retry, hash checks, and gated-model token handling.
- Local `llama-server` startup, readiness checks, target registration, cancellation, stale-process cleanup, and benchmark handoff.
- Cloud provider validation with tiny completion probes and actionable error codes.
- Background run jobs with progress, cancellation, retry, duplicate, startup recovery, and partial-result preservation.
- Local/cloud comparison views for pass rate, score, latency, p95 latency, tokens, throughput, retry count, HTTP status, served model identity, and cost.
- Artifact inspection for prompts, responses, raw provider payloads, stdout/stderr, diffs, scorer output, and run configuration.
- JSONL, CSV, Analysis JSON, Markdown, and complete report-folder exports.
- Evidence warnings for weak benchmark packs, low repetitions, missing pricing, mixed generation settings, unconfirmed served-model identity, and incomplete task coverage.

## Quick Start

Clone the repository:

```bash
git clone https://github.com/Euraika-Labs/benchforge.git
cd benchforge
```

Install local dependencies:

```bash
./scripts/bootstrap.sh
```

Check your machine:

```bash
make doctor
```

Run the standard local gate:

```bash
make test
```

Run the default local/cloud readiness smoke suite:

```bash
make benchmark-readiness
```

Start the desktop app in development mode:

```bash
cd app-scaffold
npm run tauri:dev
```

## Common Commands

```bash
make doctor                 # Check required and optional local tools
make test                   # Validate schemas, build web UI, run Rust and worker tests
make benchmark-readiness    # Offline local/cloud readiness gate
make live-cloud-smoke       # Optional real-provider smoke when API keys are configured
make package-dmg            # Build an unsigned macOS DMG
```

Focused smoke targets include:

```bash
make hf-search-smoke
make hf-download-smoke
make hf-local-cloud-basics-smoke
make cloud-provider-job-smoke
make local-runtime-discovery-smoke
make local-cloud-basics-smoke
make security-smoke
```

## Hugging Face Local Model Workflow

In the desktop app, open **Settings > Hugging Face Local Model**.

1. Save a Hugging Face token if you need gated model access.
2. Browse popular GGUF models or search by model family.
3. Inspect repo files and choose a runnable `.gguf` file.
4. Review disk, size, hash, quantization, and memory preflight checks.
5. Download the model with resumable progress.
6. Start `llama-server`.
7. Let BenchForge register the local OpenAI-compatible endpoint.
8. Run a benchmark pack such as `llm-connectivity`, `llm-basics`, or `llm-reliability`.
9. Compare the local model with cloud targets and export the report.

Public models may download without a token. Gated models require an accepted Hugging Face license and a saved token.

## Cloud Model Workflow

1. Add or select a provider target.
2. Save the required provider API key.
3. Search provider catalogs when available, or enter the model ID manually.
4. Validate the target with the built-in completion probe.
5. Choose a benchmark pack, repetitions, warmups, concurrency, and optional max-cost cap.
6. Run the benchmark and inspect results.

BenchForge normalizes provider failures into actionable categories such as `missing_key`, `auth`, `rate_limit`, `model_not_found`, `context_overflow`, `timeout`, `content_filter`, `server_error`, `malformed_response`, and `network`.

## Benchmark Packs

Built-in packs cover setup checks and model-selection tasks:

- `llm-connectivity`
- `llm-basics`
- `llm-core`
- `llm-practical`
- `llm-decision-suite`
- `llm-structured-output`
- `llm-grounded-context`
- `llm-reliability`
- `code-edit-core`
- `security-defensive`

Private packs can be created in the app or added under `.benchforge/benchmark-packs/`. Prompt tasks support scoring modes such as exact match, contains-all, regex, valid JSON, JSON field equality/contains, exact object keys, array checks, numeric tolerance, and numeric bounds.

## Results and Reports

BenchForge stores every run with reproducibility metadata and inspectable artifacts. Reports answer:

- What ran?
- Which target/model served it?
- Which tasks passed or failed?
- What did the model output?
- How fast was it?
- How much did it cost?
- Were retries, HTTP statuses, or provider errors involved?
- Is the evidence strong enough for model selection?
- How can the run be reproduced?

Exports include:

- `results.jsonl`
- `results.csv`
- `analysis.json`
- `README.md` Markdown report
- `reproducibility.json`
- `artifacts.json`
- copied prompt/response/raw/log/diff artifacts

## Security and Privacy

- API keys and tokens must never be committed.
- Target configs and exports redact secret-shaped fields.
- Code and worker benchmarks run in isolated workspaces.
- Docker-backed scoring uses network-off containers where supported.
- Security issues should be reported through [SECURITY.md](SECURITY.md), not public issues.

## Project Structure

```text
app-scaffold/        Tauri desktop app: React UI plus Rust backend
adapters/            Local, cloud, and CLI adapter definitions
benchmark-packs/     Built-in benchmark packs and task YAML
docs/                Architecture, roadmap, product, and security notes
fixtures/            Small projects used by benchmark tasks
scripts/             Bootstrap, doctor, smoke, readiness, and packaging scripts
workers/             Python worker harness integration
```

## Community

- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Contributing Guide](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)
- [Support](SUPPORT.md)
- [Governance](GOVERNANCE.md)

## License

BenchForge is licensed under the [MIT License](LICENSE).
