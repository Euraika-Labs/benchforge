# Codex prompt: adapter registry

Implement the adapter registry.

Scope:

- load YAML adapters from `../adapters/**/*.yaml`;
- validate against `specs/schemas/adapter.schema.json`;
- expose adapters through Tauri command `list_adapters`;
- implement target CRUD in SQLite;
- add redacted export for target configs;
- add validation stubs for:
  - OpenAI-compatible endpoint;
  - CLI command exists;
  - env secret exists.

Acceptance:

- invalid adapter fixture fails with clear error path;
- built-in adapters list in UI;
- user can create a mock target.
