use chrono::Utc;
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crate::paths;

pub struct AppState {
    pub conn: Mutex<Connection>,
    pub hf_server: Mutex<Option<HfServerProcess>>,
}

pub struct HfServerProcess {
    pub child: Child,
    pub port: u16,
    pub model_path: String,
    pub server_model_id: Option<String>,
    pub marker_path: Option<PathBuf>,
}

impl Drop for HfServerProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

impl HfServerProcess {
    pub fn terminate(&mut self) {
        terminate_child_process(&mut self.child);
        if let Some(path) = self.marker_path.take() {
            remove_hf_server_marker_path(&path);
        }
    }
}

pub fn terminate_child_process(child: &mut Child) {
    let pid = child.id();
    if child.try_wait().ok().flatten().is_some() {
        return;
    }

    #[cfg(unix)]
    {
        let process_group = format!("-{}", pid);
        let _ = Command::new("kill")
            .args(["-TERM", &process_group])
            .status();
        thread::sleep(Duration::from_millis(500));
        if child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("kill")
                .args(["-KILL", &process_group])
                .status();
        }
    }

    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedHfServerMarker {
    pid: u32,
    port: u16,
    model_path: String,
    started_at: String,
}

pub fn write_hf_server_marker(pid: u32, port: u16, model_path: &str) -> std::io::Result<PathBuf> {
    let marker = ManagedHfServerMarker {
        pid,
        port,
        model_path: model_path.to_string(),
        started_at: now(),
    };
    let path = hf_server_marker_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(&marker).map_err(std::io::Error::other)?;
    fs::write(&path, json)?;
    Ok(path)
}

pub fn cleanup_stale_hf_server_marker() -> std::io::Result<Option<String>> {
    let path = hf_server_marker_path();
    if !path.exists() {
        return Ok(None);
    }
    let marker = match fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<ManagedHfServerMarker>(&text).ok())
    {
        Some(marker) => marker,
        None => {
            remove_hf_server_marker_path(&path);
            return Ok(Some(
                "removed unreadable managed llama-server marker".into(),
            ));
        }
    };
    let command = process_command_line(marker.pid).unwrap_or_default();
    let cleaned = if managed_llama_command_matches_marker(&command, &marker) {
        terminate_persisted_process_group(marker.pid);
        Some(format!(
            "stopped stale BenchForge-managed llama-server pid {} on port {}",
            marker.pid, marker.port
        ))
    } else {
        Some(format!(
            "removed stale llama-server marker for inactive or unrelated pid {}",
            marker.pid
        ))
    };
    remove_hf_server_marker_path(&path);
    Ok(cleaned)
}

fn hf_server_marker_path() -> PathBuf {
    paths::app_data_dir().join("managed-llama-server.json")
}

fn remove_hf_server_marker_path(path: &Path) {
    let _ = fs::remove_file(path);
}

fn managed_llama_command_matches_marker(command: &str, marker: &ManagedHfServerMarker) -> bool {
    let lower = command.to_lowercase();
    !command.trim().is_empty()
        && lower.contains("llama-server")
        && command.contains(&marker.model_path)
        && (command.contains(&format!("--port {}", marker.port))
            || command.contains(&format!("--port={}", marker.port))
            || command.contains(&format!(" -p {}", marker.port))
            || command.contains(&format!(" -p={}", marker.port)))
}

fn process_command_line(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!command.is_empty()).then_some(command)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

fn terminate_persisted_process_group(pid: u32) {
    #[cfg(unix)]
    {
        let process_group = format!("-{}", pid);
        let _ = Command::new("kill")
            .args(["-TERM", &process_group])
            .status();
        thread::sleep(Duration::from_millis(500));
        let _ = Command::new("kill")
            .args(["-KILL", &process_group])
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
}

#[cfg(test)]
fn child_exited_successfully() -> Child {
    let mut child = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "exit 0"])
            .spawn()
            .expect("test child should spawn")
    } else {
        Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("test child should spawn")
    };
    let _ = child.wait();
    child
}

#[cfg(test)]
pub fn exited_hf_server_for_tests() -> HfServerProcess {
    HfServerProcess {
        child: child_exited_successfully(),
        port: 8080,
        model_path: "/tmp/benchforge-test-model.gguf".into(),
        server_model_id: Some("exited-test-model".into()),
        marker_path: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub adapter_id: String,
    pub config_json: String,
    pub enabled: bool,
    pub validation_status: Option<String>,
    pub validation_detail: Option<String>,
    pub validation_checked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTarget {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub adapter_id: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultRecord {
    pub id: String,
    pub run_group_id: Option<String>,
    pub target_id: String,
    pub benchmark_pack_id: String,
    pub task_id: String,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub pass_fail: Option<bool>,
    pub score: Option<f64>,
    pub score_numeric: Option<f64>,
    pub wall_time_ms: Option<f64>,
    pub setup_time_ms: Option<f64>,
    pub target_time_ms: Option<f64>,
    pub evaluation_time_ms: Option<f64>,
    pub model_call_wall_time_ms: Option<f64>,
    pub input_tokens: Option<f64>,
    pub output_tokens: Option<f64>,
    pub prompt_tokens: Option<f64>,
    pub completion_tokens: Option<f64>,
    pub reasoning_tokens: Option<f64>,
    pub cached_tokens: Option<f64>,
    pub cache_read_tokens: Option<f64>,
    pub cache_write_tokens: Option<f64>,
    pub total_tokens: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub cost_usd: Option<f64>,
    pub provider_attempts: Option<f64>,
    pub provider_retry_after_ms: Option<f64>,
    pub provider_retry_delay_ms: Option<f64>,
    pub http_status: Option<f64>,
    pub provider_time_to_first_byte_ms: Option<f64>,
    pub ttft_ms: Option<f64>,
    pub provider_time_to_first_token_ms: Option<f64>,
    pub provider_request_total_ms: Option<f64>,
    pub decode_tokens_per_sec: Option<f64>,
    pub output_tokens_per_second: Option<f64>,
    pub peak_rss_mb: Option<f64>,
    pub exit_code: Option<f64>,
    pub harness_exit_code: Option<f64>,
    pub stdout_bytes: Option<f64>,
    pub stderr_bytes: Option<f64>,
    pub files_changed: Option<f64>,
    pub lines_added: Option<f64>,
    pub lines_deleted: Option<f64>,
    pub commands_observed_count: Option<f64>,
    pub dangerous_command_hits: Option<f64>,
    pub security_finding_count: Option<f64>,
    pub security_files_scanned: Option<f64>,
    pub import_file_count: Option<f64>,
    pub import_total_file_count: Option<f64>,
    pub import_omitted_file_count: Option<f64>,
    pub import_unsupported_file_count: Option<f64>,
    pub import_truncated: Option<f64>,
    pub import_truncated_bytes: Option<f64>,
    pub provider_model: Option<String>,
    pub provider_model_source: Option<String>,
    pub finish_reason: Option<String>,
    pub pricing_assumption: Option<String>,
    pub import_format: Option<String>,
    pub import_source: Option<String>,
    pub import_path: Option<String>,
    pub summary_source: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub reproducibility: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub id: String,
    pub run_id: String,
    pub kind: String,
    pub path: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub sha256: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunJobRecord {
    pub id: String,
    pub run_group_id: String,
    pub benchmark_pack_id: String,
    pub status: String,
    pub message: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub total: usize,
    pub completed: usize,
    pub error: Option<String>,
    pub request: serde_json::Value,
    pub result_run_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfDownloadJobRecord {
    pub id: String,
    pub repo_id: String,
    pub selected_file: Option<String>,
    pub status: String,
    pub message: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub planned_bytes: Option<u64>,
    pub transferred_bytes: u64,
    pub local_dir: Option<String>,
    pub error: Option<String>,
    pub request: serde_json::Value,
    pub model: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfServerJobRecord {
    pub id: String,
    pub repo_id: String,
    pub selected_file: Option<String>,
    pub port: u16,
    pub context: u32,
    pub status: String,
    pub message: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    pub request: serde_json::Value,
    pub server_status: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunGroupRecord {
    pub id: String,
    pub benchmark_pack_id: String,
    pub target_ids: Vec<String>,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub config: serde_json::Value,
}

pub fn open_app() -> Result<Connection> {
    let dir = paths::app_data_dir();
    fs::create_dir_all(&dir).map_err(|_| rusqlite::Error::InvalidPath(dir.clone()))?;
    let conn = Connection::open(paths::db_path())?;
    configure_connection(&conn)?;
    migrate(&conn)?;
    seed_mock_target(&conn)?;
    Ok(conn)
}

pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure_connection(&conn)?;
    migrate(&conn)?;
    seed_mock_target(&conn)?;
    Ok(conn)
}

fn configure_connection(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(10))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;",
    )?;
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("../migrations/001_init.sql"))?;
    ensure_column(conn, "runs", "run_group_id", "TEXT")?;
    ensure_column(conn, "metrics", "text_value", "TEXT")?;
    ensure_column(conn, "targets", "validation_status", "TEXT")?;
    ensure_column(conn, "targets", "validation_detail", "TEXT")?;
    ensure_column(conn, "targets", "validation_checked_at", "TEXT")?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_runs_run_group_id ON runs(run_group_id)",
        [],
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS hf_download_jobs (
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
        CREATE INDEX IF NOT EXISTS idx_hf_download_jobs_started_at ON hf_download_jobs(started_at);",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS hf_server_jobs (
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
        CREATE INDEX IF NOT EXISTS idx_hf_server_jobs_started_at ON hf_server_jobs(started_at);",
    )?;
    Ok(())
}

pub fn init_state() -> Result<AppState> {
    let conn = open_app()?;
    let _ = cleanup_stale_hf_server_marker();
    recover_interrupted_run_jobs(&conn)?;
    recover_interrupted_hf_download_jobs(&conn)?;
    recover_interrupted_hf_server_jobs(&conn)?;
    Ok(AppState {
        conn: Mutex::new(conn),
        hf_server: Mutex::new(None),
    })
}

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}

pub fn seed_mock_target(conn: &Connection) -> Result<()> {
    let timestamp = now();
    conn.execute(
        "INSERT OR IGNORE INTO targets (id, name, kind, adapter_id, config_json, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)",
        params![
            "mock-agent",
            "Mock Agent",
            "mock",
            "mock",
            serde_json::json!({"mode": "deterministic-fixture-fix"}).to_string(),
            timestamp
        ],
    )?;
    Ok(())
}

pub fn list_targets(conn: &Connection) -> Result<Vec<TargetRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, adapter_id, config_json, enabled, validation_status, validation_detail, validation_checked_at FROM targets ORDER BY created_at, id",
    )?;
    let targets = stmt
        .query_map([], |row| {
            Ok(TargetRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                adapter_id: row.get(3)?,
                config_json: row.get(4)?,
                enabled: row.get::<_, i64>(5)? != 0,
                validation_status: row.get(6)?,
                validation_detail: row.get(7)?,
                validation_checked_at: row.get(8)?,
            })
        })?
        .collect();
    targets
}

