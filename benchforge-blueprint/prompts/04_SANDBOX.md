# Codex prompt: sandbox and scoring

Implement Docker/Colima scoring mode.

Scope:

- add Docker runner using `docker/runner.Dockerfile`;
- run scoring command inside container;
- disable network during scoring by default;
- mount only workspace and artifact output dir;
- enforce timeout;
- collect scoring output.

Acceptance:

- quick-smoke Python fixture scores inside Docker;
- host home directory is not mounted;
- network mode is recorded in result metadata.
