# Codex prompt: Python benchmark workers

Implement the Python worker bridge.

Scope:

- worker emits JSONL events;
- worker can run mock benchmark;
- add EvalPlus integration wrapper;
- add Terminal-Bench integration wrapper;
- add Aider wrapper stub;
- add SWE-bench wrapper stub;
- Rust core can spawn worker and import results.

Acceptance:

- `benchforge-worker run --kind mock ...` works;
- worker results appear in app result table;
- missing external benchmark dependency gives clear remediation.