pub fn get_target(conn: &Connection, id: &str) -> Result<Option<TargetRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, adapter_id, config_json, enabled, validation_status, validation_detail, validation_checked_at FROM targets WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(TargetRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        adapter_id: row.get(3)?,
        config_json: row.get(4)?,
        enabled: row.get::<_, i64>(5)? != 0,
        validation_status: row.get(6)?,
        validation_detail: row.get(7)?,
        validation_checked_at: row.get(8)?,
    }))
}

pub fn upsert_target(conn: &Connection, target: &NewTarget) -> Result<()> {
    let timestamp = now();
    conn.execute(
        "INSERT INTO targets (id, name, kind, adapter_id, config_json, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
         ON CONFLICT(id) DO UPDATE SET
           name=excluded.name,
           kind=excluded.kind,
           adapter_id=excluded.adapter_id,
           config_json=excluded.config_json,
           enabled=1,
           validation_status=NULL,
           validation_detail=NULL,
           validation_checked_at=NULL,
           updated_at=excluded.updated_at",
        params![
            target.id,
            target.name,
            target.kind,
            target.adapter_id,
            target.config.to_string(),
            timestamp
        ],
    )?;
    Ok(())
}

pub fn set_target_validation(
    conn: &Connection,
    id: &str,
    status: &str,
    detail: &str,
    checked_at: &str,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE targets
         SET validation_status = ?2,
             validation_detail = ?3,
             validation_checked_at = ?4
         WHERE id = ?1",
        params![id, status, detail, checked_at],
    )?;
    Ok(changed > 0)
}

pub fn delete_target(conn: &Connection, id: &str) -> Result<bool> {
    if id == "mock-agent" {
        return Ok(false);
    }
    let changed = conn.execute("DELETE FROM targets WHERE id = ?1", params![id])?;
    Ok(changed > 0)
}

pub fn set_target_enabled(conn: &Connection, id: &str, enabled: bool) -> Result<bool> {
    if id == "mock-agent" && !enabled {
        return Ok(false);
    }
    let changed = conn.execute(
        "UPDATE targets
         SET enabled = ?2,
             validation_status = NULL,
             validation_detail = NULL,
             validation_checked_at = NULL,
             updated_at = ?3
         WHERE id = ?1",
        params![id, if enabled { 1 } else { 0 }, now()],
    )?;
    Ok(changed > 0)
}

pub fn export_target_redacted(conn: &Connection, id: &str) -> Result<Option<serde_json::Value>> {
    let mut stmt =
        conn.prepare("SELECT id, name, kind, adapter_id, config_json FROM targets WHERE id = ?1")?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let config_json: String = row.get(4)?;
    let mut config: serde_json::Value =
        serde_json::from_str(&config_json).unwrap_or_else(|_| serde_json::json!({}));
    redact_config(&mut config);
    Ok(Some(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "name": row.get::<_, String>(1)?,
        "kind": row.get::<_, String>(2)?,
        "adapter_id": row.get::<_, String>(3)?,
        "config": config,
    })))
}

fn redact_config(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_secret_config_key(key) {
                    *value = serde_json::Value::String("[REDACTED]".into());
                } else {
                    redact_config(value);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_config(item);
            }
        }
        _ => {}
    }
}

fn is_secret_config_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "api_key"
            | "apikey"
            | "api_key_env"
            | "api_key_keychain"
            | "authorization"
            | "bearer"
            | "token"
            | "access_token"
            | "refresh_token"
            | "secret"
            | "password"
            | "private_key"
            | "client_secret"
    ) || lower.ends_with("_api_key")
        || lower.ends_with("_apikey")
        || lower.ends_with("_token")
        || lower.ends_with("_secret")
        || lower.ends_with("_password")
        || lower.ends_with("_private_key")
}

pub fn insert_run(
    conn: &Connection,
    id: &str,
    target_id: &str,
    pack_id: &str,
    task_id: &str,
    status: &str,
    started_at: &str,
    finished_at: &str,
    error_code: Option<&str>,
    error_message: Option<&str>,
    config: &serde_json::Value,
    reproducibility: &serde_json::Value,
) -> Result<()> {
    insert_run_with_group(
        conn,
        id,
        None,
        target_id,
        pack_id,
        task_id,
        status,
        started_at,
        finished_at,
        error_code,
        error_message,
        config,
        reproducibility,
    )
}

pub fn insert_run_with_group(
    conn: &Connection,
    id: &str,
    run_group_id: Option<&str>,
    target_id: &str,
    pack_id: &str,
    task_id: &str,
    status: &str,
    started_at: &str,
    finished_at: &str,
    error_code: Option<&str>,
    error_message: Option<&str>,
    config: &serde_json::Value,
    reproducibility: &serde_json::Value,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO runs
         (id, run_group_id, target_id, benchmark_pack_id, task_id, status, started_at, finished_at, error_code, error_message, config_json, reproducibility_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            id,
            run_group_id,
            target_id,
            pack_id,
            task_id,
            status,
            started_at,
            finished_at,
            error_code,
            error_message,
            config.to_string(),
            reproducibility.to_string(),
        ],
    )?;
    Ok(())
}

pub fn insert_run_group(
    conn: &Connection,
    id: &str,
    benchmark_pack_id: &str,
    target_ids: &[String],
    status: &str,
    started_at: &str,
    config: &serde_json::Value,
) -> Result<()> {
    conn.execute(
        "INSERT INTO run_groups
         (id, benchmark_pack_id, target_ids_json, status, started_at, finished_at, config_json)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
        params![
            id,
            benchmark_pack_id,
            serde_json::to_string(target_ids).unwrap_or_else(|_| "[]".into()),
            status,
            started_at,
            config.to_string()
        ],
    )?;
    Ok(())
}

pub fn update_run_group_status(
    conn: &Connection,
    id: &str,
    status: &str,
    finished_at: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE run_groups SET status = ?1, finished_at = COALESCE(?2, finished_at) WHERE id = ?3",
        params![status, finished_at, id],
    )?;
    Ok(())
}

pub fn list_run_groups(conn: &Connection) -> Result<Vec<RunGroupRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, benchmark_pack_id, target_ids_json, status, started_at, finished_at, config_json
         FROM run_groups
         ORDER BY started_at DESC, id DESC",
    )?;
    let groups = stmt.query_map([], run_group_from_row)?.collect();
    groups
}

pub fn insert_run_job(conn: &Connection, job: &RunJobRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO run_jobs
         (id, run_group_id, benchmark_pack_id, status, message, started_at, finished_at, total, completed, error, request_json, result_run_ids_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            job.id,
            job.run_group_id,
            job.benchmark_pack_id,
            job.status,
            job.message,
            job.started_at,
            job.finished_at,
            job.total as i64,
            job.completed as i64,
            job.error,
            job.request.to_string(),
            serde_json::to_string(&job.result_run_ids).unwrap_or_else(|_| "[]".into()),
        ],
    )?;
    Ok(())
}

pub fn update_run_job_progress(
    conn: &Connection,
    id: &str,
    status: &str,
    message: &str,
    total: usize,
    completed: usize,
) -> Result<()> {
    conn.execute(
        "UPDATE run_jobs
         SET status = ?1, message = ?2, total = ?3, completed = ?4
         WHERE id = ?5 AND status IN ('queued', 'running')",
        params![status, message, total as i64, completed as i64, id],
    )?;
    Ok(())
}

