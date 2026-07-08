# Data model

## Database

Use SQLite for v1. Store large artifacts as files and reference them from SQLite.

## Tables

### targets

```sql
CREATE TABLE targets (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  adapter_id TEXT NOT NULL,
  config_json TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

### benchmark_packs

```sql
CREATE TABLE benchmark_packs (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  version TEXT NOT NULL,
  path TEXT NOT NULL,
  checksum TEXT NOT NULL,
  metadata_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);
```

### tasks

```sql
CREATE TABLE tasks (
  id TEXT PRIMARY KEY,
  benchmark_pack_id TEXT NOT NULL,
  name TEXT NOT NULL,
  type TEXT NOT NULL,
  language TEXT,
  config_json TEXT NOT NULL,
  checksum TEXT NOT NULL,
  FOREIGN KEY (benchmark_pack_id) REFERENCES benchmark_packs(id)
);
```

### runs

```sql
CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  target_id TEXT NOT NULL,
  benchmark_pack_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  error_code TEXT,
  error_message TEXT,
  config_json TEXT NOT NULL,
  reproducibility_json TEXT NOT NULL,
  FOREIGN KEY (target_id) REFERENCES targets(id),
  FOREIGN KEY (benchmark_pack_id) REFERENCES benchmark_packs(id),
  FOREIGN KEY (task_id) REFERENCES tasks(id)
);
```

### run_events

```sql
CREATE TABLE run_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(id)
);
```

### metrics

```sql
CREATE TABLE metrics (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  name TEXT NOT NULL,
  value REAL,
  unit TEXT,
  source TEXT,
  FOREIGN KEY (run_id) REFERENCES runs(id)
);
```

### artifacts

```sql
CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  mime_type TEXT,
  size_bytes INTEGER,
  sha256 TEXT,
  metadata_json TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(id)
);
```

### costs

```sql
CREATE TABLE costs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  provider TEXT,
  model TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cached_tokens INTEGER,
  cache_read_tokens INTEGER,
  cache_write_tokens INTEGER,
  estimated_cost_usd REAL,
  raw_usage_json TEXT,
  FOREIGN KEY (run_id) REFERENCES runs(id)
);
```

## Run status enum

```text
queued
preparing
running
scoring
passed
failed
error
timeout
cancelled
```

## Artifact kinds

```text
stdout
stderr
transcript
git_diff
git_patch
scoring_output
workspace_snapshot
metrics_json
result_json
sarif
coverage
```

## Result JSON

Final normalized result:

```json
{
  "run_id": "...",
  "target_id": "...",
  "task_id": "...",
  "status": "passed",
  "score": 1.0,
  "metrics": {
    "wall_time_ms": 120000,
    "input_tokens": 10000,
    "output_tokens": 2000,
    "estimated_cost_usd": 0.42
  },
  "tests": {
    "total": 10,
    "passed": 10,
    "failed": 0
  },
  "artifacts": [
    {"kind":"git_diff","path":"artifacts/run.diff"}
  ],
  "safety": {
    "dangerous_command_hits": [],
    "secret_leak_hits": []
  }
}
```

## Export formats

v1:

- JSONL: one normalized result per line;
- CSV: flattened metrics for spreadsheet/reporting;
- Markdown: human report;
- zip: artifacts + metadata.

Later:

- Parquet;
- DuckDB;
- SARIF for security results;
- OpenTelemetry traces.
