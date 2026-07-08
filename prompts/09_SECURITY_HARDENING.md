# Codex prompt: security hardening

Harden the runner.

Scope:

- secret redaction;
- suspicious command detector;
- path traversal checks for artifacts;
- log escaping in UI;
- max stdout/stderr bytes;
- process group kill on timeout;
- sandbox policy recorded per run;
- warnings for unsafe host mode.

Acceptance:

- tests prove secrets are redacted;
- malicious log content is escaped;
- path traversal artifact paths are rejected;
- timed-out process is killed.