pub fn request_cancel_run_job(conn: &Connection, id: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE run_jobs
         SET status = 'cancelling', message = 'Cancellation requested'
         WHERE id = ?1 AND status IN ('queued', 'running')",
        params![id],
    )?;
    Ok(changed > 0)
}

pub fn run_job_cancellation_requested(conn: &Connection, id: &str) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT status FROM run_jobs WHERE id = ?1")?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(false);
    };
    let status: String = row.get(0)?;
    Ok(matches!(status.as_str(), "cancelling" | "cancelled"))
}

pub fn finish_run_job(
    conn: &Connection,
    id: &str,
    status: &str,
    message: &str,
    total: usize,
    completed: usize,
    finished_at: &str,
    error: Option<&str>,
    result_run_ids: &[String],
) -> Result<()> {
    conn.execute(
        "UPDATE run_jobs
         SET status = ?1, message = ?2, total = ?3, completed = ?4, finished_at = ?5, error = ?6, result_run_ids_json = ?7
         WHERE id = ?8",
        params![
            status,
            message,
            total as i64,
            completed as i64,
            finished_at,
            error,
            serde_json::to_string(result_run_ids).unwrap_or_else(|_| "[]".into()),
            id,
        ],
    )?;
    Ok(())
}

pub fn recover_interrupted_run_jobs(conn: &Connection) -> Result<usize> {
    let active_jobs = list_run_jobs(conn)?
        .into_iter()
        .filter(|job| matches!(job.status.as_str(), "queued" | "running" | "cancelling"))
        .collect::<Vec<_>>();
    let finished_at = now();
    for job in &active_jobs {
        let result_ids = list_results_for_group(conn, &job.run_group_id)?
            .into_iter()
            .map(|result| result.id)
            .collect::<Vec<_>>();
        let completed = job.completed.max(result_ids.len());
        let total = job.total.max(completed);
        let message = if result_ids.is_empty() {
            "Interrupted by app restart; no worker is attached".to_string()
        } else {
            format!(
                "Interrupted by app restart; no worker is attached ({} partial result(s) available)",
                result_ids.len()
            )
        };
        finish_run_job(
            conn,
            &job.id,
            "failed",
            &message,
            total,
            completed,
            &finished_at,
            Some("interrupted"),
            &result_ids,
        )?;
        update_run_group_status(conn, &job.run_group_id, "failed", Some(&finished_at))?;
    }
    Ok(active_jobs.len())
}

pub fn list_run_jobs(conn: &Connection) -> Result<Vec<RunJobRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_group_id, benchmark_pack_id, status, message, started_at, finished_at, total, completed, error, request_json, result_run_ids_json
         FROM run_jobs
         ORDER BY started_at DESC, id DESC",
    )?;
    let jobs = stmt.query_map([], run_job_from_row)?.collect();
    jobs
}

pub fn get_run_job(conn: &Connection, id: &str) -> Result<Option<RunJobRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_group_id, benchmark_pack_id, status, message, started_at, finished_at, total, completed, error, request_json, result_run_ids_json
         FROM run_jobs
         WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    run_job_from_row(row).map(Some)
}

pub fn clear_terminal_run_jobs(conn: &Connection) -> Result<usize> {
    conn.execute(
        "DELETE FROM run_jobs WHERE status IN ('completed', 'failed', 'cancelled')",
        [],
    )
}

pub fn insert_hf_download_job(conn: &Connection, job: &HfDownloadJobRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO hf_download_jobs
         (id, repo_id, selected_file, status, message, started_at, finished_at, planned_bytes, transferred_bytes, local_dir, error, request_json, model_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            job.id,
            job.repo_id,
            job.selected_file,
            job.status,
            job.message,
            job.started_at,
            job.finished_at,
            job.planned_bytes.map(|value| value as i64),
            job.transferred_bytes as i64,
            job.local_dir,
            job.error,
            job.request.to_string(),
            job.model.as_ref().map(|value| value.to_string()),
        ],
    )?;
    Ok(())
}

pub fn update_hf_download_job_progress(
    conn: &Connection,
    id: &str,
    status: &str,
    message: &str,
    selected_file: Option<&str>,
    planned_bytes: Option<u64>,
    transferred_bytes: u64,
    local_dir: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE hf_download_jobs
         SET status = ?1, message = ?2, selected_file = COALESCE(?3, selected_file), planned_bytes = COALESCE(?4, planned_bytes), transferred_bytes = ?5, local_dir = COALESCE(?6, local_dir)
         WHERE id = ?7 AND status IN ('queued', 'running')",
        params![
            status,
            message,
            selected_file,
            planned_bytes.map(|value| value as i64),
            transferred_bytes as i64,
            local_dir,
            id,
        ],
    )?;
    Ok(())
}

pub fn request_cancel_hf_download_job(conn: &Connection, id: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE hf_download_jobs
         SET status = 'cancelling', message = 'Cancellation requested'
         WHERE id = ?1 AND status IN ('queued', 'running')",
        params![id],
    )?;
    Ok(changed > 0)
}

pub fn hf_download_job_cancellation_requested(conn: &Connection, id: &str) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT status FROM hf_download_jobs WHERE id = ?1")?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(false);
    };
    let status: String = row.get(0)?;
    Ok(matches!(status.as_str(), "cancelling" | "cancelled"))
}

pub fn finish_hf_download_job(
    conn: &Connection,
    id: &str,
    status: &str,
    message: &str,
    finished_at: &str,
    error: Option<&str>,
    model: Option<&serde_json::Value>,
) -> Result<()> {
    conn.execute(
        "UPDATE hf_download_jobs
         SET status = ?1, message = ?2, finished_at = ?3, error = ?4, model_json = ?5
         WHERE id = ?6",
        params![
            status,
            message,
            finished_at,
            error,
            model.map(|value| value.to_string()),
            id,
        ],
    )?;
    Ok(())
}

pub fn recover_interrupted_hf_download_jobs(conn: &Connection) -> Result<usize> {
    let active_jobs = list_hf_download_jobs(conn)?
        .into_iter()
        .filter(|job| matches!(job.status.as_str(), "queued" | "running" | "cancelling"))
        .collect::<Vec<_>>();
    let finished_at = now();
    for job in &active_jobs {
        let message = interrupted_hf_download_message(job);
        finish_hf_download_job(
            conn,
            &job.id,
            "failed",
            &message,
            &finished_at,
            Some("interrupted"),
            None,
        )?;
    }
    Ok(active_jobs.len())
}

fn interrupted_hf_download_message(job: &HfDownloadJobRecord) -> String {
    let base = "Interrupted by app restart; no download worker is attached";
    if job.transferred_bytes == 0 {
        return base.into();
    }
    match job.planned_bytes.filter(|planned| *planned > 0) {
        Some(planned) => {
            let percent =
                ((job.transferred_bytes as f64 / planned as f64) * 100.0).clamp(0.0, 100.0);
            format!(
                "{} ({} of {} bytes transferred, {:.0}%; retry will reuse local partial files when possible)",
                base, job.transferred_bytes, planned, percent
            )
        }
        None => format!(
            "{} ({} bytes transferred; retry will reuse local partial files when possible)",
            base, job.transferred_bytes
        ),
    }
}

pub fn list_hf_download_jobs(conn: &Connection) -> Result<Vec<HfDownloadJobRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, repo_id, selected_file, status, message, started_at, finished_at, planned_bytes, transferred_bytes, local_dir, error, request_json, model_json
         FROM hf_download_jobs
         ORDER BY started_at DESC, id DESC",
    )?;
    let jobs = stmt.query_map([], hf_download_job_from_row)?.collect();
    jobs
}

pub fn get_hf_download_job(conn: &Connection, id: &str) -> Result<Option<HfDownloadJobRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, repo_id, selected_file, status, message, started_at, finished_at, planned_bytes, transferred_bytes, local_dir, error, request_json, model_json
         FROM hf_download_jobs
         WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    hf_download_job_from_row(row).map(Some)
}

pub fn clear_terminal_hf_download_jobs(conn: &Connection) -> Result<usize> {
    conn.execute(
        "DELETE FROM hf_download_jobs WHERE status IN ('completed', 'failed', 'cancelled')",
        [],
    )
}

pub fn insert_hf_server_job(conn: &Connection, job: &HfServerJobRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO hf_server_jobs
         (id, repo_id, selected_file, port, context, status, message, started_at, finished_at, error, request_json, server_status_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            job.id,
            job.repo_id,
            job.selected_file,
            job.port as i64,
            job.context as i64,
            job.status,
            job.message,
            job.started_at,
            job.finished_at,
            job.error,
            job.request.to_string(),
            job.server_status.as_ref().map(|value| value.to_string()),
        ],
    )?;
    Ok(())
}

pub fn update_hf_server_job_progress(
    conn: &Connection,
    id: &str,
    status: &str,
    message: &str,
    selected_file: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE hf_server_jobs
         SET status = ?1, message = ?2, selected_file = COALESCE(?3, selected_file)
         WHERE id = ?4 AND status IN ('queued', 'running', 'cancelling')",
        params![status, message, selected_file, id],
    )?;
    Ok(())
}

