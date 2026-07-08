# Codex prompt: direct model harness

Implement raw model benchmarking through a shared BenchForge harness.

Scope:

- add OpenAI-compatible client;
- add Anthropic client;
- define patch protocol:
  - model can return unified diff;
  - model can return JSON file edits;
- apply edits safely inside workspace;
- run scoring;
- optionally retry once with test failure output;
- collect usage and cost when provider returns usage.

Acceptance:

- Ollama/LM Studio target can run a prompt when endpoint is available;
- Anthropic target can run when `ANTHROPIC_API_KEY` is available;
- all provider calls can be skipped in CI using mocks.
