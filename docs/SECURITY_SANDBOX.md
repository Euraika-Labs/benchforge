# Security and sandbox plan

## Threat model

BenchForge runs untrusted code and untrusted agent behavior. Treat every model and agent as capable of:

- modifying files unexpectedly;
- running destructive shell commands;
- exfiltrating secrets through logs or network;
- installing dependencies;
- opening network connections;
- hiding malicious code in generated patches;
- producing outputs that exploit parsers or viewers;
- hanging forever or spawning child processes.

## Core rule

No agent or benchmark task should ever operate directly on the user's real project directory. Always copy or clone into an isolated workspace.

## Sandboxing levels

### Level 0: dry-run / read-only

- no shell execution;
- no file writes;
- model output only;
- useful for direct model prompt tests.

### Level 1: isolated workspace

- per-run temp directory;
- git initialized;
- only fixture copied in;
- command execution allowed on host;
- suitable only for trusted internal tasks.

### Level 2: Docker/Colima sandbox

- default for v1;
- per-run mounted workspace;
- no host home directory;
- no Docker socket inside container;
- network off by default during scoring;
- memory and CPU limits;
- timeout enforced by host process supervisor.

### Level 3: VM sandbox

- later hardening layer;
- macOS virtualization or Linux VM;
- useful for hostile benchmark packs or stronger isolation.

## Secrets handling

Secrets must be referenced, not copied into adapter files.

```yaml
api_key_env: ANTHROPIC_API_KEY
api_key_keychain: benchforge/anthropic/default
```

Rules:

- redact secret values from stdout/stderr before persistence;
- record secret names, never secret values;
- do not pass cloud API keys into scoring containers unless target execution requires them;
- separate target execution from scoring whenever possible;
- CLI agents that require auth should run with a temporary `HOME` or config directory where feasible.

## Network policy

Default:

```text
target execution: network allowed only if adapter requires provider/local endpoint access
scoring phase: network disabled
fixture setup: network disabled unless benchmark pack explicitly declares dependency fetch
```

Allowlist example:

```yaml
network:
  mode: allowlist
  allow:
    - https://api.openai.com
    - https://api.anthropic.com
    - http://localhost:11434
```

## Dangerous command detection

BenchForge cannot rely on string matching for security, but it should surface suspicious behavior.

Flag commands containing:

```text
rm -rf /
sudo
chmod -R 777
curl | sh
wget | sh
nc -e
mkfs
dd if=
security find-generic-password
cat ~/.ssh
cat ~/.aws
cat ~/.config
open /Applications
osascript
launchctl
```

For v1, these are warnings. For hardened mode, they should block execution unless explicitly allowed.

## Filesystem policy

- Workspaces live under app-managed run directory.
- Each run gets a fresh workspace.
- Task fixtures must resolve under BenchForge's checked-in `fixtures/` tree or a pack-local `fixtures/` directory before they are copied.
- Fixture copying rejects symlinks instead of following them into arbitrary host paths.
- After target execution, git diff is captured before scoring.
- Scoring runs against the generated patch, not the original agent process state.
- Artifacts are copied into an artifact directory and referenced by path.
- Worker harness target-config artifacts are redacted; private worker input files are removed after the worker exits.
- Worker harness child processes inherit only a minimal environment by default; secret-bearing host variables require explicit `env_passthrough` names.

## Process policy

- Every process has a wall-clock timeout.
- Every process gets stdout/stderr byte limits.
- Every process group is killed on cancellation/timeout.
- Child processes are cleaned up best-effort.
- Background servers started by agents are detected where possible.

## Log safety

- Logs are plain text and treated as untrusted.
- UI must escape all log content.
- ANSI escape codes should be sanitized or rendered safely.
- Secret redaction runs before persistence.
- Exports warn the user that transcripts may contain sensitive data.

## Default permission presets

```text
safe-readonly:
  file reads only
  no writes
  no shell

patch-basic:
  read/write in workspace
  shell limited to test commands
  no network in scoring

agent-full-sandboxed:
  read/write/shell in container
  network controlled by adapter
  no host mounts except workspace

unsafe-host:
  disabled by default
  requires explicit confirmation
```

## CLI agent policy

Product agent CLIs often have auto-approve or yolo flags. BenchForge must never enable these on the host workspace.

Allowed only when:

- workspace is isolated;
- sandbox level is at least Level 2;
- the UI displays a warning;
- run metadata records the dangerous permission mode.

## Security benchmark policy

Defensive security evals are in scope by default:

- insecure code detection;
- secure coding patch tasks;
- dependency pinning;
- secret leakage prevention;
- prompt injection resistance;
- sandbox escape attempt detection.

Offensive exploit generation and real target exploitation are out of scope for default packs.

## Dependency advisory policy

BenchForge's supported desktop release target is macOS. CI, packaging, Keychain storage, DMG verification, and install smoke tests run against macOS. Cargo may still record target-specific Linux webview dependencies in `Cargo.lock` because Tauri supports multiple platforms upstream.

When Dependabot flags a target-specific dependency that is not in the supported macOS build graph, verify it before dismissing the alert:

```bash
make dependency-audit
```

The audit currently checks both Apple Silicon and Intel macOS target graphs for the `glib` advisory `GHSA-wrw7-89jp-8q8g`. If BenchForge adds supported Linux desktop builds later, remove this exception and update the Tauri/Linux webview stack before release.

## Security acceptance criteria

- Running the smoke benchmark cannot modify files outside the workspace.
- Secrets are redacted in stored logs.
- Target creation rejects raw secret-shaped config fields such as API keys, authorization headers, tokens, passwords, private keys, and client secrets; use Keychain or environment references instead.
- A timed-out agent process is killed.
- Scoring can run with network disabled.
- Result metadata includes sandbox level and permission mode.
- Docker-scored result metadata includes the runner image ID or digest and Dockerfile checksum when available.
- UI warns before any host-level execution.