pub fn request_cancel_hf_server_job(conn: &Connection, id: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE hf_server_jobs
         SET status = 'cancelling', message = 'Cancellation requested'
         WHERE id = ?1 AND status IN ('queued', 'running')",
        params![id],
    )?;
    Ok(changed > 0)
}

pub fn hf_server_job_cancellation_requested(conn: &Connection, id: &str) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT status FROM hf_server_jobs WHERE id = ?1")?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(false);
    };
    let status: String = row.get(0)?;
    Ok(matches!(status.as_str(), "cancelling" | "cancelled"))
}

pub fn finish_hf_server_job(
    conn: &Connection,
    id: &str,
    status: &str,
    message: &str,
    finished_at: &str,
    error: Option<&str>,
    server_status: Option<&serde_json::Value>,
) -> Result<()> {
    conn.execute(
        "UPDATE hf_server_jobs
         SET status = ?1, message = ?2, finished_at = ?3, error = ?4, server_status_json = ?5
         WHERE id = ?6",
        params![
            status,
            message,
            finished_at,
            error,
            server_status.map(|value| value.to_string()),
            id,
        ],
    )?;
    Ok(())
}

pub fn recover_interrupted_hf_server_jobs(conn: &Connection) -> Result<usize> {
    let active_jobs = list_hf_server_jobs(conn)?
        .into_iter()
        .filter(|job| matches!(job.status.as_str(), "queued" | "running" | "cancelling"))
        .collect::<Vec<_>>();
    let finished_at = now();
    for job in &active_jobs {
        finish_hf_server_job(
            conn,
            &job.id,
            "failed",
            "Interrupted by app restart; no server-start worker is attached",
            &finished_at,
            Some("interrupted"),
            None,
        )?;
    }
    Ok(active_jobs.len())
}

pub fn list_hf_server_jobs(conn: &Connection) -> Result<Vec<HfServerJobRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, repo_id, selected_file, port, context, status, message, started_at, finished_at, error, request_json, server_status_json
         FROM hf_server_jobs
         ORDER BY started_at DESC, id DESC",
    )?;
    let jobs = stmt.query_map([], hf_server_job_from_row)?.collect();
    jobs
}

pub fn get_hf_server_job(conn: &Connection, id: &str) -> Result<Option<HfServerJobRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, repo_id, selected_file, port, context, status, message, started_at, finished_at, error, request_json, server_status_json
         FROM hf_server_jobs
         WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    hf_server_job_from_row(row).map(Some)
}

pub fn clear_terminal_hf_server_jobs(conn: &Connection) -> Result<usize> {
    conn.execute(
        "DELETE FROM hf_server_jobs WHERE status IN ('completed', 'failed', 'cancelled')",
        [],
    )
}

fn run_job_from_row(row: &rusqlite::Row<'_>) -> Result<RunJobRecord> {
    let request_json: String = row.get(10)?;
    let result_run_ids_json: String = row.get(11)?;
    let total: i64 = row.get(7)?;
    let completed: i64 = row.get(8)?;
    Ok(RunJobRecord {
        id: row.get(0)?,
        run_group_id: row.get(1)?,
        benchmark_pack_id: row.get(2)?,
        status: row.get(3)?,
        message: row.get(4)?,
        started_at: row.get(5)?,
        finished_at: row.get(6)?,
        total: total.max(0) as usize,
        completed: completed.max(0) as usize,
        error: row.get(9)?,
        request: serde_json::from_str(&request_json).unwrap_or_else(|_| serde_json::json!({})),
        result_run_ids: serde_json::from_str(&result_run_ids_json).unwrap_or_default(),
    })
}

fn hf_download_job_from_row(row: &rusqlite::Row<'_>) -> Result<HfDownloadJobRecord> {
    let planned_bytes: Option<i64> = row.get(7)?;
    let transferred_bytes: i64 = row.get(8)?;
    let request_json: String = row.get(11)?;
    let model_json: Option<String> = row.get(12)?;
    Ok(HfDownloadJobRecord {
        id: row.get(0)?,
        repo_id: row.get(1)?,
        selected_file: row.get(2)?,
        status: row.get(3)?,
        message: row.get(4)?,
        started_at: row.get(5)?,
        finished_at: row.get(6)?,
        planned_bytes: planned_bytes.map(|value| value.max(0) as u64),
        transferred_bytes: transferred_bytes.max(0) as u64,
        local_dir: row.get(9)?,
        error: row.get(10)?,
        request: serde_json::from_str(&request_json).unwrap_or_else(|_| serde_json::json!({})),
        model: model_json.and_then(|raw| serde_json::from_str(&raw).ok()),
    })
}

fn hf_server_job_from_row(row: &rusqlite::Row<'_>) -> Result<HfServerJobRecord> {
    let port: i64 = row.get(3)?;
    let context: i64 = row.get(4)?;
    let request_json: String = row.get(10)?;
    let server_status_json: Option<String> = row.get(11)?;
    Ok(HfServerJobRecord {
        id: row.get(0)?,
        repo_id: row.get(1)?,
        selected_file: row.get(2)?,
        port: port.clamp(0, u16::MAX as i64) as u16,
        context: context.clamp(0, u32::MAX as i64) as u32,
        status: row.get(5)?,
        message: row.get(6)?,
        started_at: row.get(7)?,
        finished_at: row.get(8)?,
        error: row.get(9)?,
        request: serde_json::from_str(&request_json).unwrap_or_else(|_| serde_json::json!({})),
        server_status: server_status_json.and_then(|raw| serde_json::from_str(&raw).ok()),
    })
}

fn run_group_from_row(row: &rusqlite::Row<'_>) -> Result<RunGroupRecord> {
    let target_ids_json: String = row.get(2)?;
    let config_json: String = row.get(6)?;
    Ok(RunGroupRecord {
        id: row.get(0)?,
        benchmark_pack_id: row.get(1)?,
        target_ids: serde_json::from_str(&target_ids_json).unwrap_or_default(),
        status: row.get(3)?,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        config: serde_json::from_str(&config_json).unwrap_or_else(|_| serde_json::json!({})),
    })
}

pub fn insert_metric(
    conn: &Connection,
    run_id: &str,
    name: &str,
    value: Option<f64>,
    unit: Option<&str>,
    source: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO metrics (run_id, name, value, unit, source) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![run_id, name, value, unit, source],
    )?;
    Ok(())
}

pub fn insert_metric_text(
    conn: &Connection,
    run_id: &str,
    name: &str,
    text_value: &str,
    source: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO metrics (run_id, name, text_value, source) VALUES (?1, ?2, ?3, ?4)",
        params![run_id, name, text_value, source],
    )?;
    Ok(())
}

pub fn insert_artifact(
    conn: &Connection,
    run_id: &str,
    kind: &str,
    path: &Path,
    mime_type: Option<&str>,
    sha256: Option<&str>,
    metadata: &serde_json::Value,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let size_bytes = fs::metadata(path).ok().map(|m| m.len() as i64);
    conn.execute(
        "INSERT INTO artifacts (id, run_id, kind, path, mime_type, size_bytes, sha256, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            run_id,
            kind,
            path.to_string_lossy(),
            mime_type,
            size_bytes,
            sha256,
            metadata.to_string(),
        ],
    )?;
    Ok(id)
}

