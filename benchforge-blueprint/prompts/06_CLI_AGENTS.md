# Codex prompt: CLI agent adapters

Implement CLI agent execution.

Scope:

- Codex CLI adapter using `codex exec`;
- Claude Code adapter using `claude -p` with structured output where configured;
- Mistral Vibe adapter using `vibe --prompt`;
- GitHub Copilot CLI adapter using `copilot -p`;
- support per-adapter allow/deny tool settings;
- capture transcript artifacts where CLI supports it;
- parse CLI-specific metadata when available.

Acceptance:

- missing CLI shows `cli_not_found` with remediation;
- installed CLI can run smoke benchmark in isolated workspace;
- product-agent results are labeled separately from raw model results.
