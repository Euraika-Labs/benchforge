# Security Policy

BenchForge runs local tools, model servers, worker harnesses, and provider calls. Treat benchmark packs, model outputs, imported results, and generated artifacts as untrusted.

## Supported Versions

BenchForge is pre-1.0. Security fixes are handled on the main development line until stable release branches exist.

## Reporting A Vulnerability

Please report vulnerabilities privately through GitHub Security Advisories when available. If advisories are unavailable, contact the maintainers at `bert@telkom.be` with:

- affected version or commit;
- platform and setup details;
- a minimal reproduction;
- whether secrets, files outside the workspace, provider accounts, or network access were exposed.

Do not publish exploit details until a maintainer has acknowledged the report and a mitigation plan exists.

## Scope

In scope:

- secret leakage in stored targets, logs, artifacts, diagnostics, or exports;
- benchmark tasks modifying files outside the app-managed workspace;
- sandbox bypasses, unsafe process cleanup, or unexpected network access during scoring;
- provider-key handling, Keychain integration, and redaction failures;
- unsafe report import/export behavior.

Out of scope:

- attacks requiring a compromised operating system account;
- vulnerabilities only in third-party model files, provider APIs, or external benchmark tools unless BenchForge mishandles them;
- denial-of-service from intentionally huge local models or intentionally expensive provider runs when the UI and cost controls warn correctly.

## Hardening Expectations

- Keep `.benchforge/` out of commits and exports unless explicitly redacted.
- Review full report folders before sharing; prompts, responses, diffs, and raw payloads can contain sensitive data.
- Use Docker/Colima sandboxing for code tasks when available.
- Use live cloud smoke tests only with accounts and API keys intended for testing.