pub fn list_results(conn: &Connection) -> Result<Vec<ResultRecord>> {
    let mut stmt = conn.prepare(
        "SELECT
           r.id, r.run_group_id, r.target_id, r.benchmark_pack_id, r.task_id, r.status, r.started_at, r.finished_at,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'score' ORDER BY id DESC LIMIT 1) AS score,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'wall_time_ms' ORDER BY id DESC LIMIT 1) AS wall_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'setup_time_ms' ORDER BY id DESC LIMIT 1) AS setup_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'target_time_ms' ORDER BY id DESC LIMIT 1) AS target_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'evaluation_time_ms' ORDER BY id DESC LIMIT 1) AS evaluation_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'model_call_wall_time_ms' ORDER BY id DESC LIMIT 1) AS model_call_wall_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'prompt_tokens' ORDER BY id DESC LIMIT 1) AS prompt_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'completion_tokens' ORDER BY id DESC LIMIT 1) AS completion_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'reasoning_tokens' ORDER BY id DESC LIMIT 1) AS reasoning_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'cached_tokens' ORDER BY id DESC LIMIT 1) AS cached_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'cache_read_tokens' ORDER BY id DESC LIMIT 1) AS cache_read_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'cache_write_tokens' ORDER BY id DESC LIMIT 1) AS cache_write_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'total_tokens' ORDER BY id DESC LIMIT 1) AS total_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'cost_usd' ORDER BY id DESC LIMIT 1) AS cost_usd,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_attempts' ORDER BY id DESC LIMIT 1) AS provider_attempts,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_retry_after_ms' ORDER BY id DESC LIMIT 1) AS provider_retry_after_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_retry_delay_ms' ORDER BY id DESC LIMIT 1) AS provider_retry_delay_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'http_status' ORDER BY id DESC LIMIT 1) AS http_status,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_time_to_first_byte_ms' ORDER BY id DESC LIMIT 1) AS provider_time_to_first_byte_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_time_to_first_token_ms' ORDER BY id DESC LIMIT 1) AS provider_time_to_first_token_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_request_total_ms' ORDER BY id DESC LIMIT 1) AS provider_request_total_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'output_tokens_per_second' ORDER BY id DESC LIMIT 1) AS output_tokens_per_second,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'peak_rss_mb' ORDER BY id DESC LIMIT 1) AS peak_rss_mb,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'exit_code' ORDER BY id DESC LIMIT 1) AS exit_code,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'harness_exit_code' ORDER BY id DESC LIMIT 1) AS harness_exit_code,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'stdout_bytes' ORDER BY id DESC LIMIT 1) AS stdout_bytes,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'stderr_bytes' ORDER BY id DESC LIMIT 1) AS stderr_bytes,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'files_changed' ORDER BY id DESC LIMIT 1) AS files_changed,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'lines_added' ORDER BY id DESC LIMIT 1) AS lines_added,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'lines_deleted' ORDER BY id DESC LIMIT 1) AS lines_deleted,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'commands_observed_count' ORDER BY id DESC LIMIT 1) AS commands_observed_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'dangerous_command_hits' ORDER BY id DESC LIMIT 1) AS dangerous_command_hits,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'security_finding_count' ORDER BY id DESC LIMIT 1) AS security_finding_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'security_files_scanned' ORDER BY id DESC LIMIT 1) AS security_files_scanned,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_file_count' ORDER BY id DESC LIMIT 1) AS import_file_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_total_file_count' ORDER BY id DESC LIMIT 1) AS import_total_file_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_omitted_file_count' ORDER BY id DESC LIMIT 1) AS import_omitted_file_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_unsupported_file_count' ORDER BY id DESC LIMIT 1) AS import_unsupported_file_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_truncated' ORDER BY id DESC LIMIT 1) AS import_truncated,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_truncated_bytes' ORDER BY id DESC LIMIT 1) AS import_truncated_bytes,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'provider_model' ORDER BY id DESC LIMIT 1) AS provider_model,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'provider_model_source' ORDER BY id DESC LIMIT 1) AS provider_model_source,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'finish_reason' ORDER BY id DESC LIMIT 1) AS finish_reason,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'pricing_assumption' ORDER BY id DESC LIMIT 1) AS pricing_assumption,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'import_format' ORDER BY id DESC LIMIT 1) AS import_format,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'import_source' ORDER BY id DESC LIMIT 1) AS import_source,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'import_path' ORDER BY id DESC LIMIT 1) AS import_path,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'summary_source' ORDER BY id DESC LIMIT 1) AS summary_source,
           r.error_code, r.error_message, r.reproducibility_json
         FROM runs r
         ORDER BY COALESCE(r.started_at, '') DESC, r.id DESC",
    )?;
    let results = stmt.query_map([], result_from_row)?.collect();
    results
}

pub fn list_results_for_group(conn: &Connection, run_group_id: &str) -> Result<Vec<ResultRecord>> {
    let mut stmt = conn.prepare(
        "SELECT
           r.id, r.run_group_id, r.target_id, r.benchmark_pack_id, r.task_id, r.status, r.started_at, r.finished_at,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'score' ORDER BY id DESC LIMIT 1) AS score,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'wall_time_ms' ORDER BY id DESC LIMIT 1) AS wall_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'setup_time_ms' ORDER BY id DESC LIMIT 1) AS setup_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'target_time_ms' ORDER BY id DESC LIMIT 1) AS target_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'evaluation_time_ms' ORDER BY id DESC LIMIT 1) AS evaluation_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'model_call_wall_time_ms' ORDER BY id DESC LIMIT 1) AS model_call_wall_time_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'prompt_tokens' ORDER BY id DESC LIMIT 1) AS prompt_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'completion_tokens' ORDER BY id DESC LIMIT 1) AS completion_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'reasoning_tokens' ORDER BY id DESC LIMIT 1) AS reasoning_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'cached_tokens' ORDER BY id DESC LIMIT 1) AS cached_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'cache_read_tokens' ORDER BY id DESC LIMIT 1) AS cache_read_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'cache_write_tokens' ORDER BY id DESC LIMIT 1) AS cache_write_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'total_tokens' ORDER BY id DESC LIMIT 1) AS total_tokens,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'cost_usd' ORDER BY id DESC LIMIT 1) AS cost_usd,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_attempts' ORDER BY id DESC LIMIT 1) AS provider_attempts,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_retry_after_ms' ORDER BY id DESC LIMIT 1) AS provider_retry_after_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_retry_delay_ms' ORDER BY id DESC LIMIT 1) AS provider_retry_delay_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'http_status' ORDER BY id DESC LIMIT 1) AS http_status,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_time_to_first_byte_ms' ORDER BY id DESC LIMIT 1) AS provider_time_to_first_byte_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_time_to_first_token_ms' ORDER BY id DESC LIMIT 1) AS provider_time_to_first_token_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'provider_request_total_ms' ORDER BY id DESC LIMIT 1) AS provider_request_total_ms,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'output_tokens_per_second' ORDER BY id DESC LIMIT 1) AS output_tokens_per_second,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'peak_rss_mb' ORDER BY id DESC LIMIT 1) AS peak_rss_mb,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'exit_code' ORDER BY id DESC LIMIT 1) AS exit_code,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'harness_exit_code' ORDER BY id DESC LIMIT 1) AS harness_exit_code,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'stdout_bytes' ORDER BY id DESC LIMIT 1) AS stdout_bytes,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'stderr_bytes' ORDER BY id DESC LIMIT 1) AS stderr_bytes,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'files_changed' ORDER BY id DESC LIMIT 1) AS files_changed,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'lines_added' ORDER BY id DESC LIMIT 1) AS lines_added,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'lines_deleted' ORDER BY id DESC LIMIT 1) AS lines_deleted,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'commands_observed_count' ORDER BY id DESC LIMIT 1) AS commands_observed_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'dangerous_command_hits' ORDER BY id DESC LIMIT 1) AS dangerous_command_hits,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'security_finding_count' ORDER BY id DESC LIMIT 1) AS security_finding_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'security_files_scanned' ORDER BY id DESC LIMIT 1) AS security_files_scanned,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_file_count' ORDER BY id DESC LIMIT 1) AS import_file_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_total_file_count' ORDER BY id DESC LIMIT 1) AS import_total_file_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_omitted_file_count' ORDER BY id DESC LIMIT 1) AS import_omitted_file_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_unsupported_file_count' ORDER BY id DESC LIMIT 1) AS import_unsupported_file_count,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_truncated' ORDER BY id DESC LIMIT 1) AS import_truncated,
           (SELECT value FROM metrics WHERE run_id = r.id AND name = 'import_truncated_bytes' ORDER BY id DESC LIMIT 1) AS import_truncated_bytes,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'provider_model' ORDER BY id DESC LIMIT 1) AS provider_model,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'provider_model_source' ORDER BY id DESC LIMIT 1) AS provider_model_source,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'finish_reason' ORDER BY id DESC LIMIT 1) AS finish_reason,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'pricing_assumption' ORDER BY id DESC LIMIT 1) AS pricing_assumption,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'import_format' ORDER BY id DESC LIMIT 1) AS import_format,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'import_source' ORDER BY id DESC LIMIT 1) AS import_source,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'import_path' ORDER BY id DESC LIMIT 1) AS import_path,
           (SELECT text_value FROM metrics WHERE run_id = r.id AND name = 'summary_source' ORDER BY id DESC LIMIT 1) AS summary_source,
           r.error_code, r.error_message, r.reproducibility_json
         FROM runs r
         WHERE r.run_group_id = ?1
         ORDER BY COALESCE(r.started_at, '') DESC, r.id DESC",
    )?;
    let results = stmt
        .query_map(params![run_group_id], result_from_row)?
        .collect();
    results
}

