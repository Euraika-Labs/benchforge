use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{paths, safety};

const MAX_MESSAGE_BYTES: usize = 500;
const MAX_DETAIL_BYTES: usize = 8_000;
const DEFAULT_LIMIT: usize = 25;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEventRequest {
    pub kind: String,
    #[serde(default)]
    pub level: Option<String>,
    pub message: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRecord {
    pub id: String,
    pub kind: String,
    pub level: String,
    pub message: String,
    pub detail: Option<String>,
    pub created_at: String,
    pub log_path: String,
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|value| value.to_string())
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(|value| value.to_string())
            })
            .unwrap_or_else(|| "panic payload was not text".to_string());
        let detail = panic_info.location().map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        });
        match record_event(DiagnosticEventRequest {
            kind: "backend_panic".into(),
            level: Some("error".into()),
            message,
            detail,
        }) {
            Ok(record) => eprintln!(
                "BenchForge captured a redacted panic diagnostic: {}",
                record.id
            ),
            Err(err) => eprintln!("BenchForge failed to write a panic diagnostic: {err}"),
        }
    }));
}

pub fn record_event(request: DiagnosticEventRequest) -> Result<DiagnosticRecord, String> {
    let log_path = diagnostics_log_path();
    record_event_at(&log_path, request)
}

pub fn list_recent(limit: Option<usize>) -> Result<Vec<DiagnosticRecord>, String> {
    let log_path = diagnostics_log_path();
    list_recent_at(&log_path, limit.unwrap_or(DEFAULT_LIMIT))
}

fn diagnostics_log_path() -> PathBuf {
    paths::diagnostics_dir().join("diagnostics.jsonl")
}

fn record_event_at(
    log_path: &Path,
    request: DiagnosticEventRequest,
) -> Result<DiagnosticRecord, String> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create diagnostics directory: {err}"))?;
    }

    let record = DiagnosticRecord {
        id: Uuid::new_v4().to_string(),
        kind: clean_label(&request.kind, "event"),
        level: clean_level(request.level.as_deref()),
        message: clean_text(&request.message, MAX_MESSAGE_BYTES),
        detail: request
            .detail
            .as_deref()
            .map(|detail| clean_text(detail, MAX_DETAIL_BYTES))
            .filter(|detail| !detail.trim().is_empty()),
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        log_path: log_path.display().to_string(),
    };

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|err| format!("failed to open diagnostics log: {err}"))?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(&record)
            .map_err(|err| format!("failed to serialize diagnostic: {err}"))?
    )
    .map_err(|err| format!("failed to write diagnostic: {err}"))?;
    Ok(record)
}

fn list_recent_at(log_path: &Path, limit: usize) -> Result<Vec<DiagnosticRecord>, String> {
    if !log_path.exists() {
        return Ok(Vec::new());
    }
    let file =
        File::open(log_path).map_err(|err| format!("failed to open diagnostics log: {err}"))?;
    let mut records = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|err| format!("failed to read diagnostics log: {err}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<DiagnosticRecord>(&line) {
            records.push(record);
        }
    }
    records.reverse();
    records.truncate(limit.max(1));
    Ok(records)
}

fn clean_label(value: &str, fallback: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        .take(48)
        .collect();
    if cleaned.is_empty() {
        fallback.into()
    } else {
        cleaned
    }
}

fn clean_level(value: Option<&str>) -> String {
    match value.map(|level| level.to_ascii_lowercase()).as_deref() {
        Some("debug" | "info" | "warn" | "error") => value.unwrap().to_ascii_lowercase(),
        _ => "error".into(),
    }
}

fn clean_text(value: &str, max_bytes: usize) -> String {
    safety::truncate_bytes(safety::redact_sensitive_text(value), max_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_records_are_redacted_and_recent_first() {
        let log_path =
            std::env::temp_dir().join(format!("benchforge-diagnostics-{}.jsonl", Uuid::new_v4()));

        let first = record_event_at(
            &log_path,
            DiagnosticEventRequest {
                kind: "frontend error!".into(),
                level: Some("warn".into()),
                message: "failed with sk-secretvalue123456".into(),
                detail: Some("HF_TOKEN=hf_secretvalue123456".into()),
            },
        )
        .expect("diagnostic should be written");
        let second = record_event_at(
            &log_path,
            DiagnosticEventRequest {
                kind: "backend_panic".into(),
                level: Some("error".into()),
                message: "panic".into(),
                detail: None,
            },
        )
        .expect("second diagnostic should be written");

        assert_eq!(first.kind, "frontenderror");
        assert!(!first.message.contains("sk-secret"));
        assert!(!first.detail.unwrap_or_default().contains("hf_secret"));

        let records = list_recent_at(&log_path, 10).expect("diagnostics should list");
        assert_eq!(records[0].id, second.id);
        assert_eq!(records[1].id, first.id);
        let _ = fs::remove_file(log_path);
    }
}
