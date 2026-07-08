# Product vision

## One-liner

BenchForge is the missing desktop command center for reproducible LLM and coding-agent benchmarks across local models, cloud models, and agent CLIs.

## Problem

Developers compare models badly. They paste one prompt into Claude, another into ChatGPT, a third into Ollama, eyeball the answers, and call it a benchmark. That is not a benchmark. It is a vibe check.

The market already has local model runners, chat UIs, cloud dashboards, and CLI agents. What is missing is one GUI that can answer:

```text
For my actual coding tasks, on my actual machine, under the same constraints:
which target passes tests, how fast, how expensive, how reproducible, and how safely?
```

## Product thesis

The useful comparison is not only `Claude vs Qwen`.

The useful comparison is:

```text
Claude API direct
Claude Code product agent
Claude through a standard BenchForge harness
Qwen direct through Ollama
Qwen through Copilot CLI BYOK
Qwen through the same BenchForge harness
Codex CLI as a product agent
Mistral Vibe CLI as a product agent
```

Only then can users separate model quality from agent-shell quality.

## Target users

### Local model hackers

They want to test whether Qwen, DeepSeek, Codestral, StarCoder, Devstral, Llama, Gemma, and fine-tunes are actually useful on a Mac, not just impressive in chat.

### Engineering leads

They want a repeatable way to compare provider costs, latency, and success rates before rolling a model into engineering workflows.

### Security engineers

They want to know whether an agent generates secure code, leaks secrets, tries dangerous commands, or creates patches that pass tests but introduce vulnerabilities.

### AI tool builders

They want to test their own harness against Codex CLI, Claude Code, Mistral Vibe, Copilot CLI, and raw model APIs.

## Principles

1. **Reproducibility beats leaderboard theatre.** A score without exact model version, adapter version, prompt, sandbox, timeout, retry policy, and cost model is noise.
2. **Tests beat taste.** Unit tests, integration tests, static analysis, and patch validation are primary. LLM judging is optional metadata.
3. **Model mode is not agent mode.** Direct API models and product CLIs are separate target categories.
4. **Sandbox by default.** Any target that can write files or run commands is treated as untrusted.
5. **Adapter-first architecture.** Every provider, local runtime, CLI, and benchmark harness is a plugin-like adapter.
6. **Mac-first, not Mac-only.** The product starts as a macOS desktop app, but the runner protocol should be portable.
7. **Private by default.** Runs, prompts, diffs, logs, and secrets remain local unless the user exports them.

## Product positioning

BenchForge is not:

- another chat client;
- another local model downloader;
- a single benchmark leaderboard;
- a thin wrapper around Promptfoo;
- a security exploit lab.

BenchForge is:

- a local-first benchmark orchestrator;
- a GUI for raw model and agent CLI comparisons;
- a test-driven code benchmark runner;
- a result database and diff/log viewer;
- a packaging shell around existing high-value benchmark harnesses.

## North-star metric

Time from installing the app to producing a reproducible comparison table across at least one local model, one cloud model, and one CLI agent.

## Strategic wedge

Start with a narrow but painful workflow:

```text
Benchmark Claude Code, Codex CLI, GitHub Copilot CLI, and Qwen via Ollama on the same repo-patch task.
```

Once that works, add benchmark packs and provider adapters.