fn result_from_row(row: &rusqlite::Row<'_>) -> Result<ResultRecord> {
    let reproducibility_json: String = row.get(58)?;
    Ok(ResultRecord {
        id: row.get(0)?,
        run_group_id: row.get(1)?,
        target_id: row.get(2)?,
        benchmark_pack_id: row.get(3)?,
        task_id: row.get(4)?,
        status: row.get(5)?,
        started_at: row.get(6)?,
        finished_at: row.get(7)?,
        pass_fail: Some(row.get::<_, String>(5)? == "passed"),
        score: row.get(8)?,
        score_numeric: row.get(8)?,
        wall_time_ms: row.get(9)?,
        setup_time_ms: row.get(10)?,
        target_time_ms: row.get(11)?,
        evaluation_time_ms: row.get(12)?,
        model_call_wall_time_ms: row.get(13)?,
        input_tokens: row.get(14)?,
        output_tokens: row.get(15)?,
        prompt_tokens: row.get(14)?,
        completion_tokens: row.get(15)?,
        reasoning_tokens: row.get(16)?,
        cached_tokens: row.get(17)?,
        cache_read_tokens: row.get(18)?,
        cache_write_tokens: row.get(19)?,
        total_tokens: row.get(20)?,
        estimated_cost_usd: row.get(21)?,
        cost_usd: row.get(21)?,
        provider_attempts: row.get(22)?,
        provider_retry_after_ms: row.get(23)?,
        provider_retry_delay_ms: row.get(24)?,
        http_status: row.get(25)?,
        provider_time_to_first_byte_ms: row.get(26)?,
        ttft_ms: row.get(27)?,
        provider_time_to_first_token_ms: row.get(27)?,
        provider_request_total_ms: row.get(28)?,
        decode_tokens_per_sec: row.get(29)?,
        output_tokens_per_second: row.get(29)?,
        peak_rss_mb: row.get(30)?,
        exit_code: row.get(31)?,
        harness_exit_code: row.get(32)?,
        stdout_bytes: row.get(33)?,
        stderr_bytes: row.get(34)?,
        files_changed: row.get(35)?,
        lines_added: row.get(36)?,
        lines_deleted: row.get(37)?,
        commands_observed_count: row.get(38)?,
        dangerous_command_hits: row.get(39)?,
        security_finding_count: row.get(40)?,
        security_files_scanned: row.get(41)?,
        import_file_count: row.get(42)?,
        import_total_file_count: row.get(43)?,
        import_omitted_file_count: row.get(44)?,
        import_unsupported_file_count: row.get(45)?,
        import_truncated: row.get(46)?,
        import_truncated_bytes: row.get(47)?,
        provider_model: row.get(48)?,
        provider_model_source: row.get(49)?,
        finish_reason: row.get(50)?,
        pricing_assumption: row.get(51)?,
        import_format: row.get(52)?,
        import_source: row.get(53)?,
        import_path: row.get(54)?,
        summary_source: row.get(55)?,
        error_code: row.get(56)?,
        error_message: row.get(57)?,
        reproducibility: serde_json::from_str(&reproducibility_json)
            .unwrap_or_else(|_| serde_json::json!({})),
    })
}

