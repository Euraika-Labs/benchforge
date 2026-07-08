CREATE TABLE IF NOT EXISTS targets (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  adapter_id TEXT NOT NULL,
  config_json TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  validation_status TEXT,
  validation_detail TEXT,
  validation_checked_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS adapters (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  spec_json TEXT NOT NULL,
  validation_status TEXT NOT NULL,
  validation_detail TEXT NOT NULL,
  loaded_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS benchmark_packs (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  version TEXT NOT NULL,
  path TEXT NOT NULL,
  checksum TEXT NOT NULL,
  metadata_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY,
  benchmark_pack_id TEXT NOT NULL,
  name TEXT NOT NULL,
  type TEXT NOT NULL,
  language TEXT,
  config_json TEXT NOT NULL,
  checksum TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  run_group_id TEXT,
  target_id TEXT NOT NULL,
  benchmark_pack_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  error_code TEXT,
  error_message TEXT,
  config_json TEXT NOT NULL,
  reproducibility_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS run_groups (
  id TEXT PRIMARY KEY,
  benchmark_pack_id TEXT NOT NULL,
  target_ids_json TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  config_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS run_jobs (
  id TEXT PRIMARY KEY,
  run_group_id TEXT NOT NULL,
  benchmark_pack_id TEXT NOT NULL,
  status TEXT NOT NULL,
  message TEXT NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  total INTEGER NOT NULL DEFAULT 0,
  completed INTEGER NOT NULL DEFAULT 0,
  error TEXT,
  request_json TEXT NOT NULL,
  result_run_ids_json TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS hf_download_jobs (
  id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  selected_file TEXT,
  status TEXT NOT NULL,
  message TEXT NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  planned_bytes INTEGER,
  transferred_bytes INTEGER NOT NULL DEFAULT 0,
  local_dir TEXT,
  error TEXT,
  request_json TEXT NOT NULL,
  model_json TEXT
);

CREATE TABLE IF NOT EXISTS hf_server_jobs (
  id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  selected_file TEXT,
  port INTEGER NOT NULL,
  context INTEGER NOT NULL,
  status TEXT NOT NULL,
  message TEXT NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  error TEXT,
  request_json TEXT NOT NULL,
  server_status_json TEXT
);

CREATE TABLE IF NOT EXISTS run_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  payload_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS metrics (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  name TEXT NOT NULL,
  value REAL,
  text_value TEXT,
  unit TEXT,
  source TEXT
);

CREATE TABLE IF NOT EXISTS artifacts (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  mime_type TEXT,
  size_bytes INTEGER,
  sha256 TEXT,
  metadata_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runs_started_at ON runs(started_at);
CREATE INDEX IF NOT EXISTS idx_run_jobs_started_at ON run_jobs(started_at);
CREATE INDEX IF NOT EXISTS idx_run_jobs_run_group_id ON run_jobs(run_group_id);
CREATE INDEX IF NOT EXISTS idx_hf_download_jobs_started_at ON hf_download_jobs(started_at);
CREATE INDEX IF NOT EXISTS idx_hf_server_jobs_started_at ON hf_server_jobs(started_at);
CREATE INDEX IF NOT EXISTS idx_artifacts_run_id ON artifacts(run_id);
CREATE INDEX IF NOT EXISTS idx_metrics_run_id ON metrics(run_id);