pub fn list_artifacts(conn: &Connection, run_id: Option<&str>) -> Result<Vec<ArtifactRecord>> {
    let sql = if run_id.is_some() {
        "SELECT id, run_id, kind, path, mime_type, size_bytes, sha256, metadata_json FROM artifacts WHERE run_id = ?1 ORDER BY kind, path"
    } else {
        "SELECT id, run_id, kind, path, mime_type, size_bytes, sha256, metadata_json FROM artifacts ORDER BY run_id DESC, kind, path"
    };
    let mut stmt = conn.prepare(sql)?;
    let mapper = |row: &rusqlite::Row<'_>| {
        let metadata_json: String = row.get(7)?;
        Ok(ArtifactRecord {
            id: row.get(0)?,
            run_id: row.get(1)?,
            kind: row.get(2)?,
            path: row.get(3)?,
            mime_type: row.get(4)?,
            size_bytes: row.get(5)?,
            sha256: row.get(6)?,
            metadata: serde_json::from_str(&metadata_json)
                .unwrap_or_else(|_| serde_json::json!({})),
        })
    };
    if let Some(run_id) = run_id {
        stmt.query_map(params![run_id], mapper)?.collect()
    } else {
        stmt.query_map([], mapper)?.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply() {
        let conn = open_memory().expect("db should open");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0);
    }

    #[test]
    fn migration_adds_run_group_column_to_existing_runs_table() {
        let conn = Connection::open_in_memory().expect("db should open");
        conn.execute_batch(
            "CREATE TABLE runs (
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
              reproducibility_json TEXT NOT NULL
            );",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let has_group_column: bool = conn
            .prepare("PRAGMA table_info(runs)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .any(|name| name.unwrap() == "run_group_id");
        assert!(has_group_column);
    }

    #[test]
    fn migration_adds_target_validation_columns_to_existing_targets_table() {
        let conn = Connection::open_in_memory().expect("db should open");
        conn.execute_batch(
            "CREATE TABLE targets (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              kind TEXT NOT NULL,
              adapter_id TEXT NOT NULL,
              config_json TEXT NOT NULL,
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(targets)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|name| name.unwrap())
            .collect();
        assert!(columns.contains(&"validation_status".to_string()));
        assert!(columns.contains(&"validation_detail".to_string()));
        assert!(columns.contains(&"validation_checked_at".to_string()));
    }

    #[test]
    fn mock_target_is_seeded() {
        let conn = open_memory().expect("db should open");
        let targets = list_targets(&conn).expect("targets should list");
        assert!(targets.iter().any(|target| target.id == "mock-agent"));
    }

    #[test]
    fn target_export_redacts_secrets() {
        let conn = open_memory().expect("db should open");
        upsert_target(
            &conn,
            &NewTarget {
                id: "secret-target".into(),
                name: "Secret Target".into(),
                kind: "direct_model".into(),
                adapter_id: "openai".into(),
                config: serde_json::json!({
                    "api_key": "abc",
                    "api_key_env": "OPENAI_API_KEY",
                    "model": "x",
                    "max_tokens": 512,
                    "input_price_usd_per_million_tokens": 0.25,
                    "output_price_usd_per_million_tokens": 2.0,
                    "token_usage_reporting": true
                }),
            },
        )
        .expect("target should save");
        let exported = export_target_redacted(&conn, "secret-target")
            .unwrap()
            .unwrap();
        assert_eq!(exported["config"]["api_key"], "[REDACTED]");
        assert_eq!(exported["config"]["api_key_env"], "[REDACTED]");
        assert_eq!(exported["config"]["model"], "x");
        assert_eq!(exported["config"]["max_tokens"], 512);
        assert_eq!(
            exported["config"]["input_price_usd_per_million_tokens"],
            0.25
        );
        assert_eq!(
            exported["config"]["output_price_usd_per_million_tokens"],
            2.0
        );
        assert_eq!(exported["config"]["token_usage_reporting"], true);
    }

    #[test]
    fn result_records_include_numeric_and_text_provider_metrics() {
        let conn = open_memory().expect("db should open");
        insert_run(
            &conn,
            "run-provider-metrics",
            "mock-agent",
            "llm-core",
            "task-one",
            "passed",
            "2026-07-06T14:00:00Z",
            "2026-07-06T14:00:01Z",
            None,
            None,
            &serde_json::json!({}),
            &serde_json::json!({}),
        )
        .expect("run should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "score",
            Some(1.0),
            None,
            "prompt",
        )
        .expect("score metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "prompt_tokens",
            Some(100.0),
            Some("tokens"),
            "provider",
        )
        .expect("prompt token metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "completion_tokens",
            Some(25.0),
            Some("tokens"),
            "provider",
        )
        .expect("completion token metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "total_tokens",
            Some(125.0),
            Some("tokens"),
            "provider",
        )
        .expect("total token metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "cost_usd",
            Some(0.001),
            Some("USD"),
            "pricing",
        )
        .expect("cost metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "output_tokens_per_second",
            Some(25.0),
            Some("tokens/s"),
            "provider",
        )
        .expect("throughput metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "reasoning_tokens",
            Some(7.0),
            Some("tokens"),
            "provider",
        )
        .expect("numeric provider metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "cached_tokens",
            Some(11.0),
            Some("tokens"),
            "provider",
        )
        .expect("cached token metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "cache_read_tokens",
            Some(11.0),
            Some("tokens"),
            "provider",
        )
        .expect("cache read token metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "cache_write_tokens",
            Some(5.0),
            Some("tokens"),
            "provider",
        )
        .expect("cache write token metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "setup_time_ms",
            Some(111.0),
            Some("ms"),
            "setup",
        )
        .expect("setup time metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "target_time_ms",
            Some(222.0),
            Some("ms"),
            "target",
        )
        .expect("target time metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "evaluation_time_ms",
            Some(321.0),
            Some("ms"),
            "scoring",
        )
        .expect("evaluation time metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "model_call_wall_time_ms",
            Some(789.0),
            Some("ms"),
            "provider",
        )
        .expect("model call time metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "provider_time_to_first_byte_ms",
            Some(123.4),
            Some("ms"),
            "provider",
        )
        .expect("time-to-first-byte metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "provider_time_to_first_token_ms",
            Some(234.5),
            Some("ms"),
            "provider",
        )
        .expect("time-to-first-token metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "provider_request_total_ms",
            Some(456.7),
            Some("ms"),
            "provider",
        )
        .expect("request total metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "provider_retry_delay_ms",
            Some(250.0),
            Some("ms"),
            "provider",
        )
        .expect("provider retry delay metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "peak_rss_mb",
            Some(64.5),
            Some("MB"),
            "process",
        )
        .expect("peak RSS metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "exit_code",
            Some(1.0),
            None,
            "scoring",
        )
        .expect("exit code metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "harness_exit_code",
            Some(2.0),
            None,
            "worker",
        )
        .expect("harness exit code metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "stdout_bytes",
            Some(1024.0),
            Some("bytes"),
            "scoring",
        )
        .expect("stdout byte metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "stderr_bytes",
            Some(128.0),
            Some("bytes"),
            "scoring",
        )
        .expect("stderr byte metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "files_changed",
            Some(1.0),
            None,
            "workspace",
        )
        .expect("files changed metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "lines_added",
            Some(2.0),
            Some("lines"),
            "workspace",
        )
        .expect("lines added metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "lines_deleted",
            Some(3.0),
            Some("lines"),
            "workspace",
        )
        .expect("lines deleted metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "commands_observed_count",
            Some(2.0),
            None,
            "process",
        )
        .expect("commands observed metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "dangerous_command_hits",
            Some(4.0),
            None,
            "safety",
        )
        .expect("dangerous command hit metric should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "security_finding_count",
            Some(2.0),
            None,
            "worker",
        )
        .expect("security finding count should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "security_files_scanned",
            Some(9.0),
            None,
            "worker",
        )
        .expect("security files scanned should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "import_file_count",
            Some(2.0),
            None,
            "worker",
        )
        .expect("import file count should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "import_total_file_count",
            Some(5.0),
            None,
            "worker",
        )
        .expect("import total file count should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "import_omitted_file_count",
            Some(3.0),
            None,
            "worker",
        )
        .expect("import omitted file count should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "import_unsupported_file_count",
            Some(2.0),
            None,
            "worker",
        )
        .expect("import unsupported file count should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "import_truncated",
            Some(1.0),
            None,
            "worker",
        )
        .expect("import truncated flag should insert");
        insert_metric(
            &conn,
            "run-provider-metrics",
            "import_truncated_bytes",
            Some(4096.0),
            Some("bytes"),
            "worker",
        )
        .expect("import truncated bytes should insert");
        insert_metric_text(
            &conn,
            "run-provider-metrics",
            "provider_model",
            "gpt-test",
            "provider",
        )
        .expect("text provider metric should insert");
        insert_metric_text(
            &conn,
            "run-provider-metrics",
            "provider_model_source",
            "provider",
            "provider",
        )
        .expect("provider model source metric should insert");
        insert_metric_text(
            &conn,
            "run-provider-metrics",
            "finish_reason",
            "stop",
            "provider",
        )
        .expect("finish metric should insert");
        insert_metric_text(
            &conn,
            "run-provider-metrics",
            "pricing_assumption",
            "cache_read_tokens_priced_as_input",
            "pricing",
        )
        .expect("pricing assumption metric should insert");
        insert_metric_text(
            &conn,
            "run-provider-metrics",
            "import_format",
            "jsonl",
            "worker",
        )
        .expect("import format metric should insert");
        insert_metric_text(
            &conn,
            "run-provider-metrics",
            "import_source",
            "directory",
            "worker",
        )
        .expect("import source metric should insert");
        insert_metric_text(
            &conn,
            "run-provider-metrics",
            "import_path",
            "/tmp/benchforge/results",
            "worker",
        )
        .expect("import path metric should insert");
        insert_metric_text(
            &conn,
            "run-provider-metrics",
            "summary_source",
            "json_items",
            "worker",
        )
        .expect("summary source metric should insert");

        let result = list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|result| result.id == "run-provider-metrics")
            .expect("inserted run should be present");
        assert_eq!(result.pass_fail, Some(true));
        assert_eq!(result.score, Some(1.0));
        assert_eq!(result.score_numeric, result.score);
        assert_eq!(result.prompt_tokens, Some(100.0));
        assert_eq!(result.input_tokens, result.prompt_tokens);
        assert_eq!(result.completion_tokens, Some(25.0));
        assert_eq!(result.output_tokens, result.completion_tokens);
        assert_eq!(result.total_tokens, Some(125.0));
        assert_eq!(result.cost_usd, Some(0.001));
        assert_eq!(result.estimated_cost_usd, result.cost_usd);
        assert_eq!(result.reasoning_tokens, Some(7.0));
        assert_eq!(result.cached_tokens, Some(11.0));
        assert_eq!(result.cache_read_tokens, Some(11.0));
        assert_eq!(result.cache_write_tokens, Some(5.0));
        assert_eq!(result.setup_time_ms, Some(111.0));
        assert_eq!(result.target_time_ms, Some(222.0));
        assert_eq!(result.evaluation_time_ms, Some(321.0));
        assert_eq!(result.model_call_wall_time_ms, Some(789.0));
        assert_eq!(result.provider_time_to_first_byte_ms, Some(123.4));
        assert_eq!(result.provider_time_to_first_token_ms, Some(234.5));
        assert_eq!(result.ttft_ms, result.provider_time_to_first_token_ms);
        assert_eq!(result.provider_request_total_ms, Some(456.7));
        assert_eq!(result.provider_retry_delay_ms, Some(250.0));
        assert_eq!(result.output_tokens_per_second, Some(25.0));
        assert_eq!(
            result.decode_tokens_per_sec,
            result.output_tokens_per_second
        );
        assert_eq!(result.peak_rss_mb, Some(64.5));
        assert_eq!(result.exit_code, Some(1.0));
        assert_eq!(result.harness_exit_code, Some(2.0));
        assert_eq!(result.stdout_bytes, Some(1024.0));
        assert_eq!(result.stderr_bytes, Some(128.0));
        assert_eq!(result.files_changed, Some(1.0));
        assert_eq!(result.lines_added, Some(2.0));
        assert_eq!(result.lines_deleted, Some(3.0));
        assert_eq!(result.commands_observed_count, Some(2.0));
        assert_eq!(result.dangerous_command_hits, Some(4.0));
        assert_eq!(result.security_finding_count, Some(2.0));
        assert_eq!(result.security_files_scanned, Some(9.0));
        assert_eq!(result.provider_model.as_deref(), Some("gpt-test"));
        assert_eq!(result.provider_model_source.as_deref(), Some("provider"));
        assert_eq!(result.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            result.pricing_assumption.as_deref(),
            Some("cache_read_tokens_priced_as_input")
        );
        assert_eq!(result.import_file_count, Some(2.0));
        assert_eq!(result.import_total_file_count, Some(5.0));
        assert_eq!(result.import_omitted_file_count, Some(3.0));
        assert_eq!(result.import_unsupported_file_count, Some(2.0));
        assert_eq!(result.import_truncated, Some(1.0));
        assert_eq!(result.import_truncated_bytes, Some(4096.0));
        assert_eq!(result.import_format.as_deref(), Some("jsonl"));
        assert_eq!(result.import_source.as_deref(), Some("directory"));
        assert_eq!(
            result.import_path.as_deref(),
            Some("/tmp/benchforge/results")
        );
        assert_eq!(result.summary_source.as_deref(), Some("json_items"));
    }

    #[test]
    fn list_run_groups_decodes_queue_config() {
        let conn = open_memory().expect("db should open");
        insert_run_group(
            &conn,
            "group-one",
            "llm-core",
            &["target-one".into()],
            "completed",
            "2026-07-06T12:00:00Z",
            &serde_json::json!({
                "concurrency": 2,
                "targets": [{
                    "id": "target-one",
                    "generation": {"max_tokens": 512},
                    "pricing": {"input_price_usd_per_million_tokens": 0.25}
                }]
            }),
        )
        .expect("group should insert");

        let groups = list_run_groups(&conn).expect("groups should list");

        let group = groups
            .into_iter()
            .find(|group| group.id == "group-one")
            .expect("inserted group should be present");
        assert_eq!(group.benchmark_pack_id, "llm-core");
        assert_eq!(group.target_ids, vec!["target-one".to_string()]);
        assert_eq!(group.config["concurrency"], 2);
        assert_eq!(
            group.config["targets"][0]["pricing"]["input_price_usd_per_million_tokens"],
            0.25
        );
    }

    #[test]
    fn delete_target_removes_user_target_but_protects_mock() {
        let conn = open_memory().expect("db should open");
        upsert_target(
            &conn,
            &NewTarget {
                id: "stale-target".into(),
                name: "Stale Target".into(),
                kind: "direct_model".into(),
                adapter_id: "ollama-openai".into(),
                config: serde_json::json!({"model": "old"}),
            },
        )
        .expect("target should save");

        assert!(delete_target(&conn, "stale-target").unwrap());
        assert!(get_target(&conn, "stale-target").unwrap().is_none());
        assert!(!delete_target(&conn, "mock-agent").unwrap());
        assert!(get_target(&conn, "mock-agent").unwrap().is_some());
    }

    #[test]
    fn set_target_enabled_toggles_user_target_but_protects_mock() {
        let conn = open_memory().expect("db should open");
        upsert_target(
            &conn,
            &NewTarget {
                id: "toggle-target".into(),
                name: "Toggle Target".into(),
                kind: "direct_model".into(),
                adapter_id: "ollama-openai".into(),
                config: serde_json::json!({"model": "qwen"}),
            },
        )
        .expect("target should save");
        set_target_validation(
            &conn,
            "toggle-target",
            "ok",
            "ready",
            "2026-01-01T00:00:00Z",
        )
        .expect("validation should save");

        assert!(set_target_enabled(&conn, "toggle-target", false).unwrap());
        let disabled = get_target(&conn, "toggle-target")
            .expect("target query should work")
            .expect("target should exist");
        assert!(!disabled.enabled);
        assert!(disabled.validation_status.is_none());
        assert!(disabled.validation_detail.is_none());
        assert!(disabled.validation_checked_at.is_none());

        assert!(set_target_enabled(&conn, "toggle-target", true).unwrap());
        assert!(
            get_target(&conn, "toggle-target")
                .expect("target query should work")
                .expect("target should exist")
                .enabled
        );

        assert!(!set_target_enabled(&conn, "mock-agent", false).unwrap());
        assert!(
            get_target(&conn, "mock-agent")
                .expect("target query should work")
                .expect("mock target should exist")
                .enabled
        );
    }

    #[test]
    fn upsert_target_reenables_existing_target() {
        let conn = open_memory().expect("db should open");
        upsert_target(
            &conn,
            &NewTarget {
                id: "disabled-target".into(),
                name: "Disabled Target".into(),
                kind: "direct_model".into(),
                adapter_id: "ollama-openai".into(),
                config: serde_json::json!({"model": "old"}),
            },
        )
        .expect("target should save");
        conn.execute(
            "UPDATE targets SET enabled = 0 WHERE id = ?1",
            rusqlite::params!["disabled-target"],
        )
        .expect("target should be disabled");

        upsert_target(
            &conn,
            &NewTarget {
                id: "disabled-target".into(),
                name: "Enabled Target".into(),
                kind: "direct_model".into(),
                adapter_id: "ollama-openai".into(),
                config: serde_json::json!({"model": "new"}),
            },
        )
        .expect("target should save again");

        let target = get_target(&conn, "disabled-target")
            .expect("target query should work")
            .expect("target should exist");
        assert!(target.enabled);
        assert_eq!(target.name, "Enabled Target");
    }

    #[test]
    fn target_validation_health_round_trips_and_resets_on_edit() {
        let conn = open_memory().expect("db should open");
        upsert_target(
            &conn,
            &NewTarget {
                id: "health-target".into(),
                name: "Health Target".into(),
                kind: "mock".into(),
                adapter_id: "mock".into(),
                config: serde_json::json!({"mode": "deterministic-fixture-fix"}),
            },
        )
        .expect("target should save");

        assert!(set_target_validation(
            &conn,
            "health-target",
            "ok",
            "validation succeeded",
            "2026-07-07T12:00:00Z",
        )
        .expect("validation should persist"));
        let target = get_target(&conn, "health-target")
            .expect("target query should work")
            .expect("target should exist");
        assert_eq!(target.validation_status.as_deref(), Some("ok"));
        assert_eq!(
            target.validation_detail.as_deref(),
            Some("validation succeeded")
        );
        assert_eq!(
            target.validation_checked_at.as_deref(),
            Some("2026-07-07T12:00:00Z")
        );

        upsert_target(
            &conn,
            &NewTarget {
                id: "health-target".into(),
                name: "Edited Health Target".into(),
                kind: "mock".into(),
                adapter_id: "mock".into(),
                config: serde_json::json!({"mode": "edited"}),
            },
        )
        .expect("target edit should save");
        let edited = get_target(&conn, "health-target")
            .expect("target query should work")
            .expect("target should exist");
        assert!(edited.validation_status.is_none());
        assert!(edited.validation_detail.is_none());
        assert!(edited.validation_checked_at.is_none());
    }

    #[test]
    fn recovery_marks_stale_active_jobs_interrupted() {
        let conn = open_memory().expect("db should open");
        let started_at = "2026-07-06T12:00:00Z";
        insert_run_group(
            &conn,
            "stale-group",
            "llm-basics",
            &["mock-agent".into()],
            "running",
            started_at,
            &serde_json::json!({"concurrency": 1}),
        )
        .expect("group should insert");
        insert_run_job(
            &conn,
            &RunJobRecord {
                id: "stale-job".into(),
                run_group_id: "stale-group".into(),
                benchmark_pack_id: "llm-basics".into(),
                status: "running".into(),
                message: "Running prompt".into(),
                started_at: started_at.into(),
                finished_at: None,
                total: 3,
                completed: 0,
                error: None,
                request: serde_json::json!({"target_ids": ["mock-agent"]}),
                result_run_ids: vec![],
            },
        )
        .expect("job should insert");
        insert_run_with_group(
            &conn,
            "partial-run",
            Some("stale-group"),
            "mock-agent",
            "llm-basics",
            "prompt-latency",
            "passed",
            started_at,
            started_at,
            None,
            None,
            &serde_json::json!({}),
            &serde_json::json!({}),
        )
        .expect("partial result should insert");
        insert_run_with_group(
            &conn,
            "partial-run-two",
            Some("stale-group"),
            "mock-agent",
            "llm-basics",
            "prompt-json",
            "failed",
            started_at,
            started_at,
            Some("test_failed"),
            Some("JSON expectation failed"),
            &serde_json::json!({}),
            &serde_json::json!({}),
        )
        .expect("second partial result should insert");

        assert_eq!(recover_interrupted_run_jobs(&conn).unwrap(), 1);
        let job = get_run_job(&conn, "stale-job").unwrap().unwrap();
        assert_eq!(job.status, "failed");
        assert_eq!(
            job.message,
            "Interrupted by app restart; no worker is attached (2 partial result(s) available)"
        );
        assert_eq!(job.completed, 2);
        assert_eq!(job.total, 3);
        assert_eq!(job.error.as_deref(), Some("interrupted"));
        assert!(job.finished_at.is_some());
        assert_eq!(
            job.result_run_ids,
            vec!["partial-run-two".to_string(), "partial-run".to_string()]
        );
        let group_status: String = conn
            .query_row(
                "SELECT status FROM run_groups WHERE id = 'stale-group'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(group_status, "failed");
    }

    #[test]
    fn hf_download_job_recovery_marks_active_jobs_interrupted() {
        let conn = open_memory().expect("db should open");
        insert_hf_download_job(
            &conn,
            &HfDownloadJobRecord {
                id: "hf-job".into(),
                repo_id: "org/model-GGUF".into(),
                selected_file: Some("model-Q4_K_M.gguf".into()),
                status: "running".into(),
                message: "Downloading".into(),
                started_at: "2026-07-06T12:00:00Z".into(),
                finished_at: None,
                planned_bytes: Some(100),
                transferred_bytes: 40,
                local_dir: Some("/tmp/model".into()),
                error: None,
                request: serde_json::json!({"repoId": "org/model-GGUF"}),
                model: None,
            },
        )
        .expect("download job should insert");

        assert_eq!(recover_interrupted_hf_download_jobs(&conn).unwrap(), 1);
        let job = get_hf_download_job(&conn, "hf-job").unwrap().unwrap();
        assert_eq!(job.status, "failed");
        assert_eq!(
            job.message,
            "Interrupted by app restart; no download worker is attached (40 of 100 bytes transferred, 40%; retry will reuse local partial files when possible)"
        );
        assert_eq!(job.planned_bytes, Some(100));
        assert_eq!(job.transferred_bytes, 40);
        assert_eq!(job.error.as_deref(), Some("interrupted"));
        assert!(job.finished_at.is_some());
    }

    #[test]
    fn hf_server_job_recovery_marks_active_jobs_interrupted() {
        let conn = open_memory().expect("db should open");
        insert_hf_server_job(
            &conn,
            &HfServerJobRecord {
                id: "hf-server-job".into(),
                repo_id: "org/model-GGUF".into(),
                selected_file: Some("model-Q4_K_M.gguf".into()),
                port: 8080,
                context: 2048,
                status: "running".into(),
                message: "Starting".into(),
                started_at: "2026-07-06T12:00:00Z".into(),
                finished_at: None,
                error: None,
                request: serde_json::json!({
                    "repoId": "org/model-GGUF",
                    "filename": "model-Q4_K_M.gguf",
                    "port": 8080,
                    "context": 2048
                }),
                server_status: None,
            },
        )
        .expect("server job should insert");

        assert_eq!(recover_interrupted_hf_server_jobs(&conn).unwrap(), 1);
        let job = get_hf_server_job(&conn, "hf-server-job").unwrap().unwrap();
        assert_eq!(job.status, "failed");
        assert_eq!(
            job.message,
            "Interrupted by app restart; no server-start worker is attached"
        );
        assert_eq!(job.error.as_deref(), Some("interrupted"));
        assert!(job.finished_at.is_some());
    }

    #[test]
    fn managed_hf_server_marker_matches_only_owned_llama_processes() {
        let marker = ManagedHfServerMarker {
            pid: 42,
            port: 18080,
            model_path: "/tmp/benchforge/models/org__model/model-Q4_K_M.gguf".into(),
            started_at: "2026-07-07T12:00:00Z".into(),
        };
        let command = "/opt/homebrew/bin/llama-server -m /tmp/benchforge/models/org__model/model-Q4_K_M.gguf --host 127.0.0.1 --port 18080 -c 2048";
        assert!(managed_llama_command_matches_marker(command, &marker));
        assert!(!managed_llama_command_matches_marker(
            "/opt/homebrew/bin/llama-server -m /tmp/other/model.gguf --port 18080",
            &marker
        ));
        assert!(!managed_llama_command_matches_marker(
            "/usr/bin/python3 -c 'import time; time.sleep(100)'",
            &marker
        ));
        assert!(!managed_llama_command_matches_marker(
            "/opt/homebrew/bin/llama-server -m /tmp/benchforge/models/org__model/model-Q4_K_M.gguf --port 19090",
            &marker
        ));
    }
}
