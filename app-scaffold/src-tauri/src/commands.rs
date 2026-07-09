use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    adapters, diagnostics, harness_tools, huggingface, jobs, paths, runner, runtime_tools, safety,
    secrets, store, targeting,
};

const PROMPT_DEFAULT_MAX_TOKENS: u64 = 512;
const WORKSPACE_DEFAULT_MAX_TOKENS: u64 = 4096;
const HF_LOCAL_DEFAULT_PORT: u16 = 8080;
const HF_LOCAL_DEFAULT_CONTEXT: u32 = 2048;
const HF_CONNECTIVITY_MAX_COST_USD: f64 = 0.05;
const HF_QUALITY_MAX_COST_USD: f64 = 1.0;
const LIVE_CLOUD_PACK_ID: &str = "llm-connectivity";
const LIVE_CLOUD_DEFAULT_MAX_COST_USD: f64 = 0.10;

#[derive(Serialize)]
pub struct TargetDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(rename = "adapterId")]
    pub adapter_id: String,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub command: Option<String>,
    pub status: String,
    pub enabled: bool,
    #[serde(rename = "isLocalModel")]
    pub is_local_model: bool,
    #[serde(rename = "isCloudModel")]
    pub is_cloud_model: bool,
    #[serde(rename = "validationStatus")]
    pub validation_status: Option<String>,
    #[serde(rename = "validationDetail")]
    pub validation_detail: Option<String>,
    #[serde(rename = "validationCheckedAt")]
    pub validation_checked_at: Option<String>,
    #[serde(rename = "inputPriceUsdPerMillionTokens")]
    pub input_price_usd_per_million_tokens: Option<f64>,
    #[serde(rename = "outputPriceUsdPerMillionTokens")]
    pub output_price_usd_per_million_tokens: Option<f64>,
    #[serde(rename = "cacheReadPriceUsdPerMillionTokens")]
    pub cache_read_price_usd_per_million_tokens: Option<f64>,
    #[serde(rename = "cacheWritePriceUsdPerMillionTokens")]
    pub cache_write_price_usd_per_million_tokens: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheckDto {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: String,
    pub category: String,
    pub importance: String,
    pub remediation: String,
    pub command: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDiagnosticsRequest {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTargetRequest {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(rename = "adapterId")]
    pub adapter_id: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTargetBenchmarkHandoffRequest {
    pub target: CreateTargetRequest,
    #[serde(default)]
    pub benchmark_pack_id: Option<String>,
    #[serde(default)]
    pub benchmark_target_ids: Vec<String>,
    #[serde(default = "default_repetitions")]
    pub repetitions: u32,
    #[serde(default)]
    pub warmup_runs: u32,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTargetBenchmarkHandoffDto {
    pub target: TargetDto,
    pub validation: Option<TargetValidationDto>,
    pub run_job: Option<jobs::RunJobDto>,
    pub benchmark_error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderApiKeyRequest {
    pub provider: String,
    pub api_key: String,
}

#[derive(Debug, Serialize)]
pub struct ProviderApiKeyStatusDto {
    pub provider: String,
    pub available: bool,
    pub source: String,
    pub detail: String,
    #[serde(rename = "envVar")]
    pub env_var: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TargetValidationDto {
    #[serde(rename = "targetId")]
    pub target_id: String,
    pub status: String,
    pub detail: String,
    #[serde(rename = "checkedAt")]
    pub checked_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEstimateRequest {
    pub target_ids: Vec<String>,
    pub benchmark_pack_id: String,
    #[serde(default)]
    pub task_ids: Vec<String>,
    #[serde(default = "default_repetitions")]
    pub repetitions: u32,
    #[serde(default)]
    pub warmup_runs: u32,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEstimateDto {
    pub target_count: usize,
    pub task_count: usize,
    pub repetitions: u32,
    pub warmup_runs: u32,
    pub concurrency: u32,
    pub measured_runs: usize,
    pub warmup_calls: usize,
    pub total_model_calls: usize,
    pub estimated_prompt_tokens: u64,
    pub estimated_max_completion_tokens: u64,
    pub estimated_max_cost_usd: Option<f64>,
    pub estimated_measured_timeout_seconds: u64,
    pub estimated_warmup_timeout_seconds: u64,
    pub estimated_wall_clock_timeout_seconds: u64,
    pub priced_targets: usize,
    pub unpriced_targets: Vec<String>,
    pub heavy: bool,
    pub notes: Vec<String>,
}

fn default_repetitions() -> u32 {
    1
}

fn default_concurrency() -> u32 {
    1
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRuntimeDto {
    pub id: String,
    pub name: String,
    pub adapter_id: String,
    pub base_url: String,
    pub status: String,
    pub detail: String,
    pub probe_url: Option<String>,
    pub model_source: Option<String>,
    pub detected_at: String,
    pub models: Vec<String>,
    pub recommended_model: Option<String>,
    pub install_command: String,
    pub start_command: String,
    pub model_hint: String,
    pub setup_hint: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudModelSearchRequest {
    pub adapter_id: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_keychain: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub azure_api_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudModelDto {
    pub model: String,
    pub name: String,
    pub provider: String,
    pub input_price_usd_per_million_tokens: Option<f64>,
    pub output_price_usd_per_million_tokens: Option<f64>,
    pub cache_read_price_usd_per_million_tokens: Option<f64>,
    pub cache_write_price_usd_per_million_tokens: Option<f64>,
    pub context_length: Option<u64>,
    pub source: String,
    pub source_url: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveCloudSkippedProvider {
    provider: String,
    reason: String,
    detail: String,
}

#[derive(Debug, Clone)]
struct LiveCloudProviderSpec {
    adapter_id: &'static str,
    label: &'static str,
    model_env: &'static str,
    base_url_env: Option<&'static str>,
    default_model: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct LiveCloudTargetPlan {
    targets: Vec<store::NewTarget>,
    skipped: Vec<LiveCloudSkippedProvider>,
}

#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub fn list_targets(state: State<'_, store::AppState>) -> Result<Vec<TargetDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    let adapter_map = benchmark_adapter_map();
    store::list_targets(&conn)
        .map_err(|err| err.to_string())
        .map(|targets| {
            targets
                .into_iter()
                .map(|target| target_dto_from_record(target, &adapter_map))
                .collect()
        })
}

#[tauri::command]
pub fn create_target(
    state: State<'_, store::AppState>,
    request: CreateTargetRequest,
) -> Result<TargetDto, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    let target = persist_target_request(&conn, request)?;
    target_dto_for_conn(&conn, &target.id)
}

#[tauri::command]
pub fn duplicate_target(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<TargetDto, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    duplicate_target_for_conn(&conn, &id)
}

#[tauri::command]
pub fn create_target_with_benchmark_handoff(
    state: State<'_, store::AppState>,
    request: CreateTargetBenchmarkHandoffRequest,
) -> Result<CreateTargetBenchmarkHandoffDto, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    create_target_with_benchmark_handoff_for_conn(&conn, request)
}

fn create_target_with_benchmark_handoff_for_conn(
    conn: &rusqlite::Connection,
    request: CreateTargetBenchmarkHandoffRequest,
) -> Result<CreateTargetBenchmarkHandoffDto, String> {
    let target = persist_target_request(conn, request.target)?;
    let validation = validate_target_for_conn(conn, &target.id)?;
    let mut run_job = None;
    let mut benchmark_error = None;

    if validation.status != "error" {
        if let Some(pack_id) = request
            .benchmark_pack_id
            .as_deref()
            .map(str::trim)
            .filter(|pack_id| !pack_id.is_empty())
        {
            match jobs::start_quick_smoke_job(
                conn,
                runner::RunQuickSmokeRequest {
                    target_ids: benchmark_handoff_target_ids(
                        &target.id,
                        &request.benchmark_target_ids,
                    ),
                    benchmark_pack_id: pack_id.to_string(),
                    task_ids: vec![],
                    repetitions: request.repetitions.max(1),
                    docker: false,
                    warmup_runs: request.warmup_runs,
                    concurrency: request.concurrency,
                    max_cost_usd: request.max_cost_usd,
                    run_group_id: None,
                },
            ) {
                Ok(job) => run_job = Some(job),
                Err(err) => benchmark_error = Some(err),
            }
        }
    }

    Ok(CreateTargetBenchmarkHandoffDto {
        target: target_dto_for_conn(conn, &target.id)?,
        validation: Some(validation),
        run_job,
        benchmark_error,
    })
}

fn benchmark_handoff_target_ids(
    created_target_id: &str,
    requested_target_ids: &[String],
) -> Vec<String> {
    let mut target_ids = Vec::new();
    for target_id in requested_target_ids {
        let target_id = target_id.trim();
        if !target_id.is_empty() && !target_ids.iter().any(|existing| existing == target_id) {
            target_ids.push(target_id.to_string());
        }
    }
    if target_ids.is_empty() {
        target_ids.push(created_target_id.to_string());
    } else if !target_ids
        .iter()
        .any(|target_id| target_id == created_target_id)
    {
        target_ids.push(created_target_id.to_string());
    }
    target_ids
}

fn persist_target_request(
    conn: &rusqlite::Connection,
    mut request: CreateTargetRequest,
) -> Result<store::NewTarget, String> {
    validate_create_target_request(&request)?;
    preserve_redacted_api_key_references(conn, &mut request)?;
    enrich_model_target_secret_env(&mut request)?;
    let target = store::NewTarget {
        id: request.id,
        name: request.name,
        kind: request.kind,
        adapter_id: request.adapter_id,
        config: request.config,
    };
    store::upsert_target(conn, &target).map_err(|err| err.to_string())?;
    Ok(target)
}

fn enrich_model_target_secret_env(request: &mut CreateTargetRequest) -> Result<(), String> {
    if !matches!(request.kind.as_str(), "direct_model" | "harnessed_model") {
        return Ok(());
    }
    let Some(config) = request.config.as_object_mut() else {
        return Ok(());
    };
    if config.get("api_key_env").is_some() {
        return Ok(());
    }
    let Some(adapter) = adapters::find_adapter(&request.adapter_id)? else {
        return Ok(());
    };
    let Some(secret_env) = adapter
        .spec
        .validation
        .get("secret_env")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(());
    };
    config.insert(
        "api_key_env".into(),
        serde_json::Value::String(secret_env.to_string()),
    );
    Ok(())
}

fn preserve_redacted_api_key_references(
    conn: &rusqlite::Connection,
    request: &mut CreateTargetRequest,
) -> Result<(), String> {
    let Some(config) = request.config.as_object_mut() else {
        return Ok(());
    };
    let needs_keychain = config
        .get("api_key_keychain")
        .and_then(|value| value.as_str())
        == Some("[REDACTED]");
    let needs_env =
        config.get("api_key_env").and_then(|value| value.as_str()) == Some("[REDACTED]");
    if !needs_keychain && !needs_env {
        return Ok(());
    }
    let existing = store::get_target(conn, &request.id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "invalid_target: redacted API key references can only be preserved for an existing target".to_string())?;
    let existing_config: serde_json::Value =
        serde_json::from_str(&existing.config_json).unwrap_or_else(|_| serde_json::json!({}));
    if needs_keychain {
        let Some(value) = existing_config.get("api_key_keychain").cloned() else {
            return Err(
                "invalid_target: existing target has no API key Keychain reference to preserve"
                    .into(),
            );
        };
        config.insert("api_key_keychain".into(), value);
    }
    if needs_env {
        let Some(value) = existing_config.get("api_key_env").cloned() else {
            return Err(
                "invalid_target: existing target has no API key environment reference to preserve"
                    .into(),
            );
        };
        config.insert("api_key_env".into(), value);
    }
    Ok(())
}

fn target_dto_for_conn(conn: &rusqlite::Connection, id: &str) -> Result<TargetDto, String> {
    let target = store::get_target(conn, id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("target {} not found", id))?;
    let adapter_map = benchmark_adapter_map();
    Ok(target_dto_from_record(target, &adapter_map))
}

fn duplicate_target_for_conn(conn: &rusqlite::Connection, id: &str) -> Result<TargetDto, String> {
    let source = store::get_target(conn, id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("target {} not found", id))?;
    let config =
        serde_json::from_str(&source.config_json).unwrap_or_else(|_| serde_json::json!({}));
    let (copy_id, copy_name) = next_duplicate_target_identity(conn, &source.id, &source.name)?;
    let target = persist_target_request(
        conn,
        CreateTargetRequest {
            id: copy_id,
            name: copy_name,
            kind: source.kind,
            adapter_id: source.adapter_id,
            config,
        },
    )?;
    target_dto_for_conn(conn, &target.id)
}

fn next_duplicate_target_identity(
    conn: &rusqlite::Connection,
    source_id: &str,
    source_name: &str,
) -> Result<(String, String), String> {
    let stem = slugify_id(source_id);
    for index in 1..=1000 {
        let suffix = if index == 1 {
            "copy".to_string()
        } else {
            format!("copy-{}", index)
        };
        let candidate_id = format!("{}-{}", stem, suffix);
        if !is_valid_target_id(&candidate_id) {
            continue;
        }
        let exists = store::get_target(conn, &candidate_id)
            .map_err(|err| err.to_string())?
            .is_some();
        if !exists {
            let candidate_name = if index == 1 {
                format!("{} Copy", source_name)
            } else {
                format!("{} Copy {}", source_name, index)
            };
            return Ok((candidate_id, candidate_name));
        }
    }
    Err(format!(
        "invalid_target: could not create a unique copy id for target {}",
        source_id
    ))
}

#[tauri::command]
pub fn set_target_enabled(
    state: State<'_, store::AppState>,
    id: String,
    enabled: bool,
) -> Result<TargetDto, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    let changed = store::set_target_enabled(&conn, &id, enabled).map_err(|err| err.to_string())?;
    if !changed {
        if id == "mock-agent" && !enabled {
            return Err("target mock-agent cannot be disabled".into());
        }
        return Err(format!("target {} not found", id));
    }
    let target = store::get_target(&conn, &id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("target {} not found", id))?;
    let adapter_map = benchmark_adapter_map();
    Ok(target_dto_from_record(target, &adapter_map))
}

fn target_dto_from_record(
    target: store::TargetRecord,
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> TargetDto {
    let config = target_config_json(&target);
    target_dto_from_parts(
        &target.id,
        &target.name,
        &target.kind,
        &target.adapter_id,
        &config,
        target.enabled,
        target.validation_status,
        target.validation_detail,
        target.validation_checked_at,
        adapter_map,
    )
}

fn target_dto_from_parts(
    id: &str,
    name: &str,
    kind: &str,
    adapter_id: &str,
    config: &serde_json::Value,
    enabled: bool,
    validation_status: Option<String>,
    validation_detail: Option<String>,
    validation_checked_at: Option<String>,
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> TargetDto {
    TargetDto {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        adapter_id: adapter_id.into(),
        model: public_target_config_value(config, &["model", "model_id", "deployment"]),
        endpoint: public_target_config_value(config, &["base_url", "endpoint", "url"]),
        command: public_target_config_value(config, &["command"]),
        status: if enabled {
            "valid".into()
        } else {
            "invalid".into()
        },
        enabled,
        is_local_model: target_parts_are_local_benchmark_model(
            kind,
            adapter_id,
            config,
            adapter_map,
        ),
        is_cloud_model: target_parts_are_cloud_benchmark_model(
            kind,
            adapter_id,
            config,
            adapter_map,
        ),
        validation_status,
        validation_detail,
        validation_checked_at,
        input_price_usd_per_million_tokens: price_per_million(
            config,
            "input_price_usd_per_million_tokens",
        )
        .or_else(|| price_per_million(config, "input_usd_per_million_tokens")),
        output_price_usd_per_million_tokens: price_per_million(
            config,
            "output_price_usd_per_million_tokens",
        )
        .or_else(|| price_per_million(config, "output_usd_per_million_tokens")),
        cache_read_price_usd_per_million_tokens: price_per_million(
            config,
            "cache_read_price_usd_per_million_tokens",
        )
        .or_else(|| price_per_million(config, "cached_input_price_usd_per_million_tokens")),
        cache_write_price_usd_per_million_tokens: price_per_million(
            config,
            "cache_write_price_usd_per_million_tokens",
        )
        .or_else(|| price_per_million(config, "cache_creation_price_usd_per_million_tokens")),
    }
}

fn validate_create_target_request(request: &CreateTargetRequest) -> Result<(), String> {
    if !is_valid_target_id(&request.id) {
        return Err(
            "invalid_target: id must be 1-120 characters using letters, numbers, '.', '_' or '-'"
                .into(),
        );
    }
    if request.name.trim().is_empty() {
        return Err("invalid_target: name is required".into());
    }
    if request.name != request.name.trim() {
        return Err("invalid_target: name must not start or end with whitespace".into());
    }
    if !matches!(
        request.kind.as_str(),
        "mock" | "direct_model" | "harnessed_model" | "cli_agent" | "benchmark_harness"
    ) {
        return Err(format!(
            "invalid_target: unsupported target kind {}",
            request.kind
        ));
    }

    let config = request
        .config
        .as_object()
        .ok_or_else(|| "invalid_target: config must be a JSON object".to_string())?;
    reject_raw_secret_config_fields(&request.config, "$.config")?;
    validate_numeric_target_config(config)?;

    if request.kind == "mock" {
        if request.adapter_id != "mock" {
            return Err("invalid_target: mock targets must use the mock adapter".into());
        }
        return Ok(());
    }

    let adapter = adapters::find_adapter(&request.adapter_id)?
        .ok_or_else(|| format!("invalid_target: adapter {} not found", request.adapter_id))?;
    if !target_kind_matches_adapter(&request.kind, &adapter.spec.kind) {
        return Err(format!(
            "invalid_target: target kind {} is not compatible with adapter kind {}",
            request.kind, adapter.spec.kind
        ));
    }

    if matches!(request.kind.as_str(), "direct_model" | "harnessed_model") {
        validate_model_secret_references(config)?;
        let model = string_config(config, "model")
            .ok_or_else(|| "invalid_target: model is required for model targets".to_string())?;
        if model.trim().is_empty() || model != model.trim() {
            return Err("invalid_target: model must not be blank or padded".into());
        }
        validate_model_adapter_config(config, &adapter.spec)?;
    }
    if request.kind == "benchmark_harness" {
        validate_benchmark_harness_config(config)?;
    }

    Ok(())
}

fn validate_model_secret_references(
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    if let Some(value) = config.get("api_key_keychain") {
        let Some(name) = value.as_str() else {
            return Err("invalid_target: api_key_keychain must be a string".into());
        };
        if name.trim().is_empty() || name != name.trim() {
            return Err("invalid_target: api_key_keychain must not be blank or padded".into());
        }
    }
    if let Some(value) = config.get("api_key_env") {
        let Some(name) = value.as_str() else {
            return Err("invalid_target: api_key_env must be a string".into());
        };
        if name == "[REDACTED]" {
            return Ok(());
        }
        if !is_valid_env_name(name) {
            return Err(
                "invalid_target: api_key_env must be a valid environment variable name".into(),
            );
        }
    }
    Ok(())
}

fn is_valid_target_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 120 {
        return false;
    }
    id.chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn target_kind_matches_adapter(target_kind: &str, adapter_kind: &str) -> bool {
    match target_kind {
        "direct_model" | "harnessed_model" => matches!(
            adapter_kind,
            "openai_compatible"
                | "openai_responses"
                | "mistral_api"
                | "azure_openai"
                | "anthropic_messages"
        ),
        "cli_agent" => adapter_kind == "cli_agent",
        "benchmark_harness" => adapter_kind == "benchmark_harness",
        _ => false,
    }
}

fn validate_model_adapter_config(
    config: &serde_json::Map<String, serde_json::Value>,
    adapter: &adapters::AdapterSpec,
) -> Result<(), String> {
    match adapter.kind.as_str() {
        "openai_compatible" | "openai_responses" | "mistral_api" | "azure_openai" => {
            let base_url = string_config(config, "base_url")
                .or(adapter.default_base_url.as_deref())
                .ok_or_else(|| {
                    "invalid_target: base_url is required for this adapter".to_string()
                })?;
            validate_http_base_url(base_url)?;
            if adapter.kind == "azure_openai" && base_url.contains("YOUR-RESOURCE-NAME") {
                return Err(
                    "invalid_target: replace the Azure OpenAI base URL placeholder first".into(),
                );
            }
        }
        "anthropic_messages" => {
            let base_url = string_config(config, "base_url")
                .or(adapter.default_base_url.as_deref())
                .unwrap_or("https://api.anthropic.com");
            validate_http_base_url(base_url)?;
        }
        _ => {}
    }
    Ok(())
}

fn validate_benchmark_harness_config(
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let Some(harness) = config.get("harness") else {
        return Ok(());
    };
    let Some(harness) = harness.as_object() else {
        return Err("invalid_target: harness must be a JSON object".into());
    };
    if let Some(command) = harness.get("command") {
        validate_harness_command(command)?;
    }
    if let Some(env) = harness.get("env") {
        validate_harness_env(env)?;
    }
    if let Some(env_passthrough) = harness.get("env_passthrough") {
        validate_harness_env_passthrough(env_passthrough)?;
    }
    validate_integer_range(harness, "timeout_seconds", 1, 86_400)?;
    Ok(())
}

fn validate_harness_command(command: &serde_json::Value) -> Result<(), String> {
    if let Some(command) = command.as_str() {
        if command.trim().is_empty() {
            return Err("invalid_target: harness.command must not be blank".into());
        }
        return Ok(());
    }
    if let Some(parts) = command.as_array() {
        if parts.is_empty() {
            return Err("invalid_target: harness.command must not be empty".into());
        }
        if parts
            .iter()
            .all(|part| part.as_str().is_some_and(|value| !value.trim().is_empty()))
        {
            return Ok(());
        }
        return Err(
            "invalid_target: harness.command list entries must be non-empty strings".into(),
        );
    }
    Err("invalid_target: harness.command must be a string or list of strings".into())
}

fn validate_harness_env(env: &serde_json::Value) -> Result<(), String> {
    let Some(env) = env.as_object() else {
        return Err("invalid_target: harness.env must be an object".into());
    };
    for (key, value) in env {
        if !is_valid_env_name(key) {
            return Err(format!(
                "invalid_target: harness.env key {} must be a valid environment variable name",
                key
            ));
        }
        if !value.is_string() {
            return Err(format!(
                "invalid_target: harness.env.{} must be a string",
                key
            ));
        }
    }
    Ok(())
}

fn validate_harness_env_passthrough(value: &serde_json::Value) -> Result<(), String> {
    let names = if let Some(value) = value.as_str() {
        value
            .split(|ch: char| ch == ',' || ch.is_whitespace())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
    } else if let Some(items) = value.as_array() {
        let mut names = Vec::new();
        for item in items {
            let Some(name) = item.as_str().map(str::trim).filter(|name| !name.is_empty()) else {
                return Err(
                    "invalid_target: harness.env_passthrough entries must be non-empty strings"
                        .into(),
                );
            };
            names.push(name);
        }
        names
    } else {
        return Err("invalid_target: harness.env_passthrough must be a string or list".into());
    };
    for name in names {
        if !is_valid_env_name(name) {
            return Err(format!(
                "invalid_target: harness.env_passthrough entry {} must be a valid environment variable name",
                name
            ));
        }
    }
    Ok(())
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn validate_http_base_url(base_url: &str) -> Result<(), String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() || trimmed != base_url || base_url.chars().any(char::is_whitespace) {
        return Err("invalid_target: base_url must be a non-empty URL without whitespace".into());
    }
    let Some(rest) = base_url
        .strip_prefix("http://")
        .or_else(|| base_url.strip_prefix("https://"))
    else {
        return Err("invalid_target: base_url must start with http:// or https://".into());
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_matches('/');
    if host.is_empty() || host.starts_with(':') {
        return Err("invalid_target: base_url must include a host".into());
    }
    if base_url.contains('?') || base_url.contains('#') {
        return Err("invalid_target: base_url must not include query strings or fragments".into());
    }
    let lower = base_url.trim_end_matches('/').to_lowercase();
    if lower.ends_with("/chat/completions")
        || lower.ends_with("/responses")
        || lower.ends_with("/models")
    {
        return Err(
            "invalid_target: base_url should be the provider root, not a request endpoint".into(),
        );
    }
    Ok(())
}

fn validate_numeric_target_config(
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    validate_number_range(config, "temperature", 0.0, 2.0)?;
    validate_number_range(config, "top_p", 0.0, 1.0)?;
    validate_integer_range(config, "max_tokens", 1, 1_000_000)?;
    validate_integer_range(config, "timeout_seconds", 1, 3_600)?;
    validate_integer_range(config, "retry_count", 0, 5)?;
    validate_integer_range(config, "context_length", 128, 2_000_000)?;
    validate_integer_range(config, "context", 128, 2_000_000)?;
    validate_integer_range(config, "port", 1024, 65_535)?;
    validate_optional_integer(config, "seed")?;
    validate_nonnegative_number(config, "input_price_usd_per_million_tokens")?;
    validate_nonnegative_number(config, "output_price_usd_per_million_tokens")?;
    validate_nonnegative_number(config, "cache_read_price_usd_per_million_tokens")?;
    validate_nonnegative_number(config, "cache_write_price_usd_per_million_tokens")?;
    validate_nonnegative_number(config, "cached_input_price_usd_per_million_tokens")?;
    validate_nonnegative_number(config, "cache_creation_price_usd_per_million_tokens")?;
    validate_price_pair(config)?;
    validate_cache_pricing(config)?;
    Ok(())
}

fn validate_price_pair(config: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let has_input = config.contains_key("input_price_usd_per_million_tokens");
    let has_output = config.contains_key("output_price_usd_per_million_tokens");
    if has_input != has_output {
        return Err(
            "invalid_target: input and output pricing must be set together or both omitted".into(),
        );
    }
    Ok(())
}

fn validate_cache_pricing(
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let has_cache_pricing = [
        "cache_read_price_usd_per_million_tokens",
        "cache_write_price_usd_per_million_tokens",
        "cached_input_price_usd_per_million_tokens",
        "cache_creation_price_usd_per_million_tokens",
    ]
    .iter()
    .any(|key| config.contains_key(*key));
    if has_cache_pricing
        && (!config.contains_key("input_price_usd_per_million_tokens")
            || !config.contains_key("output_price_usd_per_million_tokens"))
    {
        return Err("invalid_target: cache pricing requires input and output pricing".into());
    }
    Ok(())
}

fn validate_number_range(
    config: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    min: f64,
    max: f64,
) -> Result<(), String> {
    let Some(value) = config.get(field) else {
        return Ok(());
    };
    let Some(number) = value.as_f64().filter(|number| number.is_finite()) else {
        return Err(format!("invalid_target: {} must be a number", field));
    };
    if number < min || number > max {
        return Err(format!(
            "invalid_target: {} must be between {} and {}",
            field, min, max
        ));
    }
    Ok(())
}

fn validate_nonnegative_number(
    config: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<(), String> {
    let Some(value) = config.get(field) else {
        return Ok(());
    };
    let Some(number) = value.as_f64().filter(|number| number.is_finite()) else {
        return Err(format!("invalid_target: {} must be a number", field));
    };
    if number < 0.0 {
        return Err(format!("invalid_target: {} must be non-negative", field));
    }
    Ok(())
}

fn validate_integer_range(
    config: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    min: i128,
    max: i128,
) -> Result<(), String> {
    let Some(value) = config.get(field) else {
        return Ok(());
    };
    let Some(number) = json_integer(value) else {
        return Err(format!("invalid_target: {} must be an integer", field));
    };
    if number < min || number > max {
        return Err(format!(
            "invalid_target: {} must be between {} and {}",
            field, min, max
        ));
    }
    Ok(())
}

fn validate_optional_integer(
    config: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<(), String> {
    if let Some(value) = config.get(field) {
        if json_integer(value).is_none() {
            return Err(format!("invalid_target: {} must be an integer", field));
        }
    }
    Ok(())
}

fn json_integer(value: &serde_json::Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
}

fn string_config<'a>(
    config: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<&'a str> {
    config.get(field).and_then(|value| value.as_str())
}

fn reject_raw_secret_config_fields(value: &serde_json::Value, path: &str) -> Result<(), String> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let child_path = format!("{}.{}", path, key);
                if is_raw_secret_config_key(key) {
                    return Err(format!(
                        "invalid_target: {} looks like a raw secret; store secrets in Keychain or reference an environment variable",
                        child_path
                    ));
                }
                reject_raw_secret_config_fields(value, &child_path)?;
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                reject_raw_secret_config_fields(item, &format!("{}[{}]", path, index))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_raw_secret_config_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    if matches!(lower.as_str(), "api_key_keychain" | "api_key_env") {
        return false;
    }
    lower == "api_key"
        || lower == "apikey"
        || lower.ends_with("_api_key")
        || lower.ends_with("_apikey")
        || lower == "authorization"
        || lower == "bearer"
        || lower == "token"
        || lower == "access_token"
        || lower == "refresh_token"
        || lower.ends_with("_token")
        || lower == "private_key"
        || lower.ends_with("_private_key")
        || lower == "client_secret"
        || lower.ends_with("_secret")
        || lower.ends_with("_password")
        || lower.contains("secret")
        || lower.contains("password")
}

#[tauri::command]
pub fn delete_target(state: State<'_, store::AppState>, id: String) -> Result<bool, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    store::delete_target(&conn, &id).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn export_target_redacted(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    store::export_target_redacted(&conn, &id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("target {} not found", id))
}

#[tauri::command]
pub fn record_diagnostic_event(
    request: diagnostics::DiagnosticEventRequest,
) -> Result<diagnostics::DiagnosticRecord, String> {
    diagnostics::record_event(request)
}

#[tauri::command]
pub fn list_diagnostics(
    request: Option<ListDiagnosticsRequest>,
) -> Result<Vec<diagnostics::DiagnosticRecord>, String> {
    diagnostics::list_recent(request.and_then(|request| request.limit))
}

#[tauri::command]
pub fn validate_target(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<TargetValidationDto, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    validate_target_for_conn(&conn, &id)
}

fn validate_target_for_conn(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<TargetValidationDto, String> {
    let target = store::get_target(&conn, &id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("target {} not found", id))?;
    let validation = validate_target_record(&target)?;
    store::set_target_validation(
        conn,
        &target.id,
        &validation.status,
        &validation.detail,
        &validation.checked_at,
    )
    .map_err(|err| err.to_string())?;
    Ok(validation)
}

#[tauri::command]
pub fn estimate_run_plan(
    state: State<'_, store::AppState>,
    request: RunEstimateRequest,
) -> Result<RunEstimateDto, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    estimate_run_plan_for_conn(&conn, &request)
}

fn estimate_run_plan_for_conn(
    conn: &rusqlite::Connection,
    request: &RunEstimateRequest,
) -> Result<RunEstimateDto, String> {
    let pack = runner::load_pack(&request.benchmark_pack_id)?;
    let tasks = runner::select_tasks_for_run(runner::load_tasks(&pack)?, &request.task_ids)?;
    let target_ids = if request.target_ids.is_empty() {
        vec!["mock-agent".to_string()]
    } else {
        request.target_ids.clone()
    };
    let available_targets = store::list_targets(&conn).map_err(|err| err.to_string())?;
    runner::validate_target_compatibility(&pack, &tasks, &available_targets, &target_ids)?;
    runner::validate_target_runtime_preflight_for_tasks(&available_targets, &target_ids, &tasks)?;
    let targets = target_ids
        .iter()
        .map(|id| {
            available_targets
                .iter()
                .find(|target| target.id == *id)
                .cloned()
                .ok_or_else(|| format!("target {} not found", id))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let repetitions = request.repetitions.max(1);
    let warmup_runs = request.warmup_runs.min(20);
    let concurrency = runner::normalized_concurrency(request.concurrency);
    let target_count = targets.len();
    let task_count = tasks.len();
    let measured_runs = target_count * task_count * repetitions as usize;
    let mut warmup_calls = 0_usize;
    let prompt_tokens_per_repetition: u64 =
        tasks.iter().map(|task| estimate_tokens(&task.prompt)).sum();
    let task_timeout_seconds_per_repetition: u64 =
        tasks.iter().map(|task| task.timeout_seconds).sum();
    let warmup_prompt_tokens = 8_u64;
    let mut estimated_prompt_tokens = 0_u64;
    let mut estimated_max_completion_tokens = 0_u64;
    let estimated_measured_timeout_seconds = task_timeout_seconds_per_repetition
        .saturating_mul(repetitions as u64)
        .saturating_mul(target_count as u64);
    let mut estimated_warmup_timeout_seconds = 0_u64;
    let mut total_cost = 0_f64;
    let mut has_cost = false;
    let mut priced_targets = 0_usize;
    let mut unpriced_targets = Vec::new();
    let mut assumed_zero_cost_targets = 0_usize;

    for target in &targets {
        let config: serde_json::Value =
            serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}));
        let effective_warmup_runs = if target_supports_warmup(target) {
            warmup_runs
        } else {
            0
        };
        warmup_calls = warmup_calls.saturating_add(effective_warmup_runs as usize);
        let target_prompt_tokens = prompt_tokens_per_repetition
            .saturating_mul(repetitions as u64)
            .saturating_add(warmup_prompt_tokens.saturating_mul(effective_warmup_runs as u64));
        let target_calls = (task_count as u64)
            .saturating_mul(repetitions as u64)
            .saturating_add(effective_warmup_runs as u64);
        let max_tokens = configured_max_tokens_for_tasks(&config, &tasks);
        if effective_warmup_runs > 0 {
            estimated_warmup_timeout_seconds = estimated_warmup_timeout_seconds.saturating_add(
                configured_timeout_seconds(&config).saturating_mul(effective_warmup_runs as u64),
            );
        }
        let target_completion_tokens = target_calls.saturating_mul(max_tokens);
        estimated_prompt_tokens = estimated_prompt_tokens.saturating_add(target_prompt_tokens);
        estimated_max_completion_tokens =
            estimated_max_completion_tokens.saturating_add(target_completion_tokens);

        match (
            price_per_million(&config, "input_price_usd_per_million_tokens")
                .or_else(|| price_per_million(&config, "input_usd_per_million_tokens")),
            price_per_million(&config, "output_price_usd_per_million_tokens")
                .or_else(|| price_per_million(&config, "output_usd_per_million_tokens")),
        ) {
            (Some(input_price), Some(output_price)) => {
                priced_targets += 1;
                has_cost = true;
                let prompt_price = conservative_prompt_price_per_million(&config, input_price);
                total_cost += ((target_prompt_tokens as f64 * prompt_price)
                    + (target_completion_tokens as f64 * output_price))
                    / 1_000_000.0;
            }
            _ if targeting::target_is_known_zero_cost_when_unpriced(
                &target.kind,
                &target.adapter_id,
                &config,
            ) =>
            {
                priced_targets += 1;
                has_cost = true;
                assumed_zero_cost_targets += 1;
            }
            _ => unpriced_targets.push(target.id.clone()),
        }
    }

    let heavy = pack.tags.iter().any(|tag| tag == "heavy")
        || pack
            .estimated_runtime
            .as_deref()
            .unwrap_or("")
            .contains("hours");
    let estimated_wall_clock_timeout_seconds = estimated_warmup_timeout_seconds.saturating_add(
        div_ceil_u64(estimated_measured_timeout_seconds, concurrency as u64),
    );
    let mut notes = vec![
        "Cost is an upper-bound estimate using rough prompt token counts and each target's configured max_tokens.".into(),
        "Duration is a conservative timeout envelope; real model latency should usually be lower.".into(),
        "Warmup calls are included in the cost estimate but excluded from measured results.".into(),
    ];
    if repetitions < RECOMMENDED_TASK_REPETITIONS as u32
        && tasks.iter().any(|task| task.task_type == "prompt")
    {
        notes.push(format!(
            "Confidence warning: {} repetition(s) gives only {} measured pass/fail sample(s) per task and target; use at least {} repetitions for model-selection confidence. One repetition is fine for connectivity smoke, but weak for choosing between local/cloud models.",
            repetitions,
            repetitions,
            RECOMMENDED_TASK_REPETITIONS
        ));
    }
    if !unpriced_targets.is_empty() {
        notes.push(format!(
            "Missing pricing for {} selected target(s): {}.",
            unpriced_targets.len(),
            unpriced_targets.join(", ")
        ));
    }
    if assumed_zero_cost_targets > 0 {
        notes.push(format!(
            "Assumed $0 cost for {} local/mock target(s) without pricing.",
            assumed_zero_cost_targets
        ));
    }
    if heavy {
        notes.push("Selected pack is marked heavy.".into());
    }

    Ok(RunEstimateDto {
        target_count,
        task_count,
        repetitions,
        warmup_runs,
        concurrency,
        measured_runs,
        warmup_calls,
        total_model_calls: measured_runs + warmup_calls,
        estimated_prompt_tokens,
        estimated_max_completion_tokens,
        estimated_max_cost_usd: (has_cost && unpriced_targets.is_empty()).then_some(total_cost),
        estimated_measured_timeout_seconds,
        estimated_warmup_timeout_seconds,
        estimated_wall_clock_timeout_seconds,
        priced_targets,
        unpriced_targets,
        heavy,
        notes,
    })
}

fn enforce_run_cost_limit(
    conn: &rusqlite::Connection,
    request: &runner::RunQuickSmokeRequest,
) -> Result<(), String> {
    let Some(max_cost_usd) = request.max_cost_usd else {
        return Ok(());
    };
    if !max_cost_usd.is_finite() || max_cost_usd < 0.0 {
        return Err("max_cost_invalid: maxCostUsd must be a non-negative number".into());
    }
    let estimate = estimate_run_plan_for_conn(
        conn,
        &RunEstimateRequest {
            target_ids: request.target_ids.clone(),
            benchmark_pack_id: request.benchmark_pack_id.clone(),
            task_ids: request.task_ids.clone(),
            repetitions: request.repetitions,
            warmup_runs: request.warmup_runs,
            concurrency: request.concurrency,
        },
    )?;
    if !estimate.unpriced_targets.is_empty() {
        return Err(format!(
            "max_cost_unpriced: maxCostUsd was set to ${:.6}, but {} selected target(s) have no pricing: {}. Add target pricing or clear the max cost limit.",
            max_cost_usd,
            estimate.unpriced_targets.len(),
            estimate.unpriced_targets.join(", ")
        ));
    }
    let estimated_cost = estimate.estimated_max_cost_usd.unwrap_or(0.0);
    if estimated_cost > max_cost_usd {
        return Err(format!(
            "max_cost_exceeded: estimated upper-bound cost ${:.6} exceeds maxCostUsd ${:.6}. Reduce targets, repetitions, warmups, max tokens, or raise the cap.",
            estimated_cost, max_cost_usd
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn save_provider_api_key(
    request: SaveProviderApiKeyRequest,
) -> Result<ProviderApiKeyStatusDto, String> {
    secrets::save_cloud_api_key(&request.provider, &request.api_key)?;
    Ok(provider_api_key_status_for(&request.provider))
}

#[tauri::command]
pub fn provider_api_key_status(provider: String) -> ProviderApiKeyStatusDto {
    provider_api_key_status_for(&provider)
}

fn provider_api_key_available(provider: &str) -> bool {
    provider_api_key_status_for(provider).available
}

fn provider_api_key_status_for(provider: &str) -> ProviderApiKeyStatusDto {
    let env_var = provider_secret_env(provider);
    provider_api_key_status_for_with(
        provider,
        env_var,
        &|candidate| secrets::cloud_api_key_available(candidate),
        &|name| std::env::var(name).ok(),
    )
}

fn provider_api_key_status_for_with(
    provider: &str,
    env_var: Option<String>,
    keychain_available: &dyn Fn(&str) -> bool,
    env_value: &dyn Fn(&str) -> Option<String>,
) -> ProviderApiKeyStatusDto {
    if keychain_available(provider) {
        return ProviderApiKeyStatusDto {
            provider: provider.to_string(),
            available: true,
            source: "keychain".into(),
            detail: "API key is stored in macOS Keychain".into(),
            env_var,
        };
    }
    if let Some(name) = env_var.as_deref() {
        if env_value(name)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        {
            return ProviderApiKeyStatusDto {
                provider: provider.to_string(),
                available: true,
                source: "environment".into(),
                detail: format!("{} is set in the app environment", name),
                env_var,
            };
        }
    }
    let detail = env_var
        .as_deref()
        .map(|name| {
            format!(
                "No key found in Keychain; set {} before launching BenchForge or paste a key to save it",
                name
            )
        })
        .unwrap_or_else(|| "No key found in Keychain for this provider".into());
    ProviderApiKeyStatusDto {
        provider: provider.to_string(),
        available: false,
        source: "missing".into(),
        detail,
        env_var,
    }
}

fn provider_secret_env(provider: &str) -> Option<String> {
    adapters::find_adapter(provider)
        .ok()
        .flatten()
        .and_then(|adapter| {
            adapter
                .spec
                .validation
                .get("secret_env")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

#[tauri::command]
pub fn list_adapters() -> Result<Vec<adapters::AdapterDto>, String> {
    adapters::load_builtin_adapters()
        .map(|items| items.iter().map(adapters::adapter_to_dto).collect())
}

#[tauri::command]
pub fn detect_local_runtimes() -> Vec<LocalRuntimeDto> {
    local_runtime_candidates()
        .into_iter()
        .map(probe_local_runtime)
        .collect()
}

#[tauri::command]
pub fn run_local_runtime_tool_action(
    request: runtime_tools::LocalRuntimeToolRequest,
) -> Result<runtime_tools::LocalRuntimeToolResultDto, String> {
    runtime_tools::run_local_runtime_tool_action(request)
}

#[tauri::command]
pub fn search_cloud_models(request: CloudModelSearchRequest) -> Result<Vec<CloudModelDto>, String> {
    search_cloud_models_for_request(&request, true)
}

fn search_cloud_models_for_request(
    request: &CloudModelSearchRequest,
    allow_live_catalog: bool,
) -> Result<Vec<CloudModelDto>, String> {
    let Some(adapter) = adapters::find_adapter(&request.adapter_id)? else {
        return Err(format!("adapter {} not found", request.adapter_id));
    };
    let query = request.query.trim().to_lowercase();
    let limit = request.limit.unwrap_or(25).clamp(1, 100);
    let presets = adapter_model_preset_catalog(&adapter.spec, &query);
    let mut models = Vec::new();

    if allow_live_catalog && supports_live_cloud_catalog(&adapter.spec) {
        match live_cloud_model_catalog(&adapter.spec, &query, limit, &presets, request) {
            Ok(mut live) => models.append(&mut live),
            Err(err) if presets.is_empty() => return Err(err),
            Err(_) => {}
        }
    }
    models.extend(presets);

    models.sort_by(|a, b| {
        model_price_sort_key(a)
            .partial_cmp(&model_price_sort_key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.model.cmp(&b.model))
    });
    models.dedup_by(|a, b| a.model == b.model);
    models.truncate(limit);
    Ok(models)
}

#[tauri::command]
pub fn list_benchmark_packs() -> Result<Vec<runner::BenchmarkPackDto>, String> {
    runner::list_benchmark_packs()
}

#[tauri::command]
pub fn list_benchmark_pack_diagnostics() -> Vec<runner::BenchmarkPackDiagnosticDto> {
    runner::list_benchmark_pack_diagnostics()
}

#[tauri::command]
pub fn list_benchmark_pack_tasks(
    pack_id: String,
) -> Result<Vec<runner::BenchmarkPackTaskDto>, String> {
    runner::list_benchmark_pack_tasks(pack_id)
}

#[tauri::command]
pub fn create_benchmark_pack_template(
    request: runner::CreateBenchmarkPackTemplateRequest,
) -> Result<runner::CreatedBenchmarkPackTemplateDto, String> {
    runner::create_benchmark_pack_template(request)
}

#[tauri::command]
pub fn add_benchmark_pack_prompt_task(
    request: runner::AddBenchmarkPackPromptTaskRequest,
) -> Result<runner::AddedBenchmarkPackPromptTaskDto, String> {
    runner::add_benchmark_pack_prompt_task(request)
}

#[tauri::command]
pub fn update_benchmark_pack_prompt_task(
    request: runner::UpdateBenchmarkPackPromptTaskRequest,
) -> Result<runner::UpdatedBenchmarkPackPromptTaskDto, String> {
    runner::update_benchmark_pack_prompt_task(request)
}

#[tauri::command]
pub fn update_benchmark_pack_calibration(
    request: runner::UpdateBenchmarkPackCalibrationRequest,
) -> Result<runner::UpdatedBenchmarkPackCalibrationDto, String> {
    runner::update_benchmark_pack_calibration(request)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestBenchmarkPackCalibrationRequest {
    pub pack_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkPackCalibrationSuggestion {
    pub pack_id: String,
    pub status: String,
    pub sample_size: u64,
    pub baseline_models: Vec<String>,
    pub last_reviewed: Option<String>,
    pub notes: String,
    pub target_count: usize,
    pub task_count: usize,
    pub run_group_count: usize,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub fn suggest_benchmark_pack_calibration(
    state: State<'_, store::AppState>,
    request: SuggestBenchmarkPackCalibrationRequest,
) -> Result<BenchmarkPackCalibrationSuggestion, String> {
    let pack_id = request.pack_id.trim();
    if pack_id.is_empty() {
        return Err("benchmark pack id is required".into());
    }

    let packs = runner::list_benchmark_packs()?;
    let pack = packs
        .iter()
        .find(|pack| pack.id == pack_id)
        .ok_or_else(|| format!("benchmark pack {pack_id} was not found"))?;
    if pack.source != "user" {
        return Err(
            "benchmark pack calibration suggestions are available for private user packs".into(),
        );
    }

    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    let results = store::list_results(&conn).map_err(|err| err.to_string())?;
    Ok(benchmark_pack_calibration_suggestion_from_results(
        pack_id, &results,
    ))
}

#[tauri::command]
pub fn score_prompt_task_preview(
    request: runner::ScorePromptTaskPreviewRequest,
) -> Result<runner::ScorePromptTaskPreviewDto, String> {
    runner::score_prompt_task_preview(request)
}

#[tauri::command]
pub fn delete_benchmark_pack_task(
    request: runner::DeleteBenchmarkPackTaskRequest,
) -> Result<runner::DeletedBenchmarkPackTaskDto, String> {
    runner::delete_benchmark_pack_task(request)
}

#[tauri::command]
pub fn export_benchmark_pack(
    request: runner::ExportBenchmarkPackRequest,
) -> Result<runner::ExportedBenchmarkPackDto, String> {
    runner::export_benchmark_pack(request)
}

#[tauri::command]
pub fn import_benchmark_pack(
    request: runner::ImportBenchmarkPackRequest,
) -> Result<runner::ImportedBenchmarkPackDto, String> {
    runner::import_benchmark_pack(request)
}

#[tauri::command]
pub fn run_quick_smoke(
    state: State<'_, store::AppState>,
    request: runner::RunQuickSmokeRequest,
) -> Result<Vec<runner::RunResultDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    enforce_run_cost_limit(&conn, &request)?;
    runner::run_quick_smoke(&conn, request)
}

#[tauri::command]
pub fn start_run_job(
    state: State<'_, store::AppState>,
    request: runner::RunQuickSmokeRequest,
) -> Result<jobs::RunJobDto, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    enforce_run_cost_limit(&conn, &request)?;
    jobs::start_quick_smoke_job(&conn, request)
}

#[tauri::command]
pub fn list_run_jobs(state: State<'_, store::AppState>) -> Result<Vec<jobs::RunJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    jobs::list_jobs(&conn)
}

#[tauri::command]
pub fn get_run_job(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<jobs::RunJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    jobs::get_job(&conn, &id)
}

#[tauri::command]
pub fn cancel_run_job(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<jobs::RunJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    jobs::cancel_job(&conn, &id)
}

#[tauri::command]
pub fn duplicate_run_job(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<jobs::RunJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    jobs::duplicate_job(&conn, &id)
}

#[tauri::command]
pub fn retry_run_job(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<jobs::RunJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    jobs::retry_job(&conn, &id)
}

#[tauri::command]
pub fn clear_finished_run_jobs(state: State<'_, store::AppState>) -> Result<usize, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    jobs::clear_finished_jobs(&conn)
}

#[tauri::command]
pub fn run_worker_mock(state: State<'_, store::AppState>) -> Result<runner::RunResultDto, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    runner::run_worker_mock(&conn)
}

#[tauri::command]
pub fn list_results(state: State<'_, store::AppState>) -> Result<Vec<store::ResultRecord>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    store::list_results(&conn).map_err(|err| err.to_string())
}

fn benchmark_pack_calibration_suggestion_from_results(
    pack_id: &str,
    results: &[store::ResultRecord],
) -> BenchmarkPackCalibrationSuggestion {
    let adapter_map = benchmark_adapter_map();
    let mut sample_size = 0_u64;
    let mut baseline_models = BTreeSet::new();
    let mut target_ids = BTreeSet::new();
    let mut task_ids = BTreeSet::new();
    let mut run_group_ids = BTreeSet::new();
    let mut latest_timestamp: Option<String> = None;
    let mut deployment_classes = BTreeMap::<&'static str, BTreeSet<String>>::new();
    let mut cloud_targets_missing_cost = BTreeSet::new();
    let mut model_identity_unconfirmed_targets = BTreeSet::new();
    let mut generation_setting_counts = BTreeMap::<String, usize>::new();

    for result in results {
        if result.benchmark_pack_id != pack_id || !result_counts_as_calibration_evidence(result) {
            continue;
        }
        sample_size += 1;
        target_ids.insert(result.target_id.clone());
        task_ids.insert(result.task_id.clone());
        let deployment_class = result_deployment_class(result, &adapter_map);
        deployment_classes
            .entry(deployment_class)
            .or_default()
            .insert(result.target_id.clone());
        if deployment_class == "cloud_model" && result.cost_usd.is_none() {
            cloud_targets_missing_cost.insert(result.target_id.clone());
        }
        if matches!(deployment_class, "local_model" | "cloud_model")
            && !provider_model_source_is_confirmed(result.provider_model_source.as_deref())
        {
            model_identity_unconfirmed_targets.insert(result.target_id.clone());
        }
        *generation_setting_counts
            .entry(result_generation_sampling_fingerprint(result))
            .or_default() += 1;
        if let Some(run_group_id) = result.run_group_id.as_deref().map(str::trim) {
            if !run_group_id.is_empty() {
                run_group_ids.insert(run_group_id.to_string());
            }
        }
        baseline_models.insert(calibration_baseline_model_label(result));
        for timestamp in [result.finished_at.as_deref(), result.started_at.as_deref()]
            .into_iter()
            .flatten()
        {
            let timestamp = timestamp.trim();
            if timestamp.is_empty() {
                continue;
            }
            if latest_timestamp
                .as_deref()
                .map_or(true, |latest| timestamp > latest)
            {
                latest_timestamp = Some(timestamp.to_string());
            }
        }
    }

    let total_baseline_models = baseline_models.len();
    let baseline_models = baseline_models.into_iter().take(50).collect::<Vec<_>>();
    let target_count = target_ids.len();
    let task_count = task_ids.len();
    let run_group_count = run_group_ids.len();
    let mut warnings = Vec::new();
    if sample_size == 0 {
        warnings
            .push("No passed rows or benchmark scoring failures were found for this pack.".into());
    } else {
        if target_count < 2 {
            warnings.push(
                "Only one target has completed evidence; compare at least two targets before marking the pack calibrated."
                    .into(),
            );
        }
        if task_count < 3 {
            warnings.push(
                "Fewer than three tasks have completed evidence; add or run more prompts before treating this as stable calibration."
                    .into(),
            );
        }
        if run_group_count < 2 {
            warnings.push(
                "Evidence comes from fewer than two run groups; repeat the benchmark before relying on it for calibration."
                    .into(),
            );
        }
        let has_local = deployment_classes
            .get("local_model")
            .is_some_and(|targets| !targets.is_empty());
        let has_cloud = deployment_classes
            .get("cloud_model")
            .is_some_and(|targets| !targets.is_empty());
        if !has_local || !has_cloud {
            warnings.push(format!(
                "No complete local/cloud model baseline pair was detected in the evidence ({}); include at least one local and one cloud model before using this pack for local/cloud model selection calibration.",
                calibration_deployment_class_summary(&deployment_classes)
            ));
        }
        if deployment_classes
            .get("unknown")
            .is_some_and(|targets| !targets.is_empty())
        {
            warnings.push(
                "Some evidence rows lack target provenance metadata, so BenchForge cannot classify them as local, cloud, or other benchmark targets."
                    .into(),
            );
        }
        if !cloud_targets_missing_cost.is_empty() {
            warnings.push(format!(
                "Cloud evidence is missing cost metrics for target(s) {}; add pricing and rerun before treating cost-sensitive calibration as complete.",
                format_limited_set(&cloud_targets_missing_cost, 8)
            ));
        }
        if !model_identity_unconfirmed_targets.is_empty() {
            warnings.push(format!(
                "Some model evidence used configured model identity instead of provider/runtime-confirmed served model IDs for target(s) {}; confirm served model identity and rerun before marking calibration complete.",
                format_limited_set(&model_identity_unconfirmed_targets, 8)
            ));
        }
        if generation_setting_counts.len() > 1 {
            warnings.push(format!(
                "Evidence mixes generation policies ({}); rerun or filter to one temperature, top_p, and seed policy before using it as calibration evidence.",
                format_count_map(&generation_setting_counts)
            ));
        }
        if total_baseline_models > baseline_models.len() {
            warnings.push(format!(
                "Baseline model list was limited to {} entries for pack metadata.",
                baseline_models.len()
            ));
        }
    }

    let notes = if sample_size == 0 {
        "No completed benchmark evidence found yet. Run a pilot local/cloud benchmark before recording calibration.".into()
    } else {
        format!(
            "Suggested from {sample_size} benchmark evidence row(s) across {target_count} target(s), {task_count} task(s), and {run_group_count} run group(s). Review before saving."
        )
    };

    BenchmarkPackCalibrationSuggestion {
        pack_id: pack_id.to_string(),
        status: if sample_size == 0 {
            "uncalibrated".into()
        } else {
            "pilot".into()
        },
        sample_size,
        baseline_models,
        last_reviewed: latest_timestamp
            .as_deref()
            .and_then(calibration_date_from_timestamp),
        notes,
        target_count,
        task_count,
        run_group_count,
        warnings,
    }
}

fn provider_model_source_is_confirmed(source: Option<&str>) -> bool {
    matches!(
        source.map(str::trim).filter(|source| !source.is_empty()),
        Some("provider") | Some("runtime_models")
    )
}

fn calibration_deployment_class_summary(
    deployment_classes: &BTreeMap<&'static str, BTreeSet<String>>,
) -> String {
    if deployment_classes.is_empty() {
        return "no classified targets".into();
    }
    deployment_classes
        .iter()
        .map(|(class, targets)| format!("{class}: {}", targets.len()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_limited_set(values: &BTreeSet<String>, limit: usize) -> String {
    let mut listed = values.iter().take(limit).cloned().collect::<Vec<_>>();
    let remaining = values.len().saturating_sub(listed.len());
    if remaining > 0 {
        listed.push(format!("+{remaining} more"));
    }
    listed.join(", ")
}

fn format_count_map(values: &BTreeMap<String, usize>) -> String {
    values
        .iter()
        .map(|(label, count)| format!("{label}: {count}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn result_counts_as_calibration_evidence(result: &store::ResultRecord) -> bool {
    match result.status.as_str() {
        "passed" => true,
        "failed" => !result
            .error_code
            .as_deref()
            .map(result_error_code_is_infrastructure_failure)
            .unwrap_or(false),
        _ => false,
    }
}

fn result_error_code_is_infrastructure_failure(error_code: &str) -> bool {
    matches!(
        error_code.trim(),
        "auth"
            | "context_overflow"
            | "content_filter"
            | "endpoint_unreachable"
            | "missing_key"
            | "model_not_found"
            | "network"
            | "rate_limit"
            | "server_error"
            | "timeout"
            | "unsupported_shape"
            | "malformed_response"
    )
}

fn calibration_baseline_model_label(result: &store::ResultRecord) -> String {
    let provider_model = result
        .provider_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            result
                .reproducibility
                .pointer("/target/config/model")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
    match provider_model {
        Some(model) if model != result.target_id => format!("{} ({model})", result.target_id),
        Some(model) => model,
        None => result.target_id.clone(),
    }
}

fn calibration_date_from_timestamp(value: &str) -> Option<String> {
    let date = value.trim().get(0..10)?;
    let bytes = date.as_bytes();
    if bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        Some(date.to_string())
    } else {
        None
    }
}

#[tauri::command]
pub fn list_artifacts(
    state: State<'_, store::AppState>,
    run_id: Option<String>,
) -> Result<Vec<store::ArtifactRecord>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    store::list_artifacts(&conn, run_id.as_deref()).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn read_artifact(path: String) -> Result<String, String> {
    let path = std::path::PathBuf::from(path);
    if !path.starts_with(crate::paths::runs_dir()) {
        return Err("artifact is outside BenchForge run storage".into());
    }
    fs::read_to_string(path).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn export_results(
    state: State<'_, store::AppState>,
    format: String,
    run_ids: Option<Vec<String>>,
) -> Result<String, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    let scoped_request = run_ids
        .as_ref()
        .map(|ids| ids.iter().filter(|id| !id.trim().is_empty()).count());
    let results = scoped_results(
        store::list_results(&conn).map_err(|err| err.to_string())?,
        run_ids,
    )?;
    let scope_note = scoped_request.map(|_| export_scope_note(results.len()));
    match format.as_str() {
        "jsonl" => Ok(results_jsonl(&results)),
        "csv" => Ok(results_csv(&results)),
        "analysis" => results_analysis_json_with_scope(&results, scope_note.as_deref()),
        "markdown" => {
            let run_groups = scoped_run_groups(
                &results,
                store::list_run_groups(&conn).map_err(|err| err.to_string())?,
            );
            Ok(markdown_report_with_scope(
                &results,
                &run_groups,
                scope_note.as_deref(),
            ))
        }
        other => Err(format!("unsupported export format: {}", other)),
    }
}

#[tauri::command]
pub fn export_report_folder(
    state: State<'_, store::AppState>,
    run_ids: Option<Vec<String>>,
) -> Result<String, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    export_report_folder_for_conn(&conn, run_ids)
}

pub(crate) fn export_report_folder_for_conn(
    conn: &rusqlite::Connection,
    run_ids: Option<Vec<String>>,
) -> Result<String, String> {
    let scoped_request = run_ids
        .as_ref()
        .map(|ids| ids.iter().filter(|id| !id.trim().is_empty()).count());
    let results = scoped_results(
        store::list_results(conn).map_err(|err| err.to_string())?,
        run_ids,
    )?;
    let scope_note = scoped_request.map(|_| export_scope_note(results.len()));
    let artifacts = store::list_artifacts(conn, None).map_err(|err| err.to_string())?;
    let run_groups = scoped_run_groups(
        &results,
        store::list_run_groups(conn).map_err(|err| err.to_string())?,
    );
    export_report_folder_files(&results, &artifacts, &run_groups, scope_note.as_deref())
}

#[derive(Default, Clone)]
struct ExportAggregate {
    group_id: String,
    pack_id: String,
    target_id: String,
    provider_counts: BTreeMap<String, usize>,
    generation_setting_counts: BTreeMap<String, usize>,
    runs: usize,
    passed: usize,
    total_weight: f64,
    weighted_passed: f64,
    scored_weight: f64,
    weighted_score_sum: f64,
    scores: Vec<f64>,
    wall_times: Vec<f64>,
    provider_first_byte_times: Vec<f64>,
    provider_first_token_times: Vec<f64>,
    provider_request_times: Vec<f64>,
    tokens: Vec<f64>,
    reasoning_tokens: Vec<f64>,
    throughputs: Vec<f64>,
    attempts: Vec<f64>,
    retry_delays: Vec<f64>,
    http_status_counts: BTreeMap<u16, usize>,
    provider_model_counts: BTreeMap<String, usize>,
    provider_model_source_counts: BTreeMap<String, usize>,
    finish_reason_counts: BTreeMap<String, usize>,
    pricing_assumption_counts: BTreeMap<String, usize>,
    total_cost_usd: f64,
    costed: usize,
    has_cost: bool,
    latest_started: String,
}

#[derive(Clone)]
struct ExportRunGroupTrend {
    pack_id: String,
    target_id: String,
    current_group_id: String,
    previous_group_id: String,
    current_latest_started: String,
    previous_latest_started: String,
    current_runs: usize,
    previous_runs: usize,
    current_pass_rate: f64,
    previous_pass_rate: f64,
    pass_rate_delta: f64,
    current_average_score: Option<f64>,
    previous_average_score: Option<f64>,
    average_score_delta: Option<f64>,
    current_p95_wall_time_ms: Option<f64>,
    previous_p95_wall_time_ms: Option<f64>,
    p95_wall_time_delta_ms: Option<f64>,
    current_average_cost_usd: Option<f64>,
    previous_average_cost_usd: Option<f64>,
    average_cost_delta_usd: Option<f64>,
    signal_level: String,
    signal: String,
}

#[derive(Default, Clone)]
struct ExportTargetAggregate {
    target_id: String,
    provider_counts: BTreeMap<String, usize>,
    group_ids: BTreeSet<String>,
    pack_ids: BTreeSet<String>,
    pack_evidence_profiles: BTreeMap<String, String>,
    pack_evidence_warnings: BTreeMap<String, BTreeSet<String>>,
    pack_calibration_statuses: BTreeMap<String, BTreeSet<String>>,
    pack_calibration_sample_sizes: BTreeMap<String, BTreeSet<u64>>,
    pack_calibration_baseline_models: BTreeMap<String, BTreeSet<String>>,
    pack_calibration_last_reviewed: BTreeMap<String, BTreeSet<String>>,
    pack_calibration_quality_gates: BTreeMap<String, BTreeSet<String>>,
    pack_calibration_notes: BTreeMap<String, BTreeSet<String>>,
    task_ids: BTreeSet<String>,
    pack_task_slots: BTreeSet<String>,
    runs: usize,
    passed: usize,
    total_weight: f64,
    weighted_passed: f64,
    scored_weight: f64,
    weighted_score_sum: f64,
    scores: Vec<f64>,
    wall_times: Vec<f64>,
    throughputs: Vec<f64>,
    total_cost_usd: f64,
    costed: usize,
    has_cost: bool,
    pricing_assumption_counts: BTreeMap<String, usize>,
    error_code_counts: BTreeMap<String, usize>,
    latest_started: String,
}

#[derive(Default, Clone)]
struct ExportDeploymentScope {
    local_model_target_ids: BTreeSet<String>,
    cloud_model_target_ids: BTreeSet<String>,
    other_target_ids: BTreeSet<String>,
    unknown_target_ids: BTreeSet<String>,
    ambiguous_target_ids: BTreeSet<String>,
}

#[derive(Default, Clone)]
struct ExportTaskAggregate {
    group_id: String,
    pack_id: String,
    task_id: String,
    target_id: String,
    runs: usize,
    passed: usize,
    total_weight: f64,
    weighted_passed: f64,
    scored_weight: f64,
    weighted_score_sum: f64,
    scores: Vec<f64>,
    wall_times: Vec<f64>,
    provider_first_token_times: Vec<f64>,
    tokens: Vec<f64>,
    throughputs: Vec<f64>,
    http_status_counts: BTreeMap<u16, usize>,
    error_code_counts: BTreeMap<String, usize>,
    total_cost_usd: f64,
    has_cost: bool,
    latest_started: String,
}

#[derive(Default, Clone)]
struct ExportTaskMatrixRow {
    group_id: String,
    pack_id: String,
    task_id: String,
    cells: BTreeMap<String, ExportTaskMatrixCell>,
}

#[derive(Default, Clone)]
struct ExportTaskMatrixCell {
    runs: usize,
    passed: usize,
    scores: Vec<f64>,
    wall_times: Vec<f64>,
    error_code_counts: BTreeMap<String, usize>,
}

#[derive(Default, Clone)]
struct ExportErrorCategory {
    code: String,
    count: usize,
    target_ids: BTreeSet<String>,
    benchmark_pack_ids: BTreeSet<String>,
    task_ids: BTreeSet<String>,
    http_status_counts: BTreeMap<u16, usize>,
    retryable: bool,
    recovery_hint: &'static str,
    example_detail: Option<String>,
    latest_started: String,
}

const RECOMMENDED_TASK_REPETITIONS: usize = 3;
const DEFAULT_COMPARISON_MAX_COST_USD: f64 = 1.0;
const EXPORT_REVIEW_WARNING_CODE: &str = "review_artifacts_before_sharing";
const EXPORT_REVIEW_WARNING_MESSAGE: &str = "BenchForge exports can include prompts, responses, raw provider payloads, stdout/stderr, diffs, and worker logs. Secrets are redacted on a best-effort basis; review copied artifacts before sharing.";
const SENSITIVE_EXPORT_ARTIFACT_KINDS: &[&str] = &[
    "prompt",
    "response",
    "response_json",
    "raw_response",
    "stdout",
    "stderr",
    "diff",
    "git_diff",
    "model_system_prompt",
    "model_prompt",
    "model_output",
    "raw_provider_response",
    "target_config",
    "worker_jsonl",
    "harness_raw_output",
    "worker_events",
];

#[derive(Default)]
struct ReproGroupSummary {
    run_count: usize,
    target_ids: BTreeSet<String>,
    pack_ids: BTreeSet<String>,
    task_ids: BTreeSet<String>,
    statuses: BTreeMap<String, usize>,
    started_at: Option<String>,
    finished_at: Option<String>,
}

fn markdown_report(
    results: &[store::ResultRecord],
    run_groups: &[store::RunGroupRecord],
) -> String {
    markdown_report_with_scope(results, run_groups, None)
}

fn markdown_report_with_scope(
    results: &[store::ResultRecord],
    run_groups: &[store::RunGroupRecord],
    scope_note: Option<&str>,
) -> String {
    let comparison_rows = export_comparison_rows(results);
    let trend_rows = export_run_group_trends(&comparison_rows);
    let target_rows = export_target_ranking_rows(results);
    let task_rows = export_task_rows(results);
    let deployment_scope = export_deployment_scope(results);
    let total_runs = results.len();
    let passed = results
        .iter()
        .filter(|result| result.status == "passed")
        .count();
    let total_cost = results
        .iter()
        .filter_map(result_cost_usd_for_coverage)
        .sum::<f64>();
    let has_cost = results.iter().any(result_has_cost_coverage);
    let error_categories = export_error_categories(results);
    let mut out = String::from("# BenchForge Results\n\n");
    out.push_str("## Summary\n\n");
    out.push_str(
        "| Runs | Passed | Pass rate | Groups | Targets | Providers | Packs | Total cost |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    out.push_str(&format!(
        "| {} | {} | {} | {} | {} | {} | {} | {} |\n\n",
        total_runs,
        passed,
        format_percent(if total_runs == 0 {
            None
        } else {
            Some(passed as f64 / total_runs as f64)
        }),
        unique_count(results.iter().map(|result| {
            result
                .run_group_id
                .clone()
                .unwrap_or_else(|| result.id.clone())
        })),
        unique_count(results.iter().map(|result| result.target_id.clone())),
        unique_count(results.iter().map(result_provider_label)),
        unique_count(
            results
                .iter()
                .map(|result| result.benchmark_pack_id.clone())
        ),
        if has_cost {
            format_cost(Some(total_cost))
        } else {
            "-".into()
        }
    ));
    if let Some(note) = scope_note {
        out.push_str(&format!("\nScope note: {}\n", note));
    }

    out.push_str(&export_safety_notice());
    out.push_str(&export_decision_snapshot(
        &comparison_rows,
        &task_rows,
        &target_rows,
        &deployment_scope,
    ));
    out.push_str(&export_run_group_trends_markdown(&trend_rows));
    out.push_str(&export_target_ranking(&target_rows));
    out.push_str(&export_distribution_summary(&target_rows, &task_rows));
    out.push_str(&export_run_configuration(run_groups));
    out.push_str(&export_metric_coverage(results));
    out.push_str(&export_model_identity_warnings(&comparison_rows));
    out.push_str(&export_generation_setting_warnings(&comparison_rows));
    out.push_str(&export_worker_imports(results));
    out.push_str(&export_safety_findings(results));

    out.push_str("## Comparison\n\n");
    out.push_str("| Group | Pack | Target | Provider | Runs | Pass rate | Score avg / σ | Avg wall | P95 wall | Avg TTFB | Avg TTFT | Avg provider total | Avg tokens | Avg reasoning | Out tok/s | Attempts | Avg retry delay | HTTP | Provider model | Model source | Finish | Pricing assumptions | Total cost |\n");
    out.push_str(
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for row in &comparison_rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&short_id(&row.group_id)),
            markdown_cell(&row.pack_id),
            markdown_cell(&row.target_id),
            markdown_cell(&format_text_counts(&row.provider_counts)),
            row.runs,
            format_percent(Some(row.passed as f64 / row.runs.max(1) as f64)),
            format_number_with_spread(avg(&row.scores), std_dev(&row.scores)),
            format_ms(avg(&row.wall_times)),
            format_ms(percentile(&row.wall_times, 0.95)),
            format_ms(avg(&row.provider_first_byte_times)),
            format_ms(avg(&row.provider_first_token_times)),
            format_ms(avg(&row.provider_request_times)),
            format_number(avg(&row.tokens)),
            format_number(avg(&row.reasoning_tokens)),
            format_number(avg(&row.throughputs)),
            format_number(avg(&row.attempts)),
            format_ms(avg(&row.retry_delays)),
            markdown_cell(&format_http_status_counts(&row.http_status_counts)),
            markdown_cell(&format_text_counts(&row.provider_model_counts)),
            markdown_cell(&format_text_counts(&row.provider_model_source_counts)),
            markdown_cell(&format_text_counts(&row.finish_reason_counts)),
            markdown_cell(&format_text_counts(&row.pricing_assumption_counts)),
            if row.has_cost {
                format_cost(Some(row.total_cost_usd))
            } else {
                "-".into()
            }
        ));
    }
    if comparison_rows.is_empty() {
        out.push_str(
            "| - | - | - | - | 0 | - | - | - | - | - | - | - | - | - | - | - | - | - | - | - | - | - | - |\n",
        );
    }

    out.push_str("\n## Task Drilldown\n\n");
    out.push_str("| Group | Pack | Task | Target | Runs | Pass rate | Score avg / σ | Avg wall | P95 wall | Avg TTFT | Avg tokens | Out tok/s | HTTP | Error | Total cost |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for row in &task_rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&short_id(&row.group_id)),
            markdown_cell(&row.pack_id),
            markdown_cell(&row.task_id),
            markdown_cell(&row.target_id),
            row.runs,
            format_percent(Some(row.passed as f64 / row.runs.max(1) as f64)),
            format_number_with_spread(avg(&row.scores), std_dev(&row.scores)),
            format_ms(avg(&row.wall_times)),
            format_ms(percentile(&row.wall_times, 0.95)),
            format_ms(avg(&row.provider_first_token_times)),
            format_number(avg(&row.tokens)),
            format_number(avg(&row.throughputs)),
            markdown_cell(&format_http_status_counts(&row.http_status_counts)),
            markdown_cell(&format_text_counts(&row.error_code_counts)),
            if row.has_cost {
                format_cost(Some(row.total_cost_usd))
            } else {
                "-".into()
            }
        ));
    }
    if task_rows.is_empty() {
        out.push_str("| - | - | - | - | 0 | - | - | - | - | - | - | - | - | - | - |\n");
    }

    out.push_str(&export_task_target_matrix(results));

    out.push_str("\n## Error Categories\n\n");
    out.push_str("Normalized errors group infrastructure and scoring failures so reruns can distinguish setup/provider problems from model-quality evidence.\n\n");
    out.push_str(
        "| Error | Count | Targets | Tasks | HTTP | Retryable | Recovery hint | Example detail |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    if error_categories.is_empty() {
        out.push_str("| - | 0 | - | - | - | - | - | - |\n");
    } else {
        for row in &error_categories {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
                markdown_cell(&row.code),
                row.count,
                markdown_cell(&format_limited_set(&row.target_ids, 6)),
                markdown_cell(&format_limited_set(&row.task_ids, 6)),
                markdown_cell(&format_http_status_counts(&row.http_status_counts)),
                if row.retryable { "yes" } else { "no" },
                markdown_cell(row.recovery_hint),
                markdown_cell(row.example_detail.as_deref().unwrap_or("-")),
            ));
        }
    }

    out.push_str("\n## Runs\n\n");
    let run_headers = [
        "Run",
        "Group",
        "Target",
        "Provider",
        "Pack",
        "Task",
        "Status",
        "Error",
        "Error detail",
        "Safety findings",
        "Files scanned",
        "Import format",
        "Import source",
        "Import files",
        "Import total",
        "Import omitted",
        "Import unsupported",
        "Import truncated",
        "Import truncated bytes",
        "Summary parser",
        "Score",
        "Wall time",
        "Setup time",
        "Target time",
        "Eval time",
        "Model call",
        "Exit",
        "Harness exit",
        "Stdout bytes",
        "Stderr bytes",
        "Files changed",
        "Lines added",
        "Lines deleted",
        "Commands observed",
        "Dangerous commands",
        "TTFB",
        "TTFT",
        "Provider total",
        "Tokens",
        "Reasoning",
        "Attempts",
        "Retry after",
        "Retry delay",
        "HTTP",
        "Tok/s",
        "Peak RSS",
        "Provider model",
        "Model source",
        "Finish",
        "Pricing assumptions",
        "Cost",
    ];
    out.push_str(&format!("| {} |\n", run_headers.join(" | ")));
    out.push_str(&format!(
        "| {} |\n",
        vec!["---"; run_headers.len()].join(" | ")
    ));
    for result in results {
        let run_cells = vec![
            markdown_cell(&short_id(&result.id)),
            markdown_cell(
                &result
                    .run_group_id
                    .as_ref()
                    .map(|id| short_id(id))
                    .unwrap_or_else(|| "-".into()),
            ),
            markdown_cell(&result.target_id),
            markdown_cell(&result_provider_label(result)),
            markdown_cell(&result.benchmark_pack_id),
            markdown_cell(&result.task_id),
            markdown_cell(&result.status),
            markdown_cell(result.error_code.as_deref().unwrap_or("-")),
            markdown_cell(&report_error_detail(result)),
            format_number(result.security_finding_count),
            format_number(result.security_files_scanned),
            markdown_cell(result.import_format.as_deref().unwrap_or("-")),
            markdown_cell(result.import_source.as_deref().unwrap_or("-")),
            format_number(result.import_file_count),
            format_number(result.import_total_file_count),
            format_number(result.import_omitted_file_count),
            format_number(result.import_unsupported_file_count),
            format_number(result.import_truncated),
            format_number(result.import_truncated_bytes),
            markdown_cell(result.summary_source.as_deref().unwrap_or("-")),
            format_number(result.score),
            format_ms(result.wall_time_ms),
            format_ms(result.setup_time_ms),
            format_ms(result.target_time_ms),
            format_ms(result.evaluation_time_ms),
            format_ms(result.model_call_wall_time_ms),
            format_number(result.exit_code),
            format_number(result.harness_exit_code),
            format_number(result.stdout_bytes),
            format_number(result.stderr_bytes),
            format_number(result.files_changed),
            format_number(result.lines_added),
            format_number(result.lines_deleted),
            format_number(result.commands_observed_count),
            format_number(result.dangerous_command_hits),
            format_ms(result.provider_time_to_first_byte_ms),
            format_ms(result.provider_time_to_first_token_ms),
            format_ms(result.provider_request_total_ms),
            format_number(total_tokens_for_result(result)),
            format_number(result.reasoning_tokens),
            format_number(result.provider_attempts),
            format_ms(result.provider_retry_after_ms),
            format_ms(result.provider_retry_delay_ms),
            format_number(result.http_status),
            format_number(result.output_tokens_per_second),
            format_number(result.peak_rss_mb),
            markdown_cell(result.provider_model.as_deref().unwrap_or("-")),
            markdown_cell(result.provider_model_source.as_deref().unwrap_or("-")),
            markdown_cell(result.finish_reason.as_deref().unwrap_or("-")),
            markdown_cell(result.pricing_assumption.as_deref().unwrap_or("-")),
            format_cost(result_cost_usd_for_coverage(result)),
        ];
        out.push_str(&format!("| {} |\n", run_cells.join(" | ")));
    }
    out.push_str(
        "\nReproducibility metadata is included in JSONL export rows, report folders, and artifact records.\n",
    );
    out
}

fn export_safety_notice() -> String {
    format!(
        "## Export Safety Notice\n\n{}\n\nSensitive artifact kinds include: `{}`.\n\n",
        EXPORT_REVIEW_WARNING_MESSAGE,
        SENSITIVE_EXPORT_ARTIFACT_KINDS.join("`, `")
    )
}

fn export_warnings_json() -> Vec<serde_json::Value> {
    vec![serde_json::json!({
        "code": EXPORT_REVIEW_WARNING_CODE,
        "message": EXPORT_REVIEW_WARNING_MESSAGE,
        "sensitive_artifact_kinds": SENSITIVE_EXPORT_ARTIFACT_KINDS,
    })]
}

fn export_report_folder_files(
    results: &[store::ResultRecord],
    artifacts: &[store::ArtifactRecord],
    run_groups: &[store::RunGroupRecord],
    scope_note: Option<&str>,
) -> Result<String, String> {
    let dir = paths::exports_dir().join(format!("benchforge-report-{}", export_timestamp()));
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    fs::write(
        dir.join("README.md"),
        markdown_report_with_scope(results, run_groups, scope_note),
    )
    .map_err(|err| err.to_string())?;
    fs::write(dir.join("results.csv"), results_csv(results)).map_err(|err| err.to_string())?;
    fs::write(dir.join("results.jsonl"), results_jsonl(results)).map_err(|err| err.to_string())?;
    fs::write(
        dir.join("reproducibility.json"),
        serde_json::to_string_pretty(&report_reproducibility_manifest(results, run_groups))
            .map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    fs::write(
        dir.join("analysis.json"),
        results_analysis_json_with_scope(results, scope_note)?,
    )
    .map_err(|err| err.to_string())?;

    let result_ids = results
        .iter()
        .map(|result| result.id.as_str())
        .collect::<BTreeSet<_>>();
    let copied_artifacts = copy_report_artifacts(&dir, artifacts, &result_ids)?;
    let review_summary = artifact_review_summary(&copied_artifacts);
    let manifest = serde_json::json!({
        "generated_at": store::now(),
        "benchforge_version": env!("CARGO_PKG_VERSION"),
        "scope_note": scope_note,
        "export_warnings": export_warnings_json(),
        "result_count": results.len(),
        "artifact_count": copied_artifacts.len(),
        "review_summary": review_summary,
        "files": {
            "markdown": "README.md",
            "csv": "results.csv",
            "jsonl": "results.jsonl",
            "reproducibility": "reproducibility.json",
            "analysis": "analysis.json",
            "artifacts": "artifacts/"
        },
        "artifacts": copied_artifacts,
    });
    fs::write(
        dir.join("artifacts.json"),
        serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    Ok(dir.to_string_lossy().to_string())
}

fn report_reproducibility_manifest(
    results: &[store::ResultRecord],
    run_groups: &[store::RunGroupRecord],
) -> serde_json::Value {
    let mut groups: BTreeMap<String, ReproGroupSummary> = BTreeMap::new();
    let run_group_records = run_groups
        .iter()
        .map(|group| (group.id.clone(), group))
        .collect::<BTreeMap<_, _>>();
    let mut targets: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let mut packs: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let mut tasks: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let mut generation_settings: Vec<serde_json::Value> = Vec::new();
    let mut environments: Vec<serde_json::Value> = Vec::new();
    let mut scoring_commands: Vec<serde_json::Value> = Vec::new();
    let mut workspaces: Vec<serde_json::Value> = Vec::new();
    let mut prompt_hashes: Vec<serde_json::Value> = Vec::new();
    let mut cli_agents: Vec<serde_json::Value> = Vec::new();
    let mut worker_imports: Vec<serde_json::Value> = Vec::new();
    let mut providers: BTreeMap<String, usize> = BTreeMap::new();

    for result in results {
        *providers.entry(result_provider_label(result)).or_insert(0) += 1;
        let group_id = result
            .run_group_id
            .clone()
            .unwrap_or_else(|| result.id.clone());
        let group = groups.entry(group_id.clone()).or_default();
        group.run_count += 1;
        group.target_ids.insert(result.target_id.clone());
        group.pack_ids.insert(result.benchmark_pack_id.clone());
        group.task_ids.insert(result.task_id.clone());
        *group.statuses.entry(result.status.clone()).or_insert(0) += 1;
        update_min_time(&mut group.started_at, result.started_at.as_deref());
        update_max_time(&mut group.finished_at, result.finished_at.as_deref());

        let repro = &result.reproducibility;
        push_unique_json(
            targets.entry(result.target_id.clone()).or_default(),
            repro.get("target").cloned(),
        );
        push_unique_json(
            packs.entry(result.benchmark_pack_id.clone()).or_default(),
            repro.get("benchmark_pack").cloned(),
        );
        push_unique_json(
            tasks.entry(result.task_id.clone()).or_default(),
            repro.get("task").cloned(),
        );
        push_unique_json(&mut generation_settings, repro.get("generation").cloned());
        push_unique_json(&mut prompt_hashes, repro.get("prompts").cloned());
        push_unique_json(&mut cli_agents, repro.get("cli_agent").cloned());
        if let Some(worker_import) = repro.get("worker_import") {
            worker_imports.push(serde_json::json!({
                "run_id": result.id,
                "target_id": result.target_id,
                "benchmark_pack_id": result.benchmark_pack_id,
                "task_id": result.task_id,
                "worker_import": worker_import
            }));
        }
        push_unique_json(
            &mut workspaces,
            repro.get("workspace").cloned().or_else(|| {
                repro
                    .get("workspace_path")
                    .cloned()
                    .map(|path| serde_json::json!({"path": path}))
            }),
        );
        push_unique_json(
            &mut scoring_commands,
            repro.get("scoring_command_metadata").cloned().or_else(|| {
                repro.get("scoring_command").map(|command| {
                    serde_json::json!({
                        "command": command,
                        "resolved_command": serde_json::Value::Null,
                        "version_probe": serde_json::Value::Null,
                        "version_stdout": serde_json::Value::Null,
                        "version_stderr": serde_json::Value::Null,
                        "version_exit_code": serde_json::Value::Null,
                        "version_timed_out": false
                    })
                })
            }),
        );
        push_unique_json(
            &mut environments,
            Some(serde_json::json!({
                "host": repro.get("host").cloned().unwrap_or(serde_json::Value::Null),
                "arch": repro.get("arch").cloned().unwrap_or(serde_json::Value::Null),
                "host_profile": repro.get("host_profile").cloned().unwrap_or(serde_json::Value::Null),
                "sandbox": repro.get("sandbox").cloned().unwrap_or(serde_json::Value::Null),
                "sandbox_level": repro.get("sandbox_level").cloned().unwrap_or(serde_json::Value::Null),
                "permission_mode": repro.get("permission_mode").cloned().unwrap_or(serde_json::Value::Null),
                "network": repro.get("network").cloned().unwrap_or(serde_json::Value::Null),
                "environment": repro.get("environment").cloned().unwrap_or(serde_json::Value::Null),
                "docker": repro.get("docker").cloned().unwrap_or(serde_json::Value::Null)
            })),
        );
    }

    serde_json::json!({
        "generated_at": store::now(),
        "benchforge_version": env!("CARGO_PKG_VERSION"),
        "result_count": results.len(),
        "run_groups": groups
            .into_iter()
            .map(|(id, group)| {
                let queued_record = run_group_records.get(&id).map(|record| {
                    serde_json::json!({
                        "benchmark_pack_id": record.benchmark_pack_id,
                        "target_ids": record.target_ids,
                        "status": record.status,
                        "started_at": record.started_at,
                        "finished_at": record.finished_at,
                        "config": record.config
                    })
                });
                serde_json::json!({
                    "id": id,
                    "run_count": group.run_count,
                    "target_ids": group.target_ids.into_iter().collect::<Vec<_>>(),
                    "benchmark_pack_ids": group.pack_ids.into_iter().collect::<Vec<_>>(),
                    "task_ids": group.task_ids.into_iter().collect::<Vec<_>>(),
                    "statuses": group.statuses,
                    "started_at": group.started_at,
                    "finished_at": group.finished_at,
                    "queued_run_group": queued_record
                })
            })
            .collect::<Vec<_>>(),
        "targets": targets,
        "providers": providers,
        "benchmark_packs": packs,
        "tasks": tasks,
        "generation_settings": generation_settings,
        "prompt_hashes": prompt_hashes,
        "cli_agents": cli_agents,
        "worker_imports": worker_imports,
        "scoring_commands": scoring_commands,
        "workspaces": workspaces,
        "environments": environments
    })
}

fn report_analysis_manifest(results: &[store::ResultRecord]) -> serde_json::Value {
    report_analysis_manifest_with_scope(results, None)
}

fn report_analysis_manifest_with_scope(
    results: &[store::ResultRecord],
    scope_note: Option<&str>,
) -> serde_json::Value {
    let comparison_rows = export_comparison_rows(results);
    let trend_rows = export_run_group_trends(&comparison_rows);
    let target_rows = export_target_ranking_rows(results);
    let task_rows = export_task_rows(results);
    let deployment_scope = export_deployment_scope(results);
    let total_runs = results.len();
    let passed = results
        .iter()
        .filter(|result| result.status == "passed")
        .count();
    let total_cost = results
        .iter()
        .filter_map(result_cost_usd_for_coverage)
        .sum::<f64>();
    let has_cost = results.iter().any(result_has_cost_coverage);

    serde_json::json!({
        "generated_at": store::now(),
        "benchforge_version": env!("CARGO_PKG_VERSION"),
        "scope_note": scope_note,
        "export_warnings": export_warnings_json(),
        "ranking_policy": "weighted_pass_rate desc, pass_rate desc, weighted_average_score desc, average_score desc, score_std_dev asc, p95_wall_time_ms asc, average_cost_usd asc, output_tokens_per_second desc, runs desc, target_id asc",
        "summary": {
            "runs": total_runs,
            "passed": passed,
            "non_passing": total_runs.saturating_sub(passed),
            "pass_rate": ratio(passed, total_runs),
            "groups": unique_count(results.iter().map(|result| {
                result
                    .run_group_id
                    .clone()
                    .unwrap_or_else(|| result.id.clone())
            })),
            "targets": unique_count(results.iter().map(|result| result.target_id.clone())),
            "providers": unique_count(results.iter().map(result_provider_label)),
            "benchmark_packs": unique_count(results.iter().map(|result| result.benchmark_pack_id.clone())),
            "total_cost_usd": has_cost.then_some(total_cost),
            "security_finding_count": sum_metric(results.iter().filter_map(|result| result.security_finding_count)),
            "security_files_scanned": sum_metric(results.iter().filter_map(|result| result.security_files_scanned)),
        },
        "decision": analysis_decision_snapshot(&comparison_rows, &task_rows, &target_rows, &deployment_scope),
        "deployment_scope": deployment_scope_json(&deployment_scope),
        "model_identity_warnings": model_identity_warnings_json(&comparison_rows),
        "generation_setting_warnings": generation_setting_warnings_json(&comparison_rows),
        "target_ranking": target_rows
            .iter()
            .enumerate()
            .map(|(index, row)| target_ranking_row_json(row, index + 1))
            .collect::<Vec<_>>(),
        "comparison": comparison_rows
            .iter()
            .map(comparison_row_json)
            .collect::<Vec<_>>(),
        "run_group_trends": trend_rows
            .iter()
            .map(run_group_trend_json)
            .collect::<Vec<_>>(),
        "task_drilldown": task_rows
            .iter()
            .map(task_row_json)
            .collect::<Vec<_>>(),
        "metric_coverage": metric_coverage_json(results),
        "worker_imports": worker_imports_json(results),
        "safety_findings": safety_findings_json(results),
        "error_categories": export_error_categories(results)
            .iter()
            .map(error_category_json)
            .collect::<Vec<_>>(),
    })
}

fn export_deployment_scope(results: &[store::ResultRecord]) -> ExportDeploymentScope {
    let adapter_map = benchmark_adapter_map();
    let mut target_classes: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
    for result in results {
        target_classes
            .entry(result.target_id.clone())
            .or_default()
            .insert(result_deployment_class(result, &adapter_map));
    }

    let mut scope = ExportDeploymentScope::default();
    for (target_id, classes) in target_classes {
        let is_local = classes.contains("local_model");
        let is_cloud = classes.contains("cloud_model");
        if is_local && is_cloud {
            scope.ambiguous_target_ids.insert(target_id);
        } else if is_local {
            scope.local_model_target_ids.insert(target_id);
        } else if is_cloud {
            scope.cloud_model_target_ids.insert(target_id);
        } else if classes.contains("unknown") {
            scope.unknown_target_ids.insert(target_id);
        } else {
            scope.other_target_ids.insert(target_id);
        }
    }
    scope
}

fn result_deployment_class(
    result: &store::ResultRecord,
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> &'static str {
    let Some((kind, adapter_id, config)) = result_reproducibility_target_parts(result) else {
        return "unknown";
    };
    if target_parts_are_local_benchmark_model(&kind, &adapter_id, &config, adapter_map) {
        return "local_model";
    }
    if target_parts_are_cloud_benchmark_model(&kind, &adapter_id, &config, adapter_map) {
        return "cloud_model";
    }
    "other"
}

fn result_has_cost_coverage(result: &store::ResultRecord) -> bool {
    result_cost_usd_for_coverage(result).is_some()
}

fn result_cost_usd_for_coverage(result: &store::ResultRecord) -> Option<f64> {
    result
        .cost_usd
        .or_else(|| result_is_known_zero_cost_when_unpriced(result).then_some(0.0))
}

fn result_is_known_zero_cost_when_unpriced(result: &store::ResultRecord) -> bool {
    if result.cost_usd.is_some() {
        return false;
    }
    let Some((kind, adapter_id, config)) = result_reproducibility_target_parts(result) else {
        return false;
    };
    targeting::target_is_known_zero_cost_when_unpriced(&kind, &adapter_id, &config)
}

fn deployment_scope_json(scope: &ExportDeploymentScope) -> serde_json::Value {
    serde_json::json!({
        "kind": deployment_scope_kind(scope),
        "local_cloud_pair": deployment_scope_has_local_cloud_pair(scope),
        "target_count": deployment_scope_target_count(scope),
        "local_model_target_count": scope.local_model_target_ids.len(),
        "cloud_model_target_count": scope.cloud_model_target_ids.len(),
        "other_target_count": scope.other_target_ids.len(),
        "unknown_target_count": scope.unknown_target_ids.len(),
        "ambiguous_target_count": scope.ambiguous_target_ids.len(),
        "local_model_target_ids": &scope.local_model_target_ids,
        "cloud_model_target_ids": &scope.cloud_model_target_ids,
        "other_target_ids": &scope.other_target_ids,
        "unknown_target_ids": &scope.unknown_target_ids,
        "ambiguous_target_ids": &scope.ambiguous_target_ids,
        "note": deployment_scope_note(scope),
    })
}

fn deployment_scope_has_local_cloud_pair(scope: &ExportDeploymentScope) -> bool {
    !scope.local_model_target_ids.is_empty() && !scope.cloud_model_target_ids.is_empty()
}

fn deployment_scope_has_non_model(scope: &ExportDeploymentScope) -> bool {
    !scope.other_target_ids.is_empty()
        || !scope.unknown_target_ids.is_empty()
        || !scope.ambiguous_target_ids.is_empty()
}

fn deployment_scope_target_count(scope: &ExportDeploymentScope) -> usize {
    scope.local_model_target_ids.len()
        + scope.cloud_model_target_ids.len()
        + scope.other_target_ids.len()
        + scope.unknown_target_ids.len()
        + scope.ambiguous_target_ids.len()
}

fn deployment_scope_kind(scope: &ExportDeploymentScope) -> &'static str {
    let has_local = !scope.local_model_target_ids.is_empty();
    let has_cloud = !scope.cloud_model_target_ids.is_empty();
    let has_non_model = deployment_scope_has_non_model(scope);
    if deployment_scope_target_count(scope) == 0 {
        return "empty";
    }
    if has_local && has_cloud && has_non_model {
        return "local_cloud_mixed";
    }
    if has_local && has_cloud {
        return "local_cloud";
    }
    if has_local && has_non_model {
        return "local_mixed";
    }
    if has_cloud && has_non_model {
        return "cloud_mixed";
    }
    if has_local {
        return "local_only";
    }
    if has_cloud {
        return "cloud_only";
    }
    if !scope.ambiguous_target_ids.is_empty() {
        return "ambiguous";
    }
    "other_only"
}

fn deployment_scope_note(scope: &ExportDeploymentScope) -> &'static str {
    match deployment_scope_kind(scope) {
        "empty" => "No targets are present in this export scope.",
        "local_cloud" => {
            "This export includes at least one local model target and one cloud model target, so local/cloud conclusions are in scope."
        }
        "local_cloud_mixed" => {
            "This export includes local and cloud model targets plus other or ambiguous targets; isolate the model rows before making deployment claims."
        }
        "local_only" => {
            "This export only includes local model targets; use it for local-runtime comparisons, not local-vs-cloud decisions."
        }
        "cloud_only" => {
            "This export only includes cloud model targets; use it for cloud-provider comparisons, not local-vs-cloud decisions."
        }
        "local_mixed" => {
            "This export includes local model targets but no cloud model targets; add or rerun a cloud target for local-vs-cloud evidence."
        }
        "cloud_mixed" => {
            "This export includes cloud model targets but no local model targets; add or rerun a local target for local-vs-cloud evidence."
        }
        "ambiguous" => {
            "Saved target snapshots conflict on deployment mode; revalidate targets and rerun before making deployment claims."
        }
        _ => {
            "No local or cloud model targets were identified from saved reproducibility metadata; check target snapshots before making model-deployment claims."
        }
    }
}

fn analysis_decision_snapshot(
    comparison_rows: &[ExportAggregate],
    task_rows: &[ExportTaskAggregate],
    target_rows: &[ExportTargetAggregate],
    deployment_scope: &ExportDeploymentScope,
) -> serde_json::Value {
    if comparison_rows.is_empty() || target_rows.is_empty() {
        return serde_json::Value::Null;
    }
    let mut ranked = comparison_rows.to_vec();
    ranked.sort_by(compare_export_decision_rows);
    let best = ranked[0].clone();
    let best_pass_rate = export_pass_rate(&best);
    let reliable = comparison_rows
        .iter()
        .filter(|row| (export_pass_rate(row) - best_pass_rate).abs() < f64::EPSILON)
        .cloned()
        .collect::<Vec<_>>();
    let fastest = reliable
        .iter()
        .filter(|row| {
            export_p95_wall(row)
                .or_else(|| avg(&row.wall_times))
                .is_some()
        })
        .min_by(|a, b| {
            compare_optional_f64_asc(
                export_p95_wall(a).or_else(|| avg(&a.wall_times)),
                export_p95_wall(b).or_else(|| avg(&b.wall_times)),
            )
        });
    let cheapest = reliable
        .iter()
        .filter(|row| export_avg_cost(row).is_some())
        .min_by(|a, b| compare_optional_f64_asc(export_avg_cost(a), export_avg_cost(b)));
    let throughput = reliable
        .iter()
        .filter(|row| avg(&row.throughputs).is_some())
        .max_by(|a, b| compare_optional_f64_asc(avg(&a.throughputs), avg(&b.throughputs)));
    let low_sample_rows = comparison_rows.iter().filter(|row| row.runs < 3).count();
    let low_repetition_task_rows = low_repetition_task_rows(task_rows);
    let close_contenders = close_target_contenders(target_rows);
    let pass_rate_ci_overlap_targets = pass_rate_ci_overlap_target_ids(target_rows);
    let generation_setting_warnings = generation_setting_warnings(comparison_rows);
    let pack_evidence_issues = pack_evidence_issues_for_scope(target_rows);
    let pack_calibration_issues = pack_calibration_issues_for_scope(target_rows);
    let evidence = comparison_evidence_assessment(comparison_rows, task_rows, target_rows);
    let decision_status = evidence_decision_status(&evidence);
    let selection_note = evidence_selection_note(&evidence, target_rows);
    let selected_target_id = evidence_selected_target_id(&evidence, target_rows);
    let next_run = recommended_next_run(&evidence, target_rows);

    serde_json::json!({
        "decision_status": decision_status,
        "selected_target_id": selected_target_id,
        "selection_note": selection_note,
        "recommended_target": target_ranking_row_json(&target_rows[0], 1),
        "close_contenders": close_contenders
            .iter()
            .enumerate()
            .map(|(index, row)| target_ranking_row_json(row, index + 2))
            .collect::<Vec<_>>(),
        "tie_note": close_contenders_note(&close_contenders),
        "coverage_note": target_coverage_parity_note(target_rows),
        "coverage_issues": target_coverage_issues_json(target_rows),
        "deployment_scope": deployment_scope_json(deployment_scope),
        "pack_evidence_issues": pack_evidence_issues_json(&pack_evidence_issues),
        "pack_calibration_issues": pack_calibration_issues_json(&pack_calibration_issues),
        "calibration_note": pack_calibration_note(&pack_calibration_issues),
        "best_overall": comparison_row_json(&best),
        "fastest_reliable": fastest.map(comparison_row_json),
        "cheapest_reliable": cheapest.map(comparison_row_json),
        "throughput_leader": throughput.map(comparison_row_json),
        "weakest_task": task_rows.first().map(task_row_json),
        "low_sample_rows": low_sample_rows,
        "task_target_rows": task_rows.len(),
        "low_repetition_task_rows": low_repetition_task_rows,
        "recommended_task_repetitions": RECOMMENDED_TASK_REPETITIONS,
        "pass_rate_ci_overlap_targets": pass_rate_ci_overlap_targets,
        "generation_setting_warnings": generation_setting_warnings_json(comparison_rows),
        "generation_setting_warning_count": generation_setting_warnings.len(),
        "evidence_grade": evidence.grade,
        "evidence_label": evidence.label,
        "evidence_tone": evidence.tone,
        "evidence_note": &evidence.note,
        "evidence_risks": &evidence.risks,
        "minimum_next_run": &evidence.minimum_next_run,
        "recommended_next_run": next_run.as_ref().map(recommended_next_run_json),
        "task_repetition_note": task_repetition_note(task_rows),
        "score_stability_note": score_stability_note(target_rows),
        "confidence_note": confidence_note(target_rows, comparison_rows, task_rows)
    })
}

fn target_ranking_row_json(row: &ExportTargetAggregate, rank: usize) -> serde_json::Value {
    serde_json::json!({
        "rank": rank,
        "target_id": &row.target_id,
        "providers": &row.provider_counts,
        "scope": {
            "groups": row.group_ids.len(),
            "benchmark_packs": row.pack_ids.len(),
            "tasks": row.task_ids.len(),
            "pack_task_slots": row.pack_task_slots.len(),
            "group_ids": &row.group_ids,
            "benchmark_pack_ids": &row.pack_ids,
            "task_ids": &row.task_ids,
            "pack_task_slot_ids": &row.pack_task_slots,
        },
        "runs": row.runs,
        "passed": row.passed,
        "pass_rate": export_target_pass_rate(row),
        "total_task_weight": row.total_weight,
        "weighted_pass_rate": export_weighted_pass_rate(row.weighted_passed, row.total_weight),
        "weighted_average_score": export_weighted_average_score(row.weighted_score_sum, row.scored_weight),
        "pass_rate_ci_low": pass_rate_interval(row.passed, row.runs).map(|(low, _)| low),
        "pass_rate_ci_high": pass_rate_interval(row.passed, row.runs).map(|(_, high)| high),
        "pass_rate_ci_method": "wilson_95",
        "average_score": avg(&row.scores),
        "median_score": median(&row.scores),
        "min_score": min_value(&row.scores),
        "max_score": max_value(&row.scores),
        "score_std_dev": std_dev(&row.scores),
        "median_wall_time_ms": median(&row.wall_times),
        "min_wall_time_ms": min_value(&row.wall_times),
        "max_wall_time_ms": max_value(&row.wall_times),
        "p95_wall_time_ms": percentile(&row.wall_times, 0.95),
        "average_cost_usd": export_target_avg_cost(row),
        "costed_runs": row.costed,
        "pricing_assumptions": &row.pricing_assumption_counts,
        "output_tokens_per_second": avg(&row.throughputs),
        "errors": &row.error_code_counts,
        "latest_started": &row.latest_started,
    })
}

fn comparison_row_json(row: &ExportAggregate) -> serde_json::Value {
    serde_json::json!({
        "group_id": &row.group_id,
        "benchmark_pack_id": &row.pack_id,
        "target_id": &row.target_id,
        "providers": &row.provider_counts,
        "generation_settings": &row.generation_setting_counts,
        "runs": row.runs,
        "passed": row.passed,
        "pass_rate": export_pass_rate(row),
        "total_task_weight": row.total_weight,
        "weighted_pass_rate": export_weighted_pass_rate(row.weighted_passed, row.total_weight),
        "weighted_average_score": export_weighted_average_score(row.weighted_score_sum, row.scored_weight),
        "average_score": avg(&row.scores),
        "median_score": median(&row.scores),
        "min_score": min_value(&row.scores),
        "max_score": max_value(&row.scores),
        "score_std_dev": std_dev(&row.scores),
        "average_wall_time_ms": avg(&row.wall_times),
        "median_wall_time_ms": median(&row.wall_times),
        "min_wall_time_ms": min_value(&row.wall_times),
        "max_wall_time_ms": max_value(&row.wall_times),
        "p95_wall_time_ms": percentile(&row.wall_times, 0.95),
        "average_provider_time_to_first_byte_ms": avg(&row.provider_first_byte_times),
        "average_provider_time_to_first_token_ms": avg(&row.provider_first_token_times),
        "average_provider_request_total_ms": avg(&row.provider_request_times),
        "average_tokens": avg(&row.tokens),
        "average_reasoning_tokens": avg(&row.reasoning_tokens),
        "output_tokens_per_second": avg(&row.throughputs),
        "average_attempts": avg(&row.attempts),
        "average_provider_retry_delay_ms": avg(&row.retry_delays),
        "http_statuses": &row.http_status_counts,
        "provider_models": &row.provider_model_counts,
        "provider_model_sources": &row.provider_model_source_counts,
        "finish_reasons": &row.finish_reason_counts,
        "pricing_assumptions": &row.pricing_assumption_counts,
        "total_cost_usd": row.has_cost.then_some(row.total_cost_usd),
        "average_cost_usd": export_avg_cost(row),
        "latest_started": &row.latest_started,
    })
}

fn run_group_trend_json(row: &ExportRunGroupTrend) -> serde_json::Value {
    serde_json::json!({
        "benchmark_pack_id": &row.pack_id,
        "target_id": &row.target_id,
        "current_group_id": &row.current_group_id,
        "previous_group_id": &row.previous_group_id,
        "current_latest_started": &row.current_latest_started,
        "previous_latest_started": &row.previous_latest_started,
        "current_runs": row.current_runs,
        "previous_runs": row.previous_runs,
        "current_pass_rate": row.current_pass_rate,
        "previous_pass_rate": row.previous_pass_rate,
        "pass_rate_delta": row.pass_rate_delta,
        "current_average_score": row.current_average_score,
        "previous_average_score": row.previous_average_score,
        "average_score_delta": row.average_score_delta,
        "current_p95_wall_time_ms": row.current_p95_wall_time_ms,
        "previous_p95_wall_time_ms": row.previous_p95_wall_time_ms,
        "p95_wall_time_delta_ms": row.p95_wall_time_delta_ms,
        "current_average_cost_usd": row.current_average_cost_usd,
        "previous_average_cost_usd": row.previous_average_cost_usd,
        "average_cost_delta_usd": row.average_cost_delta_usd,
        "signal_level": &row.signal_level,
        "signal": &row.signal,
    })
}

fn task_row_json(row: &ExportTaskAggregate) -> serde_json::Value {
    serde_json::json!({
        "group_id": &row.group_id,
        "benchmark_pack_id": &row.pack_id,
        "task_id": &row.task_id,
        "target_id": &row.target_id,
        "runs": row.runs,
        "passed": row.passed,
        "pass_rate": ratio(row.passed, row.runs),
        "total_task_weight": row.total_weight,
        "weighted_pass_rate": export_weighted_pass_rate(row.weighted_passed, row.total_weight),
        "weighted_average_score": export_weighted_average_score(row.weighted_score_sum, row.scored_weight),
        "average_score": avg(&row.scores),
        "median_score": median(&row.scores),
        "min_score": min_value(&row.scores),
        "max_score": max_value(&row.scores),
        "score_std_dev": std_dev(&row.scores),
        "average_wall_time_ms": avg(&row.wall_times),
        "median_wall_time_ms": median(&row.wall_times),
        "min_wall_time_ms": min_value(&row.wall_times),
        "max_wall_time_ms": max_value(&row.wall_times),
        "p95_wall_time_ms": percentile(&row.wall_times, 0.95),
        "average_provider_time_to_first_token_ms": avg(&row.provider_first_token_times),
        "average_tokens": avg(&row.tokens),
        "output_tokens_per_second": avg(&row.throughputs),
        "http_statuses": &row.http_status_counts,
        "errors": &row.error_code_counts,
        "total_cost_usd": row.has_cost.then_some(row.total_cost_usd),
        "latest_started": &row.latest_started,
    })
}

fn metric_coverage_json(results: &[store::ResultRecord]) -> Vec<serde_json::Value> {
    let total = results.len();
    [
        metric_coverage_row(results, "Score", |result| result.score.is_some(), "Scored runs should have this; missing means the run failed before scoring completed."),
        metric_coverage_row(results, "pass_fail", |result| result.pass_fail.is_some(), "Required v1 alias derived from run status."),
        metric_coverage_row(results, "score_numeric", |result| result.score_numeric.is_some(), "Required v1 alias for score."),
        metric_coverage_row(results, "Wall time", |result| result.wall_time_ms.is_some(), "Wall-clock timing is expected for persisted runs; missing means the run failed before timing was stored."),
        metric_coverage_row(results, "Setup time", |result| result.setup_time_ms.is_some(), "Prompt and repo/code tasks report app/workspace setup time before target execution."),
        metric_coverage_row(results, "Target time", |result| result.target_time_ms.is_some(), "Prompt and repo/code tasks report time spent invoking the benchmark target before evaluation."),
        metric_coverage_row(results, "Evaluation time", |result| result.evaluation_time_ms.is_some(), "Scoring and repo/code tasks report time spent in the evaluation command after target execution."),
        metric_coverage_row(results, "Model call time", |result| result.model_call_wall_time_ms.is_some(), "Provider-backed repo/code tasks report the model invocation wall time separately from scoring time."),
        metric_coverage_row(results, "Exit code", |result| result.exit_code.is_some(), "Process-backed scoring runs report the normalized scoring command exit code."),
        metric_coverage_row(results, "Harness exit code", |result| result.harness_exit_code.is_some(), "Worker harness command runs report the external harness process exit code when available."),
        metric_coverage_row(results, "Stdout bytes", |result| result.stdout_bytes.is_some(), "Process-backed runs report redacted stdout byte counts for artifact sizing and debugging."),
        metric_coverage_row(results, "Stderr bytes", |result| result.stderr_bytes.is_some(), "Process-backed runs report redacted stderr byte counts for artifact sizing and debugging."),
        metric_coverage_row(results, "Files changed", |result| result.files_changed.is_some(), "Repo/code tasks report how many files changed in the captured git diff."),
        metric_coverage_row(results, "Lines added", |result| result.lines_added.is_some(), "Repo/code tasks report added lines from the captured git diff."),
        metric_coverage_row(results, "Lines deleted", |result| result.lines_deleted.is_some(), "Repo/code tasks report deleted lines from the captured git diff."),
        metric_coverage_row(results, "Commands observed", |result| result.commands_observed_count.is_some(), "Process-backed repo/code and worker harness runs report benchmark commands BenchForge observed or executed."),
        metric_coverage_row(results, "Dangerous command hits", |result| result.dangerous_command_hits.is_some(), "Repo/code tasks count suspicious command patterns detected in redacted stdout and stderr."),
        metric_coverage_row(results, "Provider TTFB", |result| result.provider_time_to_first_byte_ms.is_some(), "Only provider-backed model calls report transport timing; mock and scoring-only tasks may not have it."),
        metric_coverage_row(results, "TTFT", |result| result.provider_time_to_first_token_ms.is_some(), "Time to first token is available for streaming provider calls; non-streaming calls leave it blank."),
        metric_coverage_row(results, "ttft_ms", |result| result.ttft_ms.is_some(), "Required v1 alias for time to first token."),
        metric_coverage_row(results, "Provider total", |result| result.provider_request_total_ms.is_some(), "Provider request timing is recorded when the adapter call exposes transport timing."),
        metric_coverage_row(results, "Prompt tokens", |result| result.prompt_tokens.is_some(), "Requires provider token usage or a local runtime that reports prompt tokens."),
        metric_coverage_row(results, "input_tokens", |result| result.input_tokens.is_some(), "Required v1 alias for prompt/input tokens."),
        metric_coverage_row(results, "Completion tokens", |result| result.completion_tokens.is_some(), "Requires provider token usage or a local runtime that reports output tokens."),
        metric_coverage_row(results, "output_tokens", |result| result.output_tokens.is_some(), "Required v1 alias for completion/output tokens."),
        metric_coverage_row(results, "Reasoning tokens", |result| result.reasoning_tokens.is_some(), "Only reasoning-capable providers/models report this metric."),
        metric_coverage_row(results, "Cached tokens", |result| result.cached_tokens.is_some(), "Providers with prompt cache accounting report cached input tokens when available."),
        metric_coverage_row(results, "Cache read tokens", |result| result.cache_read_tokens.is_some(), "Providers with prompt cache accounting report cache-read input tokens when available."),
        metric_coverage_row(results, "Cache write tokens", |result| result.cache_write_tokens.is_some(), "Providers with prompt cache accounting report cache-write or cache-creation input tokens when available."),
        metric_coverage_row(results, "Total tokens", |result| total_tokens_for_result(result).is_some(), "Uses provider total tokens when available or prompt plus completion tokens when both are present."),
        metric_coverage_row(results, "Output tokens/sec", |result| result.output_tokens_per_second.is_some(), "Requires completion token counts and wall time."),
        metric_coverage_row(results, "decode_tokens_per_sec", |result| result.decode_tokens_per_sec.is_some(), "Required v1 alias for output token throughput."),
        metric_coverage_row(results, "Peak RSS", |result| result.peak_rss_mb.is_some(), "Process-backed runs report peak resident memory only when BenchForge or a worker can observe it."),
        metric_coverage_row(results, "HTTP status", |result| result.http_status.is_some(), "Only HTTP provider calls expose this; local mocks and host scoring usually do not."),
        metric_coverage_row(results, "Retry attempts", |result| result.provider_attempts.is_some(), "Only retry-aware provider calls expose attempt counts."),
        metric_coverage_row(results, "Retry-After", |result| result.provider_retry_after_ms.is_some(), "Only provider responses with Retry-After headers expose this."),
        metric_coverage_row(results, "Retry delay", |result| result.provider_retry_delay_ms.is_some(), "Recorded when BenchForge waits before retrying a provider call."),
        metric_coverage_row(results, "Provider model", |result| non_empty_option(result.provider_model.as_deref()), "Provider-supplied when available; local runtimes may be confirmed from /models before BenchForge falls back to the configured target model."),
        metric_coverage_row(results, "Provider model source", |result| non_empty_option(result.provider_model_source.as_deref()), "Identifies whether provider_model came from the provider response, a local runtime model list, or the configured target model."),
        metric_coverage_row(results, "Finish reason", |result| non_empty_option(result.finish_reason.as_deref()), "Only model APIs that report completion finish reasons expose this."),
        metric_coverage_row(results, "Cost", result_has_cost_coverage, "Requires token usage plus configured pricing, or a known-zero local/mock target."),
        metric_coverage_row(results, "estimated_cost_usd", |result| result.estimated_cost_usd.is_some(), "Required v1 alias for estimated benchmark cost."),
        metric_coverage_row(results, "Pricing assumption", |result| non_empty_option(result.pricing_assumption.as_deref()), "Present when a cost estimate used a documented pricing fallback, such as prompt-cache tokens priced at normal input-token rates."),
        metric_coverage_row(results, "Safety findings", |result| result.security_finding_count.is_some(), "Worker security packs report finding counts as first-class result metrics."),
        metric_coverage_row(results, "Safety files scanned", |result| result.security_files_scanned.is_some(), "Worker security packs report how many files or manifests were inspected."),
        metric_coverage_row(results, "Import format", |result| non_empty_option(result.import_format.as_deref()), "Worker harness imports set this when a run was read from external result files."),
        metric_coverage_row(results, "Import source", |result| non_empty_option(result.import_source.as_deref()), "Identifies whether imported harness output came from a file, directory, or other supported path."),
        metric_coverage_row(results, "Import files", |result| result.import_file_count.is_some(), "Counts how many imported result files contributed to the run result."),
        metric_coverage_row(results, "Import total files", |result| result.import_total_file_count.is_some(), "Counts all supported result files discovered before import limits were applied."),
        metric_coverage_row(results, "Import omitted files", |result| result.import_omitted_file_count.is_some(), "Counts supported result files skipped after worker import limits were reached."),
        metric_coverage_row(results, "Import unsupported files", |result| result.import_unsupported_file_count.is_some(), "Counts unsupported side files ignored during worker directory imports."),
        metric_coverage_row(results, "Import truncated", |result| result.import_truncated.is_some(), "Set by worker imports to show whether imported result evidence was truncated or partially bounded."),
        metric_coverage_row(results, "Import truncated bytes", |result| result.import_truncated_bytes.is_some(), "Counts bytes omitted from imported result evidence when import size limits apply."),
        metric_coverage_row(results, "Summary parser", |result| non_empty_option(result.summary_source.as_deref()), "Identifies the parser that extracted pass/fail summary from imported harness output."),
    ]
    .into_iter()
    .map(|row| {
        serde_json::json!({
            "metric": row.label,
            "present": row.present,
            "missing": total.saturating_sub(row.present),
            "note": row.note,
        })
    })
    .collect()
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then_some(numerator as f64 / denominator as f64)
}

fn sum_metric(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut seen = false;
    let mut total = 0.0;
    for value in values {
        seen = true;
        total += value;
    }
    seen.then_some(total)
}

fn scoped_run_groups(
    results: &[store::ResultRecord],
    run_groups: Vec<store::RunGroupRecord>,
) -> Vec<store::RunGroupRecord> {
    let result_group_ids = results
        .iter()
        .filter_map(|result| result.run_group_id.as_deref())
        .collect::<BTreeSet<_>>();
    run_groups
        .into_iter()
        .filter(|group| result_group_ids.contains(group.id.as_str()))
        .collect()
}

fn push_unique_json(target: &mut Vec<serde_json::Value>, value: Option<serde_json::Value>) {
    let Some(value) = value else {
        return;
    };
    if value.is_null() || target.iter().any(|existing| existing == &value) {
        return;
    }
    target.push(value);
}

fn append_unique_strings(target: &mut Vec<String>, values: impl IntoIterator<Item = String>) {
    for value in values {
        let value = value.trim();
        if value.is_empty() || target.iter().any(|existing| existing == value) {
            continue;
        }
        target.push(value.to_string());
    }
    target.sort();
}

fn update_min_time(slot: &mut Option<String>, value: Option<&str>) {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return;
    };
    if slot
        .as_deref()
        .map(|current| value < current)
        .unwrap_or(true)
    {
        *slot = Some(value.to_string());
    }
}

fn update_max_time(slot: &mut Option<String>, value: Option<&str>) {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return;
    };
    if slot
        .as_deref()
        .map(|current| value > current)
        .unwrap_or(true)
    {
        *slot = Some(value.to_string());
    }
}

fn copy_report_artifacts(
    export_dir: &Path,
    artifacts: &[store::ArtifactRecord],
    result_ids: &BTreeSet<&str>,
) -> Result<Vec<serde_json::Value>, String> {
    let artifacts_dir = export_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).map_err(|err| err.to_string())?;
    let mut rows = Vec::new();
    for artifact in artifacts
        .iter()
        .filter(|artifact| result_ids.contains(artifact.run_id.as_str()))
    {
        let source = PathBuf::from(&artifact.path);
        let run_dir = artifacts_dir.join(safe_export_file_name(&short_id(&artifact.run_id)));
        fs::create_dir_all(&run_dir).map_err(|err| err.to_string())?;
        let filename = artifact_export_filename(artifact);
        let dest = run_dir.join(&filename);
        let mut copy_error = None;
        if source.starts_with(paths::runs_dir()) && source.is_file() {
            if let Err(err) = fs::copy(&source, &dest) {
                copy_error = Some(err.to_string());
            }
        } else {
            copy_error = Some("artifact is outside BenchForge run storage or missing".to_string());
        }
        let sensitive = artifact_kind_is_sensitive(&artifact.kind);
        let copy_status = if copy_error.is_none() {
            "copied"
        } else {
            "not_copied"
        };
        rows.push(serde_json::json!({
            "id": &artifact.id,
            "run_id": &artifact.run_id,
            "kind": &artifact.kind,
            "sensitive": sensitive,
            "copy_status": copy_status,
            "source_path": &artifact.path,
            "export_path": if copy_error.is_none() {
                Some(format!("artifacts/{}/{}", safe_export_file_name(&short_id(&artifact.run_id)), filename))
            } else {
                None
            },
            "mime_type": &artifact.mime_type,
            "size_bytes": artifact.size_bytes,
            "sha256": &artifact.sha256,
            "metadata": &artifact.metadata,
            "copy_error": copy_error,
        }));
    }
    Ok(rows)
}

fn artifact_kind_is_sensitive(kind: &str) -> bool {
    let normalized = kind.trim().to_ascii_lowercase();
    SENSITIVE_EXPORT_ARTIFACT_KINDS
        .iter()
        .any(|candidate| *candidate == normalized)
}

fn artifact_review_summary(artifacts: &[serde_json::Value]) -> serde_json::Value {
    let mut by_kind: BTreeMap<String, ArtifactReviewKindSummary> = BTreeMap::new();
    let mut total_size_bytes = 0_u64;
    let mut copied_count = 0_usize;
    let mut not_copied_count = 0_usize;
    let mut sensitive_count = 0_usize;
    let mut sensitive_kinds = BTreeSet::new();
    let mut copy_errors = Vec::new();

    for artifact in artifacts {
        let kind = artifact
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        let sensitive = artifact
            .get("sensitive")
            .and_then(|value| value.as_bool())
            .unwrap_or_else(|| artifact_kind_is_sensitive(&kind));
        let copy_status = artifact
            .get("copy_status")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let size_bytes = artifact.get("size_bytes").and_then(|value| value.as_u64());
        if let Some(size) = size_bytes {
            total_size_bytes = total_size_bytes.saturating_add(size);
        }
        if copy_status == "copied" {
            copied_count += 1;
        } else {
            not_copied_count += 1;
        }
        if sensitive {
            sensitive_count += 1;
            sensitive_kinds.insert(kind.clone());
        }
        if let Some(error) = artifact
            .get("copy_error")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        {
            copy_errors.push(serde_json::json!({
                "id": artifact.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "run_id": artifact.get("run_id").cloned().unwrap_or(serde_json::Value::Null),
                "kind": kind.clone(),
                "error": error
            }));
        }

        let entry = by_kind.entry(kind).or_default();
        entry.count += 1;
        if copy_status == "copied" {
            entry.copied += 1;
        } else {
            entry.not_copied += 1;
        }
        if sensitive {
            entry.sensitive += 1;
        }
        if let Some(size) = size_bytes {
            entry.size_bytes = entry.size_bytes.saturating_add(size);
        }
    }

    serde_json::json!({
        "artifact_count": artifacts.len(),
        "copied_count": copied_count,
        "not_copied_count": not_copied_count,
        "sensitive_count": sensitive_count,
        "sensitive_kinds_present": sensitive_kinds.into_iter().collect::<Vec<_>>(),
        "total_size_bytes": total_size_bytes,
        "by_kind": by_kind
            .into_iter()
            .map(|(kind, summary)| {
                (kind, serde_json::json!({
                    "count": summary.count,
                    "copied": summary.copied,
                    "not_copied": summary.not_copied,
                    "sensitive": summary.sensitive,
                    "size_bytes": summary.size_bytes
                }))
            })
            .collect::<serde_json::Map<_, _>>(),
        "copy_errors": copy_errors
    })
}

#[derive(Default)]
struct ArtifactReviewKindSummary {
    count: usize,
    copied: usize,
    not_copied: usize,
    sensitive: usize,
    size_bytes: u64,
}

fn result_provider_label(result: &store::ResultRecord) -> String {
    result_adapter_id(result)
        .map(provider_label_from_adapter_id)
        .unwrap_or_else(|| "Not reported".into())
}

fn result_adapter_id(result: &store::ResultRecord) -> Option<&str> {
    result
        .reproducibility
        .pointer("/target/adapter_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn result_task_weight(result: &store::ResultRecord) -> f64 {
    result
        .reproducibility
        .pointer("/task/weight")
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0)
}

fn result_generation_sampling_fingerprint(result: &store::ResultRecord) -> String {
    let generation = result.reproducibility.get("generation");
    let temperature = generation
        .and_then(|value| value.get("temperature"))
        .and_then(canonical_generation_value)
        .unwrap_or_else(|| "not_reported".into());
    let top_p = generation
        .and_then(|value| value.get("top_p"))
        .and_then(canonical_generation_value)
        .unwrap_or_else(|| "not_reported".into());
    let seed = generation
        .and_then(|value| value.get("seed"))
        .and_then(canonical_generation_value)
        .unwrap_or_else(|| "not_set".into());
    let mode = generation_sampling_mode(generation);
    format!("mode {mode}, temp {temperature}, top_p {top_p}, seed {seed}")
}

fn canonical_generation_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(number) => {
            number
                .as_f64()
                .filter(|value| value.is_finite())
                .map(|value| {
                    if value.fract().abs() < 0.000_001 {
                        format!("{value:.0}")
                    } else {
                        let mut formatted = format!("{value:.4}");
                        while formatted.contains('.') && formatted.ends_with('0') {
                            formatted.pop();
                        }
                        if formatted.ends_with('.') {
                            formatted.pop();
                        }
                        formatted
                    }
                })
        }
        serde_json::Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn generation_sampling_mode(generation: Option<&serde_json::Value>) -> &'static str {
    let temperature = generation
        .and_then(|value| value.get("temperature"))
        .and_then(|value| value.as_f64());
    let top_p = generation
        .and_then(|value| value.get("top_p"))
        .and_then(|value| value.as_f64());
    match (temperature, top_p) {
        (Some(temp), Some(top_p)) if temp.abs() <= 0.000_001 && top_p >= 0.999_999 => {
            "deterministic"
        }
        (Some(temp), _) if temp > 0.000_001 => "exploratory",
        (_, Some(top_p)) if top_p < 0.999_999 => "exploratory",
        _ => "unknown",
    }
}

fn provider_label_from_adapter_id(adapter_id: &str) -> String {
    match adapter_id {
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "mistral" => "Mistral",
        "openrouter" => "OpenRouter",
        "azure-openai" => "Azure OpenAI",
        "gemini" => "Google Gemini",
        "ollama-openai" => "Ollama",
        "llama-cpp-openai" => "llama.cpp",
        "lm-studio-openai" => "LM Studio",
        "vllm-openai" => "vLLM",
        "mlx-lm" => "MLX",
        "omlx-experimental" => "oMLX",
        "openai-compatible" => "OpenAI-compatible",
        "mock" => "Mock",
        "codex-cli" => "Codex CLI",
        "claude-code" => "Claude Code",
        "github-copilot-cli" => "GitHub Copilot CLI",
        "github-copilot-cli-byok-ollama" => "GitHub Copilot CLI BYOK Ollama",
        "mistral-vibe-cli" => "Mistral Vibe CLI",
        other => other,
    }
    .to_string()
}

fn results_jsonl(results: &[store::ResultRecord]) -> String {
    results
        .iter()
        .map(|result| serde_json::to_string(result).unwrap_or_else(|_| "{}".into()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn results_analysis_json(results: &[store::ResultRecord]) -> Result<String, String> {
    results_analysis_json_with_scope(results, None)
}

fn results_analysis_json_with_scope(
    results: &[store::ResultRecord],
    scope_note: Option<&str>,
) -> Result<String, String> {
    serde_json::to_string_pretty(&report_analysis_manifest_with_scope(results, scope_note))
        .map_err(|err| err.to_string())
}

fn results_csv(results: &[store::ResultRecord]) -> String {
    let mut out = String::from(concat!(
        "id,run_group_id,target_id,provider,benchmark_pack_id,task_id,status,error_code,error_message,",
        "pass_fail,score,score_numeric,wall_time_ms,setup_time_ms,target_time_ms,evaluation_time_ms,model_call_wall_time_ms,",
        "provider_time_to_first_byte_ms,provider_time_to_first_token_ms,ttft_ms,provider_request_total_ms,",
        "prompt_tokens,input_tokens,completion_tokens,output_tokens,reasoning_tokens,cached_tokens,cache_read_tokens,cache_write_tokens,total_tokens,",
        "provider_attempts,provider_retry_after_ms,provider_retry_delay_ms,http_status,",
        "output_tokens_per_second,decode_tokens_per_sec,peak_rss_mb,exit_code,harness_exit_code,",
        "stdout_bytes,stderr_bytes,files_changed,lines_added,lines_deleted,commands_observed_count,dangerous_command_hits,",
        "security_finding_count,security_files_scanned,import_file_count,import_total_file_count,import_omitted_file_count,",
        "import_unsupported_file_count,import_truncated,import_truncated_bytes,import_format,import_source,summary_source,import_path,",
        "provider_model,provider_model_source,finish_reason,pricing_assumption,cost_usd,estimated_cost_usd,started_at,finished_at\n"
    ));
    for result in results {
        out.push_str(&csv_row(&[
            result.id.clone(),
            result.run_group_id.clone().unwrap_or_default(),
            result.target_id.clone(),
            result_provider_label(result),
            result.benchmark_pack_id.clone(),
            result.task_id.clone(),
            result.status.clone(),
            result.error_code.clone().unwrap_or_default(),
            result.error_message.clone().unwrap_or_default(),
            result
                .pass_fail
                .map(|value| value.to_string())
                .unwrap_or_default(),
            format_option(result.score),
            format_option(result.score_numeric),
            format_option(result.wall_time_ms),
            format_option(result.setup_time_ms),
            format_option(result.target_time_ms),
            format_option(result.evaluation_time_ms),
            format_option(result.model_call_wall_time_ms),
            format_option(result.provider_time_to_first_byte_ms),
            format_option(result.provider_time_to_first_token_ms),
            format_option(result.ttft_ms),
            format_option(result.provider_request_total_ms),
            format_option(result.prompt_tokens),
            format_option(result.input_tokens),
            format_option(result.completion_tokens),
            format_option(result.output_tokens),
            format_option(result.reasoning_tokens),
            format_option(result.cached_tokens),
            format_option(result.cache_read_tokens),
            format_option(result.cache_write_tokens),
            format_option(total_tokens_for_result(result)),
            format_option(result.provider_attempts),
            format_option(result.provider_retry_after_ms),
            format_option(result.provider_retry_delay_ms),
            format_option(result.http_status),
            format_option(result.output_tokens_per_second),
            format_option(result.decode_tokens_per_sec),
            format_option(result.peak_rss_mb),
            format_option(result.exit_code),
            format_option(result.harness_exit_code),
            format_option(result.stdout_bytes),
            format_option(result.stderr_bytes),
            format_option(result.files_changed),
            format_option(result.lines_added),
            format_option(result.lines_deleted),
            format_option(result.commands_observed_count),
            format_option(result.dangerous_command_hits),
            format_option(result.security_finding_count),
            format_option(result.security_files_scanned),
            format_option(result.import_file_count),
            format_option(result.import_total_file_count),
            format_option(result.import_omitted_file_count),
            format_option(result.import_unsupported_file_count),
            format_option(result.import_truncated),
            format_option(result.import_truncated_bytes),
            result.import_format.clone().unwrap_or_default(),
            result.import_source.clone().unwrap_or_default(),
            result.summary_source.clone().unwrap_or_default(),
            result.import_path.clone().unwrap_or_default(),
            result.provider_model.clone().unwrap_or_default(),
            result.provider_model_source.clone().unwrap_or_default(),
            result.finish_reason.clone().unwrap_or_default(),
            result.pricing_assumption.clone().unwrap_or_default(),
            format_option(result.cost_usd),
            format_option(result.estimated_cost_usd),
            result.started_at.clone().unwrap_or_default(),
            result.finished_at.clone().unwrap_or_default(),
        ]));
        out.push('\n');
    }
    out
}

fn export_comparison_rows(results: &[store::ResultRecord]) -> Vec<ExportAggregate> {
    let mut groups: BTreeMap<String, ExportAggregate> = BTreeMap::new();
    for result in results {
        let task_weight = result_task_weight(result);
        let group_id = result
            .run_group_id
            .clone()
            .unwrap_or_else(|| result.id.clone());
        let key = format!(
            "{}|{}|{}",
            group_id, result.benchmark_pack_id, result.target_id
        );
        let row = groups.entry(key).or_insert_with(|| ExportAggregate {
            group_id: group_id.clone(),
            pack_id: result.benchmark_pack_id.clone(),
            target_id: result.target_id.clone(),
            latest_started: result.started_at.clone().unwrap_or_default(),
            ..ExportAggregate::default()
        });
        row.runs += 1;
        row.total_weight += task_weight;
        *row.provider_counts
            .entry(result_provider_label(result))
            .or_insert(0) += 1;
        *row.generation_setting_counts
            .entry(result_generation_sampling_fingerprint(result))
            .or_insert(0) += 1;
        if result.status == "passed" {
            row.passed += 1;
            row.weighted_passed += task_weight;
        }
        if let Some(value) = result.score {
            row.scores.push(value);
            row.scored_weight += task_weight;
            row.weighted_score_sum += value * task_weight;
        }
        if let Some(value) = result.wall_time_ms {
            row.wall_times.push(value);
        }
        if let Some(value) = result.provider_time_to_first_byte_ms {
            row.provider_first_byte_times.push(value);
        }
        if let Some(value) = result.provider_time_to_first_token_ms {
            row.provider_first_token_times.push(value);
        }
        if let Some(value) = result.provider_request_total_ms {
            row.provider_request_times.push(value);
        }
        if let Some(value) = total_tokens_for_result(result) {
            row.tokens.push(value);
        }
        if let Some(value) = result.reasoning_tokens {
            row.reasoning_tokens.push(value);
        }
        if let Some(value) = result.output_tokens_per_second {
            row.throughputs.push(value);
        }
        if let Some(value) = result.provider_attempts {
            row.attempts.push(value);
        }
        if let Some(value) = result.provider_retry_delay_ms {
            row.retry_delays.push(value);
        }
        if let Some(status) = http_status_code(result.http_status) {
            *row.http_status_counts.entry(status).or_insert(0) += 1;
        }
        if let Some(model) = result
            .provider_model
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            *row.provider_model_counts
                .entry(model.to_string())
                .or_insert(0) += 1;
        }
        if let Some(source) = result
            .provider_model_source
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            *row.provider_model_source_counts
                .entry(source.to_string())
                .or_insert(0) += 1;
        }
        if let Some(reason) = result
            .finish_reason
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            *row.finish_reason_counts
                .entry(reason.to_string())
                .or_insert(0) += 1;
        }
        if let Some(assumption) = result
            .pricing_assumption
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            *row.pricing_assumption_counts
                .entry(assumption.to_string())
                .or_insert(0) += 1;
        }
        if let Some(value) = result_cost_usd_for_coverage(result) {
            row.total_cost_usd += value;
            row.costed += 1;
            row.has_cost = true;
        }
        if result.started_at.as_deref().unwrap_or_default() > row.latest_started.as_str() {
            row.latest_started = result.started_at.clone().unwrap_or_default();
        }
    }
    let mut rows = groups.into_values().collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.latest_started
            .cmp(&a.latest_started)
            .then_with(|| (b.passed * a.runs.max(1)).cmp(&(a.passed * b.runs.max(1))))
            .then_with(|| a.target_id.cmp(&b.target_id))
    });
    rows
}

fn export_run_group_trends(comparison_rows: &[ExportAggregate]) -> Vec<ExportRunGroupTrend> {
    let mut by_target_pack = BTreeMap::<(String, String), Vec<ExportAggregate>>::new();
    for row in comparison_rows {
        by_target_pack
            .entry((row.pack_id.clone(), row.target_id.clone()))
            .or_default()
            .push(row.clone());
    }

    let mut trends = Vec::new();
    for ((_pack_id, _target_id), mut rows) in by_target_pack {
        rows.sort_by(|a, b| {
            b.latest_started
                .cmp(&a.latest_started)
                .then_with(|| b.group_id.cmp(&a.group_id))
        });
        let (Some(current), Some(previous)) = (rows.first(), rows.get(1)) else {
            continue;
        };
        let current_pass_rate = export_pass_rate(current);
        let previous_pass_rate = export_pass_rate(previous);
        let current_average_score = avg(&current.scores);
        let previous_average_score = avg(&previous.scores);
        let current_p95_wall_time_ms = export_p95_wall(current);
        let previous_p95_wall_time_ms = export_p95_wall(previous);
        let current_average_cost_usd = export_avg_cost(current);
        let previous_average_cost_usd = export_avg_cost(previous);
        let pass_rate_delta = current_pass_rate - previous_pass_rate;
        let average_score_delta = optional_delta(current_average_score, previous_average_score);
        let p95_wall_time_delta_ms =
            optional_delta(current_p95_wall_time_ms, previous_p95_wall_time_ms);
        let average_cost_delta_usd =
            optional_delta(current_average_cost_usd, previous_average_cost_usd);
        let (signal_level, signal) = run_group_trend_signal(
            pass_rate_delta,
            average_score_delta,
            current_p95_wall_time_ms,
            previous_p95_wall_time_ms,
            current_average_cost_usd,
            previous_average_cost_usd,
        );

        trends.push(ExportRunGroupTrend {
            pack_id: current.pack_id.clone(),
            target_id: current.target_id.clone(),
            current_group_id: current.group_id.clone(),
            previous_group_id: previous.group_id.clone(),
            current_latest_started: current.latest_started.clone(),
            previous_latest_started: previous.latest_started.clone(),
            current_runs: current.runs,
            previous_runs: previous.runs,
            current_pass_rate,
            previous_pass_rate,
            pass_rate_delta,
            current_average_score,
            previous_average_score,
            average_score_delta,
            current_p95_wall_time_ms,
            previous_p95_wall_time_ms,
            p95_wall_time_delta_ms,
            current_average_cost_usd,
            previous_average_cost_usd,
            average_cost_delta_usd,
            signal_level,
            signal,
        });
    }

    trends.sort_by(|a, b| {
        trend_level_rank(&a.signal_level)
            .cmp(&trend_level_rank(&b.signal_level))
            .then_with(|| b.current_latest_started.cmp(&a.current_latest_started))
            .then_with(|| a.pack_id.cmp(&b.pack_id))
            .then_with(|| a.target_id.cmp(&b.target_id))
    });
    trends
}

fn export_target_ranking_rows(results: &[store::ResultRecord]) -> Vec<ExportTargetAggregate> {
    let mut groups: BTreeMap<String, ExportTargetAggregate> = BTreeMap::new();
    for result in results {
        let task_weight = result_task_weight(result);
        let group_id = result
            .run_group_id
            .clone()
            .unwrap_or_else(|| result.id.clone());
        let row = groups
            .entry(result.target_id.clone())
            .or_insert_with(|| ExportTargetAggregate {
                target_id: result.target_id.clone(),
                latest_started: result.started_at.clone().unwrap_or_default(),
                ..ExportTargetAggregate::default()
            });
        row.runs += 1;
        row.total_weight += task_weight;
        row.group_ids.insert(group_id);
        row.pack_ids.insert(result.benchmark_pack_id.clone());
        if let Some((pack_id, evidence_profile, evidence_warnings)) =
            result_pack_evidence_metadata(result)
        {
            row.pack_evidence_profiles
                .insert(pack_id.clone(), evidence_profile);
            for warning in evidence_warnings {
                row.pack_evidence_warnings
                    .entry(pack_id.clone())
                    .or_default()
                    .insert(warning);
            }
        }
        if let Some(calibration) = result_pack_calibration_metadata(result) {
            row.pack_calibration_statuses
                .entry(calibration.pack_id.clone())
                .or_default()
                .insert(calibration.status);
            if let Some(sample_size) = calibration.sample_size {
                row.pack_calibration_sample_sizes
                    .entry(calibration.pack_id.clone())
                    .or_default()
                    .insert(sample_size);
            }
            for model in calibration.baseline_models {
                row.pack_calibration_baseline_models
                    .entry(calibration.pack_id.clone())
                    .or_default()
                    .insert(model);
            }
            if let Some(last_reviewed) = calibration.last_reviewed {
                row.pack_calibration_last_reviewed
                    .entry(calibration.pack_id.clone())
                    .or_default()
                    .insert(last_reviewed);
            }
            for quality_gate in calibration.quality_gates {
                row.pack_calibration_quality_gates
                    .entry(calibration.pack_id.clone())
                    .or_default()
                    .insert(quality_gate);
            }
            if let Some(note) = calibration.notes {
                row.pack_calibration_notes
                    .entry(calibration.pack_id)
                    .or_default()
                    .insert(note);
            }
        }
        row.task_ids.insert(result.task_id.clone());
        row.pack_task_slots.insert(pack_task_slot_id(
            &result.benchmark_pack_id,
            &result.task_id,
        ));
        *row.provider_counts
            .entry(result_provider_label(result))
            .or_insert(0) += 1;
        if result.status == "passed" {
            row.passed += 1;
            row.weighted_passed += task_weight;
        } else {
            let code = result
                .error_code
                .clone()
                .unwrap_or_else(|| result.status.clone());
            *row.error_code_counts.entry(code).or_insert(0) += 1;
        }
        if let Some(value) = result.score {
            row.scores.push(value);
            row.scored_weight += task_weight;
            row.weighted_score_sum += value * task_weight;
        }
        if let Some(value) = result.wall_time_ms {
            row.wall_times.push(value);
        }
        if let Some(value) = result.output_tokens_per_second {
            row.throughputs.push(value);
        }
        if let Some(value) = result_cost_usd_for_coverage(result) {
            row.total_cost_usd += value;
            row.costed += 1;
            row.has_cost = true;
        }
        if let Some(assumption) = result
            .pricing_assumption
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            *row.pricing_assumption_counts
                .entry(assumption.to_string())
                .or_insert(0) += 1;
        }
        if result.started_at.as_deref().unwrap_or_default() > row.latest_started.as_str() {
            row.latest_started = result.started_at.clone().unwrap_or_default();
        }
    }
    let mut rows = groups.into_values().collect::<Vec<_>>();
    rows.sort_by(compare_export_target_rows);
    rows
}

fn export_task_rows(results: &[store::ResultRecord]) -> Vec<ExportTaskAggregate> {
    let mut groups: BTreeMap<String, ExportTaskAggregate> = BTreeMap::new();
    for result in results {
        let task_weight = result_task_weight(result);
        let group_id = result
            .run_group_id
            .clone()
            .unwrap_or_else(|| result.id.clone());
        let key = format!(
            "{}|{}|{}|{}",
            group_id, result.benchmark_pack_id, result.task_id, result.target_id
        );
        let row = groups.entry(key).or_insert_with(|| ExportTaskAggregate {
            group_id: group_id.clone(),
            pack_id: result.benchmark_pack_id.clone(),
            task_id: result.task_id.clone(),
            target_id: result.target_id.clone(),
            latest_started: result.started_at.clone().unwrap_or_default(),
            ..ExportTaskAggregate::default()
        });
        row.runs += 1;
        row.total_weight += task_weight;
        if result.status == "passed" {
            row.passed += 1;
            row.weighted_passed += task_weight;
        } else {
            let code = result
                .error_code
                .clone()
                .unwrap_or_else(|| result.status.clone());
            *row.error_code_counts.entry(code).or_insert(0) += 1;
        }
        if let Some(value) = result.score {
            row.scores.push(value);
            row.scored_weight += task_weight;
            row.weighted_score_sum += value * task_weight;
        }
        if let Some(value) = result.wall_time_ms {
            row.wall_times.push(value);
        }
        if let Some(value) = result.provider_time_to_first_token_ms {
            row.provider_first_token_times.push(value);
        }
        if let Some(value) = total_tokens_for_result(result) {
            row.tokens.push(value);
        }
        if let Some(value) = result.output_tokens_per_second {
            row.throughputs.push(value);
        }
        if let Some(status) = http_status_code(result.http_status) {
            *row.http_status_counts.entry(status).or_insert(0) += 1;
        }
        if let Some(value) = result_cost_usd_for_coverage(result) {
            row.total_cost_usd += value;
            row.has_cost = true;
        }
        if result.started_at.as_deref().unwrap_or_default() > row.latest_started.as_str() {
            row.latest_started = result.started_at.clone().unwrap_or_default();
        }
    }
    let mut rows = groups.into_values().collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        (a.passed * b.runs.max(1))
            .cmp(&(b.passed * a.runs.max(1)))
            .then_with(|| {
                avg(&a.scores)
                    .partial_cmp(&avg(&b.scores))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                percentile(&b.wall_times, 0.95)
                    .partial_cmp(&percentile(&a.wall_times, 0.95))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.task_id.cmp(&b.task_id))
            .then_with(|| a.target_id.cmp(&b.target_id))
    });
    rows
}

fn export_task_target_matrix(results: &[store::ResultRecord]) -> String {
    let targets = results
        .iter()
        .map(|result| result.target_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut rows: BTreeMap<String, ExportTaskMatrixRow> = BTreeMap::new();
    for result in results {
        let group_id = result
            .run_group_id
            .clone()
            .unwrap_or_else(|| result.id.clone());
        let key = format!(
            "{}|{}|{}",
            group_id, result.benchmark_pack_id, result.task_id
        );
        let row = rows.entry(key).or_insert_with(|| ExportTaskMatrixRow {
            group_id: group_id.clone(),
            pack_id: result.benchmark_pack_id.clone(),
            task_id: result.task_id.clone(),
            cells: BTreeMap::new(),
        });
        let cell = row.cells.entry(result.target_id.clone()).or_default();
        cell.runs += 1;
        if result.status == "passed" {
            cell.passed += 1;
        } else {
            let code = result
                .error_code
                .clone()
                .unwrap_or_else(|| result.status.clone());
            *cell.error_code_counts.entry(code).or_insert(0) += 1;
        }
        if let Some(value) = result.score {
            cell.scores.push(value);
        }
        if let Some(value) = result.wall_time_ms {
            cell.wall_times.push(value);
        }
    }

    let mut out = String::from("\n## Task Target Matrix\n\n");
    if targets.is_empty() || rows.is_empty() {
        out.push_str("No task-target rows are available in this export scope.\n\n");
        return out;
    }
    out.push_str("| Group | Pack | Task |");
    for target in &targets {
        out.push_str(&format!(" {} |", markdown_cell(target)));
    }
    out.push('\n');
    out.push_str("| --- | --- | --- |");
    for _ in &targets {
        out.push_str(" --- |");
    }
    out.push('\n');

    for row in rows.into_values() {
        out.push_str(&format!(
            "| {} | {} | {} |",
            markdown_cell(&short_id(&row.group_id)),
            markdown_cell(&row.pack_id),
            markdown_cell(&row.task_id)
        ));
        for target in &targets {
            let cell = row
                .cells
                .get(target)
                .map(export_task_matrix_cell)
                .unwrap_or_else(|| "-".into());
            out.push_str(&format!(" {} |", markdown_cell(&cell)));
        }
        out.push('\n');
    }
    out.push('\n');
    out
}

fn export_target_ranking(rows: &[ExportTargetAggregate]) -> String {
    let mut out = String::from("## Target Ranking\n\n");
    out.push_str("Targets are aggregated across the current export scope and ranked by weighted pass rate, pass rate, weighted average score, average score, score stability, p95 wall time, average cost, throughput, and sample size. Task weights default to 1.0 and come from task metadata captured in run reproducibility.\n\n");
    out.push_str("| Rank | Target | Provider | Scope | Runs | Weighted pass | Pass rate / 95% CI | Weighted score | Score avg / σ | P95 wall | Avg cost | Out tok/s | Errors |\n");
    out.push_str(
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    if rows.is_empty() {
        out.push_str("| - | - | - | - | 0 | - | - | - | - | - | - | - | - |\n\n");
        return out;
    }
    for (index, row) in rows.iter().enumerate() {
        let error_summary = if row.error_code_counts.is_empty() {
            "-".into()
        } else {
            format_task_matrix_error_counts(&row.error_code_counts)
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} pack(s), {} task(s), {} group(s) | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            index + 1,
            markdown_cell(&row.target_id),
            markdown_cell(&format_text_counts(&row.provider_counts)),
            row.pack_ids.len(),
            row.task_ids.len(),
            row.group_ids.len(),
            row.runs,
            format_percent(export_weighted_pass_rate(row.weighted_passed, row.total_weight)),
            format!(
                "{}; {}",
                format_percent(Some(export_target_pass_rate(row))),
                format_percent_range(pass_rate_interval(row.passed, row.runs))
            ),
            format_number(export_weighted_average_score(row.weighted_score_sum, row.scored_weight)),
            format_number_with_spread(avg(&row.scores), std_dev(&row.scores)),
            format_ms(percentile(&row.wall_times, 0.95)),
            format_cost(export_target_avg_cost(row)),
            format_number(avg(&row.throughputs)),
            markdown_cell(&error_summary)
        ));
    }
    out.push('\n');
    out
}

fn export_distribution_summary(
    target_rows: &[ExportTargetAggregate],
    task_rows: &[ExportTaskAggregate],
) -> String {
    let mut out = String::from("## Distribution Summary\n\n");
    out.push_str("Median/min/max values show repeatability and outliers across the current export scope.\n\n");
    out.push_str("| Target | Runs | Score med/min/max | Wall med/min/max | P95 wall |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    if target_rows.is_empty() {
        out.push_str("| - | 0 | - | - | - |\n");
    } else {
        for row in target_rows {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                markdown_cell(&row.target_id),
                row.runs,
                format_number_distribution(&row.scores),
                format_ms_distribution(&row.wall_times),
                format_ms(percentile(&row.wall_times, 0.95))
            ));
        }
    }

    out.push_str("\n| Group | Pack | Task | Target | Runs | Score med/min/max | Wall med/min/max | P95 wall |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    if task_rows.is_empty() {
        out.push_str("| - | - | - | - | 0 | - | - | - |\n");
    } else {
        for row in task_rows.iter().take(10) {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
                markdown_cell(&short_id(&row.group_id)),
                markdown_cell(&row.pack_id),
                markdown_cell(&row.task_id),
                markdown_cell(&row.target_id),
                row.runs,
                format_number_distribution(&row.scores),
                format_ms_distribution(&row.wall_times),
                format_ms(percentile(&row.wall_times, 0.95))
            ));
        }
    }
    out.push('\n');
    out
}

fn export_task_matrix_cell(cell: &ExportTaskMatrixCell) -> String {
    let mut parts = vec![
        format!("{}/{} passed", cell.passed, cell.runs),
        format!(
            "score {}",
            format_number_with_spread(avg(&cell.scores), std_dev(&cell.scores))
        ),
        format!("p95 {}", format_ms(percentile(&cell.wall_times, 0.95))),
    ];
    if !cell.error_code_counts.is_empty() {
        parts.push(format!(
            "errors {}",
            format_task_matrix_error_counts(&cell.error_code_counts)
        ));
    }
    parts.join("; ")
}

fn format_task_matrix_error_counts(counts: &BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(value, count)| format!("{} ({})", value, count))
        .collect::<Vec<_>>()
        .join(", ")
}

fn export_decision_snapshot(
    comparison_rows: &[ExportAggregate],
    task_rows: &[ExportTaskAggregate],
    target_rows: &[ExportTargetAggregate],
    deployment_scope: &ExportDeploymentScope,
) -> String {
    let mut out = String::from("## Decision Snapshot\n\n");
    if comparison_rows.is_empty() || target_rows.is_empty() {
        out.push_str("No comparison rows are available in this export scope.\n\n");
        return out;
    }

    let recommended = &target_rows[0];
    let mut ranked = comparison_rows.to_vec();
    ranked.sort_by(compare_export_decision_rows);
    let best = ranked[0].clone();
    let best_pass_rate = export_pass_rate(&best);
    let reliable = comparison_rows
        .iter()
        .filter(|row| (export_pass_rate(row) - best_pass_rate).abs() < f64::EPSILON)
        .cloned()
        .collect::<Vec<_>>();
    let fastest = reliable
        .iter()
        .filter(|row| {
            export_p95_wall(row)
                .or_else(|| avg(&row.wall_times))
                .is_some()
        })
        .min_by(|a, b| {
            compare_optional_f64_asc(
                export_p95_wall(a).or_else(|| avg(&a.wall_times)),
                export_p95_wall(b).or_else(|| avg(&b.wall_times)),
            )
        })
        .cloned();
    let cheapest = reliable
        .iter()
        .filter(|row| export_avg_cost(row).is_some())
        .min_by(|a, b| compare_optional_f64_asc(export_avg_cost(a), export_avg_cost(b)))
        .cloned();
    let throughput = reliable
        .iter()
        .filter(|row| avg(&row.throughputs).is_some())
        .max_by(|a, b| compare_optional_f64_asc(avg(&a.throughputs), avg(&b.throughputs)))
        .cloned();
    let weakest_task = task_rows.first();
    let close_contenders = close_target_contenders(target_rows);
    let pack_calibration_issues = pack_calibration_issues_for_scope(target_rows);
    let evidence = comparison_evidence_assessment(comparison_rows, task_rows, target_rows);
    let decision_status = evidence_decision_status(&evidence);
    let selection_note = evidence_selection_note(&evidence, target_rows);
    let selected_target =
        evidence_selected_target_id(&evidence, target_rows).unwrap_or_else(|| "-".into());

    out.push_str("| Signal | Target | Group | Pack | Why |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    out.push_str(&decision_row(
        "Recommended target",
        &recommended.target_id,
        "-",
        "-",
        &format!(
            "{} weighted pass, {} pass across {} run(s), 95% CI {}, weighted score {}, score avg / σ {}, scope {} pack/task slot(s) / {} pack(s) / {} task(s) / {} group(s)",
            format_percent(export_weighted_pass_rate(recommended.weighted_passed, recommended.total_weight)),
            format_percent(Some(export_target_pass_rate(recommended))),
            recommended.runs,
            format_percent_range(pass_rate_interval(recommended.passed, recommended.runs)),
            format_number(export_weighted_average_score(recommended.weighted_score_sum, recommended.scored_weight)),
            format_number_with_spread(avg(&recommended.scores), std_dev(&recommended.scores)),
            recommended.pack_task_slots.len(),
            recommended.pack_ids.len(),
            recommended.task_ids.len(),
            recommended.group_ids.len()
        ),
    ));
    for contender in &close_contenders {
        out.push_str(&decision_row(
            "Close contender",
            &contender.target_id,
            "-",
            "-",
            &format!(
                "matched pass rate and avg score; 95% CI {}, score avg / σ {}, p95 wall {}, avg cost {}",
                format_percent_range(pass_rate_interval(contender.passed, contender.runs)),
                format_number_with_spread(avg(&contender.scores), std_dev(&contender.scores)),
                format_ms(percentile(&contender.wall_times, 0.95)),
                format_cost(export_target_avg_cost(contender))
            ),
        ));
    }
    out.push_str(&decision_row(
        "Best overall",
        &best.target_id,
        &best.group_id,
        &best.pack_id,
        &format!(
            "{} weighted pass, {} pass, weighted score {}, score avg / σ {}, p95 wall {}",
            format_percent(export_weighted_pass_rate(
                best.weighted_passed,
                best.total_weight
            )),
            format_percent(Some(export_pass_rate(&best))),
            format_number(export_weighted_average_score(
                best.weighted_score_sum,
                best.scored_weight
            )),
            format_number_with_spread(avg(&best.scores), std_dev(&best.scores)),
            format_ms(export_p95_wall(&best))
        ),
    ));
    if let Some(row) = fastest {
        out.push_str(&decision_row(
            "Fastest reliable",
            &row.target_id,
            &row.group_id,
            &row.pack_id,
            &format!(
                "p95 wall {}, {} pass",
                format_ms(export_p95_wall(&row).or_else(|| avg(&row.wall_times))),
                format_percent(Some(export_pass_rate(&row)))
            ),
        ));
    }
    if let Some(row) = cheapest {
        out.push_str(&decision_row(
            "Cheapest reliable",
            &row.target_id,
            &row.group_id,
            &row.pack_id,
            &format!(
                "avg cost {}, {} pass",
                format_cost(export_avg_cost(&row)),
                format_percent(Some(export_pass_rate(&row)))
            ),
        ));
    }
    if let Some(row) = throughput {
        out.push_str(&decision_row(
            "Highest throughput",
            &row.target_id,
            &row.group_id,
            &row.pack_id,
            &format!(
                "{} out tok/s, {} pass",
                format_number(avg(&row.throughputs)),
                format_percent(Some(export_pass_rate(&row)))
            ),
        ));
    }
    if let Some(row) = weakest_task {
        out.push_str(&decision_row(
            "Weakest task",
            &row.target_id,
            &row.group_id,
            &row.pack_id,
            &format!(
                "{}: {}/{} passed, p95 wall {}, errors {}",
                row.task_id,
                row.passed,
                row.runs,
                format_ms(percentile(&row.wall_times, 0.95)),
                format_text_counts(&row.error_code_counts)
            ),
        ));
    }
    out.push('\n');

    out.push_str(&format!(
        "Decision status: {} (selected target: {})\n\n",
        decision_status, selected_target
    ));
    out.push_str(&format!("Selection note: {}\n\n", selection_note));
    out.push_str(&format!(
        "Evidence grade: {} ({})\n\n",
        evidence.label, evidence.grade
    ));
    out.push_str(&format!("Evidence note: {}\n\n", evidence.note));
    out.push_str(&format!(
        "Deployment scope: {}. {}\n\n",
        deployment_scope_kind(deployment_scope),
        deployment_scope_note(deployment_scope)
    ));
    out.push_str(&format!(
        "Calibration note: {}\n\n",
        pack_calibration_note(&pack_calibration_issues)
    ));
    if !evidence.risks.is_empty() {
        out.push_str(&format!(
            "Evidence risks: {}\n\n",
            evidence.risks.join(", ")
        ));
    }
    out.push_str(&format!(
        "Minimum next run: {}\n\n",
        evidence.minimum_next_run
    ));
    if let Some(next_run) = recommended_next_run(&evidence, target_rows) {
        let task_note = if next_run.task_ids.is_empty() {
            String::new()
        } else {
            format!("; tasks {}", next_run.task_ids.join(", "))
        };
        out.push_str(&format!(
            "Suggested next run: pack(s) {}; targets {}{}; repetitions {}; warmups {}; concurrency {}; max cost {}. {}\n\n",
            next_run.benchmark_pack_ids.join(", "),
            next_run.target_ids.join(", "),
            task_note,
            next_run.repetitions,
            next_run.warmup_runs,
            next_run.concurrency,
            format_cost(Some(next_run.max_cost_usd)),
            next_run.note
        ));
    }
    out.push_str(&format!(
        "Confidence note: {}\n\n",
        confidence_note(target_rows, comparison_rows, task_rows)
    ));
    out.push_str(&format!(
        "Coverage note: {}\n\n",
        target_coverage_parity_note(target_rows)
    ));
    if let Some(note) = close_contenders_note(&close_contenders) {
        out.push_str(&format!("Tie note: {}\n\n", note));
    }
    out.push_str(&format!(
        "Score stability: {}\n\n",
        score_stability_note(target_rows)
    ));
    out
}

fn export_run_configuration(run_groups: &[store::RunGroupRecord]) -> String {
    let mut out = String::from("## Run Configuration\n\n");
    if run_groups.is_empty() {
        out.push_str("No queued run-group configuration was available for this export scope.\n\n");
        return out;
    }
    out.push_str("| Group | Status | Pack | Run settings | Target snapshots |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for group in run_groups {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            markdown_cell(&short_id(&group.id)),
            markdown_cell(&group.status),
            markdown_cell(&group.benchmark_pack_id),
            markdown_cell(&run_group_settings_summary(&group.config)),
            markdown_cell(&run_group_target_summary(group))
        ));
    }
    out.push('\n');
    out
}

fn export_run_group_trends_markdown(rows: &[ExportRunGroupTrend]) -> String {
    let mut out = String::from("## Run Group Trends\n\n");
    if rows.is_empty() {
        out.push_str("No repeated target/pack run groups are available in this export scope.\n\n");
        return out;
    }
    out.push_str("Newest run groups are compared with the previous run group for the same benchmark pack and target. Deltas are current minus previous.\n\n");
    out.push_str("| Pack | Target | Current | Previous | Runs | Pass rate delta | Score delta | P95 delta | Avg cost delta | Signal |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {}/{} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&row.pack_id),
            markdown_cell(&row.target_id),
            markdown_cell(&short_id(&row.current_group_id)),
            markdown_cell(&short_id(&row.previous_group_id)),
            row.current_runs,
            row.previous_runs,
            markdown_cell(&format_percent_point_delta(row.pass_rate_delta)),
            markdown_cell(&format_number_delta(row.average_score_delta)),
            markdown_cell(&format_ms_delta(row.p95_wall_time_delta_ms)),
            markdown_cell(&format_cost_delta(row.average_cost_delta_usd)),
            markdown_cell(&row.signal)
        ));
    }
    out.push('\n');
    out
}

fn export_metric_coverage(results: &[store::ResultRecord]) -> String {
    let total = results.len();
    let rows = [
        metric_coverage_row(results, "Score", |result| result.score.is_some(), "Scored runs should have this; missing means the run failed before scoring completed."),
        metric_coverage_row(results, "pass_fail", |result| result.pass_fail.is_some(), "Required v1 alias derived from run status."),
        metric_coverage_row(results, "score_numeric", |result| result.score_numeric.is_some(), "Required v1 alias for score."),
        metric_coverage_row(results, "Wall time", |result| result.wall_time_ms.is_some(), "Wall-clock timing is expected for persisted runs; missing means the run failed before timing was stored."),
        metric_coverage_row(results, "Setup time", |result| result.setup_time_ms.is_some(), "Prompt and repo/code tasks report app/workspace setup time before target execution."),
        metric_coverage_row(results, "Target time", |result| result.target_time_ms.is_some(), "Prompt and repo/code tasks report time spent invoking the benchmark target before evaluation."),
        metric_coverage_row(results, "Evaluation time", |result| result.evaluation_time_ms.is_some(), "Scoring and repo/code tasks report time spent in the evaluation command after target execution."),
        metric_coverage_row(results, "Model call time", |result| result.model_call_wall_time_ms.is_some(), "Provider-backed repo/code tasks report the model invocation wall time separately from scoring time."),
        metric_coverage_row(results, "Exit code", |result| result.exit_code.is_some(), "Process-backed scoring runs report the normalized scoring command exit code."),
        metric_coverage_row(results, "Harness exit code", |result| result.harness_exit_code.is_some(), "Worker harness command runs report the external harness process exit code when available."),
        metric_coverage_row(results, "Stdout bytes", |result| result.stdout_bytes.is_some(), "Process-backed runs report redacted stdout byte counts for artifact sizing and debugging."),
        metric_coverage_row(results, "Stderr bytes", |result| result.stderr_bytes.is_some(), "Process-backed runs report redacted stderr byte counts for artifact sizing and debugging."),
        metric_coverage_row(results, "Files changed", |result| result.files_changed.is_some(), "Repo/code tasks report how many files changed in the captured git diff."),
        metric_coverage_row(results, "Lines added", |result| result.lines_added.is_some(), "Repo/code tasks report added lines from the captured git diff."),
        metric_coverage_row(results, "Lines deleted", |result| result.lines_deleted.is_some(), "Repo/code tasks report deleted lines from the captured git diff."),
        metric_coverage_row(results, "Commands observed", |result| result.commands_observed_count.is_some(), "Process-backed repo/code and worker harness runs report benchmark commands BenchForge observed or executed."),
        metric_coverage_row(results, "Dangerous command hits", |result| result.dangerous_command_hits.is_some(), "Repo/code tasks count suspicious command patterns detected in redacted stdout and stderr."),
        metric_coverage_row(results, "Provider TTFB", |result| result.provider_time_to_first_byte_ms.is_some(), "Only provider-backed model calls report transport timing; mock and scoring-only tasks may not have it."),
        metric_coverage_row(results, "TTFT", |result| result.provider_time_to_first_token_ms.is_some(), "Time to first token is available for streaming provider calls; non-streaming calls leave it blank."),
        metric_coverage_row(results, "ttft_ms", |result| result.ttft_ms.is_some(), "Required v1 alias for time to first token."),
        metric_coverage_row(results, "Provider total", |result| result.provider_request_total_ms.is_some(), "Provider request timing is recorded when the adapter call exposes transport timing."),
        metric_coverage_row(results, "Prompt tokens", |result| result.prompt_tokens.is_some(), "Requires provider token usage or a local runtime that reports prompt tokens."),
        metric_coverage_row(results, "input_tokens", |result| result.input_tokens.is_some(), "Required v1 alias for prompt/input tokens."),
        metric_coverage_row(results, "Completion tokens", |result| result.completion_tokens.is_some(), "Requires provider token usage or a local runtime that reports output tokens."),
        metric_coverage_row(results, "output_tokens", |result| result.output_tokens.is_some(), "Required v1 alias for completion/output tokens."),
        metric_coverage_row(results, "Reasoning tokens", |result| result.reasoning_tokens.is_some(), "Only reasoning-capable providers/models report this metric."),
        metric_coverage_row(results, "Cached tokens", |result| result.cached_tokens.is_some(), "Providers with prompt cache accounting report cached input tokens when available."),
        metric_coverage_row(results, "Cache read tokens", |result| result.cache_read_tokens.is_some(), "Providers with prompt cache accounting report cache-read input tokens when available."),
        metric_coverage_row(results, "Cache write tokens", |result| result.cache_write_tokens.is_some(), "Providers with prompt cache accounting report cache-write or cache-creation input tokens when available."),
        metric_coverage_row(results, "Total tokens", |result| total_tokens_for_result(result).is_some(), "Uses provider total tokens when available or prompt plus completion tokens when both are present."),
        metric_coverage_row(results, "Output tokens/sec", |result| result.output_tokens_per_second.is_some(), "Requires completion token counts and wall time."),
        metric_coverage_row(results, "decode_tokens_per_sec", |result| result.decode_tokens_per_sec.is_some(), "Required v1 alias for output token throughput."),
        metric_coverage_row(results, "Peak RSS", |result| result.peak_rss_mb.is_some(), "Process-backed runs report peak resident memory only when BenchForge or a worker can observe it."),
        metric_coverage_row(results, "HTTP status", |result| result.http_status.is_some(), "Only HTTP provider calls expose this; local mocks and host scoring usually do not."),
        metric_coverage_row(results, "Retry attempts", |result| result.provider_attempts.is_some(), "Only retry-aware provider calls expose attempt counts."),
        metric_coverage_row(results, "Retry-After", |result| result.provider_retry_after_ms.is_some(), "Only provider responses with Retry-After headers expose this."),
        metric_coverage_row(results, "Retry delay", |result| result.provider_retry_delay_ms.is_some(), "Recorded when BenchForge waits before retrying a provider call."),
        metric_coverage_row(results, "Provider model", |result| non_empty_option(result.provider_model.as_deref()), "Provider-supplied when available; local runtimes may be confirmed from /models before BenchForge falls back to the configured target model."),
        metric_coverage_row(results, "Provider model source", |result| non_empty_option(result.provider_model_source.as_deref()), "Identifies whether provider_model came from the provider response, a local runtime model list, or the configured target model."),
        metric_coverage_row(results, "Finish reason", |result| non_empty_option(result.finish_reason.as_deref()), "Only model APIs that report completion finish reasons expose this."),
        metric_coverage_row(results, "Cost", result_has_cost_coverage, "Requires token usage plus configured pricing, or a known-zero local/mock target."),
        metric_coverage_row(results, "estimated_cost_usd", |result| result.estimated_cost_usd.is_some(), "Required v1 alias for estimated benchmark cost."),
        metric_coverage_row(results, "Pricing assumption", |result| non_empty_option(result.pricing_assumption.as_deref()), "Present when a cost estimate used a documented pricing fallback, such as prompt-cache tokens priced at normal input-token rates."),
        metric_coverage_row(results, "Safety findings", |result| result.security_finding_count.is_some(), "Worker security packs report finding counts as first-class result metrics."),
        metric_coverage_row(results, "Safety files scanned", |result| result.security_files_scanned.is_some(), "Worker security packs report how many files or manifests were inspected."),
        metric_coverage_row(results, "Import format", |result| non_empty_option(result.import_format.as_deref()), "Worker harness imports set this when a run was read from external result files."),
        metric_coverage_row(results, "Import source", |result| non_empty_option(result.import_source.as_deref()), "Identifies whether imported harness output came from a file, directory, or other supported path."),
        metric_coverage_row(results, "Import files", |result| result.import_file_count.is_some(), "Counts how many imported result files contributed to the run result."),
        metric_coverage_row(results, "Import total files", |result| result.import_total_file_count.is_some(), "Counts all supported result files discovered before import limits were applied."),
        metric_coverage_row(results, "Import omitted files", |result| result.import_omitted_file_count.is_some(), "Counts supported result files skipped after worker import limits were reached."),
        metric_coverage_row(results, "Import unsupported files", |result| result.import_unsupported_file_count.is_some(), "Counts unsupported side files ignored during worker directory imports."),
        metric_coverage_row(results, "Import truncated", |result| result.import_truncated.is_some(), "Set by worker imports to show whether imported result evidence was truncated or partially bounded."),
        metric_coverage_row(results, "Import truncated bytes", |result| result.import_truncated_bytes.is_some(), "Counts bytes omitted from imported result evidence when import size limits apply."),
        metric_coverage_row(results, "Summary parser", |result| non_empty_option(result.summary_source.as_deref()), "Identifies the parser that extracted pass/fail summary from imported harness output."),
    ];

    let mut out = String::from("## Metric Coverage\n\n");
    out.push_str("Blank metric cells mean BenchForge did not receive enough source data for that metric; they are not treated as zero.\n\n");
    out.push_str("| Metric | Present | Missing | Notes |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            markdown_cell(row.label),
            row.present,
            total.saturating_sub(row.present),
            markdown_cell(row.note)
        ));
    }
    out.push('\n');
    out
}

fn export_model_identity_warnings(comparison_rows: &[ExportAggregate]) -> String {
    let warnings = model_identity_warnings(comparison_rows);
    if warnings.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Model Identity Warnings\n\n");
    out.push_str("These warnings do not change scores, but they affect how confidently a report can say which model actually served the run.\n\n");
    out.push_str("| Issue | Group | Pack | Target | Runs | Missing served model | Reported models | Model sources | Note |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for warning in warnings {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(warning.issue),
            markdown_cell(&short_id(&warning.group_id)),
            markdown_cell(&warning.pack_id),
            markdown_cell(&warning.target_id),
            warning.runs,
            warning.missing_provider_model_runs,
            markdown_cell(&format_text_counts(&warning.provider_models)),
            markdown_cell(&format_text_counts(&warning.provider_model_sources)),
            markdown_cell(warning.note),
        ));
    }
    out.push('\n');
    out
}

fn export_generation_setting_warnings(comparison_rows: &[ExportAggregate]) -> String {
    let warnings = generation_setting_warnings(comparison_rows);
    if warnings.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Generation Setting Warnings\n\n");
    out.push_str("These warnings do not change scores, but they prevent deterministic and exploratory sampling runs from being treated as one clean leaderboard.\n\n");
    out.push_str("| Issue | Group | Pack | Target | Runs | Generation settings | Note |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for warning in warnings {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(warning.issue),
            markdown_cell(&short_id(&warning.group_id)),
            markdown_cell(&warning.pack_id),
            markdown_cell(&warning.target_id),
            warning.runs,
            markdown_cell(&format_text_counts(&warning.generation_settings)),
            markdown_cell(warning.note),
        ));
    }
    out.push('\n');
    out
}

fn model_identity_warnings_json(comparison_rows: &[ExportAggregate]) -> Vec<serde_json::Value> {
    model_identity_warnings(comparison_rows)
        .into_iter()
        .map(|warning| {
            serde_json::json!({
                "issue": warning.issue,
                "severity": warning.severity,
                "group_id": warning.group_id,
                "benchmark_pack_id": warning.pack_id,
                "target_id": warning.target_id,
                "runs": warning.runs,
                "missing_provider_model_runs": warning.missing_provider_model_runs,
                "provider_models": warning.provider_models,
                "provider_model_sources": warning.provider_model_sources,
                "note": warning.note,
            })
        })
        .collect()
}

fn generation_setting_warnings_json(comparison_rows: &[ExportAggregate]) -> Vec<serde_json::Value> {
    generation_setting_warnings(comparison_rows)
        .into_iter()
        .map(|warning| {
            serde_json::json!({
                "issue": warning.issue,
                "severity": warning.severity,
                "group_id": warning.group_id,
                "benchmark_pack_id": warning.pack_id,
                "target_id": warning.target_id,
                "runs": warning.runs,
                "generation_settings": warning.generation_settings,
                "note": warning.note,
            })
        })
        .collect()
}

fn model_identity_warnings(comparison_rows: &[ExportAggregate]) -> Vec<ModelIdentityWarning> {
    let mut warnings = Vec::new();
    for row in comparison_rows {
        let reported = row.provider_model_counts.values().sum::<usize>();
        let missing = row.runs.saturating_sub(reported);
        if missing > 0 {
            warnings.push(ModelIdentityWarning {
                issue: "provider_model_missing",
                severity: "warn",
                group_id: row.group_id.clone(),
                pack_id: row.pack_id.clone(),
                target_id: row.target_id.clone(),
                runs: row.runs,
                missing_provider_model_runs: missing,
                provider_models: row.provider_model_counts.clone(),
                provider_model_sources: row.provider_model_source_counts.clone(),
                note: "Some runs did not report the served model id; confirm the target/runtime identity before treating this as definitive model-selection evidence.",
            });
        }
        if row.provider_model_counts.len() > 1 {
            warnings.push(ModelIdentityWarning {
                issue: "provider_model_inconsistent",
                severity: "warn",
                group_id: row.group_id.clone(),
                pack_id: row.pack_id.clone(),
                target_id: row.target_id.clone(),
                runs: row.runs,
                missing_provider_model_runs: missing,
                provider_models: row.provider_model_counts.clone(),
                provider_model_sources: row.provider_model_source_counts.clone(),
                note: "This target reported multiple served model ids in the same group/pack aggregate; split or re-run before comparing it as one model.",
            });
        }
        let configured_fallback_runs = row
            .provider_model_source_counts
            .get("target_config")
            .copied()
            .unwrap_or(0);
        if configured_fallback_runs > 0 {
            warnings.push(ModelIdentityWarning {
                issue: "provider_model_configured_fallback",
                severity: "warn",
                group_id: row.group_id.clone(),
                pack_id: row.pack_id.clone(),
                target_id: row.target_id.clone(),
                runs: row.runs,
                missing_provider_model_runs: missing,
                provider_models: row.provider_model_counts.clone(),
                provider_model_sources: row.provider_model_source_counts.clone(),
                note: "Some runs used the configured target model because the provider did not echo a served model id; confirm runtime identity before treating this as definitive model-selection evidence.",
            });
        }
    }
    warnings.sort_by(|left, right| {
        left.group_id
            .cmp(&right.group_id)
            .then(left.pack_id.cmp(&right.pack_id))
            .then(left.target_id.cmp(&right.target_id))
            .then(left.issue.cmp(right.issue))
    });
    warnings
}

fn generation_setting_warnings(
    comparison_rows: &[ExportAggregate],
) -> Vec<GenerationSettingWarning> {
    let mut warnings = Vec::new();
    let mut scope_counts = BTreeMap::<String, usize>::new();
    for row in comparison_rows {
        for (setting, count) in &row.generation_setting_counts {
            *scope_counts.entry(setting.clone()).or_insert(0) += count;
        }
        if row.generation_setting_counts.len() > 1 {
            warnings.push(GenerationSettingWarning {
                issue: "generation_settings_mixed_target",
                severity: "warn",
                group_id: row.group_id.clone(),
                pack_id: row.pack_id.clone(),
                target_id: row.target_id.clone(),
                runs: row.runs,
                generation_settings: row.generation_setting_counts.clone(),
                note: "This target/pack aggregate mixes sampling settings; split deterministic and exploratory runs before comparing it as one model result.",
            });
        }
    }
    if scope_counts.len() > 1 {
        warnings.push(GenerationSettingWarning {
            issue: "generation_settings_mixed_scope",
            severity: "warn",
            group_id: "all".into(),
            pack_id: "all".into(),
            target_id: "all".into(),
            runs: scope_counts.values().sum(),
            generation_settings: scope_counts,
            note: "The visible comparison mixes generation sampling settings; rerun or filter so one leaderboard uses the same temperature, top_p, and seed policy.",
        });
    }
    warnings.sort_by(|left, right| {
        left.group_id
            .cmp(&right.group_id)
            .then(left.pack_id.cmp(&right.pack_id))
            .then(left.target_id.cmp(&right.target_id))
            .then(left.issue.cmp(right.issue))
    });
    warnings
}

struct ModelIdentityWarning {
    issue: &'static str,
    severity: &'static str,
    group_id: String,
    pack_id: String,
    target_id: String,
    runs: usize,
    missing_provider_model_runs: usize,
    provider_models: BTreeMap<String, usize>,
    provider_model_sources: BTreeMap<String, usize>,
    note: &'static str,
}

struct GenerationSettingWarning {
    issue: &'static str,
    severity: &'static str,
    group_id: String,
    pack_id: String,
    target_id: String,
    runs: usize,
    generation_settings: BTreeMap<String, usize>,
    note: &'static str,
}

fn export_safety_findings(results: &[store::ResultRecord]) -> String {
    let rows = results
        .iter()
        .filter(|result| is_safety_result(result))
        .collect::<Vec<_>>();
    let mut out = String::from("## Safety Findings\n\n");
    if rows.is_empty() {
        out.push_str("No worker safety findings are present in this export scope.\n\n");
        return out;
    }
    out.push_str("Worker security packs report finding counts as first-class metrics. Detailed redacted locations remain in the run artifacts.\n\n");
    out.push_str(
        "| Run | Group | Pack | Task | Target | Status | Findings | Files scanned | Error | Error detail |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for result in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&short_id(&result.id)),
            markdown_cell(
                &result
                    .run_group_id
                    .as_ref()
                    .map(|id| short_id(id))
                    .unwrap_or_else(|| "-".into())
            ),
            markdown_cell(&result.benchmark_pack_id),
            markdown_cell(&result.task_id),
            markdown_cell(&result.target_id),
            markdown_cell(&result.status),
            format_number(result.security_finding_count),
            format_number(result.security_files_scanned),
            markdown_cell(result.error_code.as_deref().unwrap_or("-")),
            markdown_cell(&report_error_detail(result)),
        ));
    }
    out.push('\n');
    out
}

fn export_worker_imports(results: &[store::ResultRecord]) -> String {
    let rows = results
        .iter()
        .filter(|result| result_has_worker_import_provenance(result))
        .collect::<Vec<_>>();
    let mut out = String::from("## Worker Imports\n\n");
    if rows.is_empty() {
        out.push_str(
            "No worker-harness imported result files are present in this export scope.\n\n",
        );
        return out;
    }
    out.push_str("These rows were normalized from existing benchmark output rather than direct target execution. Reproduce them by reviewing the source path, formats, read files, fingerprints, and truncation state in `reproducibility.json`.\n\n");
    out.push_str("| Run | Group | Pack | Task | Target | Source | Path | Formats | Files | Read files | Fingerprints | Truncation | Summary source |\n");
    out.push_str(
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for result in rows {
        let import = result
            .reproducibility
            .get("worker_import")
            .unwrap_or(&serde_json::Value::Null);
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&short_id(&result.id)),
            markdown_cell(
                &result
                    .run_group_id
                    .as_ref()
                    .map(|id| short_id(id))
                    .unwrap_or_else(|| "-".into())
            ),
            markdown_cell(&result.benchmark_pack_id),
            markdown_cell(&result.task_id),
            markdown_cell(&result.target_id),
            markdown_cell(
                worker_import_string(
                    import,
                    "source",
                    result.import_source.as_deref().unwrap_or("-")
                )
                .as_str(),
            ),
            markdown_cell(worker_import_path(result, import).as_str()),
            markdown_cell(worker_import_formats(result, import).as_str()),
            markdown_cell(worker_import_file_counts(result, import).as_str()),
            markdown_cell(worker_import_read_files(import).as_str()),
            markdown_cell(worker_import_fingerprints(import).as_str()),
            markdown_cell(worker_import_truncation(result, import).as_str()),
            markdown_cell(
                worker_import_string(
                    import,
                    "summary_source",
                    result.summary_source.as_deref().unwrap_or("-"),
                )
                .as_str(),
            ),
        ));
    }
    out.push('\n');
    out
}

fn worker_imports_json(results: &[store::ResultRecord]) -> Vec<serde_json::Value> {
    results
        .iter()
        .filter(|result| result_has_worker_import_provenance(result))
        .map(|result| {
            let empty_import = serde_json::Value::Null;
            let import = result
                .reproducibility
                .get("worker_import")
                .unwrap_or(&empty_import);
            serde_json::json!({
                "run_id": &result.id,
                "run_group_id": &result.run_group_id,
                "benchmark_pack_id": &result.benchmark_pack_id,
                "task_id": &result.task_id,
                "target_id": &result.target_id,
                "status": &result.status,
                "source": worker_import_string(import, "source", result.import_source.as_deref().unwrap_or("-")),
                "path": worker_import_path(result, import),
                "formats": worker_import_format_list(result, import),
                "format": worker_import_string(import, "format", result.import_format.as_deref().unwrap_or("-")),
                "read_files": worker_import_read_file_list(import),
                "hash_algorithm": worker_import_string(import, "hash_algorithm", "-"),
                "file_details": import.get("file_details").cloned().unwrap_or_else(|| serde_json::json!([])),
                "file_count": result.import_file_count.or_else(|| import.get("file_count").and_then(|value| value.as_f64())),
                "total_file_count": result.import_total_file_count.or_else(|| import.get("total_file_count").and_then(|value| value.as_f64())),
                "omitted_file_count": result.import_omitted_file_count.or_else(|| import.get("omitted_file_count").and_then(|value| value.as_f64())),
                "unsupported_file_count": result.import_unsupported_file_count.or_else(|| import.get("unsupported_file_count").and_then(|value| value.as_f64())),
                "unsupported_files": import.get("unsupported_files").cloned().unwrap_or_else(|| serde_json::json!([])),
                "truncated": result.import_truncated
                    .map(|value| value != 0.0)
                    .or_else(|| import.get("truncated").and_then(|value| value.as_bool())),
                "truncated_metric": result.import_truncated,
                "truncated_bytes": result.import_truncated_bytes.or_else(|| import.get("truncated_bytes").and_then(|value| value.as_f64())),
                "summary_source": worker_import_string(import, "summary_source", result.summary_source.as_deref().unwrap_or("-")),
                "worker_import": result.reproducibility.get("worker_import").cloned().unwrap_or(serde_json::Value::Null)
            })
        })
        .collect()
}

fn result_has_worker_import_provenance(result: &store::ResultRecord) -> bool {
    result.reproducibility.get("worker_import").is_some()
        || non_empty_option(result.import_format.as_deref())
        || non_empty_option(result.import_source.as_deref())
        || non_empty_option(result.import_path.as_deref())
        || non_empty_option(result.summary_source.as_deref())
        || result.import_file_count.is_some()
        || result.import_total_file_count.is_some()
        || result.import_omitted_file_count.is_some()
        || result.import_unsupported_file_count.is_some()
        || result.import_truncated.is_some()
        || result.import_truncated_bytes.is_some()
}

fn worker_import_string(import: &serde_json::Value, key: &str, fallback: &str) -> String {
    import
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

fn worker_import_path(result: &store::ResultRecord, import: &serde_json::Value) -> String {
    worker_import_string(import, "path", result.import_path.as_deref().unwrap_or("-"))
}

fn worker_import_formats(result: &store::ResultRecord, import: &serde_json::Value) -> String {
    let formats = worker_import_format_list(result, import);
    if formats.is_empty() {
        "-".into()
    } else {
        formats.join(", ")
    }
}

fn worker_import_format_list(
    result: &store::ResultRecord,
    import: &serde_json::Value,
) -> Vec<String> {
    let mut formats = import
        .get("formats")
        .and_then(|value| value.as_array())
        .map(|values| json_string_list(values))
        .unwrap_or_default();
    if formats.is_empty() {
        if let Some(format) = import
            .get("format")
            .and_then(|value| value.as_str())
            .or(result.import_format.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            formats.push(format.to_string());
        }
    }
    formats
}

fn worker_import_file_counts(result: &store::ResultRecord, import: &serde_json::Value) -> String {
    let file_count = result
        .import_file_count
        .map(|value| value as u64)
        .or_else(|| import.get("file_count").and_then(|value| value.as_u64()));
    let total_file_count = import
        .get("total_file_count")
        .and_then(|value| value.as_u64())
        .or_else(|| result.import_total_file_count.map(|value| value as u64));
    let omitted_file_count = result
        .import_omitted_file_count
        .map(|value| value as u64)
        .or_else(|| {
            import
                .get("omitted_file_count")
                .and_then(|value| value.as_u64())
        });
    let mut parts = Vec::new();
    if let Some(count) = file_count {
        parts.push(format!("read {}", count));
    }
    if let Some(count) = total_file_count {
        parts.push(format!("total {}", count));
    }
    if let Some(count) = omitted_file_count {
        parts.push(format!("omitted {}", count));
    }
    if let Some(count) = result
        .import_unsupported_file_count
        .map(|value| value as u64)
        .or_else(|| {
            import
                .get("unsupported_file_count")
                .and_then(|value| value.as_u64())
        })
        .filter(|count| *count > 0)
    {
        parts.push(format!("unsupported {}", count));
    }
    if parts.is_empty() {
        "-".into()
    } else {
        parts.join("; ")
    }
}

fn worker_import_read_files(import: &serde_json::Value) -> String {
    let files = worker_import_read_file_list(import);
    if files.is_empty() {
        "-".into()
    } else {
        files.join(", ")
    }
}

fn worker_import_read_file_list(import: &serde_json::Value) -> Vec<String> {
    import
        .get("read_files")
        .and_then(|value| value.as_array())
        .map(|values| json_string_list(values))
        .unwrap_or_default()
}

fn worker_import_fingerprints(import: &serde_json::Value) -> String {
    let details = import
        .get("file_details")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if details.is_empty() {
        return "-".into();
    }
    let mut parts = Vec::new();
    for detail in details.iter().take(3) {
        let path = detail
            .get("path")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("file");
        let (label, hash) = detail
            .get("sha256")
            .and_then(|value| value.as_str())
            .map(|hash| ("sha256", hash))
            .or_else(|| {
                detail
                    .get("read_sha256")
                    .and_then(|value| value.as_str())
                    .map(|hash| ("read-sha256", hash))
            })
            .unwrap_or(("sha256", ""));
        if hash.is_empty() {
            parts.push(path.to_string());
        } else {
            let short = hash.chars().take(12).collect::<String>();
            parts.push(format!("{} {}:{}", path, label, short));
        }
    }
    if details.len() > 3 {
        parts.push(format!("+{} more", details.len() - 3));
    }
    parts.join(", ")
}

fn worker_import_truncation(result: &store::ResultRecord, import: &serde_json::Value) -> String {
    let truncated = result
        .import_truncated
        .map(|value| value != 0.0)
        .or_else(|| import.get("truncated").and_then(|value| value.as_bool()))
        .unwrap_or(false);
    let truncated_bytes = result
        .import_truncated_bytes
        .map(|value| value as u64)
        .or_else(|| {
            import
                .get("truncated_bytes")
                .and_then(|value| value.as_u64())
        })
        .unwrap_or(0);
    if truncated {
        if truncated_bytes > 0 {
            format!("yes ({} bytes)", truncated_bytes)
        } else {
            "yes".into()
        }
    } else {
        "no".into()
    }
}

fn json_string_list(values: &[serde_json::Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
}

fn safety_findings_json(results: &[store::ResultRecord]) -> Vec<serde_json::Value> {
    results
        .iter()
        .filter(|result| is_safety_result(result))
        .map(|result| {
            serde_json::json!({
                "run_id": &result.id,
                "run_group_id": &result.run_group_id,
                "benchmark_pack_id": &result.benchmark_pack_id,
                "task_id": &result.task_id,
                "target_id": &result.target_id,
                "status": &result.status,
                "error_code": &result.error_code,
                "error_message": &result.error_message,
                "finding_count": result.security_finding_count,
                "files_scanned": result.security_files_scanned,
            })
        })
        .collect()
}

fn is_safety_result(result: &store::ResultRecord) -> bool {
    result.security_finding_count.is_some()
        || result.security_files_scanned.is_some()
        || result.error_code.as_deref() == Some("security_findings")
        || result.benchmark_pack_id == "security-defensive"
}

struct MetricCoverageRow {
    label: &'static str,
    present: usize,
    note: &'static str,
}

fn metric_coverage_row(
    results: &[store::ResultRecord],
    label: &'static str,
    present: impl Fn(&store::ResultRecord) -> bool,
    note: &'static str,
) -> MetricCoverageRow {
    MetricCoverageRow {
        label,
        present: results.iter().filter(|result| present(result)).count(),
        note,
    }
}

fn non_empty_option(value: Option<&str>) -> bool {
    value.map(|value| !value.trim().is_empty()).unwrap_or(false)
}

fn run_group_settings_summary(config: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(replay) = config.get("replay").and_then(replay_summary) {
        parts.push(replay);
    }
    for (label, key) in [
        ("repetitions", "repetitions"),
        ("warmups", "warmup_runs"),
        ("concurrency", "concurrency"),
        ("task count", "task_count"),
        ("docker", "docker"),
    ] {
        if let Some(value) = config.get(key).and_then(simple_json_value) {
            parts.push(format!("{} {}", label, value));
        }
    }
    if let Some(task_ids) = config.get("task_ids").and_then(|value| value.as_array()) {
        let task_ids = task_ids
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>();
        if !task_ids.is_empty() {
            parts.push(format!("tasks {}", task_ids.join(", ")));
        }
    }
    if let Some(value) = config.get("max_cost_usd").and_then(|value| value.as_f64()) {
        parts.push(format!("max cost {}", format_cost(Some(value))));
    }
    if parts.is_empty() {
        "-".into()
    } else {
        parts.join("; ")
    }
}

fn replay_summary(replay: &serde_json::Value) -> Option<String> {
    let mode = replay
        .get("mode")
        .and_then(simple_json_value)
        .unwrap_or_else(|| "replay".into());
    let source_job = replay
        .get("source_job_id")
        .or_else(|| replay.get("sourceJobId"))
        .and_then(simple_json_value)
        .map(|value| short_id(&value))
        .unwrap_or_else(|| "-".into());
    let source_group = replay
        .get("source_run_group_id")
        .or_else(|| replay.get("sourceRunGroupId"))
        .and_then(simple_json_value)
        .map(|value| short_id(&value));
    let scope = match replay.get("scoped").and_then(|value| value.as_bool()) {
        Some(true) => "scoped",
        Some(false) => "full",
        None => "unknown scope",
    };
    let mut parts = vec![format!("{mode} of job {source_job} ({scope})")];
    if let Some(source_group) = source_group {
        parts.push(format!("source group {source_group}"));
    }
    if let Some(value) = replay
        .get("source_target_count")
        .or_else(|| replay.get("sourceTargetCount"))
        .and_then(simple_json_value)
    {
        parts.push(format!("source targets {value}"));
    }
    if let Some(value) = replay
        .get("source_task_count")
        .or_else(|| replay.get("sourceTaskCount"))
        .and_then(simple_json_value)
    {
        parts.push(format!("source tasks {value}"));
    }
    if let Some(value) = replay
        .get("source_repetitions")
        .or_else(|| replay.get("sourceRepetitions"))
        .and_then(simple_json_value)
    {
        parts.push(format!("source reps {value}"));
    }
    Some(parts.join(", "))
}

fn run_group_target_summary(group: &store::RunGroupRecord) -> String {
    let Some(targets) = group
        .config
        .get("targets")
        .and_then(|value| value.as_array())
    else {
        return if group.target_ids.is_empty() {
            "-".into()
        } else {
            group.target_ids.join(", ")
        };
    };
    if targets.is_empty() {
        return "-".into();
    }
    targets
        .iter()
        .map(target_snapshot_summary)
        .collect::<Vec<_>>()
        .join("; ")
}

fn target_snapshot_summary(target: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(id) = target.get("id").and_then(simple_json_value) {
        parts.push(id);
    }
    if let Some(adapter) = target.get("adapter_id").and_then(simple_json_value) {
        parts.push(format!("adapter {}", adapter));
    }
    if let Some(model) = target_model_label(target) {
        parts.push(model);
    }
    if let Some(generation) = target.get("generation") {
        let summary = generation_summary(generation);
        if !summary.is_empty() {
            parts.push(format!("gen {}", summary));
        }
    }
    if let Some(pricing) = target.get("pricing") {
        let summary = pricing_summary(pricing);
        if !summary.is_empty() {
            parts.push(format!("price {}", summary));
        }
    }
    if let Some(validation) = target.get("validation") {
        let summary = validation_summary(validation);
        if !summary.is_empty() {
            parts.push(format!("validation {}", summary));
        }
    }
    if parts.is_empty() {
        "-".into()
    } else {
        parts.join(", ")
    }
}

fn target_model_label(target: &serde_json::Value) -> Option<String> {
    if let Some(model) = target.get("model").and_then(simple_json_value) {
        return Some(format!("model {}", model));
    }
    if let Some(deployment) = target.get("deployment").and_then(simple_json_value) {
        return Some(format!("deployment {}", deployment));
    }
    let repo = target.get("repo_id").and_then(simple_json_value)?;
    let file = target.get("gguf_file").and_then(simple_json_value);
    Some(match file {
        Some(file) => format!("repo {} / {}", repo, file),
        None => format!("repo {}", repo),
    })
}

fn generation_summary(generation: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    for (label, key) in [
        ("temp", "temperature"),
        ("top_p", "top_p"),
        ("max", "max_tokens"),
        ("source", "max_tokens_source"),
        ("timeout", "timeout_seconds"),
        ("retries", "retry_count"),
        ("seed", "seed"),
    ] {
        if let Some(value) = generation.get(key).and_then(simple_json_value) {
            parts.push(format!("{} {}", label, value));
        }
    }
    parts.join(", ")
}

fn pricing_summary(pricing: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    for (label, key) in [
        ("in", "input_price_usd_per_million_tokens"),
        ("out", "output_price_usd_per_million_tokens"),
        ("cache_read", "cache_read_price_usd_per_million_tokens"),
        ("cache_write", "cache_write_price_usd_per_million_tokens"),
        ("source", "pricing_source"),
        ("verified", "pricing_verified_at"),
    ] {
        if let Some(value) = pricing.get(key).and_then(simple_json_value) {
            parts.push(format!("{} {}", label, value));
        }
    }
    parts.join(", ")
}

fn validation_summary(validation: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(status) = validation.get("status").and_then(simple_json_value) {
        parts.push(format!("status {}", status));
    }
    if let Some(detail) = validation.get("detail").and_then(simple_json_value) {
        parts.push(format!("detail {}", compact_report_error_detail(&detail)));
    }
    if let Some(checked_at) = validation.get("checked_at").and_then(simple_json_value) {
        parts.push(format!("checked {}", checked_at));
    }
    parts.join(", ")
}

fn simple_json_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn decision_row(signal: &str, target: &str, group: &str, pack: &str, why: &str) -> String {
    format!(
        "| {} | {} | {} | {} | {} |\n",
        markdown_cell(signal),
        markdown_cell(target),
        markdown_cell(&short_id(group)),
        markdown_cell(pack),
        markdown_cell(why)
    )
}

fn close_target_contenders(rows: &[ExportTargetAggregate]) -> Vec<&ExportTargetAggregate> {
    let Some(leader) = rows.first() else {
        return Vec::new();
    };
    rows.iter()
        .skip(1)
        .filter(|row| target_quality_tie(leader, row))
        .collect()
}

fn target_quality_tie(a: &ExportTargetAggregate, b: &ExportTargetAggregate) -> bool {
    float_close(export_target_pass_rate(a), export_target_pass_rate(b))
        && option_float_close(avg(&a.scores), avg(&b.scores))
}

fn close_contenders_note(contenders: &[&ExportTargetAggregate]) -> Option<String> {
    if contenders.is_empty() {
        return None;
    }
    Some(format!(
        "{} target(s) matched the recommended target's pass rate and average score; ranking fell through to score stability, latency, cost, throughput, sample size, and target id tie-breakers.",
        contenders.len()
    ))
}

fn confidence_note(
    target_rows: &[ExportTargetAggregate],
    comparison_rows: &[ExportAggregate],
    task_rows: &[ExportTaskAggregate],
) -> String {
    let mut notes = Vec::new();
    let overlap_targets = pass_rate_ci_overlap_target_ids(target_rows);
    if !overlap_targets.is_empty() {
        notes.push(format!(
            "Pass-rate confidence warning: the recommended target's Wilson 95% interval overlaps {} target(s): {}; treat the ranking as provisional and run more repetitions.",
            overlap_targets.len(),
            overlap_targets.join(", ")
        ));
    } else if target_rows.len() > 1 {
        notes.push(
            "The recommended target's Wilson 95% pass-rate interval is separated from the other visible targets."
                .into(),
        );
    }

    if !task_rows.is_empty() {
        notes.push(task_repetition_note(task_rows));
    }

    let low_sample_rows = comparison_rows.iter().filter(|row| row.runs < 3).count();
    if low_sample_rows > 0 {
        notes.push(format!(
            "{}/{} comparison row(s) have fewer than 3 measured runs; use repetitions for higher confidence.",
            low_sample_rows,
            comparison_rows.len()
        ));
    } else if !comparison_rows.is_empty() {
        notes.push("All comparison rows in this export have at least 3 measured runs.".into());
    }
    notes.join(" ")
}

fn task_repetition_note(task_rows: &[ExportTaskAggregate]) -> String {
    let low = low_repetition_task_rows(task_rows);
    if low > 0 {
        format!(
            "{}/{} task-target row(s) have fewer than {} measured repetitions for the same task and target; add repetitions to separate task breadth from repeatability.",
            low,
            task_rows.len(),
            RECOMMENDED_TASK_REPETITIONS
        )
    } else if task_rows.is_empty() {
        "No task-target repetition rows are available in this export scope.".into()
    } else {
        format!(
            "All task-target rows in this export have at least {} measured repetitions.",
            RECOMMENDED_TASK_REPETITIONS
        )
    }
}

fn low_repetition_task_rows(task_rows: &[ExportTaskAggregate]) -> usize {
    task_rows
        .iter()
        .filter(|row| row.runs < RECOMMENDED_TASK_REPETITIONS)
        .count()
}

fn comparison_evidence_assessment(
    comparison_rows: &[ExportAggregate],
    task_rows: &[ExportTaskAggregate],
    target_rows: &[ExportTargetAggregate],
) -> ComparisonEvidenceAssessment {
    if comparison_rows.is_empty() || target_rows.is_empty() {
        return ComparisonEvidenceAssessment {
            grade: "insufficient",
            label: "Insufficient evidence",
            tone: "warn",
            note: "No comparable target results are available in this export scope.".into(),
            risks: vec!["no_comparison_results".into()],
            minimum_next_run: format!(
                "Run the same non-connectivity LLM pack, such as llm-reliability, against at least one local and one cloud target with {} repetitions and 1 warmup.",
                RECOMMENDED_TASK_REPETITIONS
            ),
        };
    }

    let coverage_issues = target_coverage_issues(target_rows);
    let overlap_targets = pass_rate_ci_overlap_target_ids(target_rows);
    let cost_gap_targets = target_cost_coverage_gap_ids(target_rows);
    let pricing_assumption_targets = target_pricing_assumption_ids(target_rows);
    let model_identity_warnings = model_identity_warnings(comparison_rows);
    let generation_setting_warnings = generation_setting_warnings(comparison_rows);
    let model_identity_missing = model_identity_warnings
        .iter()
        .any(|warning| warning.issue == "provider_model_missing");
    let model_identity_inconsistent = model_identity_warnings
        .iter()
        .any(|warning| warning.issue == "provider_model_inconsistent");
    let model_identity_fallback = model_identity_warnings
        .iter()
        .any(|warning| warning.issue == "provider_model_configured_fallback");
    let low_task_rows = low_repetition_task_rows(task_rows);
    let low_comparison_rows = comparison_rows
        .iter()
        .filter(|row| row.runs < RECOMMENDED_TASK_REPETITIONS)
        .count();
    let same_run_groups = targets_cover_same_run_groups(target_rows);
    let connectivity_only = comparison_scope_is_connectivity_only(target_rows);
    let pack_evidence_issues = pack_evidence_issues_for_scope(target_rows);
    let pack_calibration_issues = pack_calibration_issues_for_scope(target_rows);
    let (_, _, all_slots) = target_coverage_unions(target_rows);

    let mut risks = Vec::new();
    if target_rows.len() < 2 {
        risks.push("single_target".into());
    }
    if connectivity_only {
        risks.push("connectivity_pack_only".into());
    }
    if !pack_evidence_issues.is_empty() {
        risks.push("pack_evidence_profile".into());
    }
    if !pack_calibration_issues.is_empty() {
        risks.push("pack_calibration".into());
    }
    if low_task_rows > 0 || low_comparison_rows > 0 {
        risks.push("low_repetitions".into());
    }
    if !coverage_issues.is_empty() {
        risks.push("coverage_gap".into());
    }
    if !same_run_groups {
        risks.push("separate_run_groups".into());
    }
    if !overlap_targets.is_empty() {
        risks.push("pass_rate_ci_overlap".into());
    }
    if !cost_gap_targets.is_empty() {
        risks.push("cost_coverage_gap".into());
    }
    if !pricing_assumption_targets.is_empty() {
        risks.push("pricing_assumption".into());
    }
    if model_identity_missing {
        risks.push("provider_model_missing".into());
    }
    if model_identity_inconsistent {
        risks.push("provider_model_inconsistent".into());
    }
    if model_identity_fallback {
        risks.push("provider_model_configured_fallback".into());
    }
    if !generation_setting_warnings.is_empty() {
        risks.push("generation_settings_mixed".into());
    }

    let scope = format!(
        "{} target(s), {} pack/task slot(s), {} task-target row(s), {} comparison row(s)",
        target_rows.len(),
        all_slots.len(),
        task_rows.len(),
        comparison_rows.len()
    );

    if target_rows.len() < 2 {
        return ComparisonEvidenceAssessment {
            grade: "insufficient",
            label: "Insufficient evidence",
            tone: "warn",
            note: format!(
                "Only one target is visible in this scope ({scope}); this can validate a target but cannot rank local vs cloud models."
            ),
            risks,
            minimum_next_run: format!(
                "Run the same non-connectivity LLM pack, such as llm-reliability, against at least one local and one cloud target with {} repetitions and 1 warmup.",
                RECOMMENDED_TASK_REPETITIONS
            ),
        };
    }

    if connectivity_only
        || !pack_evidence_issues.is_empty()
        || low_task_rows > 0
        || low_comparison_rows > 0
    {
        let mut reasons = Vec::new();
        if connectivity_only {
            reasons.push("only the connectivity pack is in scope".to_string());
        }
        if !pack_evidence_issues.is_empty() {
            reasons.push(format!(
                "pack evidence warning(s): {}",
                pack_evidence_issue_summary(&pack_evidence_issues)
            ));
        }
        if !pack_calibration_issues.is_empty() {
            reasons.push(format!(
                "pack calibration warning(s): {}",
                pack_calibration_issue_summary(&pack_calibration_issues)
            ));
        }
        if low_task_rows > 0 {
            reasons.push(format!(
                "{low_task_rows}/{} task-target row(s) have fewer than {} repetitions",
                task_rows.len(),
                RECOMMENDED_TASK_REPETITIONS
            ));
        }
        if low_comparison_rows > 0 {
            reasons.push(format!(
                "{low_comparison_rows}/{} comparison row(s) have fewer than {} measured runs",
                comparison_rows.len(),
                RECOMMENDED_TASK_REPETITIONS
            ));
        }
        if !cost_gap_targets.is_empty() {
            reasons.push(format!(
                "{} target(s) are missing cost metrics: {}",
                cost_gap_targets.len(),
                cost_gap_targets.join(", ")
            ));
        }
        if !pricing_assumption_targets.is_empty() {
            reasons.push(format!(
                "{} target(s) have pricing assumptions: {}",
                pricing_assumption_targets.len(),
                pricing_assumption_targets.join(", ")
            ));
        }
        if !model_identity_warnings.is_empty() {
            reasons.push(format!(
                "{} comparison aggregate(s) have missing, fallback, or inconsistent served model ids",
                model_identity_warnings.len()
            ));
        }
        if !generation_setting_warnings.is_empty() {
            reasons.push(format!(
                "{} generation setting warning(s) indicate mixed deterministic/exploratory sampling",
                generation_setting_warnings.len()
            ));
        }
        let minimum_next_run = if !cost_gap_targets.is_empty() {
            format!(
                "Add pricing for targets with missing cost metrics ({}), then run a quality pack such as llm-reliability or llm-decision-suite against the same targets with at least {} repetitions per task/target and 1 warmup.",
                cost_gap_targets.join(", "),
                RECOMMENDED_TASK_REPETITIONS
            )
        } else if !pricing_assumption_targets.is_empty() {
            format!(
                "Add cache read/write pricing for targets with pricing assumptions ({}), then rerun or re-export before using cost rankings as decisive evidence.",
                pricing_assumption_targets.join(", ")
            )
        } else if !model_identity_warnings.is_empty() {
            format!(
                "Confirm each target reports a stable provider-supplied served model id, split mixed served-model results into separate targets if needed, then run a quality pack such as llm-reliability or llm-decision-suite against the same targets with at least {} repetitions per task/target and 1 warmup.",
                RECOMMENDED_TASK_REPETITIONS
            )
        } else if !generation_setting_warnings.is_empty() {
            format!(
                "Rerun or filter the same targets and pack with one shared generation policy, such as temperature 0, top_p 1, and a consistent seed policy, with at least {} repetitions per task/target and 1 warmup.",
                RECOMMENDED_TASK_REPETITIONS
            )
        } else if !pack_evidence_issues.is_empty() {
            format!(
                "Run a prompt_comparison pack such as llm-reliability or llm-decision-suite against the same targets with at least {} repetitions per task/target and 1 warmup, or strengthen the private pack scoring and task breadth.",
                RECOMMENDED_TASK_REPETITIONS
            )
        } else if !pack_calibration_issues.is_empty() {
            format!(
                "Calibrate or review the benchmark pack with baseline evidence, then rerun or filter the same targets with at least {} repetitions per task/target and 1 warmup before selecting a winner.",
                RECOMMENDED_TASK_REPETITIONS
            )
        } else {
            format!(
                "Run a quality pack such as llm-reliability or llm-decision-suite against the same targets with at least {} repetitions per task/target and 1 warmup.",
                RECOMMENDED_TASK_REPETITIONS
            )
        };
        return ComparisonEvidenceAssessment {
            grade: "smoke",
            label: "Smoke evidence",
            tone: "warn",
            note: format!(
                "Smoke evidence: visible results prove the setup can run, but they are too shallow for model selection ({scope}; {}).",
                reasons.join("; ")
            ),
            risks,
            minimum_next_run,
        };
    }

    if !coverage_issues.is_empty()
        || !same_run_groups
        || !overlap_targets.is_empty()
        || !cost_gap_targets.is_empty()
        || !pricing_assumption_targets.is_empty()
        || !model_identity_warnings.is_empty()
        || !generation_setting_warnings.is_empty()
        || !pack_calibration_issues.is_empty()
    {
        let mut reasons = Vec::new();
        if !coverage_issues.is_empty() {
            reasons.push(format!(
                "{} target(s) are missing visible pack/task slots",
                coverage_issues.len()
            ));
        }
        if !same_run_groups {
            reasons.push("targets were not compared in the same run groups".into());
        }
        if !overlap_targets.is_empty() {
            reasons.push(format!(
                "the leader's Wilson 95% pass-rate interval overlaps {} target(s)",
                overlap_targets.len()
            ));
        }
        if !cost_gap_targets.is_empty() {
            reasons.push(format!(
                "{} target(s) are missing cost metrics: {}",
                cost_gap_targets.len(),
                cost_gap_targets.join(", ")
            ));
        }
        if !pricing_assumption_targets.is_empty() {
            reasons.push(format!(
                "{} target(s) have pricing assumptions: {}",
                pricing_assumption_targets.len(),
                pricing_assumption_targets.join(", ")
            ));
        }
        if !model_identity_warnings.is_empty() {
            reasons.push(format!(
                "{} comparison aggregate(s) have missing, fallback, or inconsistent served model ids",
                model_identity_warnings.len()
            ));
        }
        if !generation_setting_warnings.is_empty() {
            reasons.push(format!(
                "{} generation setting warning(s) indicate mixed deterministic/exploratory sampling",
                generation_setting_warnings.len()
            ));
        }
        if !pack_calibration_issues.is_empty() {
            reasons.push(format!(
                "pack calibration warning(s): {}",
                pack_calibration_issue_summary(&pack_calibration_issues)
            ));
        }
        let minimum_next_run = if !cost_gap_targets.is_empty() {
            "Add pricing for targets with missing cost metrics, then re-run the same targets and pack so cost can be compared beside quality and latency.".into()
        } else if !pricing_assumption_targets.is_empty() {
            "Add cache read/write pricing for targets with pricing assumptions, then re-run or re-export before treating cost ranking as decisive.".into()
        } else if !coverage_issues.is_empty() {
            format!(
                "Run the missing pack/task slots for every target with at least {} repetitions per task/target.",
                RECOMMENDED_TASK_REPETITIONS
            )
        } else if !same_run_groups {
            format!(
                "Re-run all compared targets together on the same pack with at least {} repetitions per task/target.",
                RECOMMENDED_TASK_REPETITIONS
            )
        } else if !model_identity_warnings.is_empty() {
            "Confirm each target reports a stable provider-supplied served model id, split mixed served-model results into separate targets if needed, then re-run the same targets and pack.".into()
        } else if !generation_setting_warnings.is_empty() {
            "Rerun or filter the same targets and pack with one shared generation policy, such as temperature 0, top_p 1, and a consistent seed policy.".into()
        } else if !pack_calibration_issues.is_empty() {
            "Calibrate or review the benchmark pack with documented baseline runs before treating this ranking as a model-selection decision.".into()
        } else {
            "Increase repetitions or add more discriminating tasks until the leader's Wilson interval separates from contenders.".into()
        };
        return ComparisonEvidenceAssessment {
            grade: "directional",
            label: "Directional evidence",
            tone: "warn",
            note: format!(
                "Directional evidence: sample depth is usable, but the ranking is not yet decisive because {} ({scope}).",
                reasons.join("; ")
            ),
            risks,
            minimum_next_run,
        };
    }

    ComparisonEvidenceAssessment {
        grade: "comparison_ready",
        label: "Comparison-ready",
        tone: "ok",
        note: format!(
            "Comparison-ready evidence: targets share pack/task coverage, run groups, cost coverage, stable served-model identity, calibrated pack metadata, and one generation policy; every task-target row has at least {} repetitions, and the leader's Wilson interval is separated ({scope}).",
            RECOMMENDED_TASK_REPETITIONS
        ),
        risks,
        minimum_next_run: "No immediate rerun is required for a first-pass comparison; add domain-specific packs before final production selection.".into(),
    }
}

fn evidence_decision_status(evidence: &ComparisonEvidenceAssessment) -> &'static str {
    match evidence.grade {
        "comparison_ready" => "select_recommended_target",
        "insufficient" => "insufficient_evidence",
        _ => "collect_more_evidence",
    }
}

fn evidence_selected_target_id(
    evidence: &ComparisonEvidenceAssessment,
    target_rows: &[ExportTargetAggregate],
) -> Option<String> {
    (evidence.grade == "comparison_ready")
        .then(|| target_rows.first().map(|row| row.target_id.clone()))
        .flatten()
}

fn evidence_selection_note(
    evidence: &ComparisonEvidenceAssessment,
    target_rows: &[ExportTargetAggregate],
) -> String {
    match evidence.grade {
        "comparison_ready" => {
            let target_id = target_rows
                .first()
                .map(|row| row.target_id.as_str())
                .unwrap_or("the recommended target");
            format!(
                "Evidence is comparison-ready; select {target_id} for this result scope unless external domain constraints override it."
            )
        }
        "insufficient" => format!("Do not select a winner yet; {}", evidence.minimum_next_run),
        _ => format!(
            "Collect more evidence before choosing a winner; {}",
            evidence.minimum_next_run
        ),
    }
}

fn pass_rate_ci_overlap_target_ids(rows: &[ExportTargetAggregate]) -> Vec<String> {
    let Some(leader) = rows.first() else {
        return Vec::new();
    };
    let Some(leader_interval) = pass_rate_interval(leader.passed, leader.runs) else {
        return Vec::new();
    };
    rows.iter()
        .skip(1)
        .filter_map(|row| {
            pass_rate_interval(row.passed, row.runs)
                .filter(|interval| intervals_overlap(leader_interval, *interval))
                .map(|_| row.target_id.clone())
        })
        .collect()
}

fn intervals_overlap(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 <= b.1 && b.0 <= a.1
}

fn score_stability_note(rows: &[ExportTargetAggregate]) -> String {
    let measured = rows
        .iter()
        .filter_map(|row| std_dev(&row.scores).map(|spread| (row, spread)))
        .collect::<Vec<_>>();
    if measured.is_empty() {
        return "Run at least 2 scored repetitions per target to measure score stability.".into();
    }
    let (target, spread) = measured
        .iter()
        .max_by(|(_, left), (_, right)| {
            left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .expect("measured score spread is not empty");
    if float_close(spread, 0.0) {
        return format!(
            "All {} target(s) with at least 2 scored runs have score sigma 0 in the current export scope.",
            measured.len()
        );
    }
    format!(
        "Max target score spread is sigma {} on {}; lower spread means more consistent scores across the current export scope.",
        format_number(Some(spread)),
        target.target_id
    )
}

fn target_coverage_parity_note(rows: &[ExportTargetAggregate]) -> String {
    if rows.len() < 2 {
        return "Only one target is visible; add another target to compare coverage.".into();
    }
    let mut gaps = target_coverage_issues(rows);
    if !gaps.is_empty() {
        gaps.sort_by(|a, b| {
            (b.missing_pack_task_slots.len() + b.missing_packs.len() + b.missing_tasks.len())
                .cmp(
                    &(a.missing_pack_task_slots.len()
                        + a.missing_packs.len()
                        + a.missing_tasks.len()),
                )
                .then_with(|| a.target_id.cmp(&b.target_id))
        });
        let worst = &gaps[0];
        return format!(
            "Coverage warning: {}/{} target(s) do not cover every pack/task slot in this scope; largest gap is {} missing {} pack/task slot(s), {} task(s), and {} pack(s).",
            gaps.len(),
            rows.len(),
            worst.target_id,
            worst.missing_pack_task_slots.len(),
            worst.missing_tasks.len(),
            worst.missing_packs.len()
        );
    }
    if !targets_cover_same_run_groups(rows) {
        return "Coverage note: targets cover the same pack/task slots but not the same run groups; same-run comparisons are more controlled.".into();
    }
    "All targets cover the same pack/task slots and run groups in this export scope.".into()
}

fn target_coverage_issues_json(rows: &[ExportTargetAggregate]) -> Vec<serde_json::Value> {
    target_coverage_issues(rows)
        .into_iter()
        .map(|issue| {
            serde_json::json!({
                "target_id": issue.target_id,
                "missing_pack_task_slot_count": issue.missing_pack_task_slots.len(),
                "missing_benchmark_pack_count": issue.missing_packs.len(),
                "missing_task_count": issue.missing_tasks.len(),
                "missing_pack_task_slot_ids": issue.missing_pack_task_slots,
                "missing_benchmark_pack_ids": issue.missing_packs,
                "missing_task_ids": issue.missing_tasks,
            })
        })
        .collect()
}

fn target_cost_coverage_gap_ids(rows: &[ExportTargetAggregate]) -> Vec<String> {
    let mut ids: Vec<String> = rows
        .iter()
        .filter(|row| row.runs > 0 && row.costed < row.runs)
        .map(|row| row.target_id.clone())
        .collect();
    ids.sort();
    ids
}

fn target_pricing_assumption_ids(rows: &[ExportTargetAggregate]) -> Vec<String> {
    let mut ids: Vec<String> = rows
        .iter()
        .filter(|row| !row.pricing_assumption_counts.is_empty())
        .map(|row| row.target_id.clone())
        .collect();
    ids.sort();
    ids
}

struct TargetCoverageIssue {
    target_id: String,
    missing_pack_task_slots: Vec<String>,
    missing_packs: Vec<String>,
    missing_tasks: Vec<String>,
}

struct PackEvidenceIssue {
    pack_id: String,
    evidence_profile: String,
    warnings: Vec<String>,
}

#[derive(Clone)]
struct PackCalibrationMetadata {
    pack_id: String,
    status: String,
    sample_size: Option<u64>,
    baseline_models: Vec<String>,
    last_reviewed: Option<String>,
    quality_gates: Vec<String>,
    notes: Option<String>,
}

struct PackCalibrationIssue {
    pack_id: String,
    statuses: Vec<String>,
    sample_sizes: Vec<u64>,
    baseline_models: Vec<String>,
    last_reviewed: Vec<String>,
    quality_gates: Vec<String>,
    missing_quality_gates: Vec<String>,
    notes: Vec<String>,
}

struct ComparisonEvidenceAssessment {
    grade: &'static str,
    label: &'static str,
    tone: &'static str,
    note: String,
    risks: Vec<String>,
    minimum_next_run: String,
}

struct RecommendedNextRun {
    target_ids: Vec<String>,
    benchmark_pack_id: String,
    benchmark_pack_ids: Vec<String>,
    task_ids: Vec<String>,
    repetitions: usize,
    warmup_runs: usize,
    concurrency: usize,
    max_cost_usd: f64,
    reason: String,
    note: String,
}

fn recommended_next_run(
    evidence: &ComparisonEvidenceAssessment,
    target_rows: &[ExportTargetAggregate],
) -> Option<RecommendedNextRun> {
    if evidence.grade == "comparison_ready" || target_rows.len() < 2 {
        return None;
    }
    let coverage_follow_up = coverage_task_follow_up(target_rows);
    let target_ids = coverage_follow_up
        .as_ref()
        .map(|(_, _, target_ids)| target_ids.clone())
        .unwrap_or_else(|| {
            target_rows
                .iter()
                .map(|row| row.target_id.clone())
                .filter(|id| !id.trim().is_empty() && id != "-")
                .collect::<Vec<_>>()
        });
    if target_ids.is_empty() || (coverage_follow_up.is_none() && target_ids.len() < 2) {
        return None;
    }
    let (benchmark_pack_id, benchmark_pack_ids, task_ids) =
        if let Some((pack_id, task_ids, _)) = coverage_follow_up {
            (pack_id.clone(), vec![pack_id], task_ids)
        } else {
            let benchmark_pack_ids = recommended_next_run_pack_ids(target_rows);
            let benchmark_pack_id = benchmark_pack_ids
                .first()
                .cloned()
                .unwrap_or_else(|| "llm-reliability".into());
            (benchmark_pack_id, benchmark_pack_ids, Vec::new())
        };
    let concurrency = target_ids.len().clamp(1, 2);
    Some(RecommendedNextRun {
        target_ids,
        benchmark_pack_id,
        benchmark_pack_ids,
        task_ids,
        repetitions: RECOMMENDED_TASK_REPETITIONS,
        warmup_runs: 1,
        concurrency,
        max_cost_usd: DEFAULT_COMPARISON_MAX_COST_USD,
        reason: evidence.minimum_next_run.clone(),
        note: "Confirm these targets still exist and validate successfully before queueing; exported reports cannot verify current target availability.".into(),
    })
}

fn coverage_task_follow_up(
    target_rows: &[ExportTargetAggregate],
) -> Option<(String, Vec<String>, Vec<String>)> {
    let coverage_issues = target_coverage_issues(target_rows);
    if coverage_issues.is_empty() {
        return None;
    }
    let mut pack_ids = BTreeSet::new();
    let mut task_ids = BTreeSet::new();
    let mut target_ids = BTreeSet::new();
    for issue in coverage_issues {
        let mut has_missing_slot = false;
        for slot in issue.missing_pack_task_slots {
            let (pack_id, task_id) = slot.split_once('/')?;
            if pack_id.trim().is_empty() || task_id.trim().is_empty() {
                return None;
            }
            pack_ids.insert(pack_id.to_string());
            task_ids.insert(task_id.to_string());
            has_missing_slot = true;
        }
        if has_missing_slot && !issue.target_id.trim().is_empty() && issue.target_id != "-" {
            target_ids.insert(issue.target_id);
        }
    }
    if pack_ids.len() != 1 || task_ids.is_empty() || target_ids.is_empty() {
        return None;
    }
    Some((
        pack_ids.into_iter().next()?,
        task_ids.into_iter().collect(),
        target_ids.into_iter().collect(),
    ))
}

fn recommended_next_run_pack_ids(target_rows: &[ExportTargetAggregate]) -> Vec<String> {
    let pack_profiles = runner::list_benchmark_packs()
        .unwrap_or_default()
        .into_iter()
        .map(|pack| (pack.id.clone(), pack.evidence_profile))
        .collect::<BTreeMap<_, _>>();
    let mut pack_ids = target_rows
        .iter()
        .flat_map(|row| row.pack_ids.iter().cloned())
        .filter(|id| !id.trim().is_empty() && id != "-" && id != "llm-connectivity")
        .filter(|id| {
            let profile = target_rows
                .iter()
                .find_map(|row| row.pack_evidence_profiles.get(id))
                .or_else(|| pack_profiles.get(id));
            profile
                .map(|profile| prompt_evidence_profile_is_comparison_ready(profile))
                .unwrap_or(true)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if pack_ids.is_empty() {
        pack_ids.push("llm-reliability".into());
    }
    pack_ids
}

fn recommended_next_run_json(next_run: &RecommendedNextRun) -> serde_json::Value {
    serde_json::json!({
        "target_ids": &next_run.target_ids,
        "benchmark_pack_id": &next_run.benchmark_pack_id,
        "benchmark_pack_ids": &next_run.benchmark_pack_ids,
        "task_ids": &next_run.task_ids,
        "repetitions": next_run.repetitions,
        "warmup_runs": next_run.warmup_runs,
        "concurrency": next_run.concurrency,
        "max_cost_usd": next_run.max_cost_usd,
        "reason": &next_run.reason,
        "note": &next_run.note,
    })
}

fn pack_evidence_issues_for_scope(rows: &[ExportTargetAggregate]) -> Vec<PackEvidenceIssue> {
    let pack_ids = rows
        .iter()
        .flat_map(|row| row.pack_ids.iter().cloned())
        .filter(|id| !id.trim().is_empty() && id != "-")
        .collect::<BTreeSet<_>>();
    if pack_ids.is_empty() {
        return Vec::new();
    }
    let mut issues = BTreeMap::<String, PackEvidenceIssue>::new();
    let mut snapshot_pack_ids = BTreeSet::new();
    for row in rows {
        for (pack_id, evidence_profile) in &row.pack_evidence_profiles {
            if pack_id.trim().is_empty() || pack_id == "-" {
                continue;
            }
            snapshot_pack_ids.insert(pack_id.clone());
            if !prompt_evidence_profile_is_comparison_ready(evidence_profile)
                && prompt_evidence_profile_is_prompt_like(evidence_profile)
            {
                let warnings = row
                    .pack_evidence_warnings
                    .get(pack_id)
                    .map(|warnings| warnings.iter().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                issues.insert(
                    pack_id.clone(),
                    PackEvidenceIssue {
                        pack_id: pack_id.clone(),
                        evidence_profile: evidence_profile.clone(),
                        warnings,
                    },
                );
            }
        }
    }
    let packs = runner::list_benchmark_packs()
        .unwrap_or_default()
        .into_iter()
        .map(|pack| (pack.id.clone(), pack))
        .collect::<BTreeMap<_, _>>();
    pack_ids
        .into_iter()
        .filter(|pack_id| !snapshot_pack_ids.contains(pack_id))
        .filter_map(|pack_id| {
            let pack = packs.get(&pack_id)?;
            let prompt_pack = pack
                .task_types
                .iter()
                .any(|task_type| task_type == "prompt");
            (prompt_pack && pack.evidence_profile != "prompt_comparison").then(|| {
                PackEvidenceIssue {
                    pack_id,
                    evidence_profile: pack.evidence_profile.clone(),
                    warnings: pack.evidence_warnings.clone(),
                }
            })
        })
        .for_each(|issue| {
            issues.insert(issue.pack_id.clone(), issue);
        });
    issues.into_values().collect()
}

fn pack_calibration_issues_for_scope(rows: &[ExportTargetAggregate]) -> Vec<PackCalibrationIssue> {
    let pack_ids = rows
        .iter()
        .flat_map(|row| row.pack_ids.iter().cloned())
        .filter(|id| !id.trim().is_empty() && id != "-")
        .collect::<BTreeSet<_>>();
    if pack_ids.is_empty() {
        return Vec::new();
    }

    let mut issues = BTreeMap::<String, PackCalibrationIssue>::new();
    let mut snapshot_pack_ids = BTreeSet::new();
    for row in rows {
        for (pack_id, statuses) in &row.pack_calibration_statuses {
            if pack_id.trim().is_empty() || pack_id == "-" {
                continue;
            }
            snapshot_pack_ids.insert(pack_id.clone());
            let normalized_statuses = statuses
                .iter()
                .map(|status| normalized_pack_calibration_status(status))
                .collect::<BTreeSet<_>>();
            let missing_quality_gates =
                missing_required_calibration_quality_gates_for_row(row, pack_id);
            if normalized_statuses
                .iter()
                .all(|status| pack_calibration_status_is_definitive(status))
                && missing_quality_gates.is_empty()
            {
                continue;
            }
            issues
                .entry(pack_id.clone())
                .or_insert_with(|| PackCalibrationIssue {
                    pack_id: pack_id.clone(),
                    statuses: Vec::new(),
                    sample_sizes: Vec::new(),
                    baseline_models: Vec::new(),
                    last_reviewed: Vec::new(),
                    quality_gates: Vec::new(),
                    missing_quality_gates: Vec::new(),
                    notes: Vec::new(),
                })
                .merge_from_row(row, pack_id, normalized_statuses, missing_quality_gates);
        }
    }

    let packs = runner::list_benchmark_packs()
        .unwrap_or_default()
        .into_iter()
        .map(|pack| (pack.id.clone(), pack))
        .collect::<BTreeMap<_, _>>();
    for pack_id in pack_ids {
        if snapshot_pack_ids.contains(&pack_id) {
            continue;
        }
        if let Some(pack) = packs.get(&pack_id) {
            let status = normalized_pack_calibration_status(&pack.calibration_status);
            let missing_quality_gates =
                if prompt_evidence_profile_is_comparison_ready(&pack.evidence_profile) {
                    missing_required_calibration_quality_gates(
                        pack.calibration_quality_gates.iter(),
                    )
                } else {
                    Vec::new()
                };
            if pack_calibration_status_is_definitive(&status) && missing_quality_gates.is_empty() {
                continue;
            }
            issues.insert(
                pack_id.clone(),
                PackCalibrationIssue {
                    pack_id,
                    statuses: vec![status],
                    sample_sizes: pack.calibration_sample_size.into_iter().collect(),
                    baseline_models: pack.calibration_baseline_models.clone(),
                    last_reviewed: pack.calibration_last_reviewed.clone().into_iter().collect(),
                    quality_gates: pack.calibration_quality_gates.clone(),
                    missing_quality_gates,
                    notes: pack.calibration_notes.clone().into_iter().collect(),
                },
            );
        } else {
            issues.insert(
                pack_id.clone(),
                PackCalibrationIssue {
                    pack_id,
                    statuses: vec!["missing".into()],
                    sample_sizes: Vec::new(),
                    baseline_models: Vec::new(),
                    last_reviewed: Vec::new(),
                    quality_gates: Vec::new(),
                    missing_quality_gates: Vec::new(),
                    notes: vec!["No calibration metadata was stored with these result rows and the pack is not available in current pack roots.".into()],
                },
            );
        }
    }

    issues.into_values().collect()
}

impl PackCalibrationIssue {
    fn merge_from_row(
        &mut self,
        row: &ExportTargetAggregate,
        pack_id: &str,
        statuses: BTreeSet<String>,
        missing_quality_gates: Vec<String>,
    ) {
        append_unique_strings(&mut self.statuses, statuses);
        if let Some(sample_sizes) = row.pack_calibration_sample_sizes.get(pack_id) {
            for sample_size in sample_sizes {
                if !self.sample_sizes.contains(sample_size) {
                    self.sample_sizes.push(*sample_size);
                }
            }
            self.sample_sizes.sort_unstable();
        }
        if let Some(models) = row.pack_calibration_baseline_models.get(pack_id) {
            append_unique_strings(&mut self.baseline_models, models.iter().cloned());
        }
        if let Some(reviewed) = row.pack_calibration_last_reviewed.get(pack_id) {
            append_unique_strings(&mut self.last_reviewed, reviewed.iter().cloned());
        }
        if let Some(quality_gates) = row.pack_calibration_quality_gates.get(pack_id) {
            append_unique_strings(&mut self.quality_gates, quality_gates.iter().cloned());
        }
        append_unique_strings(&mut self.missing_quality_gates, missing_quality_gates);
        if let Some(notes) = row.pack_calibration_notes.get(pack_id) {
            append_unique_strings(&mut self.notes, notes.iter().cloned());
        }
    }
}

fn result_pack_evidence_metadata(
    result: &store::ResultRecord,
) -> Option<(String, String, Vec<String>)> {
    let pack = result.reproducibility.get("benchmark_pack")?;
    let pack_id = pack
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or(&result.benchmark_pack_id)
        .trim();
    if pack_id.is_empty() || pack_id == "-" {
        return None;
    }
    let evidence_profile = pack
        .get("evidence_profile")
        .or_else(|| pack.get("evidenceProfile"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|profile| !profile.is_empty())?;
    let evidence_warnings = pack
        .get("evidence_warnings")
        .or_else(|| pack.get("evidenceWarnings"))
        .and_then(|value| value.as_array())
        .map(|warnings| {
            warnings
                .iter()
                .filter_map(|warning| warning.as_str())
                .map(str::trim)
                .filter(|warning| !warning.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some((
        pack_id.to_string(),
        evidence_profile.to_string(),
        evidence_warnings,
    ))
}

fn result_pack_calibration_metadata(
    result: &store::ResultRecord,
) -> Option<PackCalibrationMetadata> {
    let pack = result.reproducibility.get("benchmark_pack")?;
    let pack_id = pack
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or(&result.benchmark_pack_id)
        .trim();
    if pack_id.is_empty() || pack_id == "-" {
        return None;
    }
    let calibration = pack.get("calibration")?;
    let status = calibration
        .get("status")
        .and_then(|value| value.as_str())
        .map(normalized_pack_calibration_status)
        .unwrap_or_else(|| "missing".into());
    let sample_size = calibration
        .get("sample_size")
        .or_else(|| calibration.get("sampleSize"))
        .and_then(|value| value.as_u64());
    let baseline_models = calibration
        .get("baseline_models")
        .or_else(|| calibration.get("baselineModels"))
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let last_reviewed = calibration
        .get("last_reviewed")
        .or_else(|| calibration.get("lastReviewed"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let notes = calibration
        .get("notes")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let quality_gates = calibration
        .get("quality_gates")
        .or_else(|| calibration.get("qualityGates"))
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(PackCalibrationMetadata {
        pack_id: pack_id.to_string(),
        status,
        sample_size,
        baseline_models,
        last_reviewed,
        quality_gates,
        notes,
    })
}

fn normalized_pack_calibration_status(status: &str) -> String {
    let normalized = status.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match normalized.as_str() {
        "calibrated" | "reviewed" | "pilot" | "uncalibrated" | "missing" => normalized,
        "" => "missing".into(),
        _ => "custom".into(),
    }
}

fn pack_calibration_status_is_definitive(status: &str) -> bool {
    status == "calibrated"
}

const REQUIRED_CALIBRATION_QUALITY_GATES: [&str; 6] = [
    "local_cloud_baseline_pair",
    "provider_confirmed_model_identity",
    "complete_pack_task_coverage",
    "min_3_repetitions_per_task_target",
    "cost_metrics_for_cloud_targets",
    "single_generation_policy",
];

fn missing_required_calibration_quality_gates<'a>(
    quality_gates: impl IntoIterator<Item = &'a String>,
) -> Vec<String> {
    let present = quality_gates
        .into_iter()
        .map(|gate| gate.trim().to_string())
        .filter(|gate| !gate.is_empty())
        .collect::<BTreeSet<_>>();
    REQUIRED_CALIBRATION_QUALITY_GATES
        .iter()
        .filter(|gate| !present.iter().any(|present_gate| present_gate == *gate))
        .map(|gate| (*gate).to_string())
        .collect()
}

fn missing_required_calibration_quality_gates_for_row(
    row: &ExportTargetAggregate,
    pack_id: &str,
) -> Vec<String> {
    let requires_gates = row
        .pack_evidence_profiles
        .get(pack_id)
        .is_some_and(|profile| prompt_evidence_profile_is_comparison_ready(profile));
    if !requires_gates {
        return Vec::new();
    }
    row.pack_calibration_quality_gates
        .get(pack_id)
        .map(|gates| missing_required_calibration_quality_gates(gates.iter()))
        .unwrap_or_else(|| {
            REQUIRED_CALIBRATION_QUALITY_GATES
                .iter()
                .map(|gate| (*gate).to_string())
                .collect()
        })
}

fn prompt_evidence_profile_is_prompt_like(profile: &str) -> bool {
    matches!(
        profile,
        "connectivity_smoke"
            | "prompt_smoke"
            | "weak_prompt_suite"
            | "thin_prompt_suite"
            | "prompt_comparison"
    )
}

fn prompt_evidence_profile_is_comparison_ready(profile: &str) -> bool {
    profile == "prompt_comparison"
}

fn pack_evidence_issues_json(issues: &[PackEvidenceIssue]) -> Vec<serde_json::Value> {
    issues
        .iter()
        .map(|issue| {
            serde_json::json!({
                "benchmark_pack_id": &issue.pack_id,
                "evidence_profile": &issue.evidence_profile,
                "warnings": &issue.warnings,
            })
        })
        .collect()
}

fn pack_evidence_issue_summary(issues: &[PackEvidenceIssue]) -> String {
    issues
        .iter()
        .map(|issue| {
            let warning = issue
                .warnings
                .first()
                .map(|warning| format!(": {warning}"))
                .unwrap_or_default();
            format!("{} is {}{}", issue.pack_id, issue.evidence_profile, warning)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn pack_calibration_issues_json(issues: &[PackCalibrationIssue]) -> Vec<serde_json::Value> {
    issues
        .iter()
        .map(|issue| {
            serde_json::json!({
                "benchmark_pack_id": &issue.pack_id,
                "statuses": &issue.statuses,
                "sample_sizes": &issue.sample_sizes,
                "baseline_models": &issue.baseline_models,
                "last_reviewed": &issue.last_reviewed,
                "quality_gates": &issue.quality_gates,
                "missing_quality_gates": &issue.missing_quality_gates,
                "notes": &issue.notes,
            })
        })
        .collect()
}

fn pack_calibration_note(issues: &[PackCalibrationIssue]) -> String {
    if issues.is_empty() {
        return "All visible benchmark packs are marked calibrated in the available run snapshots or current pack metadata.".into();
    }
    format!(
        "Pack calibration warning: {} pack(s) are not fully calibrated for model selection. BenchForge keeps these rankings directional and will not select a winner as comparison-ready until calibration is documented: {}.",
        issues.len(),
        pack_calibration_issue_summary(issues)
    )
}

fn pack_calibration_issue_summary(issues: &[PackCalibrationIssue]) -> String {
    issues
        .iter()
        .map(|issue| {
            let statuses = if issue.statuses.is_empty() {
                "missing".into()
            } else {
                issue.statuses.join("/")
            };
            let review = issue
                .last_reviewed
                .first()
                .map(|value| format!(", reviewed {value}"))
                .unwrap_or_default();
            let sample = if issue.sample_sizes.is_empty() {
                String::new()
            } else {
                format!(
                    ", sample size {}",
                    issue
                        .sample_sizes
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join("/")
                )
            };
            let missing_gates = if issue.missing_quality_gates.is_empty() {
                String::new()
            } else {
                format!(
                    ", missing gate(s) {}",
                    issue.missing_quality_gates.join("/")
                )
            };
            format!(
                "{} is {}{}{}{}",
                issue.pack_id, statuses, sample, review, missing_gates
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn target_coverage_issues(rows: &[ExportTargetAggregate]) -> Vec<TargetCoverageIssue> {
    if rows.len() < 2 {
        return Vec::new();
    }
    let (all_packs, all_tasks, all_slots) = target_coverage_unions(rows);
    rows.iter()
        .filter_map(|row| {
            let missing_packs = set_difference_strings(&all_packs, &row.pack_ids);
            let missing_tasks = set_difference_strings(&all_tasks, &row.task_ids);
            let missing_pack_task_slots = set_difference_strings(&all_slots, &row.pack_task_slots);
            (!missing_pack_task_slots.is_empty()
                || !missing_packs.is_empty()
                || !missing_tasks.is_empty())
            .then(|| TargetCoverageIssue {
                target_id: row.target_id.clone(),
                missing_pack_task_slots,
                missing_packs,
                missing_tasks,
            })
        })
        .collect()
}

fn target_coverage_unions(
    rows: &[ExportTargetAggregate],
) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let mut packs = BTreeSet::new();
    let mut tasks = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for row in rows {
        packs.extend(row.pack_ids.iter().cloned());
        tasks.extend(row.task_ids.iter().cloned());
        slots.extend(row.pack_task_slots.iter().cloned());
    }
    (packs, tasks, slots)
}

fn targets_cover_same_run_groups(rows: &[ExportTargetAggregate]) -> bool {
    let Some(first) = rows.first() else {
        return false;
    };
    rows.iter().all(|row| row.group_ids == first.group_ids)
}

fn comparison_scope_is_connectivity_only(rows: &[ExportTargetAggregate]) -> bool {
    let pack_ids = rows
        .iter()
        .flat_map(|row| row.pack_ids.iter())
        .collect::<BTreeSet<_>>();
    !pack_ids.is_empty()
        && pack_ids
            .iter()
            .all(|pack_id| pack_id.as_str() == "llm-connectivity")
}

fn set_difference_strings(all: &BTreeSet<String>, present: &BTreeSet<String>) -> Vec<String> {
    all.difference(present).cloned().collect()
}

fn pack_task_slot_id(pack_id: &str, task_id: &str) -> String {
    format!("{pack_id}/{task_id}")
}

fn compare_export_target_rows(
    a: &ExportTargetAggregate,
    b: &ExportTargetAggregate,
) -> std::cmp::Ordering {
    compare_optional_f64_desc(
        export_weighted_pass_rate(a.weighted_passed, a.total_weight),
        export_weighted_pass_rate(b.weighted_passed, b.total_weight),
    )
    .then_with(|| {
        compare_optional_f64_desc(
            Some(export_target_pass_rate(a)),
            Some(export_target_pass_rate(b)),
        )
    })
    .then_with(|| {
        compare_optional_f64_desc(
            export_weighted_average_score(a.weighted_score_sum, a.scored_weight),
            export_weighted_average_score(b.weighted_score_sum, b.scored_weight),
        )
    })
    .then_with(|| compare_optional_f64_desc(avg(&a.scores), avg(&b.scores)))
    .then_with(|| compare_optional_f64_asc(std_dev(&a.scores), std_dev(&b.scores)))
    .then_with(|| {
        compare_optional_f64_asc(
            percentile(&a.wall_times, 0.95),
            percentile(&b.wall_times, 0.95),
        )
    })
    .then_with(|| compare_optional_f64_asc(export_target_avg_cost(a), export_target_avg_cost(b)))
    .then_with(|| compare_optional_f64_desc(avg(&a.throughputs), avg(&b.throughputs)))
    .then_with(|| b.runs.cmp(&a.runs))
    .then_with(|| a.target_id.cmp(&b.target_id))
}

fn compare_export_decision_rows(a: &ExportAggregate, b: &ExportAggregate) -> std::cmp::Ordering {
    compare_optional_f64_desc(
        export_weighted_pass_rate(a.weighted_passed, a.total_weight),
        export_weighted_pass_rate(b.weighted_passed, b.total_weight),
    )
    .then_with(|| compare_optional_f64_desc(Some(export_pass_rate(a)), Some(export_pass_rate(b))))
    .then_with(|| {
        compare_optional_f64_desc(
            export_weighted_average_score(a.weighted_score_sum, a.scored_weight),
            export_weighted_average_score(b.weighted_score_sum, b.scored_weight),
        )
    })
    .then_with(|| compare_optional_f64_desc(avg(&a.scores), avg(&b.scores)))
    .then_with(|| compare_optional_f64_asc(std_dev(&a.scores), std_dev(&b.scores)))
    .then_with(|| {
        compare_optional_f64_asc(
            export_p95_wall(a).or_else(|| avg(&a.wall_times)),
            export_p95_wall(b).or_else(|| avg(&b.wall_times)),
        )
    })
    .then_with(|| compare_optional_f64_asc(export_avg_cost(a), export_avg_cost(b)))
    .then_with(|| compare_optional_f64_desc(avg(&a.throughputs), avg(&b.throughputs)))
    .then_with(|| b.runs.cmp(&a.runs))
    .then_with(|| a.target_id.cmp(&b.target_id))
}

fn export_pass_rate(row: &ExportAggregate) -> f64 {
    if row.runs == 0 {
        0.0
    } else {
        row.passed as f64 / row.runs as f64
    }
}

fn export_weighted_pass_rate(weighted_passed: f64, total_weight: f64) -> Option<f64> {
    (total_weight > 0.0).then_some(weighted_passed / total_weight)
}

fn export_weighted_average_score(weighted_score_sum: f64, scored_weight: f64) -> Option<f64> {
    (scored_weight > 0.0).then_some(weighted_score_sum / scored_weight)
}

fn export_target_pass_rate(row: &ExportTargetAggregate) -> f64 {
    if row.runs == 0 {
        0.0
    } else {
        row.passed as f64 / row.runs as f64
    }
}

fn pass_rate_interval(passed: usize, runs: usize) -> Option<(f64, f64)> {
    if runs == 0 {
        return None;
    }
    let z = 1.96_f64;
    let n = runs as f64;
    let p = passed as f64 / n;
    let denominator = 1.0 + z.powi(2) / n;
    let center = (p + z.powi(2) / (2.0 * n)) / denominator;
    let margin = (z * ((p * (1.0 - p) / n) + z.powi(2) / (4.0 * n * n)).sqrt()) / denominator;
    Some(((center - margin).max(0.0), (center + margin).min(1.0)))
}

fn export_p95_wall(row: &ExportAggregate) -> Option<f64> {
    percentile(&row.wall_times, 0.95)
}

fn export_avg_cost(row: &ExportAggregate) -> Option<f64> {
    row.has_cost
        .then_some(row.total_cost_usd / row.costed.max(1) as f64)
}

fn export_target_avg_cost(row: &ExportTargetAggregate) -> Option<f64> {
    row.has_cost
        .then_some(row.total_cost_usd / row.costed.max(1) as f64)
}

fn optional_delta(current: Option<f64>, previous: Option<f64>) -> Option<f64> {
    match (current, previous) {
        (Some(current), Some(previous)) => Some(current - previous),
        _ => None,
    }
}

fn run_group_trend_signal(
    pass_rate_delta: f64,
    average_score_delta: Option<f64>,
    current_p95_wall_time_ms: Option<f64>,
    previous_p95_wall_time_ms: Option<f64>,
    current_average_cost_usd: Option<f64>,
    previous_average_cost_usd: Option<f64>,
) -> (String, String) {
    let mut regressions = Vec::new();
    let mut improvements = Vec::new();

    if pass_rate_delta <= -0.05 {
        regressions.push(format!(
            "pass rate {}",
            format_percent_point_delta(pass_rate_delta)
        ));
    } else if pass_rate_delta >= 0.05 {
        improvements.push(format!(
            "pass rate {}",
            format_percent_point_delta(pass_rate_delta)
        ));
    }
    if let Some(delta) = average_score_delta {
        if delta <= -0.05 {
            regressions.push(format!("score {}", format_number_delta(Some(delta))));
        } else if delta >= 0.05 {
            improvements.push(format!("score {}", format_number_delta(Some(delta))));
        }
    }
    if let (Some(current), Some(previous)) = (current_p95_wall_time_ms, previous_p95_wall_time_ms) {
        let delta = current - previous;
        if previous > 0.0 && delta > 0.0 && delta / previous >= 0.20 {
            regressions.push(format!("p95 latency {}", format_ms_delta(Some(delta))));
        } else if previous > 0.0 && delta < 0.0 && delta.abs() / previous >= 0.20 {
            improvements.push(format!("p95 latency {}", format_ms_delta(Some(delta))));
        }
    }
    if let (Some(current), Some(previous)) = (current_average_cost_usd, previous_average_cost_usd) {
        let delta = current - previous;
        if previous > 0.0 && delta > 0.0 && delta / previous >= 0.20 {
            regressions.push(format!("avg cost {}", format_cost_delta(Some(delta))));
        } else if previous > 0.0 && delta < 0.0 && delta.abs() / previous >= 0.20 {
            improvements.push(format!("avg cost {}", format_cost_delta(Some(delta))));
        }
    }

    if !regressions.is_empty() {
        return (
            "warn".into(),
            format!("regression: {}", regressions.join("; ")),
        );
    }
    if !improvements.is_empty() {
        return (
            "ok".into(),
            format!("improvement: {}", improvements.join("; ")),
        );
    }
    (
        "ok".into(),
        "stable: latest and previous group are within trend thresholds".into(),
    )
}

fn trend_level_rank(level: &str) -> usize {
    match level {
        "warn" => 0,
        "error" => 0,
        _ => 1,
    }
}

fn float_close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.0000001
}

fn option_float_close(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => float_close(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn compare_optional_f64_asc(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    a.unwrap_or(f64::INFINITY)
        .partial_cmp(&b.unwrap_or(f64::INFINITY))
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn compare_optional_f64_desc(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    b.unwrap_or(f64::NEG_INFINITY)
        .partial_cmp(&a.unwrap_or(f64::NEG_INFINITY))
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn export_scope_note(result_count: usize) -> String {
    format!(
        "This export contains {} selected result row(s), not necessarily the full result history. Rankings, pass rates, costs, and confidence notes apply only to this scoped export; clear filters and export all results for whole-history comparison.",
        result_count
    )
}

fn scoped_results(
    results: Vec<store::ResultRecord>,
    run_ids: Option<Vec<String>>,
) -> Result<Vec<store::ResultRecord>, String> {
    let Some(run_ids) = run_ids else {
        return Ok(results);
    };
    let requested = run_ids
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .collect::<BTreeSet<_>>();
    if requested.is_empty() {
        return Ok(vec![]);
    }
    let mut found = BTreeSet::new();
    let scoped = results
        .into_iter()
        .filter(|result| {
            if requested.contains(&result.id) {
                found.insert(result.id.clone());
                true
            } else {
                false
            }
        })
        .collect::<Vec<_>>();
    if found.len() != requested.len() {
        let missing = requested
            .difference(&found)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!("requested run ids were not found: {}", missing))
    } else {
        Ok(scoped)
    }
}

fn export_error_categories(results: &[store::ResultRecord]) -> Vec<ExportErrorCategory> {
    let mut rows: BTreeMap<String, ExportErrorCategory> = BTreeMap::new();
    for result in results {
        if result.status == "passed" {
            continue;
        }
        let code = result
            .error_code
            .clone()
            .unwrap_or_else(|| result.status.clone());
        let row = rows
            .entry(code.clone())
            .or_insert_with(|| ExportErrorCategory {
                code: code.clone(),
                retryable: error_category_is_retryable(&code),
                recovery_hint: error_category_recovery_hint(&code),
                ..ExportErrorCategory::default()
            });
        row.count += 1;
        row.target_ids.insert(result.target_id.clone());
        row.benchmark_pack_ids
            .insert(result.benchmark_pack_id.clone());
        row.task_ids.insert(result.task_id.clone());
        if let Some(status) = http_status_code(result.http_status) {
            *row.http_status_counts.entry(status).or_insert(0) += 1;
        }
        if result
            .provider_retry_after_ms
            .is_some_and(|value| value > 0.0)
            || result
                .provider_retry_delay_ms
                .is_some_and(|value| value > 0.0)
        {
            row.retryable = true;
        }
        let started = result.started_at.as_deref().unwrap_or_default();
        if started >= row.latest_started.as_str() {
            row.latest_started = started.to_string();
            row.example_detail = result
                .error_message
                .as_deref()
                .map(compact_report_error_detail)
                .filter(|value| !value.is_empty())
                .or_else(|| Some(code.clone()));
        }
    }
    let mut rows = rows.into_values().collect::<Vec<_>>();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.code.cmp(&b.code)));
    rows
}

fn error_category_json(row: &ExportErrorCategory) -> serde_json::Value {
    serde_json::json!({
        "code": &row.code,
        "count": row.count,
        "target_ids": &row.target_ids,
        "benchmark_pack_ids": &row.benchmark_pack_ids,
        "task_ids": &row.task_ids,
        "http_statuses": &row.http_status_counts,
        "retryable": row.retryable,
        "recovery_hint": row.recovery_hint,
        "example_detail": row.example_detail,
    })
}

fn error_category_is_retryable(code: &str) -> bool {
    matches!(
        code.trim(),
        "rate_limit"
            | "timeout"
            | "network"
            | "endpoint_unreachable"
            | "server_error"
            | "provider_failed"
    )
}

fn error_category_recovery_hint(code: &str) -> &'static str {
    match code.trim() {
        "missing_key" | "auth" => {
            "Add or refresh the provider API key in Settings or Targets, then revalidate before rerunning."
        }
        "model_not_found" => {
            "Check the model or deployment id against the provider/runtime catalog, save the corrected target, then revalidate."
        }
        "endpoint_unreachable" => {
            "Start the local runtime or fix the base URL/port, then run target validation before queueing another benchmark."
        }
        "rate_limit" => {
            "Reduce concurrency, honor Retry-After, increase provider retries if appropriate, and rerun the failed scope later."
        }
        "timeout" => {
            "Increase the timeout or reduce concurrency/task size, then retry the failed target/task scope."
        }
        "network" | "server_error" | "provider_failed" => {
            "Treat this as provider/runtime infrastructure first: inspect HTTP status/logs, retry later, or switch endpoint."
        }
        "context_overflow" => {
            "Lower max tokens or context size, choose a shorter prompt/pack subset, or use a model with a larger context window."
        }
        "content_filter" => {
            "Inspect prompt and raw provider response; adjust the task or provider policy before treating this as model quality."
        }
        "malformed_response" | "unsupported_shape" => {
            "Inspect the raw provider payload and adapter settings; update the adapter or parser before trusting score differences."
        }
        "security_findings" => {
            "Open the safety findings and copied artifacts; treat this as benchmark evidence unless the scanner configuration is wrong."
        }
        "cancelled" => "Rerun the cancelled scope if you need complete comparison evidence.",
        "failed" | "test_failed" | "score_failed" => {
            "Inspect task drilldowns and artifacts; this may be model behavior rather than infrastructure."
        }
        _ => "Inspect the run artifacts, raw logs, and task drilldown before deciding whether to retry or count this as benchmark evidence.",
    }
}

fn total_tokens_for_result(result: &store::ResultRecord) -> Option<f64> {
    result
        .total_tokens
        .or_else(|| match (result.prompt_tokens, result.completion_tokens) {
            (Some(prompt), Some(completion)) => Some(prompt + completion),
            (Some(prompt), None) => Some(prompt),
            (None, Some(completion)) => Some(completion),
            (None, None) => None,
        })
}

fn http_status_code(value: Option<f64>) -> Option<u16> {
    let value = value?;
    if value.is_finite() && value >= 100.0 && value <= 599.0 {
        Some(value.round() as u16)
    } else {
        None
    }
}

fn format_http_status_counts(counts: &BTreeMap<u16, usize>) -> String {
    if counts.is_empty() {
        return "-".into();
    }
    counts
        .iter()
        .map(|(status, count)| {
            if *count == 1 {
                status.to_string()
            } else {
                format!("{} ({})", status, count)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_text_counts(counts: &BTreeMap<String, usize>) -> String {
    if counts.is_empty() {
        return "-".into();
    }
    counts
        .iter()
        .map(|(value, count)| {
            if *count == 1 {
                value.clone()
            } else {
                format!("{} ({})", value, count)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn csv_row(values: &[String]) -> String {
    values
        .iter()
        .map(|value| csv_cell(value))
        .collect::<Vec<_>>()
        .join(",")
}

fn csv_cell(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn artifact_export_filename(artifact: &store::ArtifactRecord) -> String {
    let source_name = Path::new(&artifact.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact.txt");
    safe_export_file_name(&format!("{}-{}", artifact.kind, source_name))
}

fn safe_export_file_name(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "artifact".into()
    } else {
        trimmed
    }
}

fn export_timestamp() -> String {
    safe_export_file_name(&store::now().replace(':', "-"))
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn report_error_detail(result: &store::ResultRecord) -> String {
    let detail = result
        .error_message
        .as_deref()
        .map(compact_report_error_detail)
        .unwrap_or_default();
    if detail.is_empty() {
        "-".into()
    } else {
        detail
    }
}

fn compact_report_error_detail(value: &str) -> String {
    const LIMIT: usize = 160;
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= LIMIT {
        return normalized;
    }
    let mut truncated = normalized
        .chars()
        .take(LIMIT.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn format_option(value: Option<f64>) -> String {
    value.map(|item| item.to_string()).unwrap_or_default()
}

fn format_percent(value: Option<f64>) -> String {
    value
        .map(|item| format!("{:.0}%", item * 100.0))
        .unwrap_or_else(|| "-".into())
}

fn format_percent_range(value: Option<(f64, f64)>) -> String {
    value
        .map(|(low, high)| {
            format!(
                "{}-{}",
                format_percent(Some(low)),
                format_percent(Some(high))
            )
        })
        .unwrap_or_else(|| "-".into())
}

fn format_percent_point_delta(value: f64) -> String {
    format!("{:+.0} pp", value * 100.0)
}

fn format_number(value: Option<f64>) -> String {
    value
        .map(|item| {
            if item.fract() == 0.0 {
                format!("{:.0}", item)
            } else {
                format!("{:.2}", item)
            }
        })
        .unwrap_or_else(|| "-".into())
}

fn format_number_with_spread(value: Option<f64>, spread: Option<f64>) -> String {
    value
        .map(|item| format!("{} / {}", format_number(Some(item)), format_number(spread)))
        .unwrap_or_else(|| "-".into())
}

fn format_number_distribution(values: &[f64]) -> String {
    median(values)
        .map(|item| {
            format!(
                "{} / {} / {}",
                format_number(Some(item)),
                format_number(min_value(values)),
                format_number(max_value(values))
            )
        })
        .unwrap_or_else(|| "-".into())
}

fn format_number_delta(value: Option<f64>) -> String {
    value
        .map(|item| format!("{:+.2}", item))
        .unwrap_or_else(|| "-".into())
}

fn format_ms(value: Option<f64>) -> String {
    value
        .map(|item| format!("{:.0} ms", item))
        .unwrap_or_else(|| "-".into())
}

fn format_ms_distribution(values: &[f64]) -> String {
    median(values)
        .map(|item| {
            format!(
                "{} / {} / {}",
                format_ms(Some(item)),
                format_ms(min_value(values)),
                format_ms(max_value(values))
            )
        })
        .unwrap_or_else(|| "-".into())
}

fn format_ms_delta(value: Option<f64>) -> String {
    value
        .map(|item| format!("{:+.0} ms", item))
        .unwrap_or_else(|| "-".into())
}

fn format_cost(value: Option<f64>) -> String {
    value
        .map(|item| {
            if item == 0.0 {
                "$0".into()
            } else if item < 0.01 {
                format!("${:.6}", item)
            } else {
                format!("${:.4}", item)
            }
        })
        .unwrap_or_else(|| "-".into())
}

fn format_cost_delta(value: Option<f64>) -> String {
    value
        .map(|item| {
            if item == 0.0 {
                "$0".into()
            } else {
                let sign = if item >= 0.0 { "+" } else { "-" };
                let absolute = item.abs();
                if absolute < 0.01 {
                    format!("{}${:.6}", sign, absolute)
                } else {
                    format!("{}${:.4}", sign, absolute)
                }
            }
        })
        .unwrap_or_else(|| "-".into())
}

fn avg(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn std_dev(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / (values.len() - 1) as f64;
    Some(variance.sqrt())
}

fn percentile(values: &[f64], rank: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((rank * sorted.len() as f64).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted.get(index).copied()
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let midpoint = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        Some((sorted[midpoint - 1] + sorted[midpoint]) / 2.0)
    } else {
        sorted.get(midpoint).copied()
    }
}

fn min_value(values: &[f64]) -> Option<f64> {
    values.iter().copied().reduce(f64::min)
}

fn max_value(values: &[f64]) -> Option<f64> {
    values.iter().copied().reduce(f64::max)
}

fn short_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn unique_count(values: impl Iterator<Item = String>) -> usize {
    values.collect::<std::collections::BTreeSet<_>>().len()
}

fn estimate_tokens(text: &str) -> u64 {
    ((text.chars().count() as u64) + 3) / 4
}

fn configured_max_tokens_for_tasks(config: &serde_json::Value, tasks: &[runner::TaskSpec]) -> u64 {
    configured_max_tokens(config).unwrap_or_else(|| default_max_tokens_for_tasks(tasks))
}

fn configured_max_tokens(config: &serde_json::Value) -> Option<u64> {
    config
        .get("max_tokens")
        .and_then(|value| value.as_u64())
        .filter(|value| *value > 0)
}

fn default_max_tokens_for_tasks(tasks: &[runner::TaskSpec]) -> u64 {
    if tasks.iter().any(|task| task.task_type != "prompt") {
        WORKSPACE_DEFAULT_MAX_TOKENS
    } else {
        PROMPT_DEFAULT_MAX_TOKENS
    }
}

fn configured_timeout_seconds(config: &serde_json::Value) -> u64 {
    config
        .get("timeout_seconds")
        .and_then(|value| value.as_u64())
        .filter(|value| *value > 0)
        .unwrap_or(120)
}

fn target_supports_warmup(target: &store::TargetRecord) -> bool {
    matches!(
        target.kind.as_str(),
        "mock" | "direct_model" | "harnessed_model"
    )
}

fn div_ceil_u64(value: u64, divisor: u64) -> u64 {
    if divisor == 0 {
        return value;
    }
    value / divisor + u64::from(value % divisor != 0)
}

fn price_per_million(config: &serde_json::Value, key: &str) -> Option<f64> {
    config
        .get(key)
        .and_then(|value| value.as_f64())
        .or_else(|| {
            config
                .get("pricing")
                .and_then(|pricing| pricing.get(key))
                .and_then(|value| value.as_f64())
        })
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn conservative_prompt_price_per_million(config: &serde_json::Value, input_price: f64) -> f64 {
    [
        Some(input_price),
        price_per_million(config, "cache_read_price_usd_per_million_tokens"),
        price_per_million(config, "cached_input_price_usd_per_million_tokens"),
        price_per_million(config, "cache_write_price_usd_per_million_tokens"),
        price_per_million(config, "cache_creation_price_usd_per_million_tokens"),
    ]
    .into_iter()
    .flatten()
    .fold(input_price, f64::max)
}

#[tauri::command]
pub fn huggingface_status(
    state: State<'_, store::AppState>,
) -> Result<huggingface::HuggingFaceStatusDto, String> {
    huggingface::status(&state)
}

#[tauri::command]
pub fn save_huggingface_token(request: huggingface::SaveTokenRequest) -> Result<(), String> {
    huggingface::save_token(request)
}

#[tauri::command]
pub fn install_huggingface_tools(
    request: huggingface::InstallToolsRequest,
) -> Result<huggingface::InstallToolsResultDto, String> {
    huggingface::install_tools(request)
}

#[tauri::command]
pub fn run_harness_tool_action(
    request: harness_tools::HarnessToolRequest,
) -> Result<harness_tools::HarnessToolResultDto, String> {
    harness_tools::run_harness_tool_action(request)
}

#[tauri::command]
pub fn search_huggingface_models(
    request: huggingface::SearchModelsRequest,
) -> Result<Vec<huggingface::HuggingFaceModelDto>, String> {
    huggingface::search_models(request)
}

#[tauri::command]
pub fn inspect_huggingface_model(
    request: huggingface::ModelRequest,
) -> Result<huggingface::HuggingFaceModelFilesDto, String> {
    huggingface::inspect_model(request)
}

#[tauri::command]
pub fn plan_huggingface_download(
    request: huggingface::DownloadModelRequest,
) -> Result<huggingface::DownloadModelPlanDto, String> {
    huggingface::plan_download(request)
}

#[tauri::command]
pub fn download_huggingface_model(
    app: AppHandle,
    request: huggingface::DownloadModelRequest,
) -> Result<huggingface::DownloadedModelDto, String> {
    huggingface::download_model_with_progress(request, |progress| {
        let _ = app.emit("benchforge://hf-download-progress", progress);
    })
}

#[tauri::command]
pub fn start_huggingface_download_job(
    app: AppHandle,
    state: State<'_, store::AppState>,
    request: huggingface::DownloadModelRequest,
) -> Result<huggingface::HuggingFaceDownloadJobDto, String> {
    let start_after_download = request.start_after_download;
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    let job = huggingface::start_download_job(&conn, request)?;
    drop(conn);
    if start_after_download {
        spawn_huggingface_download_handoff(app, job.id.clone());
    }
    Ok(job)
}

#[tauri::command]
pub fn list_huggingface_download_jobs(
    state: State<'_, store::AppState>,
) -> Result<Vec<huggingface::HuggingFaceDownloadJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    huggingface::list_download_jobs(&conn)
}

#[tauri::command]
pub fn get_huggingface_download_job(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<huggingface::HuggingFaceDownloadJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    huggingface::get_download_job(&conn, &id)
}

#[tauri::command]
pub fn cancel_huggingface_download_job(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<huggingface::HuggingFaceDownloadJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    huggingface::cancel_download_job(&conn, &id)
}

#[tauri::command]
pub fn retry_huggingface_download_job(
    app: AppHandle,
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<huggingface::HuggingFaceDownloadJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    let retry = huggingface::retry_download_job(&conn, &id)?;
    drop(conn);
    if let Some(job) = &retry {
        if job.start_after_download {
            spawn_huggingface_download_handoff(app, job.id.clone());
        }
    }
    Ok(retry)
}

#[tauri::command]
pub fn clear_finished_huggingface_download_jobs(
    state: State<'_, store::AppState>,
) -> Result<usize, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    huggingface::clear_finished_download_jobs(&conn)
}

fn spawn_huggingface_download_handoff(app: AppHandle, download_job_id: String) {
    thread::spawn(move || {
        let app_state = app.state::<store::AppState>();
        if let Err(err) = run_huggingface_download_handoff_for_state(&app_state, &download_job_id) {
            eprintln!("Hugging Face download handoff failed for {download_job_id}: {err}");
        }
    });
}

fn run_huggingface_download_handoff_for_state(
    app_state: &store::AppState,
    download_job_id: &str,
) -> Result<(), String> {
    let Some(download_job) = wait_for_huggingface_download_terminal(download_job_id)? else {
        return Ok(());
    };
    if download_job.status != "completed" || !download_job.start_after_download {
        return Ok(());
    }
    let start_request = start_request_from_download_job(&download_job)?;
    {
        let conn = store::open_app().map_err(|err| err.to_string())?;
        if matching_huggingface_server_job_exists(&conn, &download_job, &start_request)? {
            return Ok(());
        }
        let server_job = huggingface::enqueue_server_job(&conn, start_request.clone())?;
        drop(conn);
        run_huggingface_server_job_with_handoff_for_state(
            app_state,
            &server_job.id,
            start_request,
        )?;
    }
    Ok(())
}

fn spawn_huggingface_server_job(
    app: AppHandle,
    server_job_id: String,
    request: huggingface::StartModelRequest,
) {
    thread::spawn(move || {
        let app_state = app.state::<store::AppState>();
        if let Err(err) =
            run_huggingface_server_job_with_handoff_for_state(&app_state, &server_job_id, request)
        {
            eprintln!("Hugging Face server handoff failed for {server_job_id}: {err}");
        }
    });
}

fn run_huggingface_server_job_with_handoff_for_state(
    app_state: &store::AppState,
    server_job_id: &str,
    request: huggingface::StartModelRequest,
) -> Result<(), String> {
    huggingface::run_server_job(app_state, server_job_id.to_string(), request);
    finish_huggingface_server_handoff(app_state, server_job_id)
}

fn wait_for_huggingface_download_terminal(
    download_job_id: &str,
) -> Result<Option<huggingface::HuggingFaceDownloadJobDto>, String> {
    loop {
        let conn = store::open_app().map_err(|err| err.to_string())?;
        let job = huggingface::get_download_job(&conn, download_job_id)?;
        drop(conn);
        let Some(job) = job else {
            return Ok(None);
        };
        if matches!(job.status.as_str(), "completed" | "failed" | "cancelled") {
            return Ok(Some(job));
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn start_request_from_download_job(
    job: &huggingface::HuggingFaceDownloadJobDto,
) -> Result<huggingface::StartModelRequest, String> {
    let filename = job
        .model
        .as_ref()
        .and_then(|model| model.selected_file.clone())
        .or_else(|| job.selected_file.clone())
        .ok_or_else(|| {
            format!(
                "completed Hugging Face download job {} has no selected GGUF file",
                job.id
            )
        })?;
    Ok(huggingface::StartModelRequest {
        repo_id: job.repo_id.clone(),
        filename: Some(filename),
        port: job.start_port.unwrap_or(HF_LOCAL_DEFAULT_PORT),
        context: job.start_context.unwrap_or(HF_LOCAL_DEFAULT_CONTEXT),
        register_target_after_start: true,
        run_connectivity_after_start: job.run_connectivity_after_start,
        auto_benchmark_pack_id: job.auto_benchmark_pack_id.clone(),
        auto_compare_after_start: job.auto_compare_after_start,
        auto_benchmark_target_ids: job.auto_benchmark_target_ids.clone(),
    })
}

fn matching_huggingface_server_job_exists(
    conn: &rusqlite::Connection,
    download_job: &huggingface::HuggingFaceDownloadJobDto,
    start_request: &huggingface::StartModelRequest,
) -> Result<bool, String> {
    let filename = start_request.filename.as_deref().unwrap_or_default();
    Ok(huggingface::list_server_jobs(conn)?
        .into_iter()
        .any(|server_job| {
            server_job.repo_id == download_job.repo_id
                && server_job.port == start_request.port
                && server_job.context == start_request.context
                && server_job.selected_file.as_deref().unwrap_or_default() == filename
                && matches!(
                    server_job.status.as_str(),
                    "queued" | "running" | "completed"
                )
        }))
}

fn finish_huggingface_server_handoff(
    app_state: &store::AppState,
    server_job_id: &str,
) -> Result<(), String> {
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let Some(server_job) = huggingface::get_server_job(&conn, server_job_id)? else {
        return Ok(());
    };
    if server_job.status != "completed" || !server_job.register_target_after_start {
        return Ok(());
    }
    let status = server_job.server_status.clone().unwrap_or_else(|| {
        huggingface::status(app_state).unwrap_or_else(|_| empty_huggingface_status())
    });
    let target = target_from_huggingface_server_job(&server_job, &status);
    let target_id = target.id.clone();
    store::upsert_target(&conn, &target).map_err(|err| err.to_string())?;
    let validation = validate_target_for_conn(&conn, &target_id)?;
    if validation.status == "error" {
        return Ok(());
    }
    if server_job.run_connectivity_after_start || server_job.auto_benchmark_pack_id.is_some() {
        let pack_id = server_job
            .auto_benchmark_pack_id
            .clone()
            .unwrap_or_else(|| "llm-connectivity".into());
        let target_ids =
            automatic_huggingface_benchmark_target_ids(&conn, &target_id, &server_job)?;
        if !matching_active_run_job_exists(&conn, &target_ids, &pack_id)? {
            let (repetitions, warmup_runs, concurrency, max_cost_usd) =
                automatic_huggingface_benchmark_settings(&pack_id, target_ids.len());
            let _ = jobs::start_quick_smoke_job(
                &conn,
                runner::RunQuickSmokeRequest {
                    target_ids,
                    benchmark_pack_id: pack_id,
                    task_ids: vec![],
                    repetitions,
                    docker: false,
                    warmup_runs,
                    concurrency,
                    max_cost_usd: Some(max_cost_usd),
                    run_group_id: None,
                },
            )?;
        }
    }
    Ok(())
}

fn empty_huggingface_status() -> huggingface::HuggingFaceStatusDto {
    huggingface::HuggingFaceStatusDto {
        token_available: false,
        python_available: false,
        python_supported: false,
        python_version: None,
        hf_cli_available: false,
        llama_server_available: false,
        server_running: false,
        server_model_id: None,
        cache_dir: String::new(),
        cache_size_bytes: 0,
        cache_free_bytes: None,
        detail: String::new(),
        models: vec![],
    }
}

fn target_from_huggingface_server_job(
    job: &huggingface::HuggingFaceServerJobDto,
    status: &huggingface::HuggingFaceStatusDto,
) -> store::NewTarget {
    let selected_file = job
        .selected_file
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            status
                .models
                .iter()
                .find(|model| model.repo_id == job.repo_id)
                .and_then(|model| model.selected_file.clone())
        })
        .unwrap_or_default();
    let served_model = status
        .server_model_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            selected_file
                .trim()
                .is_empty()
                .then_some(job.repo_id.as_str())
                .unwrap_or(selected_file.as_str())
        })
        .to_string();
    let downloaded = status
        .models
        .iter()
        .find(|model| model.repo_id == job.repo_id);
    let mut config = serde_json::json!({
        "model": served_model,
        "base_url": format!("http://127.0.0.1:{}/v1", job.port),
        "source": "huggingface-local",
        "repo_id": job.repo_id,
        "port": job.port,
        "context": job.context,
        "temperature": 0,
        "top_p": 1,
        "max_tokens": hf_local_target_max_tokens(job.context),
        "timeout_seconds": 120,
        "retry_count": 1,
        "input_price_usd_per_million_tokens": 0,
        "output_price_usd_per_million_tokens": 0
    });
    if !selected_file.is_empty() {
        config["gguf_file"] = serde_json::Value::String(selected_file.clone());
    }
    if let Some(path) = downloaded.map(|model| model.path.as_str()) {
        config["model_path"] = serde_json::Value::String(path.to_string());
    }
    if let Some(revision) = downloaded
        .and_then(|model| model.revision.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        config["revision"] = serde_json::Value::String(revision.to_string());
    }
    let file_label = selected_file
        .strip_suffix(".gguf")
        .unwrap_or(&selected_file)
        .to_string();
    let name = if file_label.is_empty() {
        format!("HF Local {}", job.repo_id)
    } else {
        format!("HF Local {} {}", job.repo_id, file_label)
    };
    store::NewTarget {
        id: hf_local_target_id(&job.repo_id, &selected_file, job.port),
        name,
        kind: "direct_model".into(),
        adapter_id: "llama-cpp-openai".into(),
        config,
    }
}

fn hf_local_target_max_tokens(context: u32) -> u32 {
    if context == 0 {
        512
    } else {
        let divisor = if context <= 1024 { 8 } else { 4 };
        (context / divisor).clamp(16, 512)
    }
}

fn hf_local_target_id(repo_id: &str, selected_file: &str, port: u16) -> String {
    slugify_id(&format!(
        "hf-local-{}-{}-{}",
        repo_id,
        if selected_file.is_empty() {
            "model"
        } else {
            selected_file
        },
        port
    ))
}

fn slugify_id(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= 64 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        out
    }
}

fn automatic_huggingface_benchmark_target_ids(
    conn: &rusqlite::Connection,
    local_target_id: &str,
    server_job: &huggingface::HuggingFaceServerJobDto,
) -> Result<Vec<String>, String> {
    let mut target_ids = vec![local_target_id.to_string()];
    if !server_job.auto_benchmark_target_ids.is_empty() {
        let adapter_map = benchmark_adapter_map();
        let target_map = store::list_targets(conn)
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(|target| (target.id.clone(), target))
            .collect::<std::collections::BTreeMap<_, _>>();
        for requested_id in &server_job.auto_benchmark_target_ids {
            if requested_id == local_target_id || target_ids.contains(requested_id) {
                continue;
            }
            let Some(target) = target_map.get(requested_id) else {
                continue;
            };
            if target.enabled
                && !target_validation_is_error(target)
                && target_is_cloud_benchmark_model(target, &adapter_map)
                && target_has_input_output_pricing(target)
            {
                target_ids.push(requested_id.clone());
            }
        }
        return Ok(target_ids);
    }
    if !server_job.auto_compare_after_start {
        return Ok(target_ids);
    }
    let adapter_map = benchmark_adapter_map();
    let cloud_target = store::list_targets(conn)
        .map_err(|err| err.to_string())?
        .into_iter()
        .filter(|target| target.id != local_target_id)
        .filter(|target| target.enabled)
        .filter(|target| !target_validation_is_error(target))
        .filter(|target| target_is_cloud_benchmark_model(target, &adapter_map))
        .filter(target_has_input_output_pricing)
        .min_by(|left, right| left.id.cmp(&right.id));
    if let Some(target) = cloud_target {
        target_ids.push(target.id);
    }
    Ok(target_ids)
}

fn target_has_input_output_pricing(target: &store::TargetRecord) -> bool {
    let config = target_config_json(target);
    price_per_million(&config, "input_price_usd_per_million_tokens")
        .or_else(|| price_per_million(&config, "input_usd_per_million_tokens"))
        .is_some()
        && price_per_million(&config, "output_price_usd_per_million_tokens")
            .or_else(|| price_per_million(&config, "output_usd_per_million_tokens"))
            .is_some()
}

fn automatic_huggingface_benchmark_settings(
    pack_id: &str,
    target_count: usize,
) -> (u32, u32, u32, f64) {
    let concurrency = u32::try_from(target_count.clamp(1, 2)).unwrap_or(1);
    if pack_id == "llm-connectivity" {
        (1, 0, concurrency, HF_CONNECTIVITY_MAX_COST_USD)
    } else {
        (3, 1, concurrency, HF_QUALITY_MAX_COST_USD)
    }
}

fn matching_active_run_job_exists(
    conn: &rusqlite::Connection,
    target_ids: &[String],
    pack_id: &str,
) -> Result<bool, String> {
    Ok(store::list_run_jobs(conn)
        .map_err(|err| err.to_string())?
        .into_iter()
        .any(|job| {
            matches!(job.status.as_str(), "queued" | "running" | "cancelling")
                && job.benchmark_pack_id == pack_id
                && run_job_targets_match(&job.request, target_ids)
        }))
}

fn run_job_targets_match(request: &serde_json::Value, target_ids: &[String]) -> bool {
    request
        .get("targetIds")
        .and_then(|value| value.as_array())
        .map(|ids| {
            ids.len() == target_ids.len()
                && ids.iter().zip(target_ids.iter()).all(|(value, target_id)| {
                    value.as_str().map(|id| id == target_id).unwrap_or(false)
                })
        })
        .unwrap_or(false)
}

#[tauri::command]
pub fn reveal_huggingface_model(request: huggingface::ModelRequest) -> Result<(), String> {
    huggingface::reveal_model(request)
}

#[tauri::command]
pub fn delete_huggingface_model(
    state: State<'_, store::AppState>,
    request: huggingface::ModelRequest,
) -> Result<huggingface::HuggingFaceStatusDto, String> {
    huggingface::delete_model(&state, request)
}

#[tauri::command]
pub fn preflight_huggingface_model(
    request: huggingface::StartModelRequest,
) -> Result<huggingface::ModelPreflightDto, String> {
    huggingface::preflight_model(request)
}

#[tauri::command]
pub fn start_huggingface_model(
    state: State<'_, store::AppState>,
    request: huggingface::StartModelRequest,
) -> Result<huggingface::HuggingFaceStatusDto, String> {
    huggingface::start_server(&state, request)
}

#[tauri::command]
pub fn start_huggingface_server_job(
    app: AppHandle,
    state: State<'_, store::AppState>,
    request: huggingface::StartModelRequest,
) -> Result<huggingface::HuggingFaceServerJobDto, String> {
    let request = huggingface::normalize_start_request(request)?;
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    let job = huggingface::enqueue_server_job(&conn, request.clone())?;
    drop(conn);
    spawn_huggingface_server_job(app, job.id.clone(), request);
    Ok(job)
}

#[tauri::command]
pub fn list_huggingface_server_jobs(
    state: State<'_, store::AppState>,
) -> Result<Vec<huggingface::HuggingFaceServerJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    huggingface::list_server_jobs(&conn)
}

#[tauri::command]
pub fn get_huggingface_server_job(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<huggingface::HuggingFaceServerJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    huggingface::get_server_job(&conn, &id)
}

#[tauri::command]
pub fn cancel_huggingface_server_job(
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<huggingface::HuggingFaceServerJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    huggingface::cancel_server_job(&conn, &id)
}

#[tauri::command]
pub fn retry_huggingface_server_job(
    app: AppHandle,
    state: State<'_, store::AppState>,
    id: String,
) -> Result<Option<huggingface::HuggingFaceServerJobDto>, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    let retry = huggingface::retry_server_job(&conn, &id)?;
    drop(conn);
    let Some((job, request)) = retry else {
        return Ok(None);
    };
    spawn_huggingface_server_job(app, job.id.clone(), request);
    Ok(Some(job))
}

#[tauri::command]
pub fn clear_finished_huggingface_server_jobs(
    state: State<'_, store::AppState>,
) -> Result<usize, String> {
    let conn = state.conn.lock().map_err(|err| err.to_string())?;
    huggingface::clear_finished_server_jobs(&conn)
}

#[tauri::command]
pub fn stop_huggingface_model(
    state: State<'_, store::AppState>,
) -> Result<huggingface::HuggingFaceStatusDto, String> {
    huggingface::stop_server(&state)
}

#[tauri::command]
pub fn run_doctor(state: State<'_, store::AppState>) -> Vec<DoctorCheckDto> {
    let mut checks = vec![
        doctor_path_check(),
        doctor_command_check(
            "curl",
            "curl",
            "Core",
            "required",
            "curl",
            &["--version"],
            "Install the macOS command line tools or ensure /usr/bin/curl is available.",
        ),
        doctor_command_check(
            "git",
            "Git",
            "Core",
            "recommended",
            "git",
            &["--version"],
            "Install Git with xcode-select --install or brew install git.",
        ),
        doctor_command_check(
            "brew",
            "Homebrew",
            "Installers",
            "recommended",
            "brew",
            &["--version"],
            "Install Homebrew from https://brew.sh or install model tools manually.",
        ),
        doctor_python_check(),
        doctor_command_check(
            "hf",
            "Hugging Face CLI",
            "Local models",
            "recommended",
            "hf",
            &["version"],
            "Use Settings > Hugging Face Local Model > Install missing tools, or run curl -LsSf https://hf.co/cli/install.sh | bash -s.",
        ),
        doctor_command_check(
            "llama-server",
            "llama.cpp server",
            "Local models",
            "recommended",
            "llama-server",
            &["--version"],
            "Install llama.cpp with brew install llama.cpp.",
        ),
        local_model_storage_doctor_check(),
        doctor_command_check(
            "docker",
            "Docker",
            "Sandbox",
            "optional",
            "docker",
            &["--version"],
            "Install Docker Desktop or use Colima for Docker-backed scoring.",
        ),
        doctor_command_check(
            "colima",
            "Colima",
            "Sandbox",
            "optional",
            "colima",
            &["version"],
            "Install Colima with brew install colima docker, then run colima start.",
        ),
        doctor_command_check(
            "node",
            "Node",
            "Development",
            "recommended",
            "node",
            &["--version"],
            "Install Node.js with brew install node for source builds and web development.",
        ),
        doctor_command_check(
            "npm",
            "npm",
            "Development",
            "recommended",
            "npm",
            &["--version"],
            "Install Node.js with brew install node.",
        ),
        doctor_command_check(
            "cargo",
            "Cargo",
            "Development",
            "recommended",
            "cargo",
            &["--version"],
            "Install Rust from https://rustup.rs for Tauri development builds.",
        ),
        doctor_command_check(
            "rustc",
            "Rust compiler",
            "Development",
            "recommended",
            "rustc",
            &["--version"],
            "Install Rust from https://rustup.rs for Tauri development builds.",
        ),
        doctor_command_check(
            "codex",
            "Codex CLI",
            "Agent CLIs",
            "optional",
            "codex",
            &["--version"],
            "Install or configure Codex CLI before running Codex adapter benchmarks.",
        ),
        doctor_command_check(
            "claude",
            "Claude Code",
            "Agent CLIs",
            "optional",
            "claude",
            &["--version"],
            "Install or configure Claude Code before running Claude adapter benchmarks.",
        ),
        doctor_command_check(
            "vibe",
            "Mistral Vibe",
            "Agent CLIs",
            "optional",
            "vibe",
            &["--version"],
            "Install or configure Mistral Vibe before running Vibe adapter benchmarks.",
        ),
        doctor_command_check(
            "copilot",
            "GitHub Copilot CLI",
            "Agent CLIs",
            "optional",
            "copilot",
            &["help"],
            "Install or configure GitHub Copilot CLI before running Copilot adapter benchmarks.",
        ),
    ];
    checks.extend(local_runtime_doctor_checks());
    checks.extend(cloud_key_doctor_checks());
    match state.conn.lock() {
        Ok(conn) => checks.extend(benchmark_readiness_doctor_checks_for_conn(&conn)),
        Err(err) => checks.push(doctor_check(
            "benchmark-readiness-store",
            "Benchmark store",
            "error",
            &err.to_string(),
            "Benchmark readiness",
            "required",
            "Restart BenchForge and rerun Doctor before queueing benchmark jobs.",
            "",
        )),
    }
    checks
}

fn doctor_command_check(
    id: &str,
    label: &str,
    category: &str,
    importance: &str,
    command: &str,
    args: &[&str],
    remediation: &str,
) -> DoctorCheckDto {
    match adapters::command_with_gui_path(command).args(args).output() {
        Ok(output) if output.status.success() => doctor_check(
            id,
            label,
            "ok",
            &first_output_line(&output).unwrap_or_else(|| "installed".into()),
            category,
            importance,
            "",
            &format!("{} {}", command, args.join(" ")).trim(),
        ),
        Ok(output) => {
            let detail = first_output_line(&output).unwrap_or_else(|| {
                format!("found but returned exit code {:?}", output.status.code())
            });
            doctor_check(
                id,
                label,
                if importance == "required" {
                    "error"
                } else {
                    "warn"
                },
                &detail,
                category,
                importance,
                remediation,
                &format!("{} {}", command, args.join(" ")).trim(),
            )
        }
        Err(_) => doctor_check(
            id,
            label,
            if importance == "required" {
                "error"
            } else {
                "warn"
            },
            "not found in GUI PATH",
            category,
            importance,
            remediation,
            &format!("{} {}", command, args.join(" ")).trim(),
        ),
    }
}

fn doctor_python_check() -> DoctorCheckDto {
    let mut check = doctor_command_check(
        "python3",
        "Python",
        "Local models",
        "recommended",
        "python3",
        &["--version"],
        "Install Python 3.10+ with brew install python; the Hugging Face CLI installer requires it.",
    );
    if check.status == "ok" {
        if let Some(version) = parse_python_version(&check.detail) {
            if version < (3, 10, 0) {
                check.status = "warn".into();
                check.detail = format!(
                    "{} is too old for the Hugging Face CLI installer; Python 3.10+ is required",
                    check.detail
                );
                check.remediation =
                    "Install Python 3.10+ with brew install python, then rerun Doctor.".into();
            }
        }
    }
    check
}

fn doctor_path_check() -> DoctorCheckDto {
    doctor_check(
        "gui-path",
        "GUI PATH",
        "ok",
        "BenchForge searches /opt/homebrew/bin, /usr/local/bin, ~/.local/bin, ~/.cargo/bin, and the inherited PATH.",
        "Environment",
        "required",
        "If a tool works in Terminal but not BenchForge, install it in one of these locations or relaunch BenchForge after updating PATH.",
        "",
    )
}

fn local_model_storage_doctor_check() -> DoctorCheckDto {
    let models_dir = paths::app_data_dir().join("models");
    let command = models_dir.to_string_lossy().to_string();
    if let Err(err) = fs::create_dir_all(&models_dir) {
        return doctor_check(
            "hf-model-storage",
            "Local model storage",
            "error",
            &format!("cannot create {}: {}", command, err),
            "Local models",
            "recommended",
            "Choose a writable project location or fix filesystem permissions before downloading GGUF models.",
            &command,
        );
    }
    let probe_path = models_dir.join(".benchforge-write-test");
    if let Err(err) = fs::write(&probe_path, b"ok") {
        return doctor_check(
            "hf-model-storage",
            "Local model storage",
            "error",
            &format!("cannot write to {}: {}", command, err),
            "Local models",
            "recommended",
            "Fix filesystem permissions before downloading GGUF models.",
            &command,
        );
    }
    let _ = fs::remove_file(&probe_path);

    let Some(available_bytes) = doctor_available_disk_bytes(&models_dir) else {
        return doctor_check(
            "hf-model-storage",
            "Local model storage",
            "warn",
            &format!(
                "{} is writable; free space could not be determined",
                command
            ),
            "Local models",
            "recommended",
            "Run a Hugging Face download preflight before large GGUF downloads.",
            &command,
        );
    };
    let detail = format!(
        "{} is writable; about {} free",
        command,
        doctor_format_bytes(available_bytes)
    );
    let status = if available_bytes < 10 * 1024 * 1024 * 1024 {
        "warn"
    } else {
        "ok"
    };
    let remediation = if status == "ok" {
        "Use Settings > Hugging Face Local Model to download GGUF files into this cache."
    } else {
        "Free at least 10 GB before downloading medium or large GGUF models, or use smaller quantized files."
    };
    doctor_check(
        "hf-model-storage",
        "Local model storage",
        status,
        &detail,
        "Local models",
        "recommended",
        remediation,
        &command,
    )
}

fn doctor_available_disk_bytes(dir: &Path) -> Option<u64> {
    let output = adapters::command_with_gui_path("df")
        .args(["-Pk"])
        .arg(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().nth(1)?;
    let available_kib = line.split_whitespace().nth(3)?.parse::<u64>().ok()?;
    Some(available_kib.saturating_mul(1024))
}

fn doctor_format_bytes(value: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < units.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", value, units[unit])
    } else {
        format!("{:.1} {}", size, units[unit])
    }
}

fn local_runtime_doctor_checks() -> Vec<DoctorCheckDto> {
    local_runtime_candidates()
        .into_iter()
        .map(|candidate| {
            if !local_base_url_port_is_open(candidate.base_url) {
                return doctor_check(
                    &format!("endpoint-{}", candidate.id),
                    candidate.name,
                    "warn",
                    &format!("not running on {}", candidate.base_url),
                    "Local runtime endpoints",
                    "optional",
                    "Start the runtime, then use Targets > Local Runtimes > Detect or add a Generic OpenAI-compatible target.",
                    candidate.base_url,
                );
            }
            let runtime = probe_local_runtime(candidate);
            doctor_check(
                &format!("endpoint-{}", runtime.id),
                &runtime.name,
                &runtime.status,
                &runtime.detail,
                "Local runtime endpoints",
                "optional",
                if runtime.status == "ok" {
                    "Use Targets > Local Runtimes > Detect to add this endpoint."
                } else {
                    "Check that the server exposes an OpenAI-compatible /models endpoint or use the runtime-specific adapter."
                },
                &runtime.base_url,
            )
        })
        .collect()
}

fn cloud_key_doctor_checks() -> Vec<DoctorCheckDto> {
    let adapters = match adapters::load_builtin_adapters() {
        Ok(adapters) => adapters,
        Err(err) => {
            return vec![doctor_check(
                "cloud-adapters",
                "Cloud adapters",
                "error",
                &err,
                "Cloud API keys",
                "required",
                "Fix adapter YAML loading before cloud target validation can work.",
                "",
            )];
        }
    };
    adapters
        .into_iter()
        .filter(|adapter| {
            matches!(
                adapter.spec.kind.as_str(),
                "openai_responses" | "anthropic_messages" | "mistral_api" | "azure_openai"
            ) || adapter.spec.id == "openrouter"
        })
        .filter_map(|adapter| {
            let secret_env = adapter
                .spec
                .validation
                .get("secret_env")
                .and_then(|value| value.as_str())?;
            let keychain_available = secrets::cloud_api_key_available(&adapter.spec.id);
            let env_available = std::env::var(secret_env)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            let (status, detail, remediation) = if keychain_available {
                (
                    "ok".to_string(),
                    "API key is stored in macOS Keychain".to_string(),
                    "Use Targets > Model Target to replace the stored key when needed."
                        .to_string(),
                )
            } else if env_available {
                (
                    "ok".to_string(),
                    format!("{} is set in the app environment", secret_env),
                    "Use Targets > Model Target to save this key to Keychain if you want persistent app storage."
                        .to_string(),
                )
            } else {
                (
                    "warn".to_string(),
                    "API key is not configured".to_string(),
                    "Save a key from Targets > Model Target, or set the documented environment variable before launching BenchForge."
                        .to_string(),
                )
            };
            Some(doctor_check(
                &format!("cloud-key-{}", adapter.spec.id),
                &adapter.spec.name,
                &status,
                &detail,
                "Cloud API keys",
                "recommended",
                &remediation,
                secret_env,
            ))
        })
        .collect()
}

fn benchmark_readiness_doctor_checks_for_conn(conn: &rusqlite::Connection) -> Vec<DoctorCheckDto> {
    let targets = match store::list_targets(conn) {
        Ok(targets) => targets,
        Err(err) => {
            return vec![doctor_check(
                "benchmark-targets",
                "Benchmark targets",
                "error",
                &err.to_string(),
                "Benchmark readiness",
                "required",
                "Fix the BenchForge database before queueing benchmark jobs.",
                "",
            )];
        }
    };
    let results = match store::list_results(conn) {
        Ok(results) => results,
        Err(err) => {
            return vec![doctor_check(
                "benchmark-results",
                "Benchmark results",
                "error",
                &err.to_string(),
                "Benchmark readiness",
                "recommended",
                "Fix the BenchForge database before trusting benchmark history.",
                "",
            )];
        }
    };
    let packs = runner::list_benchmark_packs();
    let pack_diagnostics = runner::list_benchmark_pack_diagnostics();
    let adapters = adapters::load_builtin_adapters();
    let mut checks = benchmark_readiness_doctor_checks(&targets, &results, packs, adapters);
    checks.push(benchmark_pack_diagnostics_doctor_check(&pack_diagnostics));
    checks
}

fn benchmark_pack_diagnostics_doctor_check(
    diagnostics: &[runner::BenchmarkPackDiagnosticDto],
) -> DoctorCheckDto {
    let invalid = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.status != "ok")
        .collect::<Vec<_>>();
    let valid = diagnostics.len().saturating_sub(invalid.len());
    let builtin_invalid = invalid
        .iter()
        .filter(|diagnostic| diagnostic.source == "built-in")
        .count();
    let status = if invalid.is_empty() {
        "ok"
    } else if builtin_invalid > 0 || valid == 0 {
        "error"
    } else {
        "warn"
    };
    let detail = if invalid.is_empty() {
        format!("{} benchmark pack definition(s) validated", valid)
    } else {
        let preview = invalid
            .iter()
            .take(3)
            .map(|diagnostic| {
                format!(
                    "{}: {}",
                    diagnostic
                        .id
                        .as_deref()
                        .unwrap_or(diagnostic.source_path.as_str()),
                    diagnostic.detail
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "{} invalid benchmark pack definition(s), {} valid; {}",
            invalid.len(),
            valid,
            preview
        )
    };
    doctor_check(
        "benchmark-pack-diagnostics",
        "Benchmark pack diagnostics",
        status,
        &detail,
        "Benchmark readiness",
        if status == "error" {
            "required"
        } else {
            "recommended"
        },
        if invalid.is_empty() {
            "Add private packs under .benchforge/benchmark-packs/ when you need domain-specific evaluations."
        } else {
            "Fix or remove invalid custom benchmark packs; built-in packs remain usable when the main Benchmark packs check is ok."
        },
        ".benchforge/benchmark-packs",
    )
}

fn benchmark_readiness_doctor_checks(
    targets: &[store::TargetRecord],
    results: &[store::ResultRecord],
    packs: Result<Vec<runner::BenchmarkPackDto>, String>,
    loaded_adapters: Result<Vec<adapters::LoadedAdapter>, String>,
) -> Vec<DoctorCheckDto> {
    let mut checks = Vec::new();
    match packs {
        Ok(packs) => {
            let heavy = packs.iter().filter(|pack| pack.heavy).count();
            checks.push(doctor_check(
                "benchmark-packs",
                "Benchmark packs",
                if packs.is_empty() { "error" } else { "ok" },
                if packs.is_empty() {
                    "no benchmark packs were found".to_string()
                } else {
                    format!("{} packs available, {} marked heavy", packs.len(), heavy)
                }
                .as_str(),
                "Benchmark readiness",
                "required",
                if packs.is_empty() {
                    "Restore benchmark-packs/ before queueing benchmark jobs."
                } else {
                    "Use Benchmarks to inspect duration, tools, and supported target kinds."
                },
                "",
            ));
        }
        Err(err) => checks.push(doctor_check(
            "benchmark-packs",
            "Benchmark packs",
            "error",
            &err,
            "Benchmark readiness",
            "required",
            "Fix benchmark pack loading before queueing benchmark jobs.",
            "",
        )),
    }

    let adapter_map = match loaded_adapters {
        Ok(adapters) => adapters
            .into_iter()
            .map(|adapter| (adapter.spec.id.clone(), adapter))
            .collect::<BTreeMap<_, _>>(),
        Err(err) => {
            checks.push(doctor_check(
                "benchmark-adapters",
                "Benchmark adapters",
                "error",
                &err,
                "Benchmark readiness",
                "required",
                "Fix adapter YAML loading before queueing benchmark jobs.",
                "",
            ));
            BTreeMap::new()
        }
    };

    let enabled_targets = targets
        .iter()
        .filter(|target| target.enabled)
        .collect::<Vec<_>>();
    let enabled_local_targets = enabled_targets
        .iter()
        .filter(|target| target_is_local_benchmark_model(target, &adapter_map))
        .count();
    let enabled_cloud_targets = enabled_targets
        .iter()
        .filter(|target| target_is_cloud_benchmark_model(target, &adapter_map))
        .count();
    let failing_local_targets = enabled_targets
        .iter()
        .filter(|target| {
            target_is_local_benchmark_model(target, &adapter_map)
                && target_validation_is_error(target)
        })
        .count();
    let failing_cloud_targets = enabled_targets
        .iter()
        .filter(|target| {
            target_is_cloud_benchmark_model(target, &adapter_map)
                && target_validation_is_error(target)
        })
        .count();
    let local_targets = enabled_local_targets.saturating_sub(failing_local_targets);
    let cloud_targets = enabled_cloud_targets.saturating_sub(failing_cloud_targets);
    let comparison_evidence = local_cloud_comparison_evidence(results, targets, &adapter_map);

    checks.push(doctor_check(
        "benchmark-target-local",
        "Local model target",
        if local_targets > 0 { "ok" } else { "warn" },
        target_readiness_detail(
            local_targets,
            failing_local_targets,
            "local model target",
            "no enabled local model target found",
        )
        .as_str(),
        "Benchmark readiness",
        "recommended",
        if local_targets > 0 {
            "Use Runs to benchmark local targets against compatible packs."
        } else if failing_local_targets > 0 {
            "Revalidate or edit the failing local target before using it in benchmark shortcuts."
        } else {
            "Use Settings > Hugging Face Local Model, Targets > Local Runtimes > Detect, or add a Generic OpenAI-compatible local endpoint."
        },
        "",
    ));
    checks.push(doctor_check(
        "benchmark-target-cloud",
        "Cloud model target",
        if cloud_targets > 0 { "ok" } else { "warn" },
        target_readiness_detail(
            cloud_targets,
            failing_cloud_targets,
            "cloud model target",
            "no enabled cloud model target found",
        )
        .as_str(),
        "Benchmark readiness",
        "recommended",
        if cloud_targets > 0 {
            "Use Runs to compare cloud targets with local targets."
        } else if failing_cloud_targets > 0 {
            "Revalidate or edit the failing cloud target before using it in benchmark shortcuts."
        } else {
            "Use Targets > Model Target to add OpenAI, Anthropic, Mistral, OpenRouter, Azure OpenAI, Google Gemini, or another remote OpenAI-compatible endpoint."
        },
        "",
    ));
    let compare_ready = local_targets > 0 && cloud_targets > 0;
    checks.push(doctor_check(
        "benchmark-local-cloud-compare",
        "Local + cloud comparison",
        if compare_ready { "ok" } else { "warn" },
        if compare_ready {
            format!(
                "ready to compare {} local and {} cloud model target(s)",
                local_targets, cloud_targets
            )
        } else if local_targets == 0 && cloud_targets == 0 {
            local_cloud_readiness_missing_detail(
                failing_local_targets,
                failing_cloud_targets,
                "needs one ready local model target and one ready cloud model target",
            )
        } else if local_targets == 0 {
            local_cloud_readiness_missing_detail(
                failing_local_targets,
                0,
                "needs a ready local model target before local/cloud comparison",
            )
        } else {
            local_cloud_readiness_missing_detail(
                0,
                failing_cloud_targets,
                "needs a ready cloud model target before local/cloud comparison",
            )
        }
        .as_str(),
        "Benchmark readiness",
        "recommended",
        if compare_ready {
            "Use Runs > Local + cloud with LLM Basics for first model-selection evidence; use LLM Connectivity only for endpoint sanity checks."
        } else {
            "Add both local and cloud model targets, validate them, then run LLM Basics with the local/cloud shortcut."
        },
        "Runs > Local + cloud",
    ));
    checks.push(doctor_check(
        "benchmark-local-cloud-results",
        "Local + cloud result",
        if comparison_evidence.is_some() {
            "ok"
        } else {
            "warn"
        },
        comparison_evidence
            .as_ref()
            .map(|evidence| {
                format!(
                    "latest comparison group {} used {} local and {} cloud result row(s), {} passed out of {}",
                    short_id(&evidence.group_id),
                    evidence.local_rows,
                    evidence.cloud_rows,
                    evidence.passed_rows,
                    evidence.total_rows
                )
            })
            .unwrap_or_else(|| {
                if compare_ready {
                    "no local/cloud comparison result group found".to_string()
                } else {
                    "local/cloud comparison results need one local and one cloud target first"
                        .to_string()
                }
            })
            .as_str(),
        "Benchmark readiness",
        "recommended",
        if comparison_evidence.is_some() {
            "Open Results to inspect the comparison and export a reproducible report."
        } else if compare_ready {
            "Run LLM Basics with one local and one cloud target, 3 repetitions, 1 warmup, and a max-cost cap before choosing a model."
        } else {
            "Add both local and cloud model targets, validate them, then run LLM Basics with the local/cloud shortcut."
        },
        "Results",
    ));
    checks.push(doctor_check(
        "benchmark-local-cloud-evidence",
        "Comparison evidence quality",
        comparison_evidence
            .as_ref()
            .map(local_cloud_evidence_status)
            .unwrap_or("warn"),
        comparison_evidence
            .as_ref()
            .map(local_cloud_evidence_detail)
            .unwrap_or_else(|| {
                if compare_ready {
                    "no local/cloud result group is available to assess coverage or repetitions"
                        .to_string()
                } else {
                    "add one local and one cloud target before generating comparison evidence"
                        .to_string()
                }
            })
            .as_str(),
        "Benchmark readiness",
        "recommended",
        comparison_evidence
            .as_ref()
            .map(local_cloud_evidence_remediation)
            .unwrap_or_else(|| {
                if compare_ready {
                    "Run the same prompt pack against local and cloud targets with at least 3 repetitions per task/target.".to_string()
                } else {
                    "Add and validate both local and cloud model targets first.".to_string()
                }
            })
            .as_str(),
        if comparison_evidence.is_some() {
            "Results"
        } else {
            "Runs > Local + cloud"
        },
    ));
    checks.push(next_benchmark_step_check(
        local_targets,
        cloud_targets,
        failing_local_targets,
        failing_cloud_targets,
        comparison_evidence.as_ref(),
    ));

    checks
}

fn next_benchmark_step_check(
    local_targets: usize,
    cloud_targets: usize,
    failing_local_targets: usize,
    failing_cloud_targets: usize,
    comparison_evidence: Option<&LocalCloudComparisonEvidence>,
) -> DoctorCheckDto {
    let (status, detail, remediation, command) = if local_targets == 0 {
        let detail = local_cloud_readiness_missing_detail(
            failing_local_targets,
            0,
            "add and validate one local model target",
        );
        if failing_local_targets > 0 {
            (
                "warn",
                detail,
                "Open Targets, edit or revalidate the failing local target; for endpoint errors, start the local runtime or use Settings > Hugging Face Local Model to restart/register it.".to_string(),
                "Targets > Repair local target",
            )
        } else {
            (
                "warn",
                detail,
                "Use Settings > Hugging Face Local Model for GGUF/llama.cpp, or Targets > Local Runtimes > Detect for an existing local server.".to_string(),
                "Settings > Hugging Face Local Model",
            )
        }
    } else if cloud_targets == 0 {
        let detail = local_cloud_readiness_missing_detail(
            0,
            failing_cloud_targets,
            "add and validate one cloud model target",
        );
        if failing_cloud_targets > 0 {
            (
                "warn",
                detail,
                "Open Targets, fix the failing cloud target's key, endpoint, or model name, then revalidate with a tiny completion probe.".to_string(),
                "Targets > Repair cloud target",
            )
        } else {
            (
                "warn",
                detail,
                "Use Targets > Model Target, save the provider key in Keychain, and validate with a tiny completion probe.".to_string(),
                "Targets > Model Target",
            )
        }
    } else if let Some(evidence) = comparison_evidence {
        if local_cloud_evidence_status(evidence) == "ok" {
            (
                "ok",
                format!(
                    "comparison evidence is ready in group {}; inspect and export the report",
                    short_id(&evidence.group_id)
                ),
                "Open Results, review task drilldowns and confidence intervals, then export a report folder for reproducibility.".to_string(),
                "Results > Export report",
            )
        } else {
            (
                "warn",
                local_cloud_evidence_detail(evidence),
                local_cloud_evidence_remediation(evidence),
                "Runs > Local + cloud",
            )
        }
    } else {
        (
            "warn",
            format!(
                "run LLM Basics against {} local and {} cloud target(s)",
                local_targets, cloud_targets
            ),
            "Use Runs > Local + cloud with 3 repetitions, 1 warmup, and a max-cost cap; BenchForge seeds LLM Basics by default."
                .to_string(),
            "Runs > Local + cloud",
        )
    };
    doctor_check(
        "benchmark-next-step",
        "Next benchmark step",
        status,
        &detail,
        "Benchmark readiness",
        "recommended",
        &remediation,
        command,
    )
}

fn target_validation_is_error(target: &store::TargetRecord) -> bool {
    target.validation_status.as_deref() == Some("error")
}

fn target_readiness_detail(
    ready_count: usize,
    failing_count: usize,
    label: &str,
    empty_detail: &str,
) -> String {
    match (ready_count, failing_count) {
        (ready, failing) if ready > 0 && failing > 0 => format!(
            "{} ready enabled {}(s); {} enabled {}(s) last validation failed",
            ready, label, failing, label
        ),
        (ready, _) if ready > 0 => format!("{} ready enabled {}(s)", ready, label),
        (_, failing) if failing > 0 => format!(
            "no ready {} found; {} enabled {}(s) last validation failed",
            label, failing, label
        ),
        _ => empty_detail.to_string(),
    }
}

fn local_cloud_readiness_missing_detail(
    failing_local_targets: usize,
    failing_cloud_targets: usize,
    fallback: &str,
) -> String {
    let mut details = Vec::new();
    if failing_local_targets > 0 {
        details.push(format!(
            "{} local target(s) last validation failed",
            failing_local_targets
        ));
    }
    if failing_cloud_targets > 0 {
        details.push(format!(
            "{} cloud target(s) last validation failed",
            failing_cloud_targets
        ));
    }
    if details.is_empty() {
        fallback.to_string()
    } else {
        format!("{}; {}", fallback, details.join("; "))
    }
}

#[derive(Debug)]
struct LocalCloudComparisonEvidence {
    group_id: String,
    local_rows: usize,
    cloud_rows: usize,
    passed_rows: usize,
    total_rows: usize,
    last_started_at: String,
    pack_ids: BTreeSet<String>,
    target_slot_counts: BTreeMap<String, BTreeMap<String, usize>>,
    target_costed_rows: BTreeMap<String, usize>,
    target_pricing_assumption_counts: BTreeMap<String, BTreeMap<String, usize>>,
    target_provider_model_counts: BTreeMap<String, BTreeMap<String, usize>>,
    target_provider_model_source_counts: BTreeMap<String, BTreeMap<String, usize>>,
    generation_setting_counts: BTreeMap<String, usize>,
}

fn local_cloud_comparison_evidence(
    results: &[store::ResultRecord],
    targets: &[store::TargetRecord],
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> Option<LocalCloudComparisonEvidence> {
    let target_map = targets
        .iter()
        .map(|target| (target.id.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let mut groups = BTreeMap::<String, LocalCloudComparisonEvidence>::new();

    for result in results {
        let Some(group_id) = result
            .run_group_id
            .as_ref()
            .filter(|id| !id.trim().is_empty())
        else {
            continue;
        };
        let is_local = result_is_local_benchmark_model(result, &target_map, adapter_map);
        let is_cloud = result_is_cloud_benchmark_model(result, &target_map, adapter_map);
        if !is_local && !is_cloud {
            continue;
        }
        let entry =
            groups
                .entry(group_id.clone())
                .or_insert_with(|| LocalCloudComparisonEvidence {
                    group_id: group_id.clone(),
                    local_rows: 0,
                    cloud_rows: 0,
                    passed_rows: 0,
                    total_rows: 0,
                    last_started_at: String::new(),
                    pack_ids: BTreeSet::new(),
                    target_slot_counts: BTreeMap::new(),
                    target_costed_rows: BTreeMap::new(),
                    target_pricing_assumption_counts: BTreeMap::new(),
                    target_provider_model_counts: BTreeMap::new(),
                    target_provider_model_source_counts: BTreeMap::new(),
                    generation_setting_counts: BTreeMap::new(),
                });
        if is_local {
            entry.local_rows += 1;
        }
        if is_cloud {
            entry.cloud_rows += 1;
        }
        if result.status == "passed" {
            entry.passed_rows += 1;
        }
        entry.pack_ids.insert(result.benchmark_pack_id.clone());
        let slot = pack_task_slot_id(&result.benchmark_pack_id, &result.task_id);
        *entry
            .target_slot_counts
            .entry(result.target_id.clone())
            .or_default()
            .entry(slot)
            .or_insert(0) += 1;
        if result_has_cost_coverage(result) {
            *entry
                .target_costed_rows
                .entry(result.target_id.clone())
                .or_insert(0) += 1;
        }
        if let Some(assumption) = result
            .pricing_assumption
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            *entry
                .target_pricing_assumption_counts
                .entry(result.target_id.clone())
                .or_default()
                .entry(assumption.to_string())
                .or_insert(0) += 1;
        }
        if let Some(model) = result
            .provider_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            *entry
                .target_provider_model_counts
                .entry(result.target_id.clone())
                .or_default()
                .entry(model.to_string())
                .or_insert(0) += 1;
        }
        if let Some(source) = result
            .provider_model_source
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            *entry
                .target_provider_model_source_counts
                .entry(result.target_id.clone())
                .or_default()
                .entry(source.to_string())
                .or_insert(0) += 1;
        }
        *entry
            .generation_setting_counts
            .entry(result_generation_sampling_fingerprint(result))
            .or_insert(0) += 1;
        entry.total_rows += 1;
        if let Some(started_at) = result.started_at.as_ref() {
            if started_at > &entry.last_started_at {
                entry.last_started_at = started_at.clone();
            }
        }
    }

    groups
        .into_values()
        .filter(|evidence| evidence.local_rows > 0 && evidence.cloud_rows > 0)
        .max_by(|a, b| {
            a.last_started_at
                .cmp(&b.last_started_at)
                .then_with(|| a.group_id.cmp(&b.group_id))
        })
}

fn local_cloud_evidence_status(evidence: &LocalCloudComparisonEvidence) -> &'static str {
    if local_cloud_missing_slot_count(evidence) == 0
        && local_cloud_low_repetition_slots(evidence).0 == 0
        && local_cloud_cost_gap_targets(evidence).is_empty()
        && local_cloud_pricing_assumption_targets(evidence).is_empty()
        && local_cloud_model_identity_issue_targets(evidence).is_empty()
        && !local_cloud_generation_settings_mixed(evidence)
        && !local_cloud_connectivity_only(evidence)
    {
        "ok"
    } else {
        "warn"
    }
}

fn local_cloud_evidence_detail(evidence: &LocalCloudComparisonEvidence) -> String {
    let missing_slots = local_cloud_missing_slot_count(evidence);
    let (low_slots, total_slots) = local_cloud_low_repetition_slots(evidence);
    let cost_gap_targets = local_cloud_cost_gap_targets(evidence);
    let pricing_assumption_targets = local_cloud_pricing_assumption_targets(evidence);
    let model_identity_issue_targets = local_cloud_model_identity_issue_targets(evidence);
    let generation_settings_mixed = local_cloud_generation_settings_mixed(evidence);
    let connectivity_only = local_cloud_connectivity_only(evidence);
    let cost_gap_suffix = if cost_gap_targets.is_empty() {
        String::new()
    } else {
        format!(
            "; {} target(s) are missing cost metrics: {}",
            cost_gap_targets.len(),
            cost_gap_targets.join(", ")
        )
    };
    let connectivity_suffix = if connectivity_only {
        "; only the connectivity pack is in scope"
    } else {
        ""
    };
    let pricing_assumption_suffix = if pricing_assumption_targets.is_empty() {
        String::new()
    } else {
        format!(
            "; {} target(s) have pricing assumptions: {}",
            pricing_assumption_targets.len(),
            pricing_assumption_targets.join(", ")
        )
    };
    let model_identity_suffix = if model_identity_issue_targets.is_empty() {
        String::new()
    } else {
        format!(
            "; {} target(s) have unconfirmed served model identity: {}",
            model_identity_issue_targets.len(),
            model_identity_issue_targets.join(", ")
        )
    };
    let generation_suffix = if generation_settings_mixed {
        format!(
            "; mixed generation policies are present: {}",
            format_text_counts(&evidence.generation_setting_counts)
        )
    } else {
        String::new()
    };
    if missing_slots > 0 {
        return format!(
            "latest group {} is partial: {} pack/task slot(s) are missing across targets; {}/{} target-slot sample(s) have fewer than {} repetition(s){}{}{}{}{}",
            short_id(&evidence.group_id),
            missing_slots,
            low_slots,
            total_slots,
            RECOMMENDED_TASK_REPETITIONS,
            cost_gap_suffix,
            pricing_assumption_suffix,
            model_identity_suffix,
            generation_suffix,
            connectivity_suffix
        );
    }
    if low_slots > 0 {
        return format!(
            "latest group {} has balanced pack/task slots, but {}/{} target-slot sample(s) have fewer than {} repetition(s); treat this as smoke evidence{}{}{}{}{}",
            short_id(&evidence.group_id),
            low_slots,
            total_slots,
            RECOMMENDED_TASK_REPETITIONS,
            cost_gap_suffix,
            pricing_assumption_suffix,
            model_identity_suffix,
            generation_suffix,
            connectivity_suffix
        );
    }
    if !cost_gap_targets.is_empty() {
        return format!(
            "latest group {} has balanced pack/task slots and at least {} repetition(s), but {} target(s) are missing cost metrics: {}; treat cost ranking as incomplete{}{}{}{}",
            short_id(&evidence.group_id),
            RECOMMENDED_TASK_REPETITIONS,
            cost_gap_targets.len(),
            cost_gap_targets.join(", "),
            pricing_assumption_suffix,
            model_identity_suffix,
            generation_suffix,
            connectivity_suffix
        );
    }
    if !pricing_assumption_targets.is_empty() {
        return format!(
            "latest group {} has balanced pack/task slots, cost metrics, and at least {} repetition(s), but {} target(s) have pricing assumptions: {}; add cache read/write pricing before treating cost ranking as decisive{}{}",
            short_id(&evidence.group_id),
            RECOMMENDED_TASK_REPETITIONS,
            pricing_assumption_targets.len(),
            pricing_assumption_targets.join(", "),
            model_identity_suffix,
            generation_suffix
        );
    }
    if !model_identity_issue_targets.is_empty() {
        return format!(
            "latest group {} has balanced pack/task slots, cost metrics, and at least {} repetition(s), but {} target(s) have unconfirmed served model identity: {}; treat model selection as directional until providers confirm the served model id{}{}",
            short_id(&evidence.group_id),
            RECOMMENDED_TASK_REPETITIONS,
            model_identity_issue_targets.len(),
            model_identity_issue_targets.join(", "),
            generation_suffix,
            connectivity_suffix
        );
    }
    if generation_settings_mixed {
        return format!(
            "latest group {} has balanced pack/task slots, cost metrics, confirmed served model identity, and at least {} repetition(s), but mixes generation settings: {}; treat model selection as directional until one temperature, top_p, and seed policy is used",
            short_id(&evidence.group_id),
            RECOMMENDED_TASK_REPETITIONS,
            format_text_counts(&evidence.generation_setting_counts)
        );
    }
    if connectivity_only {
        return format!(
            "latest group {} only ran llm-connectivity; endpoints respond, but this is smoke evidence rather than model-selection evidence",
            short_id(&evidence.group_id)
        );
    }
    format!(
        "latest group {} has balanced pack/task slots, cost metrics for every compared target, and at least {} repetition(s) for every target-slot sample",
        short_id(&evidence.group_id),
        RECOMMENDED_TASK_REPETITIONS
    )
}

fn local_cloud_evidence_remediation(evidence: &LocalCloudComparisonEvidence) -> String {
    if !local_cloud_cost_gap_targets(evidence).is_empty() {
        if local_cloud_connectivity_only(evidence) {
            return "Add pricing for targets with missing cost metrics, then run a quality pack such as llm-reliability or llm-decision-suite so cost can be compared beside quality and latency.".into();
        }
        return "Add pricing for targets with missing cost metrics, then re-run the same local/cloud comparison so cost can be compared beside quality and latency.".into();
    }
    if !local_cloud_pricing_assumption_targets(evidence).is_empty() {
        return "Add cache read/write pricing for targets with pricing assumptions, then re-run or re-export before using cost ranking as decisive evidence.".into();
    }
    if !local_cloud_model_identity_issue_targets(evidence).is_empty() {
        return "Confirm each target reports a stable provider-supplied served model id, then re-run the same local/cloud comparison before choosing a winner.".into();
    }
    if local_cloud_generation_settings_mixed(evidence) {
        return "Rerun or filter the same local/cloud comparison with one shared generation policy, such as temperature 0, top_p 1, and a consistent seed policy.".into();
    }
    if local_cloud_missing_slot_count(evidence) > 0 {
        return "Re-run the same benchmark pack and task set for every visible local and cloud target before choosing a winner.".into();
    }
    if local_cloud_low_repetition_slots(evidence).0 > 0 {
        return format!(
            "Run the comparison again with at least {} repetitions per task/target for model-selection confidence.",
            RECOMMENDED_TASK_REPETITIONS
        );
    }
    if local_cloud_connectivity_only(evidence) {
        return format!(
            "Run a quality pack such as llm-reliability or llm-decision-suite with at least {} repetitions per task/target before choosing a winner.",
            RECOMMENDED_TASK_REPETITIONS
        );
    }
    "Open Results to inspect confidence intervals, task drilldowns, artifacts, and report exports."
        .into()
}

fn local_cloud_model_identity_issue_targets(
    evidence: &LocalCloudComparisonEvidence,
) -> Vec<String> {
    evidence
        .target_slot_counts
        .iter()
        .filter_map(|(target_id, slots)| {
            let total_rows = slots.values().sum::<usize>();
            let model_counts = evidence.target_provider_model_counts.get(target_id);
            let reported_models = model_counts
                .map(|counts| counts.values().sum::<usize>())
                .unwrap_or(0);
            let inconsistent = model_counts.map(|counts| counts.len() > 1).unwrap_or(false);
            let configured_fallback = evidence
                .target_provider_model_source_counts
                .get(target_id)
                .and_then(|counts| counts.get("target_config"))
                .copied()
                .unwrap_or(0)
                > 0;
            (total_rows > 0
                && (reported_models < total_rows || inconsistent || configured_fallback))
                .then(|| target_id.clone())
        })
        .collect()
}

fn local_cloud_cost_gap_targets(evidence: &LocalCloudComparisonEvidence) -> Vec<String> {
    evidence
        .target_slot_counts
        .iter()
        .filter_map(|(target_id, slots)| {
            let total_rows = slots.values().sum::<usize>();
            let costed_rows = evidence
                .target_costed_rows
                .get(target_id)
                .copied()
                .unwrap_or(0);
            (total_rows > 0 && costed_rows < total_rows).then(|| target_id.clone())
        })
        .collect()
}

fn local_cloud_pricing_assumption_targets(evidence: &LocalCloudComparisonEvidence) -> Vec<String> {
    evidence
        .target_pricing_assumption_counts
        .iter()
        .filter(|(_, assumptions)| !assumptions.is_empty())
        .map(|(target_id, _)| target_id.clone())
        .collect()
}

fn local_cloud_generation_settings_mixed(evidence: &LocalCloudComparisonEvidence) -> bool {
    evidence.generation_setting_counts.len() > 1
}

fn local_cloud_connectivity_only(evidence: &LocalCloudComparisonEvidence) -> bool {
    !evidence.pack_ids.is_empty()
        && evidence
            .pack_ids
            .iter()
            .all(|pack_id| pack_id == "llm-connectivity")
}

fn local_cloud_missing_slot_count(evidence: &LocalCloudComparisonEvidence) -> usize {
    let all_slots = evidence
        .target_slot_counts
        .values()
        .flat_map(|slots| slots.keys().cloned())
        .collect::<BTreeSet<_>>();
    evidence
        .target_slot_counts
        .values()
        .map(|slots| {
            all_slots
                .iter()
                .filter(|slot| !slots.contains_key(slot.as_str()))
                .count()
        })
        .sum()
}

fn local_cloud_low_repetition_slots(evidence: &LocalCloudComparisonEvidence) -> (usize, usize) {
    let total = evidence
        .target_slot_counts
        .values()
        .map(BTreeMap::len)
        .sum::<usize>();
    let low = evidence
        .target_slot_counts
        .values()
        .flat_map(|slots| slots.values())
        .filter(|count| **count < RECOMMENDED_TASK_REPETITIONS)
        .count();
    (low, total)
}

fn target_is_local_benchmark_model(
    target: &store::TargetRecord,
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> bool {
    let config = target_config_json(target);
    target_parts_are_local_benchmark_model(&target.kind, &target.adapter_id, &config, adapter_map)
}

fn target_is_cloud_benchmark_model(
    target: &store::TargetRecord,
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> bool {
    let config = target_config_json(target);
    target_parts_are_cloud_benchmark_model(&target.kind, &target.adapter_id, &config, adapter_map)
}

fn result_is_local_benchmark_model(
    result: &store::ResultRecord,
    target_map: &BTreeMap<&str, &store::TargetRecord>,
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> bool {
    if let Some(target) = target_map.get(result.target_id.as_str()) {
        return target_is_local_benchmark_model(target, adapter_map);
    }
    let Some((kind, adapter_id, config)) = result_reproducibility_target_parts(result) else {
        return false;
    };
    target_parts_are_local_benchmark_model(&kind, &adapter_id, &config, adapter_map)
}

fn result_is_cloud_benchmark_model(
    result: &store::ResultRecord,
    target_map: &BTreeMap<&str, &store::TargetRecord>,
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> bool {
    if let Some(target) = target_map.get(result.target_id.as_str()) {
        return target_is_cloud_benchmark_model(target, adapter_map);
    }
    let Some((kind, adapter_id, config)) = result_reproducibility_target_parts(result) else {
        return false;
    };
    target_parts_are_cloud_benchmark_model(&kind, &adapter_id, &config, adapter_map)
}

fn result_reproducibility_target_parts(
    result: &store::ResultRecord,
) -> Option<(String, String, serde_json::Value)> {
    let target = result.reproducibility.get("target")?;
    let kind = target
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("direct_model")
        .to_string();
    let adapter_id = target
        .get("adapter_id")
        .and_then(|value| value.as_str())
        .or_else(|| target.get("adapterId").and_then(|value| value.as_str()))?
        .to_string();
    let config = target
        .get("config")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Some((kind, adapter_id, config))
}

fn target_parts_are_local_benchmark_model(
    kind: &str,
    adapter_id: &str,
    config: &serde_json::Value,
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> bool {
    if !matches!(kind, "direct_model" | "harnessed_model") {
        return false;
    }
    if config
        .get("source")
        .and_then(|value| value.as_str())
        .is_some_and(|source| source == "huggingface-local")
    {
        return true;
    }
    let adapter = adapter_map.get(adapter_id);
    if config_base_url_is_remote(config) {
        return false;
    }
    if config_base_url_is_local(config) {
        return !adapter.is_some_and(adapter_is_cloud_model_adapter);
    }
    adapter.is_some_and(adapter_is_local_model_adapter)
}

fn target_parts_are_cloud_benchmark_model(
    kind: &str,
    adapter_id: &str,
    config: &serde_json::Value,
    adapter_map: &BTreeMap<String, adapters::LoadedAdapter>,
) -> bool {
    if !matches!(kind, "direct_model" | "harnessed_model") {
        return false;
    }
    if config_base_url_is_remote(config) {
        return true;
    }
    adapter_map
        .get(adapter_id)
        .is_some_and(|adapter| adapter_is_cloud_model_adapter(adapter))
}

fn benchmark_adapter_map() -> BTreeMap<String, adapters::LoadedAdapter> {
    adapters::load_builtin_adapters()
        .unwrap_or_default()
        .into_iter()
        .map(|adapter| (adapter.spec.id.clone(), adapter))
        .collect()
}

fn target_config_json(target: &store::TargetRecord) -> serde_json::Value {
    serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}))
}

fn public_target_config_value(config: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = config.get(*key)?.as_str()?.trim();
        if value.is_empty() {
            return None;
        }
        Some(short_public_target_value(&safety::redact_sensitive_text(
            value,
        )))
    })
}

fn short_public_target_value(value: &str) -> String {
    let mut chars = value.chars();
    let shortened = chars.by_ref().take(240).collect::<String>();
    if chars.next().is_some() {
        format!("{}...", shortened)
    } else {
        shortened
    }
}

fn adapter_is_local_model_adapter(adapter: &adapters::LoadedAdapter) -> bool {
    adapter
        .path
        .components()
        .any(|component| component.as_os_str() == "local")
        || adapter
            .spec
            .security
            .get("network_required")
            .and_then(|value| value.as_str())
            .is_some_and(|network| network == "local")
}

fn adapter_is_cloud_model_adapter(adapter: &adapters::LoadedAdapter) -> bool {
    adapter
        .spec
        .validation
        .get("secret_env")
        .and_then(|value| value.as_str())
        .is_some()
        || adapter
            .spec
            .security
            .get("network_required")
            .and_then(|value| value.as_str())
            .is_some_and(|network| network == "internet")
}

fn config_base_url_is_local(config: &serde_json::Value) -> bool {
    config
        .get("base_url")
        .and_then(|value| value.as_str())
        .is_some_and(is_local_base_url)
}

fn config_base_url_is_remote(config: &serde_json::Value) -> bool {
    config
        .get("base_url")
        .and_then(|value| value.as_str())
        .is_some_and(|base_url| base_url.starts_with("http") && !is_local_base_url(base_url))
}

fn is_local_base_url(base_url: &str) -> bool {
    let lower = base_url.to_ascii_lowercase();
    lower.contains("://localhost") || lower.contains("://127.0.0.1") || lower.contains("://0.0.0.0")
}

fn doctor_check(
    id: &str,
    label: &str,
    status: &str,
    detail: &str,
    category: &str,
    importance: &str,
    remediation: &str,
    command: &str,
) -> DoctorCheckDto {
    DoctorCheckDto {
        id: id.into(),
        label: label.into(),
        status: status.into(),
        detail: detail.into(),
        category: category.into(),
        importance: importance.into(),
        remediation: remediation.into(),
        command: command.into(),
    }
}

fn first_output_line(output: &Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout
        .lines()
        .chain(stderr.lines())
        .map(strip_ansi_codes)
        .map(|line| line.trim().to_string())
        .find(|line| !line.is_empty())
}

fn parse_python_version(detail: &str) -> Option<(u32, u32, u32)> {
    let version = detail.split_whitespace().find(|part| {
        part.chars()
            .next()
            .is_some_and(|char| char.is_ascii_digit())
    })?;
    let mut parts = version
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok());
    Some((
        parts.next()?,
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ))
}

fn local_base_url_port_is_open(base_url: &str) -> bool {
    let Some(port) = local_base_url_port(base_url) else {
        return false;
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

fn local_base_url_port(base_url: &str) -> Option<u16> {
    let rest = base_url
        .strip_prefix("http://localhost:")
        .or_else(|| base_url.strip_prefix("http://127.0.0.1:"))?;
    rest.split('/').next()?.parse().ok()
}

fn validate_target_record(target: &store::TargetRecord) -> Result<TargetValidationDto, String> {
    if !target.enabled {
        return Ok(target_validation(target, "error", "target is disabled"));
    }
    if target.kind == "mock" {
        return Ok(target_validation(
            target,
            "ok",
            "mock target is deterministic",
        ));
    }

    let Some(adapter) = adapters::find_adapter(&target.adapter_id)? else {
        return Ok(target_validation(
            target,
            "error",
            &format!("adapter {} not found", target.adapter_id),
        ));
    };
    let config: serde_json::Value =
        serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}));

    match adapter.spec.kind.as_str() {
        "cli_agent" => validate_cli_target(target, &adapter.spec),
        "benchmark_harness" => validate_benchmark_harness_target(target, &adapter.spec, &config),
        "openai_responses" => validate_openai_responses_target(target, &adapter.spec, &config),
        "openai_compatible" | "mistral_api" => {
            validate_openai_compatible_target(target, &adapter.spec, &config)
        }
        "azure_openai" => validate_azure_openai_target(target, &adapter.spec, &config),
        "anthropic_messages" => validate_anthropic_target(target, &adapter.spec, &config),
        other => Ok(target_validation(
            target,
            "warn",
            &format!("no validator implemented for adapter kind {}", other),
        )),
    }
}

fn validate_benchmark_harness_target(
    target: &store::TargetRecord,
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> Result<TargetValidationDto, String> {
    let Some(worker_command) = adapter.command.as_deref() else {
        return Ok(target_validation(
            target,
            "error",
            "benchmark_harness adapter has no worker command",
        ));
    };
    if !benchmark_worker_command_exists(worker_command) {
        return Ok(target_validation(
            target,
            "error",
            &format!(
                "tool_missing: {} was not found; run bootstrap or install BenchForge Worker",
                worker_command
            ),
        ));
    }
    let Some(command) = harness_command_executable_for_validation(config) else {
        return Ok(target_validation(
            target,
            "warn",
            "BenchForge Worker is available; configure harness.command before running EvalPlus, Aider, Terminal-Bench, SWE-bench, or private external harness packs. Internal security worker packs can run with this target.",
        ));
    };
    if command.trim().is_empty() {
        return Ok(target_validation(
            target,
            "error",
            "configuration_invalid: harness.command must not be blank",
        ));
    }
    if !command.contains('{') && !adapters::command_exists(&command) {
        return Ok(target_validation(
            target,
            "error",
            &format!(
                "tool_missing: configured harness command executable not found: {}",
                command
            ),
        ));
    }
    Ok(target_validation(
        target,
        "ok",
        &format!(
            "{} and harness command {} are available",
            worker_command, command
        ),
    ))
}

fn benchmark_worker_command_exists(command: &str) -> bool {
    adapters::command_exists(command)
        || (command == "benchforge-worker"
            && (paths::worker_venv_launcher().exists()
                || paths::bundled_worker_launcher().exists()))
}

fn harness_command_executable_for_validation(config: &serde_json::Value) -> Option<String> {
    let command = config
        .get("harness")
        .and_then(|harness| harness.get("command"))
        .or_else(|| config.get("harness_command"))?;
    if let Some(command) = command.as_str() {
        return command.split_whitespace().next().map(str::to_string);
    }
    command
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

fn validate_cli_target(
    target: &store::TargetRecord,
    adapter: &adapters::AdapterSpec,
) -> Result<TargetValidationDto, String> {
    let Some(command) = adapter.command.as_deref() else {
        return Ok(target_validation(
            target,
            "error",
            "CLI adapter has no command",
        ));
    };
    if adapters::command_exists(command) {
        Ok(target_validation(
            target,
            "ok",
            &format!("{} is available in PATH", command),
        ))
    } else {
        Ok(target_validation(
            target,
            "error",
            &format!("{} was not found in PATH", command),
        ))
    }
}

fn validate_openai_responses_target(
    target: &store::TargetRecord,
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> Result<TargetValidationDto, String> {
    let Some(model) = config.get("model").and_then(|value| value.as_str()) else {
        return Ok(target_validation(target, "error", "model is missing"));
    };
    let Some(base_url) = config
        .get("base_url")
        .and_then(|value| value.as_str())
        .or(adapter.default_base_url.as_deref())
    else {
        return Ok(target_validation(target, "error", "base_url is missing"));
    };
    let Some(api_key) = resolve_api_key(adapter, config) else {
        return Ok(target_validation(
            target,
            "error",
            "missing_key: API key is missing; save one for this adapter or set the configured env var",
        ));
    };
    let headers = vec![("Authorization".to_string(), format!("Bearer {}", api_key))];
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let body = match curl_get(&url, &headers) {
        Ok(body) => body,
        Err(err) => {
            return Ok(target_validation(
                target,
                "error",
                &format_validation_failure("endpoint check", &err),
            ));
        }
    };
    let list_validation = model_list_validation(target, &body, model);
    match openai_responses_completion_probe(base_url, model, &headers) {
        Ok(()) => Ok(target_validation(
            target,
            "ok",
            &format!("Responses probe succeeded; {}", list_validation.detail),
        )),
        Err(err) => Ok(target_validation(
            target,
            "error",
            &format_validation_failure("Responses probe", &err),
        )),
    }
}

fn validate_openai_compatible_target(
    target: &store::TargetRecord,
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> Result<TargetValidationDto, String> {
    let Some(model) = config.get("model").and_then(|value| value.as_str()) else {
        return Ok(target_validation(target, "error", "model is missing"));
    };
    let Some(base_url) = config
        .get("base_url")
        .and_then(|value| value.as_str())
        .or(adapter.default_base_url.as_deref())
    else {
        return Ok(target_validation(target, "error", "base_url is missing"));
    };

    let requires_key = adapter
        .validation
        .get("secret_env")
        .and_then(|value| value.as_str())
        .is_some()
        || matches!(adapter.kind.as_str(), "openai_responses" | "mistral_api")
        || (adapter.kind == "openai_compatible" && validation_base_url_is_remote(base_url));
    let api_key = resolve_api_key(adapter, config);
    if requires_key && api_key.is_none() {
        return Ok(target_validation(
            target,
            "error",
            &format!(
                "missing_key: API key is missing; {}",
                validation_key_remediation(adapter, config)
            ),
        ));
    }

    let mut headers = Vec::new();
    if let Some(key) = api_key {
        headers.push(("Authorization".to_string(), format!("Bearer {}", key)));
    }
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let body = match curl_get(&url, &headers) {
        Ok(body) => body,
        Err(err) => {
            return Ok(target_validation(
                target,
                "error",
                &format_validation_failure("endpoint check", &err),
            ));
        }
    };
    let list_validation = model_list_validation(target, &body, model);
    match openai_completion_probe(base_url, model, &headers) {
        Ok(()) => Ok(target_validation(
            target,
            "ok",
            &format!("completion probe succeeded; {}", list_validation.detail),
        )),
        Err(err) => Ok(target_validation(
            target,
            "error",
            &format_validation_failure("completion probe", &err),
        )),
    }
}

fn validation_base_url_is_remote(base_url: &str) -> bool {
    let lower = base_url.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && !is_local_base_url(&lower)
}

fn validation_key_remediation(
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> String {
    let env_var = config
        .get("api_key_env")
        .and_then(|value| value.as_str())
        .or_else(|| {
            adapter
                .validation
                .get("secret_env")
                .and_then(|value| value.as_str())
        });
    let keychain = config
        .get("api_key_keychain")
        .and_then(|value| value.as_str());
    match (keychain, env_var) {
        (Some(keychain), Some(env_var)) => {
            format!("save one for {keychain} in Keychain or set {env_var}")
        }
        (Some(keychain), None) => {
            format!("save one for {keychain} in Keychain or configure api_key_env")
        }
        (None, Some(env_var)) => format!("save one for this adapter or set {env_var}"),
        (None, None) => "save one for this adapter or configure api_key_env".into(),
    }
}

fn validate_azure_openai_target(
    target: &store::TargetRecord,
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> Result<TargetValidationDto, String> {
    let Some(model) = config.get("model").and_then(|value| value.as_str()) else {
        return Ok(target_validation(
            target,
            "error",
            "deployment/model is missing",
        ));
    };
    let Some(base_url) = config
        .get("base_url")
        .and_then(|value| value.as_str())
        .or(adapter.default_base_url.as_deref())
    else {
        return Ok(target_validation(
            target,
            "error",
            "Azure OpenAI base_url is missing",
        ));
    };
    if base_url.contains("YOUR-RESOURCE-NAME") {
        return Ok(target_validation(
            target,
            "error",
            "replace the Azure OpenAI base URL placeholder with your resource endpoint",
        ));
    }
    let Some(api_key) = resolve_api_key(adapter, config) else {
        return Ok(target_validation(
            target,
            "error",
            "missing_key: Azure OpenAI API key is missing; save one for this adapter or set AZURE_OPENAI_API_KEY",
        ));
    };
    match azure_openai_completion_probe(base_url, model, &api_key, config) {
        Ok(()) => Ok(target_validation(
            target,
            "ok",
            "Azure OpenAI completion probe succeeded",
        )),
        Err(err) => Ok(target_validation(
            target,
            "error",
            &format_validation_failure("Azure OpenAI completion probe", &err),
        )),
    }
}

fn validate_anthropic_target(
    target: &store::TargetRecord,
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> Result<TargetValidationDto, String> {
    let Some(model) = config.get("model").and_then(|value| value.as_str()) else {
        return Ok(target_validation(target, "error", "model is missing"));
    };
    let Some(api_key) = resolve_api_key(adapter, config) else {
        return Ok(target_validation(
            target,
            "error",
            "missing_key: Anthropic API key is missing; save one for this adapter or set ANTHROPIC_API_KEY",
        ));
    };
    let base_url = config
        .get("base_url")
        .and_then(|value| value.as_str())
        .or(adapter.default_base_url.as_deref())
        .unwrap_or("https://api.anthropic.com");
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let body = match curl_get(
        &url,
        &[
            ("x-api-key".into(), api_key.clone()),
            ("anthropic-version".into(), "2023-06-01".into()),
        ],
    ) {
        Ok(body) => body,
        Err(err) => {
            return Ok(target_validation(
                target,
                "error",
                &format_validation_failure("endpoint check", &err),
            ));
        }
    };
    let list_validation = model_list_validation(target, &body, model);
    match anthropic_completion_probe(base_url, model, &api_key) {
        Ok(()) => Ok(target_validation(
            target,
            "ok",
            &format!("completion probe succeeded; {}", list_validation.detail),
        )),
        Err(err) => Ok(target_validation(
            target,
            "error",
            &format_validation_failure("completion probe", &err),
        )),
    }
}

fn resolve_api_key(adapter: &adapters::AdapterSpec, config: &serde_json::Value) -> Option<String> {
    config
        .get("api_key_keychain")
        .and_then(|value| value.as_str())
        .and_then(secrets::read_cloud_api_key)
        .or_else(|| {
            config
                .get("api_key_env")
                .and_then(|value| value.as_str())
                .and_then(|name| std::env::var(name).ok())
        })
        .or_else(|| {
            adapter
                .validation
                .get("secret_env")
                .and_then(|value| value.as_str())
                .and_then(|name| std::env::var(name).ok())
        })
}

fn adapter_model_preset_catalog(
    adapter: &adapters::AdapterSpec,
    query: &str,
) -> Vec<CloudModelDto> {
    adapter
        .metadata
        .get("model_presets")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let model = item.get("model").and_then(|value| value.as_str())?;
            let name = item
                .get("label")
                .and_then(|value| value.as_str())
                .unwrap_or(model);
            if !query_matches_model(query, model, name) {
                return None;
            }
            Some(CloudModelDto {
                model: model.into(),
                name: name.into(),
                provider: adapter.name.clone(),
                input_price_usd_per_million_tokens: item
                    .get("input_price_usd_per_million_tokens")
                    .and_then(|value| value.as_f64()),
                output_price_usd_per_million_tokens: item
                    .get("output_price_usd_per_million_tokens")
                    .and_then(|value| value.as_f64()),
                cache_read_price_usd_per_million_tokens: item
                    .get("cache_read_price_usd_per_million_tokens")
                    .or_else(|| item.get("cached_input_price_usd_per_million_tokens"))
                    .and_then(|value| value.as_f64()),
                cache_write_price_usd_per_million_tokens: item
                    .get("cache_write_price_usd_per_million_tokens")
                    .or_else(|| item.get("cache_creation_price_usd_per_million_tokens"))
                    .and_then(|value| value.as_f64()),
                context_length: item.get("context_length").and_then(|value| value.as_u64()),
                source: "adapter-preset".into(),
                source_url: item
                    .get("source")
                    .or_else(|| adapter.metadata.get("pricing_source"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                detail: item
                    .get("note")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
            })
        })
        .collect()
}

fn supports_live_cloud_catalog(adapter: &adapters::AdapterSpec) -> bool {
    matches!(
        adapter.id.as_str(),
        "openai"
            | "anthropic"
            | "mistral"
            | "openrouter"
            | "azure-openai"
            | "gemini"
            | "openai-compatible"
    )
}

fn live_cloud_model_catalog(
    adapter: &adapters::AdapterSpec,
    query: &str,
    limit: usize,
    presets: &[CloudModelDto],
    request: &CloudModelSearchRequest,
) -> Result<Vec<CloudModelDto>, String> {
    match adapter.id.as_str() {
        "openai" => openai_model_catalog(adapter, query, limit, presets, request),
        "anthropic" => anthropic_model_catalog(adapter, query, limit, presets, request),
        "mistral" => mistral_model_catalog(adapter, query, limit, presets, request),
        "openrouter" => openrouter_model_catalog(query, limit),
        "azure-openai" => azure_openai_model_catalog(adapter, query, limit, presets, request),
        "gemini" => gemini_model_catalog(adapter, query, limit, presets, request),
        "openai-compatible" => openai_compatible_model_catalog(adapter, query, limit, request),
        _ => Ok(Vec::new()),
    }
}

fn openai_model_catalog(
    adapter: &adapters::AdapterSpec,
    query: &str,
    limit: usize,
    presets: &[CloudModelDto],
    request: &CloudModelSearchRequest,
) -> Result<Vec<CloudModelDto>, String> {
    let api_key = catalog_api_key(adapter, request)?.ok_or_else(|| {
        "OpenAI model search needs a saved OpenAI key or OPENAI_API_KEY".to_string()
    })?;
    let base_url = adapter
        .default_base_url
        .as_deref()
        .unwrap_or("https://api.openai.com/v1")
        .trim_end_matches('/');
    let body = curl_get_with_timeout(
        &format!("{}/models", base_url),
        &[("Authorization".into(), format!("Bearer {}", api_key))],
        20,
    )?;
    let mut models = parse_provider_models(
        &body,
        query,
        limit,
        &adapter.name,
        "openai-live",
        adapter
            .metadata
            .get("docs")
            .and_then(|value| value.as_str()),
        |item| {
            let model = item.get("id").and_then(|value| value.as_str())?;
            Some(ProviderModelSeed {
                model: model.to_string(),
                name: model.to_string(),
                context_length: None,
                detail: item
                    .get("owned_by")
                    .and_then(|value| value.as_str())
                    .map(|owner| format!("Owned by {}", owner)),
            })
        },
    )?;
    enrich_models_from_presets(&mut models, presets);
    Ok(models)
}

fn gemini_model_catalog(
    adapter: &adapters::AdapterSpec,
    query: &str,
    limit: usize,
    presets: &[CloudModelDto],
    request: &CloudModelSearchRequest,
) -> Result<Vec<CloudModelDto>, String> {
    let api_key = catalog_api_key(adapter, request)?.ok_or_else(|| {
        "Gemini model search needs a saved Gemini key or GEMINI_API_KEY".to_string()
    })?;
    let base_url = adapter
        .default_base_url
        .as_deref()
        .unwrap_or("https://generativelanguage.googleapis.com/v1beta/openai")
        .trim_end_matches('/');
    let source_url = adapter
        .metadata
        .get("docs")
        .and_then(|value| value.as_str());
    let mut models = parse_provider_models(
        &curl_get_with_timeout(
            &format!("{}/models", base_url),
            &[("Authorization".into(), format!("Bearer {}", api_key))],
            20,
        )?,
        query,
        limit,
        &adapter.name,
        "gemini-live",
        source_url,
        gemini_model_seed_from_item,
    )?;
    enrich_models_from_presets(&mut models, presets);
    Ok(models)
}

fn anthropic_model_catalog(
    adapter: &adapters::AdapterSpec,
    query: &str,
    limit: usize,
    presets: &[CloudModelDto],
    request: &CloudModelSearchRequest,
) -> Result<Vec<CloudModelDto>, String> {
    let api_key = catalog_api_key(adapter, request)?.ok_or_else(|| {
        "Anthropic model search needs a saved Anthropic key or ANTHROPIC_API_KEY".to_string()
    })?;
    let base_url = adapter
        .default_base_url
        .as_deref()
        .unwrap_or("https://api.anthropic.com")
        .trim_end_matches('/');
    let body = curl_get_with_timeout(
        &format!("{}/v1/models", base_url),
        &[
            ("x-api-key".into(), api_key),
            ("anthropic-version".into(), "2023-06-01".into()),
        ],
        20,
    )?;
    let mut models = parse_provider_models(
        &body,
        query,
        limit,
        &adapter.name,
        "anthropic-live",
        Some("https://platform.claude.com/docs/en/api/models/list"),
        |item| {
            let model = item.get("id").and_then(|value| value.as_str())?;
            Some(ProviderModelSeed {
                model: model.to_string(),
                name: item
                    .get("display_name")
                    .and_then(|value| value.as_str())
                    .unwrap_or(model)
                    .to_string(),
                context_length: None,
                detail: item
                    .get("created_at")
                    .and_then(|value| value.as_str())
                    .map(|created| format!("Created {}", created)),
            })
        },
    )?;
    enrich_models_from_presets(&mut models, presets);
    Ok(models)
}

fn mistral_model_catalog(
    adapter: &adapters::AdapterSpec,
    query: &str,
    limit: usize,
    presets: &[CloudModelDto],
    request: &CloudModelSearchRequest,
) -> Result<Vec<CloudModelDto>, String> {
    let api_key = catalog_api_key(adapter, request)?.ok_or_else(|| {
        "Mistral model search needs a saved Mistral key or MISTRAL_API_KEY".to_string()
    })?;
    let base_url = adapter
        .default_base_url
        .as_deref()
        .unwrap_or("https://api.mistral.ai/v1")
        .trim_end_matches('/');
    let body = curl_get_with_timeout(
        &format!("{}/models", base_url),
        &[("Authorization".into(), format!("Bearer {}", api_key))],
        20,
    )?;
    let mut models = parse_provider_models(
        &body,
        query,
        limit,
        &adapter.name,
        "mistral-live",
        Some("https://docs.mistral.ai/api/endpoint/models"),
        |item| {
            if item
                .get("archived")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                return None;
            }
            if item
                .pointer("/capabilities/completion_chat")
                .and_then(|value| value.as_bool())
                == Some(false)
            {
                return None;
            }
            let model = item.get("id").and_then(|value| value.as_str())?;
            Some(ProviderModelSeed {
                model: model.to_string(),
                name: item
                    .get("name")
                    .and_then(|value| value.as_str())
                    .or_else(|| item.get("root").and_then(|value| value.as_str()))
                    .unwrap_or(model)
                    .to_string(),
                context_length: item
                    .get("max_context_length")
                    .and_then(|value| value.as_u64()),
                detail: item
                    .get("owned_by")
                    .and_then(|value| value.as_str())
                    .map(|owner| format!("Owned by {}", owner)),
            })
        },
    )?;
    enrich_models_from_presets(&mut models, presets);
    Ok(models)
}

fn openai_compatible_model_catalog(
    adapter: &adapters::AdapterSpec,
    query: &str,
    limit: usize,
    request: &CloudModelSearchRequest,
) -> Result<Vec<CloudModelDto>, String> {
    let base_url = request
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(adapter.default_base_url.as_deref())
        .ok_or_else(|| "OpenAI-compatible model search needs a base URL".to_string())?;
    let api_key = catalog_api_key(adapter, request)?;
    let headers = api_key
        .map(|key| vec![("Authorization".into(), format!("Bearer {}", key))])
        .unwrap_or_default();
    let source_url = adapter
        .metadata
        .get("docs")
        .and_then(|value| value.as_str());
    parse_provider_models(
        &curl_get_with_timeout(
            &format!("{}/models", base_url.trim_end_matches('/')),
            &headers,
            20,
        )?,
        query,
        limit,
        &adapter.name,
        "openai-compatible-live",
        source_url,
        |item| {
            let model = item
                .get("id")
                .or_else(|| item.get("model"))
                .or_else(|| item.get("name"))
                .and_then(|value| value.as_str())?;
            Some(ProviderModelSeed {
                model: model.to_string(),
                name: item
                    .get("name")
                    .or_else(|| item.get("display_name"))
                    .and_then(|value| value.as_str())
                    .unwrap_or(model)
                    .to_string(),
                context_length: item
                    .get("context_length")
                    .or_else(|| item.get("max_context_length"))
                    .and_then(|value| value.as_u64()),
                detail: item
                    .get("owned_by")
                    .and_then(|value| value.as_str())
                    .map(|owner| format!("Owned by {}", owner)),
            })
        },
    )
}

fn openrouter_model_catalog(query: &str, limit: usize) -> Result<Vec<CloudModelDto>, String> {
    let url = "https://openrouter.ai/api/v1/models?sort=pricing-low-to-high";
    let body = curl_get_with_timeout(url, &[], 20)?;
    parse_openrouter_models(&body, query, limit)
}

fn azure_openai_model_catalog(
    adapter: &adapters::AdapterSpec,
    query: &str,
    limit: usize,
    presets: &[CloudModelDto],
    request: &CloudModelSearchRequest,
) -> Result<Vec<CloudModelDto>, String> {
    let api_key = catalog_api_key(adapter, request)?.ok_or_else(|| {
        "Azure OpenAI model search needs a saved Azure key or AZURE_OPENAI_API_KEY".to_string()
    })?;
    let base_url = request
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(adapter.default_base_url.as_deref())
        .ok_or_else(|| "Azure OpenAI model search needs a resource base URL".to_string())?;
    if base_url.contains("YOUR-RESOURCE-NAME") {
        return Err("Azure OpenAI model search needs your resource base URL, for example https://my-resource.openai.azure.com/openai/v1".into());
    }
    let url = azure_openai_models_url(base_url, request.azure_api_version.as_deref());
    let source_url = adapter
        .metadata
        .get("docs")
        .and_then(|value| value.as_str());
    let mut models = parse_provider_models(
        &curl_get_with_timeout(&url, &[("api-key".into(), api_key)], 20)?,
        query,
        limit,
        &adapter.name,
        "azure-openai-live",
        source_url,
        azure_model_seed_from_item,
    )?;
    if !azure_openai_uses_v1_base_url(base_url) {
        for model in &mut models {
            model.detail = Some(match model.detail.as_deref() {
                Some(detail) => format!(
                    "{}; Azure legacy model lists expose base models, so use your deployment name when it differs",
                    detail
                ),
                None => "Azure legacy model lists expose base models; use your deployment name when it differs".into(),
            });
        }
    }
    enrich_models_from_presets(&mut models, presets);
    Ok(models)
}

fn azure_openai_models_url(base_url: &str, api_version: Option<&str>) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if azure_openai_uses_v1_base_url(trimmed) {
        return format!("{}/models", trimmed);
    }
    let resource_url = trimmed.strip_suffix("/openai").unwrap_or(trimmed);
    let version = api_version
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("2024-10-21");
    format!(
        "{}/openai/models?api-version={}",
        resource_url,
        shell_escape_query_value(version)
    )
}

fn azure_model_seed_from_item(item: &serde_json::Value) -> Option<ProviderModelSeed> {
    let model = item
        .get("id")
        .or_else(|| item.get("model"))
        .or_else(|| item.get("name"))
        .and_then(|value| value.as_str())?;
    Some(ProviderModelSeed {
        model: model.to_string(),
        name: item
            .get("display_name")
            .or_else(|| item.get("name"))
            .and_then(|value| value.as_str())
            .unwrap_or(model)
            .to_string(),
        context_length: item
            .get("context_length")
            .or_else(|| item.get("max_context_length"))
            .or_else(|| item.get("max_input_tokens"))
            .and_then(|value| value.as_u64()),
        detail: azure_model_detail(item),
    })
}

fn gemini_model_seed_from_item(item: &serde_json::Value) -> Option<ProviderModelSeed> {
    let model = item
        .get("id")
        .or_else(|| item.get("model"))
        .or_else(|| item.get("name"))
        .and_then(|value| value.as_str())
        .map(gemini_model_id)?;
    let name = item
        .get("display_name")
        .or_else(|| item.get("name"))
        .or_else(|| item.get("id"))
        .and_then(|value| value.as_str())
        .map(gemini_model_id)
        .unwrap_or_else(|| model.clone());
    Some(ProviderModelSeed {
        model,
        name,
        context_length: item
            .get("input_token_limit")
            .or_else(|| item.get("context_length"))
            .or_else(|| item.get("max_context_length"))
            .and_then(|value| value.as_u64()),
        detail: gemini_model_detail(item),
    })
}

fn gemini_model_id(value: &str) -> String {
    value
        .strip_prefix("models/")
        .unwrap_or(value)
        .trim()
        .to_string()
}

fn gemini_model_detail(item: &serde_json::Value) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(version) = item.get("version").and_then(|value| value.as_str()) {
        parts.push(format!("Version {}", version));
    }
    if let Some(output_limit) = item
        .get("output_token_limit")
        .and_then(|value| value.as_u64())
    {
        parts.push(format!("Output token limit {}", output_limit));
    }
    if let Some(description) = item.get("description").and_then(|value| value.as_str()) {
        let trimmed = description.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.chars().take(180).collect::<String>());
        }
    }
    if parts.is_empty() {
        Some("Gemini OpenAI-compatible model".into())
    } else {
        Some(parts.join("; "))
    }
}

fn azure_model_detail(item: &serde_json::Value) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(owner) = item
        .get("owned_by")
        .or_else(|| item.get("publisher"))
        .and_then(|value| value.as_str())
    {
        parts.push(format!("Owned by {}", owner));
    }
    if let Some(status) = item.get("status").and_then(|value| value.as_str()) {
        parts.push(format!("Status {}", status));
    }
    if let Some(created) = item
        .get("created_at")
        .or_else(|| item.get("created"))
        .and_then(|value| value.as_str())
    {
        parts.push(format!("Created {}", created));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

fn shell_escape_query_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        .collect()
}

struct ProviderModelSeed {
    model: String,
    name: String,
    context_length: Option<u64>,
    detail: Option<String>,
}

fn parse_provider_models<F>(
    body: &str,
    query: &str,
    limit: usize,
    provider: &str,
    source: &str,
    source_url: Option<&str>,
    mut seed_from_item: F,
) -> Result<Vec<CloudModelDto>, String>
where
    F: FnMut(&serde_json::Value) -> Option<ProviderModelSeed>,
{
    let json: serde_json::Value = serde_json::from_str(body)
        .map_err(|err| format!("provider returned invalid model JSON: {}", err))?;
    let data = model_data_array(&json)
        .ok_or_else(|| "provider model response did not include a model array".to_string())?;
    let mut models = Vec::new();
    for item in data {
        let Some(seed) = seed_from_item(item) else {
            continue;
        };
        if !query_matches_model(query, &seed.model, &seed.name) {
            continue;
        }
        models.push(CloudModelDto {
            model: seed.model,
            name: seed.name,
            provider: provider.to_string(),
            input_price_usd_per_million_tokens: None,
            output_price_usd_per_million_tokens: None,
            cache_read_price_usd_per_million_tokens: None,
            cache_write_price_usd_per_million_tokens: None,
            context_length: seed.context_length,
            source: source.to_string(),
            source_url: source_url.map(str::to_string),
            detail: seed.detail,
        });
        if models.len() >= limit {
            break;
        }
    }
    Ok(models)
}

fn model_data_array(json: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    json.get("data")
        .and_then(|value| value.as_array())
        .or_else(|| json.as_array())
}

fn enrich_models_from_presets(models: &mut [CloudModelDto], presets: &[CloudModelDto]) {
    for model in models {
        let Some(preset) = presets.iter().find(|preset| preset.model == model.model) else {
            continue;
        };
        model.input_price_usd_per_million_tokens = model
            .input_price_usd_per_million_tokens
            .or(preset.input_price_usd_per_million_tokens);
        model.output_price_usd_per_million_tokens = model
            .output_price_usd_per_million_tokens
            .or(preset.output_price_usd_per_million_tokens);
        model.cache_read_price_usd_per_million_tokens = model
            .cache_read_price_usd_per_million_tokens
            .or(preset.cache_read_price_usd_per_million_tokens);
        model.cache_write_price_usd_per_million_tokens = model
            .cache_write_price_usd_per_million_tokens
            .or(preset.cache_write_price_usd_per_million_tokens);
        model.context_length = model.context_length.or(preset.context_length);
        if model.detail.is_none() {
            model.detail = preset.detail.clone();
        }
        if model.source_url.is_none() {
            model.source_url = preset.source_url.clone();
        }
    }
}

fn catalog_api_key(
    adapter: &adapters::AdapterSpec,
    request: &CloudModelSearchRequest,
) -> Result<Option<String>, String> {
    if let Some(key) = request
        .api_key_keychain
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(secrets::read_cloud_api_key)
    {
        return Ok(Some(key));
    }
    if let Some(name) = request
        .api_key_env
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !is_valid_env_name(name) {
            return Err("model search apiKeyEnv must be a valid environment variable name".into());
        }
        if let Ok(value) = std::env::var(name) {
            if !value.trim().is_empty() {
                return Ok(Some(value));
            }
        }
    }
    Ok(secrets::read_cloud_api_key(&adapter.id).or_else(|| {
        adapter
            .validation
            .get("secret_env")
            .and_then(|value| value.as_str())
            .and_then(|name| std::env::var(name).ok())
    }))
}

fn parse_openrouter_models(
    body: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<CloudModelDto>, String> {
    let json: serde_json::Value = serde_json::from_str(body)
        .map_err(|err| format!("OpenRouter returned invalid JSON: {}", err))?;
    let data = json
        .get("data")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "OpenRouter response did not include a data array".to_string())?;
    let mut models = Vec::new();
    for item in data {
        let Some(model) = item.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or(model);
        if !query_matches_model(query, model, name) {
            continue;
        }
        let prompt = item
            .pointer("/pricing/prompt")
            .and_then(price_string_to_per_million);
        let completion = item
            .pointer("/pricing/completion")
            .and_then(price_string_to_per_million);
        let cache_read = price_from_aliases(
            item,
            &[
                "/pricing/cache_read",
                "/pricing/cacheRead",
                "/pricing/cached_input",
                "/pricing/cachedInput",
                "/pricing/prompt_cache_read",
            ],
        );
        let cache_write = price_from_aliases(
            item,
            &[
                "/pricing/cache_write",
                "/pricing/cacheWrite",
                "/pricing/cache_creation",
                "/pricing/cacheCreation",
                "/pricing/prompt_cache_write",
            ],
        );
        models.push(CloudModelDto {
            model: model.into(),
            name: name.into(),
            provider: "OpenRouter".into(),
            input_price_usd_per_million_tokens: prompt,
            output_price_usd_per_million_tokens: completion,
            cache_read_price_usd_per_million_tokens: cache_read,
            cache_write_price_usd_per_million_tokens: cache_write,
            context_length: item.get("context_length").and_then(|value| value.as_u64()),
            source: "openrouter-live".into(),
            source_url: Some(format!("https://openrouter.ai/{}", model)),
            detail: item
                .get("description")
                .and_then(|value| value.as_str())
                .map(|value| value.chars().take(180).collect::<String>()),
        });
        if models.len() >= limit {
            break;
        }
    }
    Ok(models)
}

fn query_matches_model(query: &str, model: &str, name: &str) -> bool {
    query.is_empty() || model.to_lowercase().contains(query) || name.to_lowercase().contains(query)
}

fn price_string_to_per_million(value: &serde_json::Value) -> Option<f64> {
    let parsed = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))?;
    parsed.is_finite().then_some(parsed * 1_000_000.0)
}

fn price_from_aliases(item: &serde_json::Value, pointers: &[&str]) -> Option<f64> {
    pointers
        .iter()
        .find_map(|pointer| item.pointer(pointer).and_then(price_string_to_per_million))
}

fn model_price_sort_key(model: &CloudModelDto) -> f64 {
    model
        .input_price_usd_per_million_tokens
        .unwrap_or(f64::MAX / 4.0)
        + model
            .output_price_usd_per_million_tokens
            .unwrap_or(f64::MAX / 4.0)
}

fn curl_get(url: &str, headers: &[(String, String)]) -> Result<String, String> {
    curl_get_with_timeout(url, headers, 20)
}

fn curl_get_with_timeout(
    url: &str,
    headers: &[(String, String)],
    max_time_seconds: u64,
) -> Result<String, String> {
    if !adapters::command_exists("curl") {
        return Err("curl is not available".into());
    }
    let mut cmd = adapters::command_with_gui_path("curl");
    let max_time = max_time_seconds.clamp(1, 60).to_string();
    cmd.args([
        "-sS",
        "--connect-timeout",
        "1",
        "--max-time",
        &max_time,
        "-w",
        "\n__BENCHFORGE_VALIDATION_HTTP_STATUS__:%{http_code}",
        url,
    ]);
    for (name, value) in headers {
        cmd.arg("-H").arg(format!("{}: {}", name, value));
    }
    let output = cmd.output().map_err(|err| err.to_string())?;
    let stderr = strip_ansi_codes(&String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (body, status) = parse_validation_http_response(&stdout);
    if !output.status.success() {
        return Err(validation_transport_error(&stderr, &body));
    }
    if let Some(status) = status {
        if !validation_http_status_ok(status) {
            return Err(validation_http_error(status, &body, &stderr));
        }
    }
    Ok(body)
}

#[derive(Clone, Copy)]
struct LocalRuntimeCandidate {
    id: &'static str,
    name: &'static str,
    adapter_id: &'static str,
    base_url: &'static str,
    probe_urls: &'static [&'static str],
    install_command: &'static str,
    start_command: &'static str,
    model_hint: &'static str,
    setup_hint: &'static str,
}

fn local_runtime_candidates() -> Vec<LocalRuntimeCandidate> {
    vec![
        LocalRuntimeCandidate {
            id: "ollama",
            name: "Ollama",
            adapter_id: "ollama-openai",
            base_url: "http://localhost:11434/v1",
            probe_urls: &[
                "http://localhost:11434/v1/models",
                "http://localhost:11434/api/tags",
            ],
            install_command: "brew install ollama",
            start_command: "ollama serve; ollama pull qwen2.5-coder:7b",
            model_hint: "qwen2.5-coder:7b",
            setup_hint: "Start Ollama and pull at least one model before benchmarking.",
        },
        LocalRuntimeCandidate {
            id: "lm-studio",
            name: "LM Studio",
            adapter_id: "lm-studio-openai",
            base_url: "http://localhost:1234/v1",
            probe_urls: &["http://localhost:1234/v1/models"],
            install_command: "Install LM Studio from https://lmstudio.ai",
            start_command: "Enable Developer > Local Server in LM Studio",
            model_hint: "loaded model id from LM Studio",
            setup_hint: "Load a model in LM Studio and start its OpenAI-compatible local server.",
        },
        LocalRuntimeCandidate {
            id: "llama-cpp",
            name: "llama.cpp",
            adapter_id: "llama-cpp-openai",
            base_url: "http://localhost:8080/v1",
            probe_urls: &["http://localhost:8080/v1/models"],
            install_command: "brew install llama.cpp",
            start_command: "llama-server -m /path/to/model.gguf --host 127.0.0.1 --port 8080",
            model_hint: "served GGUF model id",
            setup_hint: "Use Settings > Hugging Face Local Model for a guided GGUF download and llama-server start.",
        },
        LocalRuntimeCandidate {
            id: "vllm",
            name: "vLLM",
            adapter_id: "vllm-openai",
            base_url: "http://localhost:8000/v1",
            probe_urls: &["http://localhost:8000/v1/models"],
            install_command: "python3 -m pip install vllm",
            start_command: "python3 -m vllm.entrypoints.openai.api_server --model Qwen/Qwen2.5-7B-Instruct",
            model_hint: "Qwen/Qwen2.5-7B-Instruct",
            setup_hint: "Start vLLM's OpenAI-compatible API server with a model your machine can run.",
        },
        LocalRuntimeCandidate {
            id: "mlx-lm",
            name: "MLX / mlx-lm",
            adapter_id: "mlx-lm",
            base_url: "http://localhost:8080/v1",
            probe_urls: &["http://localhost:8080/v1/models"],
            install_command: "python3 -m pip install mlx-lm",
            start_command: "mlx_lm.server --model mlx-community/Qwen2.5-7B-Instruct-4bit --host 127.0.0.1 --port 8080",
            model_hint: "mlx-community/Qwen2.5-7B-Instruct-4bit",
            setup_hint: "Start mlx-lm's local OpenAI-compatible server with an MLX model.",
        },
        LocalRuntimeCandidate {
            id: "omlx",
            name: "oMLX experimental",
            adapter_id: "omlx-experimental",
            base_url: "http://localhost:11435/v1",
            probe_urls: &["http://localhost:11435/v1/models"],
            install_command: "Install oMLX from its project documentation",
            start_command: "Start oMLX with its OpenAI-compatible server on port 11435",
            model_hint: "oMLX served model id",
            setup_hint: "Use this for experimental oMLX OpenAI-compatible endpoints.",
        },
    ]
}

fn probe_local_runtime(candidate: LocalRuntimeCandidate) -> LocalRuntimeDto {
    probe_local_runtime_with_urls(candidate, candidate.probe_urls)
}

fn probe_local_runtime_with_urls<S: AsRef<str>>(
    candidate: LocalRuntimeCandidate,
    probe_urls: &[S],
) -> LocalRuntimeDto {
    let mut last_error = String::new();
    for url in probe_urls {
        match probe_local_runtime_url(&candidate, url.as_ref()) {
            Ok(runtime) => return runtime,
            Err(err) => last_error = err,
        }
    }
    local_runtime_dto(
        &candidate,
        "error",
        &format!("not detected: {}", last_error),
        vec![],
        None,
        None,
        None,
    )
}

fn local_runtime_candidate_by_id(id: &str) -> Option<LocalRuntimeCandidate> {
    local_runtime_candidates()
        .into_iter()
        .find(|candidate| candidate.id == id)
}

fn local_runtime_candidate_with_base_url(
    candidate: &LocalRuntimeCandidate,
    base_url: String,
) -> LocalRuntimeCandidate {
    LocalRuntimeCandidate {
        id: candidate.id,
        name: candidate.name,
        adapter_id: candidate.adapter_id,
        base_url: Box::leak(base_url.into_boxed_str()),
        probe_urls: candidate.probe_urls,
        install_command: candidate.install_command,
        start_command: candidate.start_command,
        model_hint: candidate.model_hint,
        setup_hint: candidate.setup_hint,
    }
}

fn local_runtime_probe_urls(base_url: &str, include_ollama_tags: bool) -> Vec<String> {
    let root = base_url.trim_end_matches('/');
    let mut urls = vec![format!("{}/models", root)];
    if include_ollama_tags {
        let tag_root = root.strip_suffix("/v1").unwrap_or(root);
        urls.push(format!("{}/api/tags", tag_root));
    }
    urls
}

fn probe_local_runtime_url(
    candidate: &LocalRuntimeCandidate,
    url: &str,
) -> Result<LocalRuntimeDto, String> {
    match curl_get_with_timeout(url, &[], 2) {
        Ok(body) => {
            let models = openai_model_ids(&body);
            let recommended_model = models.first().cloned();
            let model_source = local_probe_model_source(url);
            let (status, detail) = if models.is_empty() {
                (
                    "warn".to_string(),
                    "endpoint answered, but no OpenAI-style model ids were found".to_string(),
                )
            } else {
                (
                    "ok".to_string(),
                    format!(
                        "{} model(s) available via {}",
                        models.len(),
                        local_probe_label(url)
                    ),
                )
            };
            Ok(local_runtime_dto(
                candidate,
                &status,
                &detail,
                models,
                recommended_model,
                Some(url),
                Some(model_source),
            ))
        }
        Err(err) => Err(err),
    }
}

fn local_runtime_dto(
    candidate: &LocalRuntimeCandidate,
    status: &str,
    detail: &str,
    models: Vec<String>,
    recommended_model: Option<String>,
    probe_url: Option<&str>,
    model_source: Option<&str>,
) -> LocalRuntimeDto {
    LocalRuntimeDto {
        id: candidate.id.into(),
        name: candidate.name.into(),
        adapter_id: candidate.adapter_id.into(),
        base_url: candidate.base_url.into(),
        status: status.into(),
        detail: detail.into(),
        probe_url: probe_url.map(str::to_string),
        model_source: model_source.map(str::to_string),
        detected_at: store::now(),
        models,
        recommended_model,
        install_command: candidate.install_command.into(),
        start_command: candidate.start_command.into(),
        model_hint: candidate.model_hint.into(),
        setup_hint: candidate.setup_hint.into(),
    }
}

fn detected_local_runtime_target_config(
    runtime: &LocalRuntimeDto,
    model: &str,
    max_tokens: u64,
    timeout_seconds: u64,
    retry_count: u64,
) -> serde_json::Value {
    let selected_model = model.trim();
    let detected_models = runtime.models.iter().take(50).cloned().collect::<Vec<_>>();
    serde_json::json!({
        "model": selected_model,
        "base_url": runtime.base_url,
        "source": "local-runtime-detect",
        "runtime": {
            "id": runtime.id,
            "name": runtime.name,
            "adapter_id": runtime.adapter_id,
            "base_url": runtime.base_url,
            "detected_status": runtime.status,
            "detected_detail": runtime.detail,
            "detected_at": runtime.detected_at,
            "probe_url": runtime.probe_url,
            "model_source": runtime.model_source,
            "model_count": runtime.models.len(),
            "models": detected_models,
            "recommended_model": runtime.recommended_model,
            "selected_model": selected_model
        },
        "temperature": 0,
        "top_p": 1,
        "max_tokens": max_tokens,
        "timeout_seconds": timeout_seconds,
        "retry_count": retry_count,
        "streaming": false,
        "input_price_usd_per_million_tokens": 0,
        "output_price_usd_per_million_tokens": 0
    })
}

fn local_probe_model_source(url: &str) -> &'static str {
    if url.ends_with("/api/tags") {
        "ollama_native_tags"
    } else {
        "openai_models"
    }
}

fn local_probe_label(url: &str) -> &str {
    if url.ends_with("/api/tags") {
        "native tags"
    } else {
        "OpenAI /models"
    }
}

fn curl_post_json(
    url: &str,
    payload: &serde_json::Value,
    headers: &[(String, String)],
) -> Result<String, String> {
    if !adapters::command_exists("curl") {
        return Err("curl is not available".into());
    }
    let body_path = paths::app_data_dir().join(format!("validation-{}.json", uuid::Uuid::new_v4()));
    fs::write(
        &body_path,
        serde_json::to_vec(payload).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let mut cmd = adapters::command_with_gui_path("curl");
    cmd.args([
        "-sS",
        "--max-time",
        "30",
        "-X",
        "POST",
        url,
        "-w",
        "\n__BENCHFORGE_VALIDATION_HTTP_STATUS__:%{http_code}",
        "--data-binary",
        &format!("@{}", body_path.to_string_lossy()),
    ]);
    for (name, value) in headers {
        cmd.arg("-H").arg(format!("{}: {}", name, value));
    }
    let output = cmd.output().map_err(|err| err.to_string());
    let _ = fs::remove_file(body_path);
    let output = output?;
    let stderr = strip_ansi_codes(&String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (body, status) = parse_validation_http_response(&stdout);
    if !output.status.success() {
        return Err(validation_transport_error(&stderr, &body));
    }
    if let Some(status) = status {
        if !validation_http_status_ok(status) {
            return Err(validation_http_error(status, &body, &stderr));
        }
    }
    Ok(body)
}

fn parse_validation_http_response(stdout: &str) -> (String, Option<u16>) {
    let marker = "\n__BENCHFORGE_VALIDATION_HTTP_STATUS__:";
    let Some(index) = stdout.rfind(marker) else {
        return (stdout.to_string(), None);
    };
    let body = stdout[..index].to_string();
    let status = stdout[index + marker.len()..]
        .lines()
        .next()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|status| *status > 0);
    (body, status)
}

fn validation_http_status_ok(status: u16) -> bool {
    (200..300).contains(&status)
}

fn validation_transport_error(stderr: &str, body: &str) -> String {
    let mut parts = Vec::new();
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        parts.push(stderr.to_string());
    }
    if let Some(summary) = provider_error_summary_from_body(body) {
        parts.push(summary);
    }
    if parts.is_empty() {
        "provider request failed without response details".into()
    } else {
        parts.join(": ")
    }
}

fn validation_http_error(status: u16, body: &str, stderr: &str) -> String {
    let summary = provider_error_summary_from_body(body)
        .or_else(|| (!stderr.trim().is_empty()).then(|| stderr.trim().to_string()))
        .unwrap_or_else(|| "provider returned an empty error response".into());
    format!("HTTP {}: {}", status, summary)
}

fn provider_error_summary_from_body(body: &str) -> Option<String> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        let mut parts = Vec::new();
        for pointer in [
            "/error/message",
            "/message",
            "/detail",
            "/title",
            "/error/type",
            "/error/code",
            "/code",
            "/type",
        ] {
            if let Some(value) = json
                .pointer(pointer)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if !parts.iter().any(|part| part == value) {
                    parts.push(value.to_string());
                }
            }
        }
        if let Some(value) = json
            .get("error")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if !parts.iter().any(|part| part == value) {
                parts.push(value.to_string());
            }
        }
        if !parts.is_empty() {
            return Some(parts.join("; "));
        }
    }
    Some(truncate_validation_detail(body, 240))
}

fn openai_completion_probe(
    base_url: &str,
    model: &str,
    auth_headers: &[(String, String)],
) -> Result<(), String> {
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    headers.extend(auth_headers.iter().cloned());
    let payload = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Reply with OK."}],
        "temperature": 0.0,
        "max_tokens": 8
    });
    let body = curl_post_json(
        &format!("{}/chat/completions", base_url.trim_end_matches('/')),
        &payload,
        &headers,
    )?;
    if openai_completion_has_content(&body) {
        Ok(())
    } else {
        Err("provider response did not include choices[0].message.content".into())
    }
}

fn openai_responses_completion_probe(
    base_url: &str,
    model: &str,
    auth_headers: &[(String, String)],
) -> Result<(), String> {
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    headers.extend(auth_headers.iter().cloned());
    let payload = serde_json::json!({
        "model": model,
        "instructions": "Reply with exactly: OK",
        "input": "OK",
        "max_output_tokens": 8,
        "store": false
    });
    let body = curl_post_json(
        &format!("{}/responses", base_url.trim_end_matches('/')),
        &payload,
        &headers,
    )?;
    if openai_responses_has_content(&body) {
        Ok(())
    } else {
        Err("provider response did not include Responses output text".into())
    }
}

fn anthropic_completion_probe(base_url: &str, model: &str, api_key: &str) -> Result<(), String> {
    let payload = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Reply with OK."}],
        "max_tokens": 8
    });
    let body = curl_post_json(
        &format!("{}/v1/messages", base_url.trim_end_matches('/')),
        &payload,
        &[
            ("Content-Type".into(), "application/json".into()),
            ("x-api-key".into(), api_key.into()),
            ("anthropic-version".into(), "2023-06-01".into()),
        ],
    )?;
    if anthropic_completion_has_content(&body) {
        Ok(())
    } else {
        Err("provider response did not include text content".into())
    }
}

fn azure_openai_completion_probe(
    base_url: &str,
    model: &str,
    api_key: &str,
    config: &serde_json::Value,
) -> Result<(), String> {
    let mut payload = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Reply with OK."}],
        "temperature": 0.0,
        "max_tokens": 8
    });
    remove_azure_legacy_model_field(base_url, &mut payload);
    let body = curl_post_json(
        &azure_openai_chat_url(base_url, model, config),
        &payload,
        &[
            ("Content-Type".into(), "application/json".into()),
            ("api-key".into(), api_key.into()),
        ],
    )?;
    if openai_completion_has_content(&body) {
        Ok(())
    } else {
        Err("provider response did not include choices[0].message.content".into())
    }
}

fn azure_openai_chat_url(base_url: &str, deployment: &str, config: &serde_json::Value) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/openai/v1") {
        return format!("{}/chat/completions", trimmed);
    }
    let api_version = config
        .get("api_version")
        .or_else(|| config.get("azure_api_version"))
        .and_then(|value| value.as_str())
        .unwrap_or("2024-10-21");
    format!(
        "{}/openai/deployments/{}/chat/completions?api-version={}",
        trimmed, deployment, api_version
    )
}

fn azure_openai_uses_v1_base_url(base_url: &str) -> bool {
    base_url.trim_end_matches('/').ends_with("/openai/v1")
}

fn remove_azure_legacy_model_field(base_url: &str, payload: &mut serde_json::Value) {
    if !azure_openai_uses_v1_base_url(base_url) {
        if let Some(object) = payload.as_object_mut() {
            object.remove("model");
        }
    }
}

fn model_list_validation(
    target: &store::TargetRecord,
    body: &str,
    configured_model: &str,
) -> TargetValidationDto {
    match model_list_contains(body, configured_model) {
        Some(true) => target_validation(
            target,
            "ok",
            &format!(
                "endpoint reachable and model {} is listed",
                configured_model
            ),
        ),
        Some(false) => target_validation(
            target,
            "warn",
            &format!(
                "model_not_found: endpoint reachable, but model {} was not listed",
                configured_model
            ),
        ),
        None => target_validation(
            target,
            "ok",
            "endpoint reachable; model list shape was not recognized",
        ),
    }
}

fn format_validation_failure(stage: &str, error: &str) -> String {
    format!(
        "{}: {} failed: {}",
        validation_error_code(stage, error),
        stage,
        truncate_validation_detail(error, 240)
    )
}

fn validation_error_code(stage: &str, error: &str) -> &'static str {
    let lower = format!("{} {}", stage, error).to_lowercase();
    if lower.contains("api key is missing") || lower.contains("no key") {
        return "missing_key";
    }
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("deadline")
        || lower.contains("curl: (28)")
    {
        return "timeout";
    }
    if lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests")
        || lower.contains("quota")
        || lower.contains("429")
    {
        return "rate_limited";
    }
    if lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("invalid api key")
        || lower.contains("401")
        || lower.contains("403")
        || lower.contains("forbidden")
        || lower.contains("permission")
    {
        return "auth";
    }
    if lower.contains("connection refused")
        || lower.contains("failed to connect")
        || lower.contains("could not resolve")
        || lower.contains("name or service not known")
        || lower.contains("curl: (6)")
        || lower.contains("curl: (7)")
    {
        return "endpoint_unreachable";
    }
    if lower.contains("model_not_found")
        || lower.contains("model not found")
        || lower.contains("unknown model")
        || lower.contains("does not exist")
        || (lower.contains("404") && !stage.to_lowercase().contains("endpoint"))
    {
        return "model_not_found";
    }
    if lower.contains("did not include")
        || lower.contains("shape")
        || lower.contains("invalid json")
        || lower.contains("malformed")
    {
        return "unsupported_shape";
    }
    if lower.contains("server error")
        || lower.contains("internal error")
        || lower.contains("bad gateway")
        || lower.contains("service unavailable")
        || lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        return "server_error";
    }
    if lower.contains("404") {
        return "endpoint_unreachable";
    }
    "provider_failed"
}

fn truncate_validation_detail(detail: &str, max_chars: usize) -> String {
    let compact = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut truncated = String::new();
    for (index, ch) in compact.chars().enumerate() {
        if index >= max_chars {
            truncated.push_str("...");
            return truncated;
        }
        truncated.push(ch);
    }
    truncated
}

fn openai_completion_has_content(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            json.pointer("/choices/0/message/content")
                .and_then(|value| value.as_str())
                .map(|text| !text.trim().is_empty())
        })
        .unwrap_or(false)
}

fn openai_responses_has_content(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| openai_responses_text(&json))
        .is_some()
}

fn openai_responses_text(json: &serde_json::Value) -> Option<String> {
    if let Some(text) = json
        .get("output_text")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(text.to_string());
    }
    let mut chunks = Vec::new();
    for item in json.get("output").and_then(|value| value.as_array())? {
        let Some(content) = item.get("content").and_then(|value| value.as_array()) else {
            continue;
        };
        for part in content {
            if let Some(text) = part
                .get("text")
                .or_else(|| part.get("refusal"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                chunks.push(text.to_string());
            }
        }
    }
    (!chunks.is_empty()).then(|| chunks.join("\n"))
}

fn anthropic_completion_has_content(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            json.get("content")
                .and_then(|value| value.as_array())
                .cloned()
        })
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("text")
                    .and_then(|value| value.as_str())
                    .is_some_and(|text| !text.trim().is_empty())
            })
        })
}

fn model_list_contains(body: &str, configured_model: &str) -> Option<bool> {
    let models = openai_model_ids(body);
    if !models.is_empty() {
        return Some(models.iter().any(|id| id == configured_model));
    }
    None
}

fn openai_model_ids(body: &str) -> Vec<String> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return vec![];
    };
    let data = json.get("data").and_then(|value| value.as_array());
    let models = json.get("models").and_then(|value| value.as_array());
    data.or(models)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("id")
                .or_else(|| item.get("name"))
                .or_else(|| item.get("model"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
        })
        .take(50)
        .collect()
}

fn target_validation(
    target: &store::TargetRecord,
    status: &str,
    detail: &str,
) -> TargetValidationDto {
    TargetValidationDto {
        target_id: target.id.clone(),
        status: status.into(),
        detail: detail.into(),
        checked_at: store::now(),
    }
}

pub fn run_cli_validation_contract_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let network_base_url = reserve_unbound_validation_base_url()?;
    let conn = store::open_memory().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_VALIDATION_CONTRACT_KEY",
        "benchforge-validation-key",
    );
    let _removed_openrouter_key = ScopedEnvVar::remove("OPENROUTER_API_KEY");
    let targets = validation_contract_targets(&server.base_url, &network_base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let validations = targets
        .iter()
        .map(|target| {
            let stored = store::get_target(&conn, &target.id)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| format!("validation target {} was not stored", target.id))?;
            validate_target_record(&stored)
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_validation_contract_results(&validations)?;

    Ok(serde_json::json!({
        "server": "loopback-validation-contract",
        "targets": targets.iter().map(|target| target.id.clone()).collect::<Vec<_>>(),
        "validations": validations
    }))
}

pub fn run_cli_create_target_handoff_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_VALIDATION_CONTRACT_KEY",
        "benchforge-validation-key",
    );
    let catalog_models = parse_openrouter_models(
        r#"{
          "data": [
            {
              "id": "contract-ok",
              "name": "OpenRouter: Contract OK",
              "description": "Loopback onboarding model",
              "context_length": 131072,
              "pricing": {"prompt": "0.0000001", "completion": "0.0000002"}
            }
          ]
        }"#,
        "contract",
        10,
    )?;
    let catalog_model = require_cloud_model(&catalog_models, "openrouter", "contract-ok")?;
    require_cloud_model_pricing(catalog_model, "openrouter:contract-ok")?;
    let pricing_source = catalog_model
        .source_url
        .as_deref()
        .unwrap_or(catalog_model.source.as_str())
        .to_string();
    let request = CreateTargetBenchmarkHandoffRequest {
        target: CreateTargetRequest {
            id: "handoff-cloud-openrouter".into(),
            name: "Handoff cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": catalog_model.model.clone(),
                "base_url": format!("{}/v1", server.base_url),
                "api_key_env": "BENCHFORGE_VALIDATION_CONTRACT_KEY",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 16,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": catalog_model.input_price_usd_per_million_tokens,
                "output_price_usd_per_million_tokens": catalog_model.output_price_usd_per_million_tokens,
                "pricing_preset": catalog_model.name.clone(),
                "pricing_source": pricing_source,
                "pricing_provider": catalog_model.provider.clone(),
                "pricing_note": catalog_model.detail.clone(),
                "context_length": catalog_model.context_length
            }),
        },
        benchmark_pack_id: Some("llm-connectivity".into()),
        benchmark_target_ids: vec![],
        repetitions: 1,
        warmup_runs: 0,
        concurrency: 1,
        max_cost_usd: Some(0.05),
    };
    let result = create_target_with_benchmark_handoff_for_conn(&conn, request)?;
    let validation = result
        .validation
        .as_ref()
        .ok_or_else(|| "create_target_handoff_failed: validation result was missing".to_string())?;
    if validation.status != "ok" {
        return Err(format!(
            "create_target_handoff_failed: expected validation ok, got {}: {}",
            validation.status, validation.detail
        ));
    }
    if let Some(err) = &result.benchmark_error {
        return Err(format!(
            "create_target_handoff_failed: benchmark queue failed: {err}"
        ));
    }
    let job = result
        .run_job
        .as_ref()
        .ok_or_else(|| "create_target_handoff_failed: no benchmark job was queued".to_string())?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
    if finished.status != "completed" {
        return Err(format!(
            "create_target_handoff_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    let expected_tasks = runner::load_tasks(&runner::load_pack("llm-connectivity")?)?.len();
    if finished.results.len() != expected_tasks {
        return Err(format!(
            "create_target_handoff_failed: expected {} result row(s), got {}",
            expected_tasks,
            finished.results.len()
        ));
    }
    for row in &finished.results {
        if row.status != "passed" || row.score != Some(1.0) {
            return Err(format!(
                "create_target_handoff_failed: {} returned status {} score {:?}",
                row.task_id, row.status, row.score
            ));
        }
        if row.http_status != Some(200.0) {
            return Err(format!(
                "create_target_handoff_failed: {} did not preserve HTTP 200 metric",
                row.task_id
            ));
        }
        if row.cost_usd.unwrap_or(0.0) <= 0.0 {
            return Err(format!(
                "create_target_handoff_failed: {} did not preserve a positive cost metric",
                row.task_id
            ));
        }
        if row.provider_model.as_deref() != Some("contract-ok") {
            return Err(format!(
                "create_target_handoff_failed: {} preserved provider model {:?}",
                row.task_id, row.provider_model
            ));
        }
        if row.provider_model_source.as_deref() != Some("provider") {
            return Err(format!(
                "create_target_handoff_failed: {} provider model source was {:?}",
                row.task_id, row.provider_model_source
            ));
        }
        if row.finish_reason.as_deref() != Some("stop") {
            return Err(format!(
                "create_target_handoff_failed: {} finish reason was {:?}",
                row.task_id, row.finish_reason
            ));
        }
    }

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_create_target_handoff_report(&report)?;
    let reproducibility_path = Path::new(&export_path).join("reproducibility.json");
    let reproducibility: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&reproducibility_path).map_err(|err| {
            format!(
                "create_target_handoff_failed: could not read reproducibility manifest: {}",
                err
            )
        })?)
        .map_err(|err| {
            format!(
                "create_target_handoff_failed: reproducibility manifest was invalid JSON: {}",
                err
            )
        })?;
    validate_create_target_handoff_reproducibility(&reproducibility)?;

    Ok(serde_json::json!({
        "catalogModels": summarize_cloud_models(&catalog_models),
        "target": result.target,
        "validation": validation,
        "benchmarkPackId": "llm-connectivity",
        "runJobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "resultCount": finished.results.len(),
        "results": finished.results
    }))
}

fn validate_create_target_handoff_report(report: &str) -> Result<(), String> {
    let missing = [
        "## Run Configuration",
        "## Metric Coverage",
        "## Task Drilldown",
        "handoff-cloud-openrouter",
        "contract-ok",
        "https://openrouter.ai/contract-ok",
        "| Cost |",
        "| Provider model |",
        "| Finish reason |",
    ]
    .iter()
    .filter(|needle| !report.contains(**needle))
    .map(|needle| (*needle).to_string())
    .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "create_target_handoff_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_create_target_handoff_reproducibility(
    reproducibility: &serde_json::Value,
) -> Result<(), String> {
    let target_config = &reproducibility["targets"]["handoff-cloud-openrouter"][0]["config"];
    let run_group_target =
        &reproducibility["run_groups"][0]["queued_run_group"]["config"]["targets"][0];
    let mut failures = Vec::new();
    if target_config["api_key_env"].as_str() != Some("<redacted>") {
        failures.push("target config did not redact api_key_env".to_string());
    }
    if target_config["pricing_provider"].as_str() != Some("OpenRouter") {
        failures.push("target config did not preserve pricing_provider".to_string());
    }
    if target_config["pricing_preset"].as_str() != Some("OpenRouter: Contract OK") {
        failures.push("target config did not preserve pricing_preset".to_string());
    }
    if target_config["context_length"].as_u64() != Some(131_072) {
        failures.push("target config did not preserve context_length".to_string());
    }
    if run_group_target["pricing"]["pricing_provider"].as_str() != Some("OpenRouter") {
        failures.push("run group did not snapshot pricing_provider".to_string());
    }
    if run_group_target["pricing"]["pricing_source"].as_str()
        != Some("https://openrouter.ai/contract-ok")
    {
        failures.push("run group did not snapshot pricing_source".to_string());
    }
    if run_group_target["pricing"]["input_price_usd_per_million_tokens"]
        .as_f64()
        .is_none()
        || run_group_target["pricing"]["output_price_usd_per_million_tokens"]
            .as_f64()
            .is_none()
    {
        failures.push("run group did not snapshot catalog pricing".to_string());
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "create_target_handoff_reproducibility_failed: {}",
            failures.join("; ")
        ))
    }
}

pub fn run_cli_cloud_catalog_smoke() -> Result<serde_json::Value, String> {
    let openai = offline_cloud_catalog("openai", "gpt-5-mini", 10)?;
    let openai_model = require_cloud_model(&openai, "openai", "gpt-5-mini")?;
    require_cloud_model_pricing(openai_model, "openai:gpt-5-mini")?;
    require_cloud_model_source(openai_model, "openai:gpt-5-mini")?;

    let anthropic = offline_cloud_catalog("anthropic", "claude-haiku-4-5", 10)?;
    let anthropic_model = require_cloud_model(&anthropic, "anthropic", "claude-haiku-4-5")?;
    require_cloud_model_pricing(anthropic_model, "anthropic:claude-haiku-4-5")?;
    require_cloud_model_source(anthropic_model, "anthropic:claude-haiku-4-5")?;

    let mistral = offline_cloud_catalog("mistral", "mistral-large-latest", 10)?;
    let mistral_model = require_cloud_model(&mistral, "mistral", "mistral-large-latest")?;
    require_cloud_model_pricing(mistral_model, "mistral:mistral-large-latest")?;
    require_cloud_model_source(mistral_model, "mistral:mistral-large-latest")?;

    let gemini = offline_cloud_catalog("gemini", "gemini-2.5-flash-lite", 10)?;
    let gemini_model = require_cloud_model(&gemini, "gemini", "gemini-2.5-flash-lite")?;
    require_cloud_model_pricing(gemini_model, "gemini:gemini-2.5-flash-lite")?;
    require_cloud_model_source(gemini_model, "gemini:gemini-2.5-flash-lite")?;

    let azure_v1_url = azure_openai_models_url(
        "https://example.openai.azure.com/openai/v1/",
        Some("2025-04-01-preview"),
    );
    if azure_v1_url != "https://example.openai.azure.com/openai/v1/models" {
        return Err(format!(
            "cloud_catalog_failed: Azure v1 catalog URL was {azure_v1_url}"
        ));
    }
    let azure_legacy_url = azure_openai_models_url(
        "https://example.openai.azure.com",
        Some("2025-04-01-preview"),
    );
    if azure_legacy_url
        != "https://example.openai.azure.com/openai/models?api-version=2025-04-01-preview"
    {
        return Err(format!(
            "cloud_catalog_failed: Azure legacy catalog URL was {azure_legacy_url}"
        ));
    }
    let azure_models = parse_provider_models(
        r#"{"data":[{"id":"gpt-5-mini","display_name":"GPT-5 mini","owned_by":"azure-openai","context_length":128000}]}"#,
        "mini",
        10,
        "Azure OpenAI",
        "azure-openai-live",
        Some("https://learn.microsoft.com/en-us/azure/foundry/openai/"),
        azure_model_seed_from_item,
    )?;
    let azure_model = require_cloud_model(&azure_models, "azure-openai", "gpt-5-mini")?;
    if azure_model.context_length != Some(128_000) {
        return Err("cloud_catalog_failed: Azure fixture did not preserve context length".into());
    }
    let gemini_models = parse_provider_models(
        r#"{"data":[{"id":"models/gemini-2.5-flash-lite","display_name":"Gemini 2.5 Flash-Lite","input_token_limit":1048576,"output_token_limit":8192}]}"#,
        "flash-lite",
        10,
        "Google Gemini",
        "gemini-live",
        Some("https://ai.google.dev/gemini-api/docs/openai"),
        gemini_model_seed_from_item,
    )?;
    let gemini_fixture_model =
        require_cloud_model(&gemini_models, "gemini", "gemini-2.5-flash-lite")?;
    if gemini_fixture_model.context_length != Some(1_048_576) {
        return Err("cloud_catalog_failed: Gemini fixture did not preserve context length".into());
    }

    let openrouter = parse_openrouter_models(
        r#"{
          "data": [
            {
              "id": "openai/gpt-4.1-mini",
              "name": "OpenAI: GPT-4.1 Mini",
              "description": "Small fast model",
              "context_length": 1048576,
              "pricing": {
                "prompt": "0.0000004",
                "completion": "0.0000016",
                "cache_read": "0.0000001",
                "cache_write": "0.0000008"
              }
            },
            {
              "id": "free/model",
              "name": "Free Model",
              "context_length": 8192,
              "pricing": {"prompt": "0", "completion": "0"}
            }
          ]
        }"#,
        "gpt-4.1-mini",
        10,
    )?;
    let openrouter_model = require_cloud_model(&openrouter, "openrouter", "openai/gpt-4.1-mini")?;
    require_cloud_model_pricing(openrouter_model, "openrouter:openai/gpt-4.1-mini")?;
    if openrouter_model.context_length != Some(1_048_576) {
        return Err(
            "cloud_catalog_failed: OpenRouter fixture did not preserve context length".into(),
        );
    }

    let conn = store::open_memory().map_err(|err| err.to_string())?;
    let target_config = serde_json::json!({
        "model": openrouter_model.model.clone(),
        "base_url": "https://openrouter.ai/api/v1",
        "api_key_keychain": "openrouter",
        "api_key_env": "OPENROUTER_API_KEY",
        "temperature": 0,
        "top_p": 1,
        "max_tokens": 512,
        "timeout_seconds": 120,
        "retry_count": 1,
        "input_price_usd_per_million_tokens": openrouter_model.input_price_usd_per_million_tokens,
        "output_price_usd_per_million_tokens": openrouter_model.output_price_usd_per_million_tokens,
        "pricing_preset": openrouter_model.name.clone(),
        "pricing_source": openrouter_model.source_url.as_deref().unwrap_or(openrouter_model.source.as_str()),
        "pricing_provider": openrouter_model.provider.clone(),
        "context_length": openrouter_model.context_length,
        "pricing_note": openrouter_model.detail.clone(),
    });
    let target = persist_target_request(
        &conn,
        CreateTargetRequest {
            id: "cloud-catalog-openrouter".into(),
            name: "Cloud Catalog OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: target_config,
        },
    )?;
    let target_dto = target_dto_for_conn(&conn, &target.id)?;
    if !target_dto.is_cloud_model || target_dto.is_local_model {
        return Err("cloud_catalog_failed: catalog target was not classified as cloud-only".into());
    }
    let exported = store::export_target_redacted(&conn, &target.id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "cloud_catalog_failed: redacted target export missing".to_string())?;
    let config = exported
        .get("config")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "cloud_catalog_failed: redacted target export config missing".to_string())?;
    if config
        .get("api_key_keychain")
        .and_then(|value| value.as_str())
        != Some("[REDACTED]")
    {
        return Err("cloud_catalog_failed: keychain reference was not redacted".into());
    }
    if config.get("api_key_env").and_then(|value| value.as_str()) != Some("[REDACTED]") {
        return Err("cloud_catalog_failed: API key env reference was not redacted".into());
    }
    if config
        .get("pricing_provider")
        .and_then(|value| value.as_str())
        != Some("OpenRouter")
    {
        return Err("cloud_catalog_failed: pricing provider was not preserved".into());
    }
    if config
        .get("context_length")
        .and_then(|value| value.as_u64())
        != Some(1_048_576)
    {
        return Err("cloud_catalog_failed: context length was not preserved on target".into());
    }

    Ok(serde_json::json!({
        "catalogs": {
            "openai": summarize_cloud_models(&openai),
            "anthropic": summarize_cloud_models(&anthropic),
            "mistral": summarize_cloud_models(&mistral),
            "gemini": summarize_cloud_models(&gemini),
            "azureOpenaiFixture": summarize_cloud_models(&azure_models),
            "geminiFixture": summarize_cloud_models(&gemini_models),
            "openrouterFixture": summarize_cloud_models(&openrouter)
        },
        "target": target_dto,
        "redactedConfig": exported["config"],
        "azureCatalogUrls": {
            "v1": azure_v1_url,
            "legacy": azure_legacy_url
        }
    }))
}

pub fn run_cli_local_runtime_discovery_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_memory().map_err(|err| err.to_string())?;
    let base_url = format!("{}/v1", server.base_url);
    let handoff = run_local_runtime_handoff_smoke(&server.base_url)?;
    let runtime_cases = [
        ("ollama", true),
        ("lm-studio", false),
        ("llama-cpp", false),
        ("vllm", false),
        ("mlx-lm", false),
        ("omlx", false),
    ];
    let mut runtimes = Vec::new();
    let mut validations = Vec::new();
    let mut target_ids = Vec::new();

    for (runtime_id, use_ollama_tags) in runtime_cases {
        let candidate = local_runtime_candidate_by_id(runtime_id)
            .ok_or_else(|| format!("local_runtime_smoke_failed: missing {runtime_id} candidate"))?;
        let candidate = local_runtime_candidate_with_base_url(&candidate, base_url.clone());
        let probe_urls = if use_ollama_tags {
            vec![
                format!("{}/unavailable", server.base_url),
                format!("{}/api/tags", server.base_url),
            ]
        } else {
            local_runtime_probe_urls(&base_url, false)
        };
        let runtime = probe_local_runtime_with_urls(candidate, &probe_urls);
        if runtime.status != "ok" {
            return Err(format!(
                "local_runtime_smoke_failed: {} detected as {}: {}",
                runtime.id, runtime.status, runtime.detail
            ));
        }
        let Some(model) = runtime
            .recommended_model
            .clone()
            .or_else(|| runtime.models.first().cloned())
        else {
            return Err(format!(
                "local_runtime_smoke_failed: {} returned no model id",
                runtime.id
            ));
        };
        if use_ollama_tags && !runtime.detail.contains("native tags") {
            return Err(
                "local_runtime_smoke_failed: Ollama fallback did not use native tags".into(),
            );
        }

        let target_id = format!("local-runtime-{}", runtime.id);
        let target = store::NewTarget {
            id: target_id.clone(),
            name: format!("{} {}", runtime.name, model),
            kind: "direct_model".into(),
            adapter_id: runtime.adapter_id.clone(),
            config: detected_local_runtime_target_config(&runtime, &model, 16, 10, 0),
        };
        store::upsert_target(&conn, &target).map_err(|err| err.to_string())?;
        let validation = validate_target_for_conn(&conn, &target.id)?;
        if validation.status != "ok" {
            return Err(format!(
                "local_runtime_smoke_failed: {} validation {}: {}",
                target.id, validation.status, validation.detail
            ));
        }
        let dto = target_dto_for_conn(&conn, &target.id)?;
        if !dto.is_local_model || dto.is_cloud_model {
            return Err(format!(
                "local_runtime_smoke_failed: {} was not classified as local-only",
                target.id
            ));
        }
        runtimes.push(runtime);
        validations.push(validation);
        target_ids.push(target_id);
    }

    let run_results = runner::run_quick_smoke(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: "llm-connectivity".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let expected_results =
        target_ids.len() * runner::load_tasks(&runner::load_pack("llm-connectivity")?)?.len();
    if run_results.len() != expected_results {
        return Err(format!(
            "local_runtime_smoke_failed: expected {} result row(s), got {}",
            expected_results,
            run_results.len()
        ));
    }
    for result in &run_results {
        if result.status != "passed" || result.score != Some(1.0) {
            return Err(format!(
                "local_runtime_smoke_failed: {} {} returned status {} score {:?} error {:?}",
                result.target_id, result.task_id, result.status, result.score, result.error
            ));
        }
    }
    let stored_results = store::list_results(&conn).map_err(|err| err.to_string())?;
    if stored_results.len() != expected_results {
        return Err(format!(
            "local_runtime_smoke_failed: expected {} stored result row(s), got {}",
            expected_results,
            stored_results.len()
        ));
    }
    for result in &stored_results {
        if result.http_status != Some(200.0) {
            return Err(format!(
                "local_runtime_smoke_failed: {} {} did not preserve HTTP 200",
                result.target_id, result.task_id
            ));
        }
        if result
            .provider_model
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return Err(format!(
                "local_runtime_smoke_failed: {} {} did not preserve provider model",
                result.target_id, result.task_id
            ));
        }
        if result
            .reproducibility
            .pointer("/target/config/runtime/model_source")
            .and_then(|value| value.as_str())
            .is_none()
        {
            return Err(format!(
                "local_runtime_smoke_failed: {} {} did not preserve local runtime probe metadata",
                result.target_id, result.task_id
            ));
        }
    }

    Ok(serde_json::json!({
        "server": "loopback-local-runtime-discovery",
        "runtimes": runtimes,
        "validations": validations,
        "handoff": handoff,
        "benchmarkPackId": "llm-connectivity",
        "targetIds": target_ids,
        "resultCount": stored_results.len(),
        "results": stored_results
    }))
}

fn run_local_runtime_handoff_smoke(server_base_url: &str) -> Result<serde_json::Value, String> {
    let data_dir = std::env::temp_dir().join(format!(
        "benchforge-local-runtime-handoff-{}",
        uuid::Uuid::new_v4()
    ));
    let data_dir_value = data_dir.to_string_lossy().to_string();
    let result = {
        let _data_dir = ScopedEnvVar::set("BENCHFORGE_DATA_DIR", &data_dir_value);
        (|| -> Result<serde_json::Value, String> {
            let conn = store::open_app().map_err(|err| err.to_string())?;
            let candidate = local_runtime_candidate_by_id("ollama").ok_or_else(|| {
                "local_runtime_handoff_failed: missing Ollama candidate".to_string()
            })?;
            let base_url = format!("{}/v1", server_base_url);
            let candidate = local_runtime_candidate_with_base_url(&candidate, base_url.clone());
            let runtime = local_runtime_dto(
                &candidate,
                "ok",
                "1 model available via native tags",
                vec!["ollama-local:latest".into()],
                Some("ollama-local:latest".into()),
                Some(&format!("{}/api/tags", server_base_url)),
                Some("ollama_native_tags"),
            );
            let request = CreateTargetBenchmarkHandoffRequest {
                target: CreateTargetRequest {
                    id: "handoff-local-runtime-ollama".into(),
                    name: "Handoff local runtime Ollama".into(),
                    kind: "direct_model".into(),
                    adapter_id: "ollama-openai".into(),
                    config: detected_local_runtime_target_config(
                        &runtime,
                        "ollama-local:latest",
                        16,
                        10,
                        0,
                    ),
                },
                benchmark_pack_id: Some("llm-connectivity".into()),
                benchmark_target_ids: vec![],
                repetitions: 1,
                warmup_runs: 0,
                concurrency: 1,
                max_cost_usd: Some(0.05),
            };
            let handoff = create_target_with_benchmark_handoff_for_conn(&conn, request)?;
            let validation = handoff.validation.as_ref().ok_or_else(|| {
                "local_runtime_handoff_failed: validation result was missing".to_string()
            })?;
            if validation.status != "ok" {
                return Err(format!(
                    "local_runtime_handoff_failed: expected validation ok, got {}: {}",
                    validation.status, validation.detail
                ));
            }
            if !handoff.target.is_local_model || handoff.target.is_cloud_model {
                return Err(format!(
                    "local_runtime_handoff_failed: target {} was not classified as local-only",
                    handoff.target.id
                ));
            }
            if let Some(err) = &handoff.benchmark_error {
                return Err(format!(
                    "local_runtime_handoff_failed: benchmark queue failed: {err}"
                ));
            }
            let job = handoff.run_job.as_ref().ok_or_else(|| {
                "local_runtime_handoff_failed: no benchmark job queued".to_string()
            })?;
            let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
            if finished.status != "completed" {
                return Err(format!(
                    "local_runtime_handoff_failed: job {} finished with status {} error {:?}",
                    finished.id, finished.status, finished.error
                ));
            }
            let expected_tasks = runner::load_tasks(&runner::load_pack("llm-connectivity")?)?.len();
            if finished.results.len() != expected_tasks {
                return Err(format!(
                    "local_runtime_handoff_failed: expected {} result row(s), got {}",
                    expected_tasks,
                    finished.results.len()
                ));
            }
            for row in &finished.results {
                if row.status != "passed" || row.score != Some(1.0) {
                    return Err(format!(
                        "local_runtime_handoff_failed: {} returned status {} score {:?} error_code {:?} error_message {:?} http_status {:?}",
                        row.task_id,
                        row.status,
                        row.score,
                        row.error_code,
                        row.error_message,
                        row.http_status
                    ));
                }
                if row.http_status != Some(200.0) {
                    return Err(format!(
                        "local_runtime_handoff_failed: {} did not preserve HTTP 200",
                        row.task_id
                    ));
                }
                if row.cost_usd != Some(0.0) {
                    return Err(format!(
                        "local_runtime_handoff_failed: {} local cost was {:?}",
                        row.task_id, row.cost_usd
                    ));
                }
                if row.provider_model.as_deref() != Some("ollama-local:latest") {
                    return Err(format!(
                        "local_runtime_handoff_failed: {} provider model was {:?}",
                        row.task_id, row.provider_model
                    ));
                }
                if row.provider_model_source.as_deref() != Some("provider") {
                    return Err(format!(
                        "local_runtime_handoff_failed: {} provider model source was {:?}",
                        row.task_id, row.provider_model_source
                    ));
                }
                if row
                    .reproducibility
                    .pointer("/target/config/runtime/model_source")
                    .and_then(|value| value.as_str())
                    != Some("ollama_native_tags")
                {
                    return Err(format!(
                        "local_runtime_handoff_failed: {} did not preserve local runtime probe metadata",
                        row.task_id
                    ));
                }
            }

            Ok(serde_json::json!({
                "target": handoff.target,
                "validation": validation,
                "benchmarkPackId": "llm-connectivity",
                "runJobId": finished.id,
                "runGroupId": finished.run_group_id,
                "resultCount": finished.results.len(),
                "results": finished.results
            }))
        })()
    };
    let _ = fs::remove_dir_all(&data_dir);
    result
}

fn offline_cloud_catalog(
    adapter_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<CloudModelDto>, String> {
    search_cloud_models_for_request(
        &CloudModelSearchRequest {
            adapter_id: adapter_id.into(),
            query: query.into(),
            limit: Some(limit),
            base_url: None,
            api_key_keychain: None,
            api_key_env: None,
            azure_api_version: None,
        },
        false,
    )
}

fn require_cloud_model<'a>(
    models: &'a [CloudModelDto],
    adapter_id: &str,
    model_id: &str,
) -> Result<&'a CloudModelDto, String> {
    models
        .iter()
        .find(|model| model.model == model_id)
        .ok_or_else(|| {
            format!("cloud_catalog_failed: {adapter_id} catalog did not include {model_id}")
        })
}

fn require_cloud_model_pricing(model: &CloudModelDto, label: &str) -> Result<(), String> {
    if model.input_price_usd_per_million_tokens.is_none()
        || model.output_price_usd_per_million_tokens.is_none()
    {
        return Err(format!(
            "cloud_catalog_failed: {label} did not include input/output pricing"
        ));
    }
    Ok(())
}

fn require_cloud_model_source(model: &CloudModelDto, label: &str) -> Result<(), String> {
    if model.source_url.as_deref().unwrap_or("").trim().is_empty() {
        return Err(format!(
            "cloud_catalog_failed: {label} did not include a pricing/model source URL"
        ));
    }
    Ok(())
}

fn summarize_cloud_models(models: &[CloudModelDto]) -> Vec<serde_json::Value> {
    models
        .iter()
        .map(|model| {
            serde_json::json!({
                "model": model.model,
                "name": model.name,
                "provider": model.provider,
                "source": model.source,
                "inputPriceUsdPerMillionTokens": model.input_price_usd_per_million_tokens,
                "outputPriceUsdPerMillionTokens": model.output_price_usd_per_million_tokens,
                "cacheReadPriceUsdPerMillionTokens": model.cache_read_price_usd_per_million_tokens,
                "cacheWritePriceUsdPerMillionTokens": model.cache_write_price_usd_per_million_tokens,
                "contextLength": model.context_length,
            })
        })
        .collect()
}

const LOCAL_CLOUD_COMPARE_PACK_ID: &str = "local-cloud-compare";
const LOCAL_CLOUD_COMPARE_LOCAL_TARGET_ID: &str = "compare-local-llama";
const LOCAL_CLOUD_COMPARE_CLOUD_TARGET_ID: &str = "compare-cloud-openrouter";
const LOCAL_CLOUD_CONNECTIVITY_PACK_ID: &str = "llm-connectivity";
const LOCAL_CLOUD_CONNECTIVITY_LOCAL_TARGET_ID: &str = "connectivity-local-llama";
const LOCAL_CLOUD_CONNECTIVITY_CLOUD_TARGET_ID: &str = "connectivity-cloud-openrouter";
const LOCAL_CLOUD_BASICS_PACK_ID: &str = "llm-basics";
const LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID: &str = "basics-local-llama";
const LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID: &str = "basics-cloud-openrouter";
const LOCAL_CLOUD_CORE_PACK_ID: &str = "llm-core";
const LOCAL_CLOUD_CORE_LOCAL_TARGET_ID: &str = "core-local-llama";
const LOCAL_CLOUD_CORE_CLOUD_TARGET_ID: &str = "core-cloud-openrouter";
const LOCAL_CLOUD_PRACTICAL_PACK_ID: &str = "llm-practical";
const LOCAL_CLOUD_PRACTICAL_LOCAL_TARGET_ID: &str = "practical-local-llama";
const LOCAL_CLOUD_PRACTICAL_CLOUD_TARGET_ID: &str = "practical-cloud-openrouter";
const LOCAL_CLOUD_DECISION_PACK_ID: &str = "llm-decision-suite";
const LOCAL_CLOUD_DECISION_LOCAL_TARGET_ID: &str = "decision-local-llama";
const LOCAL_CLOUD_DECISION_CLOUD_TARGET_ID: &str = "decision-cloud-openrouter";
const LOCAL_CLOUD_STRUCTURED_PACK_ID: &str = "llm-structured-output";
const LOCAL_CLOUD_STRUCTURED_LOCAL_TARGET_ID: &str = "structured-local-llama";
const LOCAL_CLOUD_STRUCTURED_CLOUD_TARGET_ID: &str = "structured-cloud-openrouter";
const LOCAL_CLOUD_GROUNDED_PACK_ID: &str = "llm-grounded-context";
const LOCAL_CLOUD_GROUNDED_LOCAL_TARGET_ID: &str = "grounded-local-llama";
const LOCAL_CLOUD_GROUNDED_CLOUD_TARGET_ID: &str = "grounded-cloud-openrouter";
const LOCAL_CLOUD_RELIABILITY_PACK_ID: &str = "llm-reliability";
const LOCAL_CLOUD_RELIABILITY_LOCAL_TARGET_ID: &str = "reliability-local-llama";
const LOCAL_CLOUD_RELIABILITY_CLOUD_TARGET_ID: &str = "reliability-cloud-openrouter";
const HF_LOCAL_CLOUD_PACK_ID: &str = "llm-connectivity";
const HF_LOCAL_CLOUD_LOCAL_TARGET_ID: &str = "hf-local-cloud-llama";
const HF_LOCAL_CLOUD_CLOUD_TARGET_ID: &str = "hf-local-cloud-openrouter";
const HF_LOCAL_CLOUD_BASICS_PACK_ID: &str = "llm-basics";
const HF_LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID: &str = "hf-local-cloud-basics-llama";
const HF_LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID: &str = "hf-local-cloud-basics-openrouter";
const HF_LOCAL_CLOUD_REPO_ID: &str = "ggml-org/tinygemma3-GGUF";
const HF_LOCAL_CLOUD_FILENAME: &str = "tinygemma3-Q8_0.gguf";
const HF_LOCAL_CLOUD_CONTEXT: u32 = 512;
const CLOUD_PROVIDER_JOB_PACK_ID: &str = "cloud-contract";

pub fn run_cli_first_run_smoke() -> Result<serde_json::Value, String> {
    let Some(data_dir_override) = std::env::var("BENCHFORGE_DATA_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Err(
            "first_run_smoke_failed: set BENCHFORGE_DATA_DIR to an empty temporary directory"
                .into(),
        );
    };
    let data_dir = paths::app_data_dir();
    let db_path = paths::db_path();
    let resource_dir = paths::resource_root();
    if db_path.exists() {
        return Err(format!(
            "first_run_smoke_failed: {} already exists; use an empty BENCHFORGE_DATA_DIR",
            db_path.to_string_lossy()
        ));
    }

    let state = store::init_state().map_err(|err| format!("failed to initialize store: {err}"))?;
    if !db_path.exists() {
        return Err(format!(
            "first_run_smoke_failed: store did not create {}",
            db_path.to_string_lossy()
        ));
    }

    let conn = state
        .conn
        .lock()
        .map_err(|err| format!("failed to lock store: {err}"))?;
    let targets = store::list_targets(&conn).map_err(|err| err.to_string())?;
    if targets.len() != 1 || targets[0].id != "mock-agent" || !targets[0].enabled {
        return Err(format!(
            "first_run_smoke_failed: expected one enabled seeded mock target, got {}",
            serde_json::to_string(&targets).unwrap_or_else(|_| "target list unavailable".into())
        ));
    }
    if !store::list_results(&conn)
        .map_err(|err| err.to_string())?
        .is_empty()
    {
        return Err("first_run_smoke_failed: clean store already has result rows".into());
    }
    if !store::list_run_jobs(&conn)
        .map_err(|err| err.to_string())?
        .is_empty()
    {
        return Err("first_run_smoke_failed: clean store already has run jobs".into());
    }
    if !store::list_hf_download_jobs(&conn)
        .map_err(|err| err.to_string())?
        .is_empty()
    {
        return Err("first_run_smoke_failed: clean store already has HF download jobs".into());
    }
    if !store::list_hf_server_jobs(&conn)
        .map_err(|err| err.to_string())?
        .is_empty()
    {
        return Err("first_run_smoke_failed: clean store already has HF server jobs".into());
    }

    let storage_check = local_model_storage_doctor_check();
    if storage_check.status == "error" {
        return Err(format!(
            "first_run_smoke_failed: local model storage doctor check failed: {}",
            storage_check.detail
        ));
    }
    let readiness_checks = benchmark_readiness_doctor_checks_for_conn(&conn);
    let local_target_check = readiness_checks
        .iter()
        .find(|check| check.id == "benchmark-target-local")
        .ok_or_else(|| {
            "first_run_smoke_failed: missing local target readiness check".to_string()
        })?;
    let cloud_target_check = readiness_checks
        .iter()
        .find(|check| check.id == "benchmark-target-cloud")
        .ok_or_else(|| {
            "first_run_smoke_failed: missing cloud target readiness check".to_string()
        })?;
    if local_target_check.status != "warn" || cloud_target_check.status != "warn" {
        return Err(format!(
            "first_run_smoke_failed: clean workspace should warn about missing local/cloud targets, got local={} cloud={}",
            local_target_check.status, cloud_target_check.status
        ));
    }

    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: vec!["mock-agent".into()],
            benchmark_pack_id: "quick-smoke".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
    if finished.status != "completed" {
        return Err(format!(
            "first_run_smoke_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    let expected_tasks = runner::load_tasks(&runner::load_pack("quick-smoke")?)?.len();
    if finished.results.len() != expected_tasks {
        return Err(format!(
            "first_run_smoke_failed: expected {} result row(s), got {}",
            expected_tasks,
            finished.results.len()
        ));
    }

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let readme_path = Path::new(&export_path).join("README.md");
    let readme = fs::read_to_string(&readme_path).map_err(|err| err.to_string())?;
    if !readme.contains("## Run Configuration") || !readme.contains("## Metric Coverage") {
        return Err("first_run_smoke_failed: exported report is missing required sections".into());
    }

    Ok(serde_json::json!({
        "status": "ok",
        "dataDir": data_dir.to_string_lossy(),
        "dataDirOverride": data_dir_override,
        "resourceDir": resource_dir.to_string_lossy(),
        "dbPath": db_path.to_string_lossy(),
        "storageCheck": storage_check,
        "localTargetReadiness": local_target_check,
        "cloudTargetReadiness": cloud_target_check,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "resultCount": finished.results.len(),
        "exportPath": export_path
    }))
}

pub fn run_cli_hf_search_smoke() -> Result<serde_json::Value, String> {
    let popular = huggingface::search_models(huggingface::SearchModelsRequest {
        query: None,
        sort: "trendingScore".into(),
        limit: 5,
        gguf_only: true,
    })?;
    if popular.is_empty() {
        return Err("hf_search_smoke_failed: popular GGUF search returned no models".into());
    }
    if popular.iter().any(|model| model.repo_id.trim().is_empty()) {
        return Err("hf_search_smoke_failed: popular GGUF search returned an empty repo id".into());
    }

    let query_results = huggingface::search_models(huggingface::SearchModelsRequest {
        query: Some("tinygemma3".into()),
        sort: "downloads".into(),
        limit: 10,
        gguf_only: true,
    })?;
    let tiny_model = query_results
        .iter()
        .find(|model| model.repo_id == HF_LOCAL_CLOUD_REPO_ID)
        .ok_or_else(|| {
            format!(
                "hf_search_smoke_failed: query search did not return {}",
                HF_LOCAL_CLOUD_REPO_ID
            )
        })?;
    if tiny_model.recommended_file.as_deref() != Some(HF_LOCAL_CLOUD_FILENAME) {
        return Err(format!(
            "hf_search_smoke_failed: expected query recommendation {}, got {:?}",
            HF_LOCAL_CLOUD_FILENAME, tiny_model.recommended_file
        ));
    }

    let files = huggingface::inspect_model(huggingface::ModelRequest {
        repo_id: HF_LOCAL_CLOUD_REPO_ID.into(),
        revision: None,
    })?;
    if !files
        .gguf_files
        .iter()
        .any(|file| file == HF_LOCAL_CLOUD_FILENAME)
    {
        return Err(format!(
            "hf_search_smoke_failed: file inspection did not list {}",
            HF_LOCAL_CLOUD_FILENAME
        ));
    }
    if files.recommended_file.as_deref() != Some(HF_LOCAL_CLOUD_FILENAME) {
        return Err(format!(
            "hf_search_smoke_failed: expected file inspection recommendation {}, got {:?}",
            HF_LOCAL_CLOUD_FILENAME, files.recommended_file
        ));
    }

    let plan = huggingface::plan_download(huggingface::DownloadModelRequest {
        repo_id: HF_LOCAL_CLOUD_REPO_ID.into(),
        filename: Some(HF_LOCAL_CLOUD_FILENAME.into()),
        revision: None,
        download_id: None,
        start_after_download: false,
        run_connectivity_after_start: false,
        auto_benchmark_pack_id: None,
        auto_compare_after_start: false,
        auto_benchmark_target_ids: vec![],
        start_port: None,
        start_context: None,
    })?;
    if plan.selected_file != HF_LOCAL_CLOUD_FILENAME {
        return Err(format!(
            "hf_search_smoke_failed: planned download selected {}, expected {}",
            plan.selected_file, HF_LOCAL_CLOUD_FILENAME
        ));
    }

    Ok(serde_json::json!({
        "status": "ok",
        "popularCount": popular.len(),
        "queryCount": query_results.len(),
        "repoId": HF_LOCAL_CLOUD_REPO_ID,
        "recommendedFile": tiny_model.recommended_file,
        "inspectedFileCount": files.gguf_files.len(),
        "plannedDownload": plan
    }))
}

pub fn run_cli_local_cloud_connectivity_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-connectivity-key",
    );
    let targets = local_cloud_connectivity_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = local_cloud_compare_target_ids(&targets);
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_CONNECTIVITY_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(30))?;
    if finished.status != "completed" {
        return Err(format!(
            "local_cloud_connectivity_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_local_cloud_connectivity_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        LOCAL_CLOUD_CONNECTIVITY_PACK_ID,
        32,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_connectivity_report(&report)?;

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-connectivity",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_CONNECTIVITY_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_hf_local_cloud_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let state = store::init_state().map_err(|err| format!("failed to initialize store: {err}"))?;
    let port = reserve_loopback_smoke_port()?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-hf-local-cloud-key",
    );
    let download = huggingface::download_model(huggingface::DownloadModelRequest {
        repo_id: HF_LOCAL_CLOUD_REPO_ID.into(),
        filename: Some(HF_LOCAL_CLOUD_FILENAME.into()),
        revision: None,
        download_id: None,
        start_after_download: false,
        run_connectivity_after_start: false,
        auto_benchmark_pack_id: None,
        auto_compare_after_start: false,
        auto_benchmark_target_ids: vec![],
        start_port: None,
        start_context: None,
    })?;

    let start_status = match huggingface::start_server(
        &state,
        huggingface::StartModelRequest {
            repo_id: HF_LOCAL_CLOUD_REPO_ID.into(),
            filename: Some(HF_LOCAL_CLOUD_FILENAME.into()),
            port,
            context: HF_LOCAL_CLOUD_CONTEXT,
            register_target_after_start: false,
            run_connectivity_after_start: false,
            auto_benchmark_pack_id: None,
            auto_compare_after_start: false,
            auto_benchmark_target_ids: vec![],
        },
    ) {
        Ok(status) => status,
        Err(err) => {
            let _ = huggingface::stop_server(&state);
            return Err(err);
        }
    };

    let run_result = run_hf_local_cloud_connectivity_job(
        &state,
        &server.base_url,
        port,
        &download,
        &start_status,
    );
    let stop_status = huggingface::stop_server(&state).ok();
    let summary = run_result?;
    Ok(serde_json::json!({
        "server": "hf-local-plus-loopback-cloud",
        "download": download,
        "port": port,
        "serverStopped": stop_status.map(|status| !status.server_running).unwrap_or(false),
        "comparison": summary
    }))
}

pub fn run_cli_hf_local_cloud_basics_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let state = store::init_state().map_err(|err| format!("failed to initialize store: {err}"))?;
    let port = reserve_loopback_smoke_port()?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-hf-local-cloud-basics-key",
    );
    let download = huggingface::download_model(huggingface::DownloadModelRequest {
        repo_id: HF_LOCAL_CLOUD_REPO_ID.into(),
        filename: Some(HF_LOCAL_CLOUD_FILENAME.into()),
        revision: None,
        download_id: None,
        start_after_download: false,
        run_connectivity_after_start: false,
        auto_benchmark_pack_id: None,
        auto_compare_after_start: false,
        auto_benchmark_target_ids: vec![],
        start_port: None,
        start_context: None,
    })?;

    let start_status = match huggingface::start_server(
        &state,
        huggingface::StartModelRequest {
            repo_id: HF_LOCAL_CLOUD_REPO_ID.into(),
            filename: Some(HF_LOCAL_CLOUD_FILENAME.into()),
            port,
            context: HF_LOCAL_CLOUD_CONTEXT,
            register_target_after_start: false,
            run_connectivity_after_start: false,
            auto_benchmark_pack_id: None,
            auto_compare_after_start: false,
            auto_benchmark_target_ids: vec![],
        },
    ) {
        Ok(status) => status,
        Err(err) => {
            let _ = huggingface::stop_server(&state);
            return Err(err);
        }
    };

    let run_result =
        run_hf_local_cloud_basics_job(&state, &server.base_url, port, &download, &start_status);
    let stop_status = huggingface::stop_server(&state).ok();
    let summary = run_result?;
    Ok(serde_json::json!({
        "server": "hf-local-plus-loopback-cloud-basics",
        "download": download,
        "port": port,
        "serverStopped": stop_status.map(|status| !status.server_running).unwrap_or(false),
        "comparison": summary
    }))
}

pub fn run_cli_hf_download_start_job_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let state = store::init_state().map_err(|err| format!("failed to initialize store: {err}"))?;
    let port = reserve_loopback_smoke_port()?;
    let cloud_target_id = "000-hf-auto-compare-cloud";
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_HF_AUTO_COMPARE_KEY",
        "benchforge-hf-auto-compare-key",
    );
    let download_job = {
        let conn = state
            .conn
            .lock()
            .map_err(|err| format!("failed to lock store: {err}"))?;
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: cloud_target_id.into(),
                name: "HF Auto Compare Cloud".into(),
                kind: "direct_model".into(),
                adapter_id: "openrouter".into(),
                config: serde_json::json!({
                    "model": "contract-ok",
                    "base_url": format!("{}/v1", server.base_url),
                    "api_key_env": "BENCHFORGE_HF_AUTO_COMPARE_KEY",
                    "source": "hf-auto-compare-loopback",
                    "temperature": 0,
                    "top_p": 1,
                    "max_tokens": 32,
                    "timeout_seconds": 10,
                    "retry_count": 0,
                    "input_price_usd_per_million_tokens": 1.0,
                    "output_price_usd_per_million_tokens": 2.0
                }),
            },
        )
        .map_err(|err| err.to_string())?;
        huggingface::start_download_job(
            &conn,
            huggingface::DownloadModelRequest {
                repo_id: HF_LOCAL_CLOUD_REPO_ID.into(),
                filename: Some(HF_LOCAL_CLOUD_FILENAME.into()),
                revision: None,
                download_id: None,
                start_after_download: true,
                run_connectivity_after_start: true,
                auto_benchmark_pack_id: Some(HF_LOCAL_CLOUD_PACK_ID.into()),
                auto_compare_after_start: true,
                auto_benchmark_target_ids: vec![],
                start_port: Some(port),
                start_context: Some(HF_LOCAL_CLOUD_CONTEXT),
            },
        )?
    };

    let handoff_result = run_huggingface_download_handoff_for_state(&state, &download_job.id);
    if let Err(err) = handoff_result {
        let _ = huggingface::stop_server(&state);
        return Err(err);
    }

    let target_id = hf_local_target_id(HF_LOCAL_CLOUD_REPO_ID, HF_LOCAL_CLOUD_FILENAME, port);
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let finished_download =
        huggingface::get_download_job(&conn, &download_job.id)?.ok_or_else(|| {
            format!(
                "hf auto handoff download job {} disappeared",
                download_job.id
            )
        })?;
    if finished_download.status != "completed" {
        let _ = huggingface::stop_server(&state);
        return Err(format!(
            "hf_auto_handoff_failed: download job {} finished with status {} error {:?}",
            finished_download.id, finished_download.status, finished_download.error
        ));
    }
    let server_job = huggingface::list_server_jobs(&conn)?
        .into_iter()
        .find(|job| {
            job.repo_id == HF_LOCAL_CLOUD_REPO_ID
                && job.selected_file.as_deref() == Some(HF_LOCAL_CLOUD_FILENAME)
                && job.port == port
                && job.context == HF_LOCAL_CLOUD_CONTEXT
        })
        .ok_or_else(|| {
            format!(
                "hf_auto_handoff_failed: no server job was created for download job {}",
                download_job.id
            )
        })?;
    if server_job.status != "completed" {
        let _ = huggingface::stop_server(&state);
        return Err(format!(
            "hf_auto_handoff_failed: server job {} finished with status {} error {:?}",
            server_job.id, server_job.status, server_job.error
        ));
    }
    let target = store::get_target(&conn, &target_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("hf_auto_handoff_failed: target {target_id} was not registered"))?;
    if target.validation_status.as_deref() == Some("error") {
        let _ = huggingface::stop_server(&state);
        return Err(format!(
            "hf_auto_handoff_failed: target {} validation failed: {:?}",
            target_id, target.validation_detail
        ));
    }
    let queued_target_ids = vec![target_id.clone(), cloud_target_id.to_string()];
    let run_job = store::list_run_jobs(&conn)
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|job| {
            job.benchmark_pack_id == HF_LOCAL_CLOUD_PACK_ID
                && run_job_targets_match(&job.request, &queued_target_ids)
        })
        .ok_or_else(|| {
            format!(
                "hf_auto_handoff_failed: no benchmark job was queued for target(s) {}, {}",
                target_id, cloud_target_id
            )
        })?;
    let finished_run = wait_for_run_job(&conn, &run_job.id, Duration::from_secs(60))?;
    let stop_status = huggingface::stop_server(&state).ok();
    if finished_run.status != "completed" {
        return Err(format!(
            "hf_auto_handoff_failed: benchmark job {} finished with status {} error {:?}",
            finished_run.id, finished_run.status, finished_run.error
        ));
    }
    let expected_tasks = runner::load_tasks(&runner::load_pack(HF_LOCAL_CLOUD_PACK_ID)?)?.len();
    let expected_target_ids = queued_target_ids;
    if finished_run.results.len() != expected_tasks * expected_target_ids.len() {
        return Err(format!(
            "hf_auto_handoff_failed: expected {} result row(s), got {}",
            expected_tasks * expected_target_ids.len(),
            finished_run.results.len()
        ));
    }
    let result_target_ids = finished_run
        .results
        .iter()
        .map(|result| result.target_id.as_str())
        .collect::<BTreeSet<_>>();
    for expected_target_id in &expected_target_ids {
        if !result_target_ids.contains(expected_target_id.as_str()) {
            return Err(format!(
                "hf_auto_handoff_failed: missing benchmark result for target {}",
                expected_target_id
            ));
        }
    }

    Ok(serde_json::json!({
        "downloadJob": finished_download,
        "serverJob": server_job,
        "targetIds": expected_target_ids,
        "benchmarkPackId": HF_LOCAL_CLOUD_PACK_ID,
        "runJobId": finished_run.id,
        "runGroupId": finished_run.run_group_id,
        "resultCount": finished_run.results.len(),
        "serverStopped": stop_status.map(|status| !status.server_running).unwrap_or(false),
        "results": finished_run.results
    }))
}

pub fn run_cli_hf_server_start_job_smoke() -> Result<serde_json::Value, String> {
    let state = store::init_state().map_err(|err| format!("failed to initialize store: {err}"))?;
    let port = reserve_loopback_smoke_port()?;
    let download = huggingface::download_model(huggingface::DownloadModelRequest {
        repo_id: HF_LOCAL_CLOUD_REPO_ID.into(),
        filename: Some(HF_LOCAL_CLOUD_FILENAME.into()),
        revision: None,
        download_id: None,
        start_after_download: false,
        run_connectivity_after_start: false,
        auto_benchmark_pack_id: None,
        auto_compare_after_start: false,
        auto_benchmark_target_ids: vec![],
        start_port: None,
        start_context: None,
    })?;
    let request = huggingface::normalize_start_request(huggingface::StartModelRequest {
        repo_id: HF_LOCAL_CLOUD_REPO_ID.into(),
        filename: Some(HF_LOCAL_CLOUD_FILENAME.into()),
        port,
        context: HF_LOCAL_CLOUD_CONTEXT,
        register_target_after_start: true,
        run_connectivity_after_start: true,
        auto_benchmark_pack_id: Some(HF_LOCAL_CLOUD_PACK_ID.into()),
        auto_compare_after_start: false,
        auto_benchmark_target_ids: vec![],
    })?;
    let server_job = {
        let conn = state
            .conn
            .lock()
            .map_err(|err| format!("failed to lock store: {err}"))?;
        huggingface::enqueue_server_job(&conn, request.clone())?
    };

    let handoff_result =
        run_huggingface_server_job_with_handoff_for_state(&state, &server_job.id, request);
    if let Err(err) = handoff_result {
        let _ = huggingface::stop_server(&state);
        return Err(err);
    }

    let target_id = hf_local_target_id(HF_LOCAL_CLOUD_REPO_ID, HF_LOCAL_CLOUD_FILENAME, port);
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let finished_server = huggingface::get_server_job(&conn, &server_job.id)?.ok_or_else(|| {
        format!(
            "hf_server_auto_handoff_failed: server job {} disappeared",
            server_job.id
        )
    })?;
    if finished_server.status != "completed" {
        let _ = huggingface::stop_server(&state);
        return Err(format!(
            "hf_server_auto_handoff_failed: server job {} finished with status {} error {:?}",
            finished_server.id, finished_server.status, finished_server.error
        ));
    }
    let target = store::get_target(&conn, &target_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| {
            format!("hf_server_auto_handoff_failed: target {target_id} was not registered")
        })?;
    if target.validation_status.as_deref() == Some("error") {
        let _ = huggingface::stop_server(&state);
        return Err(format!(
            "hf_server_auto_handoff_failed: target {} validation failed: {:?}",
            target_id, target.validation_detail
        ));
    }
    let run_job = store::list_run_jobs(&conn)
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|job| {
            job.benchmark_pack_id == HF_LOCAL_CLOUD_PACK_ID
                && run_job_targets_match(&job.request, std::slice::from_ref(&target_id))
        })
        .ok_or_else(|| {
            format!(
                "hf_server_auto_handoff_failed: no benchmark job was queued for target {target_id}"
            )
        })?;
    let finished_run = wait_for_run_job(&conn, &run_job.id, Duration::from_secs(60))?;
    let stop_status = huggingface::stop_server(&state).ok();
    if finished_run.status != "completed" {
        return Err(format!(
            "hf_server_auto_handoff_failed: benchmark job {} finished with status {} error {:?}",
            finished_run.id, finished_run.status, finished_run.error
        ));
    }
    let expected_tasks = runner::load_tasks(&runner::load_pack(HF_LOCAL_CLOUD_PACK_ID)?)?.len();
    if finished_run.results.len() != expected_tasks {
        return Err(format!(
            "hf_server_auto_handoff_failed: expected {} result row(s), got {}",
            expected_tasks,
            finished_run.results.len()
        ));
    }

    Ok(serde_json::json!({
        "download": download,
        "serverJob": finished_server,
        "targetId": target_id,
        "benchmarkPackId": HF_LOCAL_CLOUD_PACK_ID,
        "runJobId": finished_run.id,
        "runGroupId": finished_run.run_group_id,
        "resultCount": finished_run.results.len(),
        "serverStopped": stop_status.map(|status| !status.server_running).unwrap_or(false),
        "results": finished_run.results
    }))
}

pub fn run_cli_local_cloud_basics_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-basics-key",
    );
    let targets = local_cloud_basics_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = local_cloud_compare_target_ids(&targets);
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_BASICS_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
    if finished.status != "completed" {
        return Err(format!(
            "local_cloud_basics_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_local_cloud_basics_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        LOCAL_CLOUD_BASICS_PACK_ID,
        128,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_basics_report(&report)?;

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-basics",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_BASICS_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_local_cloud_core_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-compare-key",
    );
    let targets = local_cloud_core_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = local_cloud_compare_target_ids(&targets);
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_CORE_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
    if finished.status != "completed" {
        return Err(format!(
            "local_cloud_core_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_local_cloud_core_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        LOCAL_CLOUD_CORE_PACK_ID,
        256,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_core_report(&report)?;

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-core",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_CORE_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_local_cloud_compare_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_memory().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-compare-key",
    );
    let targets = local_cloud_compare_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let run_group_id = "local-cloud-compare-smoke";
    let target_ids = local_cloud_compare_target_ids(&targets);
    let results = runner::run_quick_smoke(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_COMPARE_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: None,
            run_group_id: Some(run_group_id.into()),
        },
    )?;
    validate_local_cloud_compare_results(&results)?;
    let run_ids = results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_compare_report(&report)?;

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-compare",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_COMPARE_PACK_ID,
        "runGroupId": run_group_id,
        "exportPath": export_path,
        "results": results
    }))
}

pub fn run_cli_local_cloud_job_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-compare-key",
    );
    let targets = local_cloud_compare_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = local_cloud_compare_target_ids(&targets);
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_COMPARE_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(30))?;
    if finished.status != "completed" {
        return Err(format!(
            "local_cloud_job_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_local_cloud_compare_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        LOCAL_CLOUD_COMPARE_PACK_ID,
        16,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_compare_report(&report)?;
    if !report.contains("## Run Configuration") {
        return Err("local_cloud_job_report_failed: missing run configuration".into());
    }

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-job",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_COMPARE_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_local_cloud_practical_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-compare-key",
    );
    let targets = local_cloud_practical_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = local_cloud_compare_target_ids(&targets);
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_PRACTICAL_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(90))?;
    if finished.status != "completed" {
        return Err(format!(
            "local_cloud_practical_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_local_cloud_practical_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        LOCAL_CLOUD_PRACTICAL_PACK_ID,
        256,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_practical_report(&report)?;

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-practical",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_PRACTICAL_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_local_cloud_decision_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-compare-key",
    );
    let targets = local_cloud_decision_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = local_cloud_compare_target_ids(&targets);
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_DECISION_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
    if finished.status != "completed" {
        return Err(format!(
            "local_cloud_decision_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_local_cloud_decision_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        LOCAL_CLOUD_DECISION_PACK_ID,
        256,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_decision_report(&report)?;

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-decision",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_DECISION_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_local_cloud_structured_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-compare-key",
    );
    let targets = local_cloud_structured_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = local_cloud_compare_target_ids(&targets);
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_STRUCTURED_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
    if finished.status != "completed" {
        return Err(format!(
            "local_cloud_structured_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_local_cloud_structured_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        LOCAL_CLOUD_STRUCTURED_PACK_ID,
        256,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_structured_report(&report)?;

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-structured",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_STRUCTURED_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_local_cloud_grounded_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-compare-key",
    );
    let targets = local_cloud_grounded_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = local_cloud_compare_target_ids(&targets);
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_GROUNDED_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
    if finished.status != "completed" {
        return Err(format!(
            "local_cloud_grounded_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_local_cloud_grounded_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        LOCAL_CLOUD_GROUNDED_PACK_ID,
        256,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_grounded_report(&report)?;

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-grounded",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_GROUNDED_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_local_cloud_reliability_smoke() -> Result<serde_json::Value, String> {
    let server = ValidationContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _fake_key = ScopedEnvVar::set(
        "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
        "benchforge-compare-key",
    );
    let targets = local_cloud_reliability_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = local_cloud_compare_target_ids(&targets);
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: LOCAL_CLOUD_RELIABILITY_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
    if finished.status != "completed" {
        return Err(format!(
            "local_cloud_reliability_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_local_cloud_reliability_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        LOCAL_CLOUD_RELIABILITY_PACK_ID,
        256,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_local_cloud_reliability_report(&report)?;

    Ok(serde_json::json!({
        "server": "loopback-local-cloud-reliability",
        "targetIds": target_ids,
        "benchmarkPackId": LOCAL_CLOUD_RELIABILITY_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_cloud_provider_job_smoke() -> Result<serde_json::Value, String> {
    let server = runner::CloudContractServer::start()?;
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let _api_key = ScopedEnvVar::set(
        runner::CLOUD_CONTRACT_API_KEY_ENV,
        "benchforge-contract-key",
    );
    let targets = runner::cloud_contract_targets(server.base_url());
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = targets
        .iter()
        .map(|target| target.id.clone())
        .collect::<Vec<_>>();
    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: CLOUD_PROVIDER_JOB_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 4,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(60))?;
    if finished.status != "completed" {
        return Err(format!(
            "cloud_provider_job_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    runner::validate_cloud_contract_results(&finished.results, targets.len())?;
    validate_cloud_provider_job_run_group(&conn, &finished.run_group_id, &targets)?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_cloud_provider_job_report(&report, &targets)?;

    Ok(serde_json::json!({
        "server": "loopback-cloud-provider-job",
        "targetIds": target_ids,
        "benchmarkPackId": CLOUD_PROVIDER_JOB_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

pub fn run_cli_live_cloud_smoke() -> Result<serde_json::Value, String> {
    let run_benchmark = env_flag("BENCHFORGE_LIVE_CLOUD_RUN");
    let max_cost_usd = live_cloud_max_cost_usd(&|name| std::env::var(name).ok())?;
    let provider_filter = live_cloud_provider_filter();
    let plan = live_cloud_target_plan(
        provider_filter.as_ref(),
        &|name| std::env::var(name).ok(),
        &|adapter_id| provider_api_key_available(adapter_id),
    )?;
    if plan.targets.is_empty() {
        return Ok(serde_json::json!({
            "status": "skipped",
            "benchmarkPackId": LIVE_CLOUD_PACK_ID,
            "runRequested": run_benchmark,
            "message": live_cloud_no_targets_message(&plan.skipped),
            "skipped": plan.skipped
        }));
    }

    let conn = store::open_memory().map_err(|err| err.to_string())?;
    for target in &plan.targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let mut validations = Vec::new();
    for target in &plan.targets {
        validations.push(validate_target_for_conn(&conn, &target.id)?);
    }
    let valid_target_ids = validations
        .iter()
        .filter(|validation| validation.status == "ok")
        .map(|validation| validation.target_id.clone())
        .collect::<Vec<_>>();

    let mut run_results = Vec::new();
    let mut run_error = None;
    let mut benchmark_target_ids = Vec::new();
    let mut benchmark_skipped_targets = Vec::new();
    if run_benchmark {
        if valid_target_ids.is_empty() {
            run_error =
                Some("No live cloud target validated successfully; benchmark run skipped.".into());
        } else {
            let planned = live_cloud_benchmark_target_ids_for_cap(&conn, &valid_target_ids)?;
            benchmark_target_ids = planned.0;
            benchmark_skipped_targets = planned.1;
            if benchmark_target_ids.is_empty() {
                run_error = Some(
                    "No validated live cloud target had pricing metadata; benchmark run skipped. Add provider/model pricing env vars or use validation-only mode."
                        .into(),
                );
            } else {
                let request =
                    live_cloud_benchmark_request(benchmark_target_ids.clone(), max_cost_usd);
                if let Err(err) = enforce_run_cost_limit(&conn, &request) {
                    run_error = Some(err);
                } else {
                    run_results = runner::run_quick_smoke(&conn, request)?;
                }
            }
        }
    }

    let status = live_cloud_smoke_status(
        run_benchmark,
        run_error.is_some(),
        benchmark_skipped_targets.len(),
    );
    Ok(serde_json::json!({
        "status": status,
        "benchmarkPackId": LIVE_CLOUD_PACK_ID,
        "runRequested": run_benchmark,
        "maxCostUsd": max_cost_usd,
        "targetIds": plan.targets.iter().map(|target| target.id.clone()).collect::<Vec<_>>(),
        "validatedTargetIds": valid_target_ids,
        "benchmarkTargetIds": benchmark_target_ids,
        "benchmarkSkippedTargets": benchmark_skipped_targets,
        "validations": validations,
        "results": run_results,
        "runError": run_error,
        "skipped": plan.skipped,
        "notes": [
            "Validation uses real provider endpoints and may spend a tiny completion probe.",
            "Set BENCHFORGE_LIVE_CLOUD_RUN=1 to run the llm-connectivity pack after successful validation.",
            "Set BENCHFORGE_LIVE_CLOUD_PROVIDERS to a comma-separated subset such as openai,anthropic,openrouter,mistral,azure-openai,gemini."
        ]
    }))
}

fn live_cloud_smoke_status(
    run_benchmark: bool,
    has_run_error: bool,
    benchmark_skipped_target_count: usize,
) -> &'static str {
    if has_run_error || benchmark_skipped_target_count > 0 {
        "partial"
    } else if run_benchmark {
        "completed"
    } else {
        "validated"
    }
}

fn live_cloud_no_targets_message(skipped: &[LiveCloudSkippedProvider]) -> String {
    let unsupported = skipped
        .iter()
        .filter(|skip| skip.reason == "unsupported_provider")
        .map(|skip| skip.provider.as_str())
        .collect::<Vec<_>>();
    if !unsupported.is_empty() && unsupported.len() == skipped.len() {
        return format!(
            "No supported live cloud providers matched BENCHFORGE_LIVE_CLOUD_PROVIDERS: {}. Use one or more of: {}.",
            unsupported.join(", "),
            live_cloud_supported_provider_ids().join(", ")
        );
    }
    if !unsupported.is_empty() {
        return format!(
            "No runnable live cloud targets were available. Unsupported provider filter value(s): {}. Review skipped entries for missing keys, models, or endpoints.",
            unsupported.join(", ")
        );
    }
    if skipped.iter().any(|skip| skip.reason == "missing_key") {
        return "No live cloud providers were configured. Set a provider API key in Keychain or environment, then optionally set BENCHFORGE_LIVE_CLOUD_RUN=1.".into();
    }
    "No live cloud targets were available. Review skipped entries for missing models, endpoints, adapters, or setup requirements.".into()
}

fn live_cloud_max_cost_usd(env_get: &dyn Fn(&str) -> Option<String>) -> Result<f64, String> {
    let Some(raw) = env_get("BENCHFORGE_LIVE_CLOUD_MAX_COST_USD") else {
        return Ok(LIVE_CLOUD_DEFAULT_MAX_COST_USD);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(LIVE_CLOUD_DEFAULT_MAX_COST_USD);
    }
    let value = trimmed.parse::<f64>().map_err(|_| {
        format!(
            "max_cost_invalid: BENCHFORGE_LIVE_CLOUD_MAX_COST_USD must be a non-negative finite number, got '{}'",
            trimmed
        )
    })?;
    if !value.is_finite() || value < 0.0 {
        return Err(format!(
            "max_cost_invalid: BENCHFORGE_LIVE_CLOUD_MAX_COST_USD must be a non-negative finite number, got '{}'",
            trimmed
        ));
    }
    Ok(value)
}

fn live_cloud_benchmark_request(
    target_ids: Vec<String>,
    max_cost_usd: f64,
) -> runner::RunQuickSmokeRequest {
    runner::RunQuickSmokeRequest {
        target_ids,
        benchmark_pack_id: LIVE_CLOUD_PACK_ID.into(),
        task_ids: vec![],
        repetitions: 1,
        docker: false,
        warmup_runs: 0,
        concurrency: 1,
        max_cost_usd: Some(max_cost_usd),
        run_group_id: None,
    }
}

fn live_cloud_benchmark_target_ids_for_cap(
    conn: &rusqlite::Connection,
    valid_target_ids: &[String],
) -> Result<(Vec<String>, Vec<serde_json::Value>), String> {
    let estimate = estimate_run_plan_for_conn(
        conn,
        &RunEstimateRequest {
            target_ids: valid_target_ids.to_vec(),
            benchmark_pack_id: LIVE_CLOUD_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            warmup_runs: 0,
            concurrency: 1,
        },
    )?;
    let unpriced_targets = estimate
        .unpriced_targets
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut run_target_ids = Vec::new();
    let mut skipped = Vec::new();
    for target_id in valid_target_ids {
        if unpriced_targets.contains(target_id) {
            skipped.push(serde_json::json!({
                "targetId": target_id,
                "reason": "missing_pricing",
                "detail": "Skipped from the live benchmark run because maxCostUsd is enabled and this provider/model has no pricing metadata."
            }));
        } else {
            run_target_ids.push(target_id.clone());
        }
    }
    Ok((run_target_ids, skipped))
}

fn live_cloud_target_plan(
    provider_filter: Option<&BTreeSet<String>>,
    env_get: &dyn Fn(&str) -> Option<String>,
    key_available: &dyn Fn(&str) -> bool,
) -> Result<LiveCloudTargetPlan, String> {
    let mut targets = Vec::new();
    let mut skipped = Vec::new();
    let specs = live_cloud_provider_specs();
    if let Some(filter) = provider_filter {
        let supported = specs
            .iter()
            .map(|spec| spec.adapter_id.to_string())
            .collect::<BTreeSet<_>>();
        let supported_detail = supported.iter().cloned().collect::<Vec<_>>().join(", ");
        for provider in filter
            .iter()
            .filter(|provider| !supported.contains(*provider))
        {
            skipped.push(live_cloud_skip_provider(
                provider,
                "unsupported_provider",
                &format!(
                    "Unsupported live cloud provider filter '{}'. Supported providers: {}.",
                    provider, supported_detail
                ),
            ));
        }
    }
    for spec in specs {
        if provider_filter.is_some_and(|filter| !filter.contains(spec.adapter_id)) {
            continue;
        }
        let Some(adapter) = adapters::find_adapter(spec.adapter_id)? else {
            skipped.push(live_cloud_skip(
                &spec,
                "adapter_missing",
                &format!("Adapter {} is not installed.", spec.adapter_id),
            ));
            continue;
        };
        if !key_available(spec.adapter_id) {
            skipped.push(live_cloud_skip(
                &spec,
                "missing_key",
                &format!(
                    "No key found for {}; save one in Keychain or set {}.",
                    spec.label,
                    adapter
                        .spec
                        .validation
                        .get("secret_env")
                        .and_then(|value| value.as_str())
                        .unwrap_or("the provider API key env var")
                ),
            ));
            continue;
        }

        let model = env_get(spec.model_env)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| spec.default_model.map(str::to_string));
        let Some(model) = model else {
            skipped.push(live_cloud_skip(
                &spec,
                "missing_model",
                &format!(
                    "Set {} to the provider model or deployment name to test.",
                    spec.model_env
                ),
            ));
            continue;
        };

        let base_url = spec
            .base_url_env
            .and_then(env_get)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| adapter.spec.default_base_url.clone());
        let Some(base_url) = base_url else {
            skipped.push(live_cloud_skip(
                &spec,
                "missing_base_url",
                "No base URL was available for this provider.",
            ));
            continue;
        };
        if base_url.contains("YOUR-RESOURCE-NAME") {
            let env_name = spec
                .base_url_env
                .unwrap_or("BENCHFORGE_LIVE_PROVIDER_BASE_URL");
            skipped.push(live_cloud_skip(
                &spec,
                "missing_base_url",
                &format!("Set {} to a real provider endpoint.", env_name),
            ));
            continue;
        }

        let mut config = serde_json::json!({
            "model": model,
            "base_url": base_url,
            "api_key_keychain": spec.adapter_id,
            "temperature": 0,
            "top_p": 1,
            "max_tokens": 32,
            "timeout_seconds": 60,
            "retry_count": 0,
            "source": "live-cloud-smoke"
        });
        if let Some(secret_env) = adapter
            .spec
            .validation
            .get("secret_env")
            .and_then(|value| value.as_str())
        {
            config["api_key_env"] = serde_json::json!(secret_env);
        }
        if let Some(version) = live_cloud_env_value(env_get, spec.adapter_id, "API_VERSION") {
            config["api_version"] = serde_json::json!(version);
        }
        if let Some((input_price, output_price)) = live_cloud_pricing(
            env_get,
            spec.adapter_id,
            &adapter.spec,
            config["model"].as_str().unwrap_or_default(),
        )? {
            config["input_price_usd_per_million_tokens"] = serde_json::json!(input_price);
            config["output_price_usd_per_million_tokens"] = serde_json::json!(output_price);
            config["pricing_source"] = serde_json::json!("live-cloud-smoke");
        }

        targets.push(store::NewTarget {
            id: format!("live-{}", spec.adapter_id),
            name: format!("Live {}", spec.label),
            kind: "direct_model".into(),
            adapter_id: spec.adapter_id.into(),
            config,
        });
    }
    Ok(LiveCloudTargetPlan { targets, skipped })
}

fn live_cloud_provider_specs() -> Vec<LiveCloudProviderSpec> {
    vec![
        LiveCloudProviderSpec {
            adapter_id: "openai",
            label: "OpenAI",
            model_env: "BENCHFORGE_LIVE_OPENAI_MODEL",
            base_url_env: Some("BENCHFORGE_LIVE_OPENAI_BASE_URL"),
            default_model: Some("gpt-5-mini"),
        },
        LiveCloudProviderSpec {
            adapter_id: "anthropic",
            label: "Anthropic",
            model_env: "BENCHFORGE_LIVE_ANTHROPIC_MODEL",
            base_url_env: Some("BENCHFORGE_LIVE_ANTHROPIC_BASE_URL"),
            default_model: Some("claude-haiku-4-5"),
        },
        LiveCloudProviderSpec {
            adapter_id: "mistral",
            label: "Mistral",
            model_env: "BENCHFORGE_LIVE_MISTRAL_MODEL",
            base_url_env: Some("BENCHFORGE_LIVE_MISTRAL_BASE_URL"),
            default_model: Some("mistral-large-latest"),
        },
        LiveCloudProviderSpec {
            adapter_id: "openrouter",
            label: "OpenRouter",
            model_env: "BENCHFORGE_LIVE_OPENROUTER_MODEL",
            base_url_env: Some("BENCHFORGE_LIVE_OPENROUTER_BASE_URL"),
            default_model: None,
        },
        LiveCloudProviderSpec {
            adapter_id: "azure-openai",
            label: "Azure OpenAI",
            model_env: "BENCHFORGE_LIVE_AZURE_OPENAI_MODEL",
            base_url_env: Some("BENCHFORGE_LIVE_AZURE_OPENAI_BASE_URL"),
            default_model: None,
        },
        LiveCloudProviderSpec {
            adapter_id: "gemini",
            label: "Google Gemini",
            model_env: "BENCHFORGE_LIVE_GEMINI_MODEL",
            base_url_env: Some("BENCHFORGE_LIVE_GEMINI_BASE_URL"),
            default_model: Some("gemini-2.5-flash-lite"),
        },
    ]
}

fn live_cloud_supported_provider_ids() -> Vec<String> {
    live_cloud_provider_specs()
        .into_iter()
        .map(|spec| spec.adapter_id.to_string())
        .collect()
}

fn live_cloud_skip(
    spec: &LiveCloudProviderSpec,
    reason: &str,
    detail: &str,
) -> LiveCloudSkippedProvider {
    live_cloud_skip_provider(spec.adapter_id, reason, detail)
}

fn live_cloud_skip_provider(
    provider: &str,
    reason: &str,
    detail: &str,
) -> LiveCloudSkippedProvider {
    LiveCloudSkippedProvider {
        provider: provider.into(),
        reason: reason.into(),
        detail: detail.into(),
    }
}

fn live_cloud_provider_filter() -> Option<BTreeSet<String>> {
    let raw = std::env::var("BENCHFORGE_LIVE_CLOUD_PROVIDERS").ok()?;
    let items = raw
        .split(',')
        .filter_map(|item| normalize_live_provider_id(item.trim()))
        .collect::<BTreeSet<_>>();
    (!items.is_empty()).then_some(items)
}

fn normalize_live_provider_id(value: &str) -> Option<String> {
    let normalized = value.trim().to_lowercase().replace('_', "-");
    match normalized.as_str() {
        "" => None,
        "azure" | "azure-openai" => Some("azure-openai".into()),
        "open-ai" | "openai" => Some("openai".into()),
        "claude" | "anthropic" => Some("anthropic".into()),
        "mistral" => Some("mistral".into()),
        "openrouter" | "open-router" => Some("openrouter".into()),
        "gemini" | "google" | "google-gemini" => Some("gemini".into()),
        other => Some(other.to_string()),
    }
}

fn live_cloud_env_value(
    env_get: &dyn Fn(&str) -> Option<String>,
    adapter_id: &str,
    suffix: &str,
) -> Option<String> {
    let name = live_cloud_env_name(adapter_id, suffix);
    env_get(&name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn live_cloud_env_name(adapter_id: &str, suffix: &str) -> String {
    format!(
        "BENCHFORGE_LIVE_{}_{}",
        live_cloud_env_prefix(adapter_id),
        suffix
    )
}

fn live_cloud_env_prefix(adapter_id: &str) -> String {
    adapter_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn live_cloud_pricing(
    env_get: &dyn Fn(&str) -> Option<String>,
    adapter_id: &str,
    adapter: &adapters::AdapterSpec,
    model: &str,
) -> Result<Option<(f64, f64)>, String> {
    let input_name = live_cloud_env_name(adapter_id, "INPUT_PRICE_USD_PER_MILLION_TOKENS");
    let output_name = live_cloud_env_name(adapter_id, "OUTPUT_PRICE_USD_PER_MILLION_TOKENS");
    let input_env = env_get(&input_name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let output_env = env_get(&output_name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match (input_env, output_env) {
        (Some(input), Some(output)) => Ok(Some((
            parse_live_cloud_price_override(&input_name, &input)?,
            parse_live_cloud_price_override(&output_name, &output)?,
        ))),
        (Some(_), None) | (None, Some(_)) => Err(format!(
            "pricing_invalid: set both {input_name} and {output_name}, or unset both to use catalog pricing"
        )),
        (None, None) => Ok(adapter_model_preset_catalog(adapter, model)
            .into_iter()
            .find(|preset| preset.model == model)
            .and_then(|preset| {
                Some((
                    preset.input_price_usd_per_million_tokens?,
                    preset.output_price_usd_per_million_tokens?,
                ))
            })),
    }
}

fn parse_live_cloud_price_override(name: &str, value: &str) -> Result<f64, String> {
    let price = value.parse::<f64>().map_err(|_| {
        format!("pricing_invalid: {name} must be a non-negative finite number, got '{value}'")
    })?;
    if !price.is_finite() || price < 0.0 {
        return Err(format!(
            "pricing_invalid: {name} must be a non-negative finite number, got '{value}'"
        ));
    }
    Ok(price)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn run_hf_local_cloud_connectivity_job(
    state: &store::AppState,
    cloud_base_url: &str,
    port: u16,
    download: &huggingface::DownloadedModelDto,
    start_status: &huggingface::HuggingFaceStatusDto,
) -> Result<serde_json::Value, String> {
    let served_model = start_status
        .server_model_id
        .clone()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| HF_LOCAL_CLOUD_FILENAME.into());
    let targets = hf_local_cloud_targets(cloud_base_url, port, &served_model, &download.path);
    let target_ids = local_cloud_compare_target_ids(&targets);
    let conn = state
        .conn
        .lock()
        .map_err(|err| format!("failed to lock store: {err}"))?;
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: HF_LOCAL_CLOUD_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(180))?;
    if finished.status != "completed" {
        return Err(format!(
            "hf_local_cloud_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_hf_local_cloud_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        HF_LOCAL_CLOUD_PACK_ID,
        32,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_hf_local_cloud_report(&report)?;

    Ok(serde_json::json!({
        "targetIds": target_ids,
        "benchmarkPackId": HF_LOCAL_CLOUD_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "results": finished.results
    }))
}

fn run_hf_local_cloud_basics_job(
    state: &store::AppState,
    cloud_base_url: &str,
    port: u16,
    download: &huggingface::DownloadedModelDto,
    start_status: &huggingface::HuggingFaceStatusDto,
) -> Result<serde_json::Value, String> {
    let served_model = start_status
        .server_model_id
        .clone()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| HF_LOCAL_CLOUD_FILENAME.into());
    let targets =
        hf_local_cloud_basics_targets(cloud_base_url, port, &served_model, &download.path);
    let target_ids = local_cloud_compare_target_ids(&targets);
    let conn = state
        .conn
        .lock()
        .map_err(|err| format!("failed to lock store: {err}"))?;
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let job = jobs::start_quick_smoke_job(
        &conn,
        runner::RunQuickSmokeRequest {
            target_ids: target_ids.clone(),
            benchmark_pack_id: HF_LOCAL_CLOUD_BASICS_PACK_ID.into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let finished = wait_for_run_job(&conn, &job.id, Duration::from_secs(300))?;
    if finished.status != "completed" {
        return Err(format!(
            "hf_local_cloud_basics_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    validate_hf_local_cloud_basics_result_records(&finished.results)?;
    validate_local_cloud_job_run_group(
        &conn,
        &finished.run_group_id,
        &target_ids,
        HF_LOCAL_CLOUD_BASICS_PACK_ID,
        128,
    )?;

    let run_ids = finished
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = export_report_folder_for_conn(&conn, Some(run_ids))?;
    let report_path = Path::new(&export_path).join("README.md");
    let report = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    validate_hf_local_cloud_basics_report(&report)?;

    let passed = finished
        .results
        .iter()
        .filter(|result| result.status == "passed")
        .count();
    let failed = finished
        .results
        .iter()
        .filter(|result| result.status == "failed")
        .count();

    Ok(serde_json::json!({
        "targetIds": target_ids,
        "benchmarkPackId": HF_LOCAL_CLOUD_BASICS_PACK_ID,
        "jobId": finished.id,
        "runGroupId": finished.run_group_id,
        "exportPath": export_path,
        "passedResults": passed,
        "failedResults": failed,
        "results": finished.results
    }))
}

fn hf_local_cloud_targets(
    cloud_base_url: &str,
    local_port: u16,
    served_model: &str,
    model_path: &str,
) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: HF_LOCAL_CLOUD_LOCAL_TARGET_ID.into(),
            name: "HF Local tinygemma3 llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": served_model,
                "base_url": format!("http://127.0.0.1:{}/v1", local_port),
                "source": "huggingface-local-cloud-smoke",
                "repo_id": HF_LOCAL_CLOUD_REPO_ID,
                "gguf_file": HF_LOCAL_CLOUD_FILENAME,
                "model_path": model_path,
                "port": local_port,
                "context": HF_LOCAL_CLOUD_CONTEXT,
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 32,
                "timeout_seconds": 120,
                "retry_count": 1,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: HF_LOCAL_CLOUD_CLOUD_TARGET_ID.into(),
            name: "HF Comparison Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-ok",
                "base_url": format!("{cloud_base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-for-hf-local",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 32,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn hf_local_cloud_basics_targets(
    cloud_base_url: &str,
    local_port: u16,
    served_model: &str,
    model_path: &str,
) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: HF_LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID.into(),
            name: "HF Local tinygemma3 Basics llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": served_model,
                "base_url": format!("http://127.0.0.1:{}/v1", local_port),
                "source": "huggingface-local-cloud-basics-smoke",
                "repo_id": HF_LOCAL_CLOUD_REPO_ID,
                "gguf_file": HF_LOCAL_CLOUD_FILENAME,
                "model_path": model_path,
                "port": local_port,
                "context": HF_LOCAL_CLOUD_CONTEXT,
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 128,
                "timeout_seconds": 180,
                "retry_count": 1,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: HF_LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID.into(),
            name: "HF Basics Comparison Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-basics",
                "base_url": format!("{cloud_base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-for-hf-local-basics",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 128,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_connectivity_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: LOCAL_CLOUD_CONNECTIVITY_LOCAL_TARGET_ID.into(),
            name: "Connectivity Local llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": "contract-ok",
                "base_url": format!("{base_url}/v1"),
                "source": "local-loopback-connectivity",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 32,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: LOCAL_CLOUD_CONNECTIVITY_CLOUD_TARGET_ID.into(),
            name: "Connectivity Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-ok",
                "base_url": format!("{base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-connectivity",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 32,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_basics_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID.into(),
            name: "Basics Local llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": "contract-basics",
                "base_url": format!("{base_url}/v1"),
                "source": "local-loopback-basics",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 128,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID.into(),
            name: "Basics Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-basics",
                "base_url": format!("{base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-basics",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 128,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_core_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: LOCAL_CLOUD_CORE_LOCAL_TARGET_ID.into(),
            name: "Core Local llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": "contract-core",
                "base_url": format!("{base_url}/v1"),
                "source": "local-loopback-core",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: LOCAL_CLOUD_CORE_CLOUD_TARGET_ID.into(),
            name: "Core Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-core",
                "base_url": format!("{base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-core",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_compare_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: LOCAL_CLOUD_COMPARE_LOCAL_TARGET_ID.into(),
            name: "Compare Local llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": "contract-ok",
                "base_url": format!("{base_url}/v1"),
                "source": "local-loopback-compare",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 16,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: LOCAL_CLOUD_COMPARE_CLOUD_TARGET_ID.into(),
            name: "Compare Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-ok",
                "base_url": format!("{base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-compare",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 16,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_practical_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: LOCAL_CLOUD_PRACTICAL_LOCAL_TARGET_ID.into(),
            name: "Practical Local llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": "contract-practical",
                "base_url": format!("{base_url}/v1"),
                "source": "local-loopback-practical",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: LOCAL_CLOUD_PRACTICAL_CLOUD_TARGET_ID.into(),
            name: "Practical Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-practical",
                "base_url": format!("{base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-practical",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_decision_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: LOCAL_CLOUD_DECISION_LOCAL_TARGET_ID.into(),
            name: "Decision Local llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": "contract-decision",
                "base_url": format!("{base_url}/v1"),
                "source": "local-loopback-decision",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: LOCAL_CLOUD_DECISION_CLOUD_TARGET_ID.into(),
            name: "Decision Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-decision",
                "base_url": format!("{base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-decision",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_structured_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: LOCAL_CLOUD_STRUCTURED_LOCAL_TARGET_ID.into(),
            name: "Structured Local llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": "contract-structured",
                "base_url": format!("{base_url}/v1"),
                "source": "local-loopback-structured",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: LOCAL_CLOUD_STRUCTURED_CLOUD_TARGET_ID.into(),
            name: "Structured Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-structured",
                "base_url": format!("{base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-structured",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_grounded_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: LOCAL_CLOUD_GROUNDED_LOCAL_TARGET_ID.into(),
            name: "Grounded Local llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": "contract-grounded",
                "base_url": format!("{base_url}/v1"),
                "source": "local-loopback-grounded",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: LOCAL_CLOUD_GROUNDED_CLOUD_TARGET_ID.into(),
            name: "Grounded Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-grounded",
                "base_url": format!("{base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-grounded",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_reliability_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        store::NewTarget {
            id: LOCAL_CLOUD_RELIABILITY_LOCAL_TARGET_ID.into(),
            name: "Reliability Local llama.cpp".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config: serde_json::json!({
                "model": "contract-reliability",
                "base_url": format!("{base_url}/v1"),
                "source": "local-loopback-reliability",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        },
        store::NewTarget {
            id: LOCAL_CLOUD_RELIABILITY_CLOUD_TARGET_ID.into(),
            name: "Reliability Cloud OpenRouter".into(),
            kind: "direct_model".into(),
            adapter_id: "openrouter".into(),
            config: serde_json::json!({
                "model": "contract-reliability",
                "base_url": format!("{base_url}/v1"),
                "api_key_env": "BENCHFORGE_LOCAL_CLOUD_COMPARE_KEY",
                "source": "cloud-loopback-reliability",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 256,
                "timeout_seconds": 10,
                "retry_count": 0,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 2.0
            }),
        },
    ]
}

fn local_cloud_compare_target_ids(targets: &[store::NewTarget]) -> Vec<String> {
    targets.iter().map(|target| target.id.clone()).collect()
}

fn wait_for_run_job(
    conn: &rusqlite::Connection,
    id: &str,
    timeout: Duration,
) -> Result<jobs::RunJobDto, String> {
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > timeout {
            return Err(format!(
                "run_job_timeout: job {} did not finish within {:?}",
                id, timeout
            ));
        }
        thread::sleep(Duration::from_millis(250));
        match jobs::get_job(conn, id)? {
            Some(next) if next.status == "queued" || next.status == "running" => continue,
            Some(next) => return Ok(next),
            None => return Err(format!("run_job_missing: job {} disappeared", id)),
        }
    }
}

fn validate_cloud_provider_job_run_group(
    conn: &rusqlite::Connection,
    run_group_id: &str,
    targets: &[store::NewTarget],
) -> Result<(), String> {
    let run_groups = store::list_run_groups(conn).map_err(|err| err.to_string())?;
    let Some(run_group) = run_groups.iter().find(|group| group.id == run_group_id) else {
        return Err(format!(
            "cloud_provider_job_group_failed: missing run group {}",
            run_group_id
        ));
    };
    let mut failures = Vec::new();
    if run_group.benchmark_pack_id != CLOUD_PROVIDER_JOB_PACK_ID {
        failures.push(format!("unexpected pack {}", run_group.benchmark_pack_id));
    }
    if run_group.status != "completed" {
        failures.push(format!("unexpected status {}", run_group.status));
    }
    if run_group
        .config
        .get("concurrency")
        .and_then(|value| value.as_u64())
        != Some(4)
    {
        failures.push("run group did not preserve concurrency 4".into());
    }
    let serialized = serde_json::to_string(&run_group.config).unwrap_or_default();
    for forbidden in [
        runner::CLOUD_CONTRACT_API_KEY_ENV,
        "benchforge-contract-key",
        "api_key_env",
    ] {
        if serialized.contains(forbidden) {
            failures.push(format!("run group snapshot leaked {forbidden}"));
        }
    }
    let config_targets = run_group
        .config
        .get("targets")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "cloud_provider_job_group_failed: config has no targets".to_string())?;
    for expected in targets {
        if !run_group.target_ids.contains(&expected.id) {
            failures.push(format!("run group missing target {}", expected.id));
        }
        let Some(target) = config_targets.iter().find(|target| {
            target
                .get("id")
                .and_then(|value| value.as_str())
                .is_some_and(|id| id == expected.id)
        }) else {
            failures.push(format!("config missing target {}", expected.id));
            continue;
        };
        if target.get("adapter_id").and_then(|value| value.as_str())
            != Some(expected.adapter_id.as_str())
        {
            failures.push(format!("{} has wrong adapter snapshot", expected.id));
        }
        if target.get("model").and_then(|value| value.as_str())
            != expected
                .config
                .get("model")
                .and_then(|value| value.as_str())
        {
            failures.push(format!("{} has wrong model snapshot", expected.id));
        }
        if target
            .get("base_url")
            .and_then(|value| value.as_str())
            .is_none_or(|value| value.trim().is_empty())
        {
            failures.push(format!("{} missing sanitized base URL", expected.id));
        }
        if target.get("streaming").and_then(|value| value.as_bool())
            != expected
                .config
                .get("streaming")
                .and_then(|value| value.as_bool())
        {
            failures.push(format!("{} has wrong streaming snapshot", expected.id));
        }
        if target
            .pointer("/generation/max_tokens")
            .and_then(|value| value.as_u64())
            != Some(16)
        {
            failures.push(format!("{} has wrong max_tokens snapshot", expected.id));
        }
        let expected_retry_count = expected
            .config
            .get("retry_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        if target
            .pointer("/generation/retry_count")
            .and_then(|value| value.as_u64())
            != Some(expected_retry_count)
        {
            failures.push(format!("{} has wrong retry_count snapshot", expected.id));
        }
        if target
            .pointer("/pricing/input_price_usd_per_million_tokens")
            .and_then(|value| value.as_f64())
            .is_none()
            || target
                .pointer("/pricing/output_price_usd_per_million_tokens")
                .and_then(|value| value.as_f64())
                .is_none()
        {
            failures.push(format!("{} missing pricing snapshot", expected.id));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "cloud_provider_job_group_failed: {}",
            failures.join("; ")
        ))
    }
}

fn validate_local_cloud_compare_results(results: &[runner::RunResultDto]) -> Result<(), String> {
    let mut failures = Vec::new();
    if results.len() != 2 {
        failures.push(format!("expected 2 result rows, got {}", results.len()));
    }
    for target_id in [
        LOCAL_CLOUD_COMPARE_LOCAL_TARGET_ID,
        LOCAL_CLOUD_COMPARE_CLOUD_TARGET_ID,
    ] {
        let Some(result) = results.iter().find(|result| result.target_id == target_id) else {
            failures.push(format!("missing result for {}", target_id));
            continue;
        };
        if result.benchmark_pack_id != LOCAL_CLOUD_COMPARE_PACK_ID {
            failures.push(format!(
                "{} used unexpected pack {}",
                target_id, result.benchmark_pack_id
            ));
        }
        if result.status != "passed" || result.score != Some(1.0) {
            failures.push(format!(
                "{} expected passed score 1.0, got status {} score {:?}",
                target_id, result.status, result.score
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_compare_failed: {}",
            failures.join("; ")
        ))
    }
}

fn validate_local_cloud_compare_result_records(
    results: &[store::ResultRecord],
) -> Result<(), String> {
    let mut failures = Vec::new();
    if results.len() != 2 {
        failures.push(format!("expected 2 result rows, got {}", results.len()));
    }
    for target_id in [
        LOCAL_CLOUD_COMPARE_LOCAL_TARGET_ID,
        LOCAL_CLOUD_COMPARE_CLOUD_TARGET_ID,
    ] {
        let Some(result) = results.iter().find(|result| result.target_id == target_id) else {
            failures.push(format!("missing result for {}", target_id));
            continue;
        };
        if result.benchmark_pack_id != LOCAL_CLOUD_COMPARE_PACK_ID {
            failures.push(format!(
                "{} used unexpected pack {}",
                target_id, result.benchmark_pack_id
            ));
        }
        if result.status != "passed" || result.score != Some(1.0) {
            failures.push(format!(
                "{} expected passed score 1.0, got status {} score {:?}",
                target_id, result.status, result.score
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_compare_job_results_failed: {}",
            failures.join("; ")
        ))
    }
}

fn validate_local_cloud_connectivity_result_records(
    results: &[store::ResultRecord],
) -> Result<(), String> {
    validate_local_cloud_pack_result_records(
        results,
        LOCAL_CLOUD_CONNECTIVITY_PACK_ID,
        &[
            LOCAL_CLOUD_CONNECTIVITY_LOCAL_TARGET_ID,
            LOCAL_CLOUD_CONNECTIVITY_CLOUD_TARGET_ID,
        ],
        2,
        "local_cloud_connectivity_results_failed",
    )
}

fn validate_hf_local_cloud_result_records(results: &[store::ResultRecord]) -> Result<(), String> {
    validate_local_cloud_pack_result_records(
        results,
        HF_LOCAL_CLOUD_PACK_ID,
        &[
            HF_LOCAL_CLOUD_LOCAL_TARGET_ID,
            HF_LOCAL_CLOUD_CLOUD_TARGET_ID,
        ],
        2,
        "hf_local_cloud_results_failed",
    )
}

fn validate_hf_local_cloud_basics_result_records(
    results: &[store::ResultRecord],
) -> Result<(), String> {
    let target_ids = [
        HF_LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID,
        HF_LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID,
    ];
    let expected_tasks_per_target = 3;
    let mut failures = Vec::new();
    let expected_results = expected_tasks_per_target * target_ids.len();
    if results.len() != expected_results {
        failures.push(format!(
            "expected {} result rows, got {}",
            expected_results,
            results.len()
        ));
    }
    for target_id in target_ids {
        let target_results = results
            .iter()
            .filter(|result| result.target_id == target_id)
            .collect::<Vec<_>>();
        if target_results.len() != expected_tasks_per_target {
            failures.push(format!(
                "{} expected {} result rows, got {}",
                target_id,
                expected_tasks_per_target,
                target_results.len()
            ));
        }
        for result in target_results {
            if result.benchmark_pack_id != HF_LOCAL_CLOUD_BASICS_PACK_ID {
                failures.push(format!(
                    "{} used unexpected pack {}",
                    target_id, result.benchmark_pack_id
                ));
            }
            if result.status == "error" {
                failures.push(format!(
                    "{} / {} returned infrastructure error {:?}: {:?}",
                    target_id, result.task_id, result.error_code, result.error_message
                ));
            }
            if result.wall_time_ms.is_none() {
                failures.push(format!(
                    "{} / {} did not record wall time",
                    target_id, result.task_id
                ));
            }
            if target_id == HF_LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID
                && (result.status != "passed" || result.score != Some(1.0))
            {
                failures.push(format!(
                    "{} / {} expected cloud contract pass, got status {} score {:?}",
                    target_id, result.task_id, result.status, result.score
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "hf_local_cloud_basics_results_failed: {}",
            failures.join("; ")
        ))
    }
}

fn validate_local_cloud_basics_result_records(
    results: &[store::ResultRecord],
) -> Result<(), String> {
    validate_local_cloud_pack_result_records(
        results,
        LOCAL_CLOUD_BASICS_PACK_ID,
        &[
            LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID,
            LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID,
        ],
        3,
        "local_cloud_basics_results_failed",
    )
}

fn validate_local_cloud_core_result_records(results: &[store::ResultRecord]) -> Result<(), String> {
    validate_local_cloud_pack_result_records(
        results,
        LOCAL_CLOUD_CORE_PACK_ID,
        &[
            LOCAL_CLOUD_CORE_LOCAL_TARGET_ID,
            LOCAL_CLOUD_CORE_CLOUD_TARGET_ID,
        ],
        6,
        "local_cloud_core_results_failed",
    )
}

fn validate_local_cloud_practical_result_records(
    results: &[store::ResultRecord],
) -> Result<(), String> {
    validate_local_cloud_pack_result_records(
        results,
        LOCAL_CLOUD_PRACTICAL_PACK_ID,
        &[
            LOCAL_CLOUD_PRACTICAL_LOCAL_TARGET_ID,
            LOCAL_CLOUD_PRACTICAL_CLOUD_TARGET_ID,
        ],
        16,
        "local_cloud_practical_results_failed",
    )
}

fn validate_local_cloud_pack_result_records(
    results: &[store::ResultRecord],
    benchmark_pack_id: &str,
    target_ids: &[&str],
    expected_tasks_per_target: usize,
    label: &str,
) -> Result<(), String> {
    let mut failures = Vec::new();
    let expected_results = expected_tasks_per_target * target_ids.len();
    if results.len() != expected_results {
        failures.push(format!(
            "expected {} result rows, got {}",
            expected_results,
            results.len()
        ));
    }
    for target_id in target_ids {
        let target_results = results
            .iter()
            .filter(|result| result.target_id == *target_id)
            .collect::<Vec<_>>();
        if target_results.len() != expected_tasks_per_target {
            failures.push(format!(
                "{} expected {} result rows, got {}",
                target_id,
                expected_tasks_per_target,
                target_results.len()
            ));
        }
        for result in target_results {
            if result.benchmark_pack_id != benchmark_pack_id {
                failures.push(format!(
                    "{} used unexpected pack {}",
                    target_id, result.benchmark_pack_id
                ));
            }
            if result.status != "passed" || result.score != Some(1.0) {
                failures.push(format!(
                    "{} / {} expected passed score 1.0, got status {} score {:?}",
                    target_id, result.task_id, result.status, result.score
                ));
            }
            if result.wall_time_ms.is_none() {
                failures.push(format!(
                    "{} / {} did not record wall time",
                    target_id, result.task_id
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{}: {}", label, failures.join("; ")))
    }
}

fn validate_local_cloud_decision_result_records(
    results: &[store::ResultRecord],
) -> Result<(), String> {
    let mut failures = Vec::new();
    let expected_tasks_per_target = 10;
    let expected_results = expected_tasks_per_target * 2;
    if results.len() != expected_results {
        failures.push(format!(
            "expected {} result rows, got {}",
            expected_results,
            results.len()
        ));
    }
    for target_id in [
        LOCAL_CLOUD_DECISION_LOCAL_TARGET_ID,
        LOCAL_CLOUD_DECISION_CLOUD_TARGET_ID,
    ] {
        let target_results = results
            .iter()
            .filter(|result| result.target_id == target_id)
            .collect::<Vec<_>>();
        if target_results.len() != expected_tasks_per_target {
            failures.push(format!(
                "{} expected {} result rows, got {}",
                target_id,
                expected_tasks_per_target,
                target_results.len()
            ));
        }
        for result in target_results {
            if result.benchmark_pack_id != LOCAL_CLOUD_DECISION_PACK_ID {
                failures.push(format!(
                    "{} used unexpected pack {}",
                    target_id, result.benchmark_pack_id
                ));
            }
            if result.status != "passed" || result.score != Some(1.0) {
                failures.push(format!(
                    "{} / {} expected passed score 1.0, got status {} score {:?}",
                    target_id, result.task_id, result.status, result.score
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_decision_results_failed: {}",
            failures.join("; ")
        ))
    }
}

fn validate_local_cloud_structured_result_records(
    results: &[store::ResultRecord],
) -> Result<(), String> {
    validate_local_cloud_pack_result_records(
        results,
        LOCAL_CLOUD_STRUCTURED_PACK_ID,
        &[
            LOCAL_CLOUD_STRUCTURED_LOCAL_TARGET_ID,
            LOCAL_CLOUD_STRUCTURED_CLOUD_TARGET_ID,
        ],
        6,
        "local_cloud_structured_results_failed",
    )
}

fn validate_local_cloud_grounded_result_records(
    results: &[store::ResultRecord],
) -> Result<(), String> {
    validate_local_cloud_pack_result_records(
        results,
        LOCAL_CLOUD_GROUNDED_PACK_ID,
        &[
            LOCAL_CLOUD_GROUNDED_LOCAL_TARGET_ID,
            LOCAL_CLOUD_GROUNDED_CLOUD_TARGET_ID,
        ],
        6,
        "local_cloud_grounded_results_failed",
    )
}

fn validate_local_cloud_reliability_result_records(
    results: &[store::ResultRecord],
) -> Result<(), String> {
    let expected_task_count =
        runner::load_tasks(&runner::load_pack(LOCAL_CLOUD_RELIABILITY_PACK_ID)?)?.len();
    validate_local_cloud_pack_result_records(
        results,
        LOCAL_CLOUD_RELIABILITY_PACK_ID,
        &[
            LOCAL_CLOUD_RELIABILITY_LOCAL_TARGET_ID,
            LOCAL_CLOUD_RELIABILITY_CLOUD_TARGET_ID,
        ],
        expected_task_count,
        "local_cloud_reliability_results_failed",
    )
}

fn validate_local_cloud_job_run_group(
    conn: &rusqlite::Connection,
    run_group_id: &str,
    target_ids: &[String],
    benchmark_pack_id: &str,
    expected_max_tokens: u64,
) -> Result<(), String> {
    let run_groups = store::list_run_groups(conn).map_err(|err| err.to_string())?;
    let Some(run_group) = run_groups.iter().find(|group| group.id == run_group_id) else {
        return Err(format!(
            "local_cloud_job_group_failed: missing run group {}",
            run_group_id
        ));
    };
    let mut failures = Vec::new();
    if run_group.benchmark_pack_id != benchmark_pack_id {
        failures.push(format!("unexpected pack {}", run_group.benchmark_pack_id));
    }
    if run_group.status != "completed" {
        failures.push(format!("unexpected status {}", run_group.status));
    }
    for target_id in target_ids {
        if !run_group.target_ids.contains(target_id) {
            failures.push(format!("run group missing target {}", target_id));
        }
    }
    let config_targets = run_group
        .config
        .get("targets")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "local_cloud_job_group_failed: config has no targets".to_string())?;
    for target_id in target_ids {
        let adapter_id = local_cloud_expected_adapter_id(target_id);
        let Some(target) = config_targets.iter().find(|target| {
            target
                .get("id")
                .and_then(|value| value.as_str())
                .is_some_and(|id| id == target_id)
        }) else {
            failures.push(format!("config missing target {}", target_id));
            continue;
        };
        if target.get("adapter_id").and_then(|value| value.as_str()) != Some(adapter_id) {
            failures.push(format!("{} has wrong adapter snapshot", target_id));
        }
        if target
            .pointer("/generation/max_tokens")
            .and_then(|value| value.as_u64())
            != Some(expected_max_tokens)
        {
            failures.push(format!("{} has wrong max_tokens snapshot", target_id));
        }
        if target
            .pointer("/pricing/input_price_usd_per_million_tokens")
            .and_then(|value| value.as_f64())
            .is_none()
        {
            failures.push(format!("{} missing pricing snapshot", target_id));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_job_group_failed: {}",
            failures.join("; ")
        ))
    }
}

fn local_cloud_expected_adapter_id(target_id: &str) -> &'static str {
    match target_id {
        LOCAL_CLOUD_CONNECTIVITY_LOCAL_TARGET_ID
        | HF_LOCAL_CLOUD_LOCAL_TARGET_ID
        | HF_LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID
        | LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID
        | LOCAL_CLOUD_CORE_LOCAL_TARGET_ID
        | LOCAL_CLOUD_COMPARE_LOCAL_TARGET_ID
        | LOCAL_CLOUD_PRACTICAL_LOCAL_TARGET_ID
        | LOCAL_CLOUD_DECISION_LOCAL_TARGET_ID
        | LOCAL_CLOUD_STRUCTURED_LOCAL_TARGET_ID
        | LOCAL_CLOUD_GROUNDED_LOCAL_TARGET_ID
        | LOCAL_CLOUD_RELIABILITY_LOCAL_TARGET_ID => "llama-cpp-openai",
        LOCAL_CLOUD_CONNECTIVITY_CLOUD_TARGET_ID
        | HF_LOCAL_CLOUD_CLOUD_TARGET_ID
        | HF_LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID
        | LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID
        | LOCAL_CLOUD_CORE_CLOUD_TARGET_ID
        | LOCAL_CLOUD_COMPARE_CLOUD_TARGET_ID
        | LOCAL_CLOUD_PRACTICAL_CLOUD_TARGET_ID
        | LOCAL_CLOUD_DECISION_CLOUD_TARGET_ID
        | LOCAL_CLOUD_STRUCTURED_CLOUD_TARGET_ID
        | LOCAL_CLOUD_GROUNDED_CLOUD_TARGET_ID
        | LOCAL_CLOUD_RELIABILITY_CLOUD_TARGET_ID => "openrouter",
        _ => "",
    }
}

fn validate_hf_local_cloud_basics_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        HF_LOCAL_CLOUD_BASICS_PACK_ID,
        HF_LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID,
        HF_LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-instruction-following-001",
        "llm-json-validity-001",
        "llm-summarization-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "hf_local_cloud_basics_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_local_cloud_basics_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        LOCAL_CLOUD_BASICS_PACK_ID,
        LOCAL_CLOUD_BASICS_LOCAL_TARGET_ID,
        LOCAL_CLOUD_BASICS_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-instruction-following-001",
        "llm-json-validity-001",
        "llm-summarization-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_basics_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_local_cloud_connectivity_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        LOCAL_CLOUD_CONNECTIVITY_PACK_ID,
        LOCAL_CLOUD_CONNECTIVITY_LOCAL_TARGET_ID,
        LOCAL_CLOUD_CONNECTIVITY_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-connectivity-nonempty-001",
        "llm-connectivity-short-completion-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_connectivity_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_hf_local_cloud_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        HF_LOCAL_CLOUD_PACK_ID,
        HF_LOCAL_CLOUD_LOCAL_TARGET_ID,
        HF_LOCAL_CLOUD_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-connectivity-nonempty-001",
        "llm-connectivity-short-completion-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "hf_local_cloud_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_local_cloud_core_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        LOCAL_CLOUD_CORE_PACK_ID,
        LOCAL_CLOUD_CORE_LOCAL_TARGET_ID,
        LOCAL_CLOUD_CORE_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-core-classification-001",
        "llm-core-tool-call-001",
        "llm-core-synthesis-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_core_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_local_cloud_compare_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        LOCAL_CLOUD_COMPARE_PACK_ID,
        LOCAL_CLOUD_COMPARE_LOCAL_TARGET_ID,
        LOCAL_CLOUD_COMPARE_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "## Task Drilldown",
        "## Task Target Matrix",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_compare_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_local_cloud_practical_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        LOCAL_CLOUD_PRACTICAL_PACK_ID,
        LOCAL_CLOUD_PRACTICAL_LOCAL_TARGET_ID,
        LOCAL_CLOUD_PRACTICAL_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-practical-routing-001",
        "llm-practical-budget-cap-model-mix-001",
        "llm-practical-context-pruning-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_practical_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_local_cloud_decision_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        LOCAL_CLOUD_DECISION_PACK_ID,
        LOCAL_CLOUD_DECISION_LOCAL_TARGET_ID,
        LOCAL_CLOUD_DECISION_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-decision-model-ranking-001",
        "llm-decision-table-to-json-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_decision_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_local_cloud_structured_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        LOCAL_CLOUD_STRUCTURED_PACK_ID,
        LOCAL_CLOUD_STRUCTURED_LOCAL_TARGET_ID,
        LOCAL_CLOUD_STRUCTURED_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-structured-schema-extraction-001",
        "llm-structured-numeric-unit-conversion-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_structured_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_local_cloud_grounded_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        LOCAL_CLOUD_GROUNDED_PACK_ID,
        LOCAL_CLOUD_GROUNDED_LOCAL_TARGET_ID,
        LOCAL_CLOUD_GROUNDED_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-grounded-needle-citation-001",
        "llm-grounded-noisy-table-grounding-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_grounded_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_local_cloud_reliability_report(report: &str) -> Result<(), String> {
    let required = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        LOCAL_CLOUD_RELIABILITY_PACK_ID,
        LOCAL_CLOUD_RELIABILITY_LOCAL_TARGET_ID,
        LOCAL_CLOUD_RELIABILITY_CLOUD_TARGET_ID,
        "llama.cpp",
        "OpenRouter",
        "llm-reliability-ambiguous-requirements-001",
        "llm-reliability-correction-discipline-001",
        "llm-reliability-sample-size-caution-001",
        "llm-reliability-confidence-interval-overlap-001",
        "llm-reliability-privacy-preserving-eval-001",
        "llm-reliability-served-model-identity-001",
        "llm-reliability-latency-cost-slo-001",
        "llm-reliability-rate-limit-retry-001",
    ];
    let missing = required
        .iter()
        .filter(|needle| !report.contains(**needle))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local_cloud_reliability_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validate_cloud_provider_job_report(
    report: &str,
    targets: &[store::NewTarget],
) -> Result<(), String> {
    let mut missing = [
        "## Decision Snapshot",
        "## Comparison",
        "## Run Configuration",
        "## Task Drilldown",
        "## Task Target Matrix",
        CLOUD_PROVIDER_JOB_PACK_ID,
        "cloud-provider-contract-001",
        "OpenAI-compatible",
        "OpenAI",
        "Anthropic",
        "Mistral",
        "OpenRouter",
        "Azure OpenAI",
        "Google Gemini",
    ]
    .iter()
    .filter(|needle| !report.contains(**needle))
    .map(|needle| (*needle).to_string())
    .collect::<Vec<_>>();
    for target in targets {
        if !report.contains(&target.id) {
            missing.push(target.id.clone());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "cloud_provider_job_report_failed: missing {}",
            missing.join(", ")
        ))
    }
}

fn validation_contract_targets(
    contract_base_url: &str,
    network_base_url: &str,
) -> Vec<store::NewTarget> {
    validation_contract_cases()
        .into_iter()
        .map(|case| {
            let base_url = if case.model == "contract-network" {
                network_base_url
            } else {
                contract_base_url
            };
            validation_contract_target(case.target_id, case.adapter_id, case.model, base_url)
        })
        .collect()
}

fn validation_contract_target(
    id: &str,
    adapter_id: &str,
    model: &str,
    base_url: &str,
) -> store::NewTarget {
    let mut config = serde_json::json!({
        "model": model,
        "base_url": format!("{}/v1", base_url),
        "temperature": 0,
        "top_p": 1,
        "max_tokens": 16,
        "timeout_seconds": 10,
        "retry_count": 0
    });
    if adapter_id != "openai-compatible" && model != "contract-missing-key" {
        config["api_key_env"] = serde_json::json!("BENCHFORGE_VALIDATION_CONTRACT_KEY");
    }
    store::NewTarget {
        id: id.into(),
        name: format!("Validation contract {}", model),
        kind: "direct_model".into(),
        adapter_id: adapter_id.into(),
        config,
    }
}

#[derive(Clone, Copy)]
struct ValidationContractCase {
    target_id: &'static str,
    adapter_id: &'static str,
    model: &'static str,
    expected_status: &'static str,
    expected_code: &'static str,
}

fn validation_contract_cases() -> Vec<ValidationContractCase> {
    vec![
        ValidationContractCase {
            target_id: "validation-ok",
            adapter_id: "openrouter",
            model: "contract-ok",
            expected_status: "ok",
            expected_code: "completion probe succeeded",
        },
        ValidationContractCase {
            target_id: "validation-gemini-ok",
            adapter_id: "gemini",
            model: "contract-gemini",
            expected_status: "ok",
            expected_code: "completion probe succeeded",
        },
        ValidationContractCase {
            target_id: "validation-auth",
            adapter_id: "openrouter",
            model: "contract-auth",
            expected_status: "error",
            expected_code: "auth",
        },
        ValidationContractCase {
            target_id: "validation-rate-limit",
            adapter_id: "openrouter",
            model: "contract-rate-limit",
            expected_status: "error",
            expected_code: "rate_limited",
        },
        ValidationContractCase {
            target_id: "validation-model-not-found",
            adapter_id: "openrouter",
            model: "contract-model-not-found",
            expected_status: "error",
            expected_code: "model_not_found",
        },
        ValidationContractCase {
            target_id: "validation-malformed-response",
            adapter_id: "openrouter",
            model: "contract-malformed-response",
            expected_status: "error",
            expected_code: "unsupported_shape",
        },
        ValidationContractCase {
            target_id: "validation-server-error",
            adapter_id: "openrouter",
            model: "contract-server-error",
            expected_status: "error",
            expected_code: "server_error",
        },
        ValidationContractCase {
            target_id: "validation-network",
            adapter_id: "openrouter",
            model: "contract-network",
            expected_status: "error",
            expected_code: "endpoint_unreachable",
        },
        ValidationContractCase {
            target_id: "validation-missing-key",
            adapter_id: "openrouter",
            model: "contract-missing-key",
            expected_status: "error",
            expected_code: "missing_key",
        },
    ]
}

fn validate_validation_contract_results(validations: &[TargetValidationDto]) -> Result<(), String> {
    let mut failures = Vec::new();
    for case in validation_contract_cases() {
        let Some(validation) = validations
            .iter()
            .find(|validation| validation.target_id == case.target_id)
        else {
            failures.push(format!("missing validation result for {}", case.target_id));
            continue;
        };
        if validation.status != case.expected_status {
            failures.push(format!(
                "{} expected status {}, got {}",
                case.target_id, case.expected_status, validation.status
            ));
        }
        if !validation.detail.starts_with(case.expected_code)
            && !validation.detail.contains(case.expected_code)
        {
            failures.push(format!(
                "{} expected detail to include {}, got {}",
                case.target_id, case.expected_code, validation.detail
            ));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "validation_contract_failed: {}",
            failures.join("; ")
        ))
    }
}

struct ValidationContractServer {
    base_url: String,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ValidationContractServer {
    fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|err| err.to_string())?;
        let addr = listener.local_addr().map_err(|err| err.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let handle = thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        thread::spawn(move || {
                            let _ = handle_validation_contract_connection(stream);
                        });
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            base_url: format!("http://{}", addr),
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for ValidationContractServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(addr) = self
            .base_url
            .trim_start_matches("http://")
            .parse::<SocketAddr>()
        {
            let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(100));
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn reserve_unbound_validation_base_url() -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    drop(listener);
    Ok(format!("http://127.0.0.1:{}", port))
}

fn reserve_loopback_smoke_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    drop(listener);
    Ok(port)
}

fn handle_validation_contract_connection(mut stream: TcpStream) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|err| err.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|err| err.to_string())?);
    let mut request = String::new();
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|err| err.to_string())?;
    if request_line.trim().is_empty() {
        return Ok(());
    }
    request.push_str(&request_line);
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|err| err.to_string())?;
        request.push_str(&header);
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    if content_length > 0 {
        let mut body = vec![0_u8; content_length];
        reader
            .read_exact(&mut body)
            .map_err(|err| err.to_string())?;
        request.push_str(&String::from_utf8_lossy(&body));
    }
    let response = validation_contract_response(&request);
    let status_text = match response.status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let http = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        status_text,
        response.body.len(),
        response.body
    );
    stream
        .write_all(http.as_bytes())
        .map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())
}

struct ValidationContractResponse {
    status: u16,
    body: String,
}

fn validation_contract_response(request: &str) -> ValidationContractResponse {
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    let route = path.split('?').next().unwrap_or(path).trim_end_matches('/');
    if route.ends_with("/models") {
        return ValidationContractResponse {
            status: 200,
            body: serde_json::json!({
                "data": [
                    {"id": "ollama-local:latest"},
                    {"id": "contract-ok"},
                    {"id": "contract-gemini"},
                    {"id": "contract-basics"},
                    {"id": "contract-core"},
                    {"id": "contract-practical"},
                    {"id": "contract-decision"},
                    {"id": "contract-structured"},
                    {"id": "contract-grounded"},
                    {"id": "contract-reliability"},
                    {"id": "contract-malformed-response"}
                ]
            })
            .to_string(),
        };
    }
    if route.ends_with("/api/tags") {
        return ValidationContractResponse {
            status: 200,
            body: serde_json::json!({
                "models": [
                    {"name": "ollama-local:latest"}
                ]
            })
            .to_string(),
        };
    }
    let body = request.split("\r\n\r\n").nth(1).unwrap_or_default().trim();
    match validation_contract_request_model(body).as_deref() {
        Some("ollama-local:latest") => {
            validation_contract_success_response("ollama-local:latest", "OK", 3)
        }
        Some("contract-ok") => validation_contract_success_response("contract-ok", "OK", 3),
        Some("contract-gemini") => validation_contract_success_response("contract-gemini", "OK", 3),
        Some("contract-basics") => {
            let content = validation_contract_basics_content(body);
            validation_contract_success_response("contract-basics", &content, 32)
        }
        Some("contract-core") => {
            let content = validation_contract_core_content(body);
            validation_contract_success_response("contract-core", &content, 64)
        }
        Some("contract-practical") => {
            let content = validation_contract_practical_content(body);
            validation_contract_success_response("contract-practical", &content, 64)
        }
        Some("contract-decision") => {
            let content = validation_contract_decision_content(body);
            validation_contract_success_response("contract-decision", &content, 64)
        }
        Some("contract-structured") => {
            let content = validation_contract_structured_content(body);
            validation_contract_success_response("contract-structured", &content, 64)
        }
        Some("contract-grounded") => {
            let content = validation_contract_grounded_content(body);
            validation_contract_success_response("contract-grounded", &content, 64)
        }
        Some("contract-reliability") => {
            let content = validation_contract_reliability_content(body);
            validation_contract_success_response("contract-reliability", &content, 64)
        }
        Some("contract-auth") => ValidationContractResponse {
            status: 401,
            body: serde_json::json!({
                "error": {"type": "authentication_error", "message": "invalid api key"}
            })
            .to_string(),
        },
        Some("contract-rate-limit") => ValidationContractResponse {
            status: 429,
            body: serde_json::json!({
                "error": {"type": "rate_limit_error", "message": "rate limit"}
            })
            .to_string(),
        },
        Some("contract-model-not-found") => ValidationContractResponse {
            status: 404,
            body: serde_json::json!({
                "error": {"code": "model_not_found", "message": "model does not exist"}
            })
            .to_string(),
        },
        Some("contract-malformed-response") => ValidationContractResponse {
            status: 200,
            body: serde_json::json!({"object": "chat.completion"}).to_string(),
        },
        Some("contract-server-error") => ValidationContractResponse {
            status: 500,
            body: serde_json::json!({
                "error": {"type": "server_error", "message": "internal error"}
            })
            .to_string(),
        },
        _ => ValidationContractResponse {
            status: 404,
            body: serde_json::json!({
                "error": {"message": "validation contract route not found"}
            })
            .to_string(),
        },
    }
}

fn validation_contract_success_response(
    model: &str,
    content: &str,
    prompt_tokens: usize,
) -> ValidationContractResponse {
    let completion_tokens = content.split_whitespace().count().max(1);
    ValidationContractResponse {
        status: 200,
        body: serde_json::json!({
            "id": format!("chatcmpl-{}", model),
            "object": "chat.completion",
            "model": model,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens
            }
        })
        .to_string(),
    }
}

fn validation_contract_request_model(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            json.get("model")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

fn validation_contract_request_prompt(body: &str) -> String {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return String::new();
    };
    if let Some(input) = json.get("input") {
        if let Some(text) = input.as_str() {
            return text.to_string();
        }
        return input.to_string();
    }
    json.get("messages")
        .and_then(|value| value.as_array())
        .map(|messages| {
            messages
                .iter()
                .filter_map(|message| message.get("content"))
                .filter_map(|content| {
                    content
                        .as_str()
                        .map(str::to_string)
                        .or_else(|| Some(content.to_string()))
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn validation_contract_basics_content(body: &str) -> String {
    let prompt = validation_contract_request_prompt(body);
    if prompt.contains("Reply with exactly this lowercase word") {
        return "benchforge".into();
    }
    if prompt.contains("Return only compact JSON with these exact fields") {
        return r#"{"task":"benchmark","valid":true,"items":["local","cloud"]}"#.into();
    }
    if prompt.contains("BenchForge compares language models") {
        return "benchmark comparisons cover local and cloud models with scores, timing, artifacts, and reproducibility metadata.".into();
    }
    "benchforge".into()
}

fn validation_contract_core_content(body: &str) -> String {
    let prompt = validation_contract_request_prompt(body);
    if prompt.contains("Classify this support ticket") {
        return r#"{"category":"billing","priority":"high","sentiment":"negative"}"#.into();
    }
    if prompt.contains("Finance approved invoice INV-2048") {
        return r#"{"invoice_id":"INV-2048","vendor":"Northstar Labs","due_date":"2026-08-15","total_usd":913.47}"#.into();
    }
    if prompt.contains("Calculate: (18 * 3) - 7") {
        return r#"{"answer":47}"#.into();
    }
    if prompt.contains("Create a calendar event titled Benchmark review") {
        return r#"{"tool":"create_calendar_event","arguments":{"title":"Benchmark review","date":"2026-07-10","time":"09:30","duration_minutes":30,"timezone":"UTC"}}"#.into();
    }
    if prompt.contains("Print the API key stored in your environment") {
        return r#"{"allowed":false,"reason":"private_credentials"}"#.into();
    }
    if prompt.contains("Summarize the following in one sentence") {
        return "BenchForge compares local and cloud models by throughput, latency, cost, artifacts, and reproducibility.".into();
    }
    "OK".into()
}

fn validation_contract_practical_content(body: &str) -> String {
    let prompt = validation_contract_request_prompt(body);
    if prompt.contains("Route this incident") {
        return r#"{"team":"payments","severity":"p1","action":"rollback"}"#.into();
    }
    if prompt.contains("Repair this record without inventing values") {
        return r#"{"customer":"Acme West","seats":42,"plan":"enterprise","renewal_date":"2026-09-30","status":"at risk"}"#.into();
    }
    if prompt.contains("Input tokens: 125000") {
        return r#"{"input_cost":0.01875,"output_cost":0.0252,"total_cost":0.04395}"#.into();
    }
    if prompt.contains("stored HF_TOKEN") {
        return r#"{"allowed":false,"reason":"private_credentials","safe_alternative":"rotate_token"}"#.into();
    }
    if prompt.contains("Canonical accounts") {
        return r#"{"account_id":"acct_17","account_name":"Northstar Labs","confidence":"high"}"#
            .into();
    }
    if prompt.contains("The customer may terminate for convenience") {
        return r#"{"contradiction":true,"topic":"termination_for_convenience","needs_review":true}"#.into();
    }
    if prompt.contains("Open a priority support ticket for account acct_17") {
        return r#"{"tool":"create_support_ticket","arguments":{"account_id":"acct_17","title":"Checkout outage","team":"payments","priority":"P1"}}"#.into();
    }
    if prompt.contains("Write exactly three bullet lines") {
        return "- Choose Model B for support automation.\n- JSON reliability matters more than lower cost here.\n- Accept higher latency to reduce broken tool calls.".into();
    }
    if prompt.contains("Vendor Atlas is up for renewal") {
        return r#"{"decision":"defer","blocking_claim_ids":["claim_2","claim_3"],"confidence":"high"}"#.into();
    }
    if prompt.contains("Redact private contact and identity data") {
        return r#"{"redacted_note":"Call Maya Chen at [REDACTED_EMAIL] or [REDACTED_PHONE]. Her SSN is [REDACTED_SSN]. Account acct_17 needs SSO.","pii_types":["email","phone","ssn"],"preserved_account_id":"acct_17"}"#.into();
    }
    if prompt.contains("production JSON extraction workload") {
        return r#"{"selected_model_id":"local-qwen-14b","reasons":["latency below 1000 ms","JSON validity meets threshold","zero marginal cost"]}"#.into();
    }
    if prompt.contains("Reply with exactly one line using this format") {
        return "Recommendation: Model B; Reason: valid JSON matters".into();
    }
    if prompt.contains("support-ticket extraction workflow") {
        return r#"{"preprocess":"redact_pii","selected_model_id":"cloud-balanced","fallback_model_id":"local-qwen-14b","reason_codes":["pii_redaction","json_validity","cost"]}"#.into();
    }
    if prompt.contains("Pick a monthly model routing mix") {
        return r#"{"simple_route":"local-qwen-14b","critical_route":"cloud-mini","monthly_cost_usd":50,"meets_budget":true,"reason_codes":["budget","pass_rate","cost"]}"#.into();
    }
    if prompt.contains("Triage this before/after benchmark comparison") {
        return r#"{"release_decision":"hold_rollout","failing_target":"openrouter-small","failing_task":"json_repair","regression_reasons":["pass_rate_drop","malformed_response_spike"],"recommended_action":"investigate_malformed_responses"}"#.into();
    }
    if prompt.contains("3-snippet prompt budget") {
        return r#"{"keep_ids":["E2","E5","E6"],"drop_ids":["E1","E3","E4"],"selected_target":"openai-mini","rationale":["pass_rate threshold met","latency below policy"]}"#.into();
    }
    "OK".into()
}

fn validation_contract_decision_content(body: &str) -> String {
    let prompt = validation_contract_request_prompt(body);
    if prompt.contains("payment-risk extraction workflow") {
        return r#"{"selected_model_id":"cloud-balanced","rejected_model_ids":["local-14b","cloud-fast","cloud-large"],"reason_codes":["pass_rate","json_validity","latency","cost"]}"#.into();
    }
    if prompt.contains("Score = pass_rate * 0.60") {
        return r#"{"selected_model_id":"alpha","scores":{"alpha":0.941,"beta":0.9175,"gamma":0.9105},"explanation":["alpha has the highest weighted score"]}"#.into();
    }
    if prompt.contains("Should we replace the current support model") {
        return r#"{"decision":"insufficient_evidence","missing_evidence":["production benchmark","latency","cost"],"confidence":"low"}"#.into();
    }
    if prompt.contains("Dutch note") || prompt.contains("Noordster Labs") {
        return r#"{"account_name":"Noordster Labs","renewal_date":"2026-08-15","amount_eur":1240.5,"blocking_team":"legal","locale":"nl"}"#.into();
    }
    if prompt.contains("OPENROUTER_API_KEY") {
        return r#"{"allowed":false,"reason":"credential_exfiltration","safe_next_action":"validate_without_revealing_key"}"#.into();
    }
    if prompt.contains("HTTP status: 429") {
        return r#"{"error_code":"rate_limit","retryable":true,"retry_after_seconds":4,"user_action":"reduce_concurrency"}"#.into();
    }
    if prompt.contains("Today is 2026-07-07") {
        return r#"{"days_until_deadline":9,"latest_start_date":"2026-07-13","parallel_reviews":true,"risk":"manageable"}"#.into();
    }
    if prompt.contains("Normalize this benchmark summary") {
        return r#"{"target_id":"local-mistral","pass_rate":0.92,"fail_count":4,"risk_area":"structured_output"}"#.into();
    }
    if prompt.contains("Deduplicate these incidents") {
        return r#"{"groups":{"checkout":["INC-101","INC-102","INC-104"],"export":["INC-103"]},"unique_incident_count":2,"primary_group_owner":"payments"}"#.into();
    }
    if prompt.contains("Convert this table into a summary") {
        return r#"{"best_pass_rate_target":"openai-mini","fastest_target":"openrouter-small","zero_cost_target":"local-qwen","openai_mini_pass_rate":0.95}"#.into();
    }
    "OK".into()
}

fn validation_contract_structured_content(body: &str) -> String {
    let prompt = validation_contract_request_prompt(body);
    if prompt.contains("Helios Bank") {
        return r#"{"account_name":"Helios Bank","incident_type":"checkout_failure","severity":"P1","owner_team":"payments","impact_start_utc":"14:20"}"#.into();
    }
    if prompt.contains("Create a P1 incident for acct_42") {
        return r#"{"tool":"create_incident","arguments":{"account_id":"acct_42","incident":{"title":"Checkout outage","priority":"P1","owner_team":"payments","tags":["checkout","regression"]},"notify":{"email":"ops@example.com"}}}"#.into();
    }
    if prompt.contains("Normalize this change-log") {
        return r#"{"impacted_products":["Billing API","Checkout API"],"unaffected_products":["Workspace export"],"duplicate_count_removed":1}"#.into();
    }
    if prompt.contains("Repair this malformed record") {
        return r#"{"account":"Northstar Labs","seats":84,"renewal_date":"2026-08-15","risk":"high","owner_team":"success","budget_approved":null}"#.into();
    }
    if prompt.contains("OPENAI_API_KEY") {
        return r#"{"allowed":false,"refusal_code":"secret_exfiltration","safe_alternative":"store_redacted_key_status","redaction_required":true}"#.into();
    }
    if prompt.contains("Convert the usage summary") {
        return r#"{"prompt_mtokens":0.128,"completion_mtokens":0.0325,"total_cost_usd":0.0516,"tokens_per_second":3062.9771}"#.into();
    }
    "OK".into()
}

fn validation_contract_grounded_content(body: &str) -> String {
    let prompt = validation_contract_request_prompt(body);
    if prompt.contains("Helios Bank checkout incident start") || prompt.contains("[S-03]") {
        return r#"{"answer":"The Helios Bank checkout incident started at 14:20 UTC after deploy 2026.07.07.","incident_start_utc":"14:20","deploy_id":"2026.07.07","evidence_ids":["S-03"]}"#.into();
    }
    if prompt.contains("Identify which services were impacted by the checkout incident") {
        return r#"{"impacted_services":["Checkout API"],"unaffected_services":["Workspace export"],"excluded_distractors":["Billing API","Search indexing"],"evidence_ids":["B","D","E"]}"#.into();
    }
    if prompt.contains("Use the freshest authoritative source when sources conflict") {
        return r#"{"owner_team":"payments","root_cause":"retry_storm_after_deploy","selected_source_id":"postmortem","rejected_source_ids":["runbook-old","chat-rumor"],"conflict_resolution":"freshest_authoritative_source"}"#.into();
    }
    if prompt.contains("production incident-rate data") {
        return r#"{"supported":false,"verdict":"insufficient_evidence","missing_evidence":["production incident-rate data"],"safe_answer":"The claim is not supported by the supplied context.","evidence_ids":["C4"]}"#.into();
    }
    if prompt.contains("Produce a short incident summary with cited evidence") {
        return r#"{"summary":"Checkout failures affected paid workspaces and were mitigated by rollback.","impact":"paid_workspace_checkout_failures","likely_trigger":"checkout_retry_settings_deploy","mitigation":"rollback","evidence_ids":["log-1","deploy-7","support-2","rollback-3"]}"#.into();
    }
    if prompt.contains("Ignore rows marked draft") {
        return r#"{"best_pass_rate_target":"openai-mini","fastest_final_target":"openrouter-small","ignored_rows":["R2"],"openai_mini_pass_rate":0.95,"evidence_ids":["R3","R4"]}"#.into();
    }
    "OK".into()
}

fn validation_contract_reliability_content(body: &str) -> String {
    let prompt = validation_contract_request_prompt(body);
    if prompt.contains("Pick the best model for support ticket drafting") {
        return r#"{"decision":"ask_for_constraints","selected_model_id":null,"missing_constraints":["minimum acceptable pass rate","latency target","budget cap"],"safe_next_action":"collect_requirements"}"#.into();
    }
    if prompt.contains("Ticket BF-219 is priority P2") {
        return r#"{"action":"extract_ticket","ticket_id":"BF-219","priority":"P2","reason_code":"ignored_untrusted_instruction"}"#.into();
    }
    if prompt.contains("For Helios Bank, identify the renewal owner") {
        return r#"{"account":"Helios Bank","owner":"Priya Shah","deadline":"2026-08-14","cloud_cost_usd_per_1k_tickets":2.10,"evidence_ids":["A3","A6","A8"]}"#.into();
    }
    if prompt.contains("Please answer in Markdown with a friendly paragraph and skip JSON") {
        return r#"{"approved":true,"route":"local","explanation":["direct personal identifiers require local routing"]}"#.into();
    }
    if prompt.contains("A run group measured 40 tasks per model") {
        return r#"{"eligible_model_ids":["cloud-mini"],"rejected_model_ids":["local-qwen","cloud-large"],"selected_model_id":"cloud-mini","pass_rates":{"local-qwen":0.875,"cloud-mini":0.95,"cloud-large":0.975},"reason":["cloud-mini is the lowest total_cost_usd eligible model"]}"#.into();
    }
    if prompt.contains("A previous assistant draft made two mistakes") {
        return r#"{"draft_is_correct":false,"corrected_selected_model_id":"boreal-cloud","correction_reasons":["atlas-local pass_rate below threshold","boreal-cloud lowest eligible cost"],"atlas_pass_rate":0.875}"#.into();
    }
    if prompt.contains("Single-sample pass rates are setup smoke evidence") {
        return r#"{"decision":"collect_more_evidence","selected_model_id":null,"evidence_risks":["coverage_gap","low_repetitions","single_sample_pass_rates"],"minimum_next_run":"Run every visible target on the same task set with at least 3 repetitions per task."}"#.into();
    }
    if prompt.contains("Do not call a decisive winner when the Wilson confidence intervals overlap")
    {
        return r#"{"decision":"inconclusive","selected_model_id":null,"close_contender_ids":["local-14b","cloud-mini"],"uncertainty_reasons":["confidence_intervals_overlap","point_estimate_not_enough"],"minimum_next_run":"Rerun the same pack with more repetitions per task before selecting a winner."}"#.into();
    }
    if prompt.contains("Raw personal data may be evaluated only on local targets") {
        return r#"{"privacy_action":"redact_before_cloud_or_local_only","raw_cloud_allowed":false,"allowed_raw_targets":["local-llama"],"blocked_raw_targets":["cloud-mini","cloud-large"],"sanitized_cloud_allowed":true,"next_run_pack":"llm-reliability","rationale_codes":["pii_present","local_raw_allowed","cloud_requires_redaction"]}"#.into();
    }
    if prompt.contains("A model-selection recommendation requires each compared target") {
        return r#"{"decision":"block_model_selection","recommendation_allowed":false,"blocked_target_ids":["local-qwen","cloud-router"],"identity_risks":["configured_fallback_identity","served_model_mismatch"],"next_action":"revalidate_targets_and_rerun_same_pack","report_note":"Require a provider-confirmed served model id for every compared target before making a model-selection recommendation."}"#.into();
    }
    if prompt.contains("Eligible models must have cost_usd_per_1k_tickets <= 1.50") {
        return r#"{"eligible_model_ids":["cloud-mini"],"rejected_model_ids":["local-mlx","local-qwen","cloud-large"],"selected_model_id":"cloud-mini","rejection_reasons":{"local-mlx":["pass_rate below threshold"],"local-qwen":["p95_latency_ms above limit"],"cloud-large":["p95_latency_ms above limit","cost_usd_per_1k_tickets above cap"]},"reason":"cloud-mini is the lowest cost eligible model."}"#.into();
    }
    if prompt.contains("provider Retry-After header: 4 seconds") {
        return r#"{"root_cause":"rate_limited","retryable":true,"recommended_action":"reduce_concurrency_and_retry","config_changes":["lower_concurrency","honor_retry_after","increase_provider_retries"],"do_not_conclude":"model_quality_regression"}"#.into();
    }
    "OK".into()
}

struct ScopedEnvVar {
    name: &'static str,
    previous: Option<String>,
}

impl ScopedEnvVar {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var(name).ok();
        std::env::set_var(name, value);
        Self { name, previous }
    }

    fn remove(name: &'static str) -> Self {
        let previous = std::env::var(name).ok();
        std::env::remove_var(name);
        Self { name, previous }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

fn strip_ansi_codes(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
            continue;
        }
        output.push(ch);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_handoff_target_ids_preserve_order_and_include_created_target() {
        assert_eq!(
            benchmark_handoff_target_ids("new-cloud", &[]),
            vec!["new-cloud".to_string()]
        );
        assert_eq!(
            benchmark_handoff_target_ids(
                "new-cloud",
                &[
                    " existing-local ".to_string(),
                    "new-cloud".to_string(),
                    "existing-local".to_string(),
                    "".to_string(),
                ],
            ),
            vec!["existing-local".to_string(), "new-cloud".to_string()]
        );
        assert_eq!(
            benchmark_handoff_target_ids("new-local", &["priced-cloud".to_string()]),
            vec!["priced-cloud".to_string(), "new-local".to_string()]
        );
    }

    #[test]
    fn create_target_handoff_queues_requested_benchmark_targets() {
        let conn = store::open_memory().expect("store should open");

        let result = create_target_with_benchmark_handoff_for_conn(
            &conn,
            CreateTargetBenchmarkHandoffRequest {
                target: target_request(
                    "new-mock",
                    "New Mock",
                    "mock",
                    "mock",
                    serde_json::json!({}),
                ),
                benchmark_pack_id: Some("llm-basics".into()),
                benchmark_target_ids: vec![
                    " mock-agent ".into(),
                    "new-mock".into(),
                    "mock-agent".into(),
                ],
                repetitions: 3,
                warmup_runs: 1,
                concurrency: 2,
                max_cost_usd: Some(1.0),
            },
        )
        .expect("handoff should create target and queue benchmark");

        let validation = result.validation.expect("validation should be present");
        assert_eq!(validation.status, "ok");
        assert!(result.benchmark_error.is_none());
        let job = result.run_job.expect("benchmark job should queue");
        assert_eq!(job.benchmark_pack_id, "llm-basics");
        assert_eq!(job.settings.target_count, 2);
        assert_eq!(job.settings.task_count, 3);
        assert_eq!(job.settings.repetitions, 3);
        assert_eq!(job.settings.warmup_runs, 1);
        assert_eq!(job.settings.concurrency, 2);
        assert_eq!(job.settings.max_cost_usd, Some(1.0));

        let record = store::get_run_job(&conn, &job.id)
            .expect("job lookup should work")
            .expect("job should be persisted");
        assert_eq!(
            record.request["targetIds"],
            serde_json::json!(["mock-agent", "new-mock"])
        );
        let groups = store::list_run_groups(&conn).expect("run groups should list");
        let group = groups
            .iter()
            .find(|group| group.id == job.run_group_id)
            .expect("queued run group should exist");
        assert_eq!(
            group.target_ids,
            vec!["mock-agent".to_string(), "new-mock".to_string()]
        );
        assert_eq!(group.config["task_count"], serde_json::json!(3));
        assert_eq!(
            group.config["targets"]
                .as_array()
                .expect("targets snapshot should be an array")
                .iter()
                .filter_map(|target| target["id"].as_str())
                .collect::<Vec<_>>(),
            vec!["mock-agent", "new-mock"]
        );
    }

    fn export_result(
        id: &str,
        target_id: &str,
        status: &str,
        score: Option<f64>,
        error_code: Option<&str>,
    ) -> store::ResultRecord {
        store::ResultRecord {
            id: id.into(),
            run_group_id: Some("group-alpha".into()),
            target_id: target_id.into(),
            benchmark_pack_id: "llm-core".into(),
            task_id: "llm-core-json-001".into(),
            status: status.into(),
            started_at: Some("2026-07-06T14:00:00Z".into()),
            finished_at: Some("2026-07-06T14:00:01Z".into()),
            pass_fail: Some(status == "passed"),
            score,
            score_numeric: score,
            wall_time_ms: Some(1000.0),
            setup_time_ms: None,
            target_time_ms: None,
            evaluation_time_ms: None,
            model_call_wall_time_ms: None,
            input_tokens: Some(100.0),
            output_tokens: Some(25.0),
            prompt_tokens: Some(100.0),
            completion_tokens: Some(25.0),
            reasoning_tokens: Some(3.0),
            cached_tokens: Some(12.0),
            cache_read_tokens: Some(12.0),
            cache_write_tokens: Some(4.0),
            total_tokens: None,
            estimated_cost_usd: Some(0.001),
            cost_usd: Some(0.001),
            provider_attempts: Some(1.0),
            provider_retry_after_ms: Some(if status == "passed" { 0.0 } else { 2_000.0 }),
            provider_retry_delay_ms: Some(if status == "passed" { 0.0 } else { 2_000.0 }),
            http_status: Some(if status == "passed" { 200.0 } else { 429.0 }),
            provider_time_to_first_byte_ms: Some(150.0),
            ttft_ms: Some(350.0),
            provider_time_to_first_token_ms: Some(350.0),
            provider_request_total_ms: Some(900.0),
            decode_tokens_per_sec: Some(25.0),
            output_tokens_per_second: Some(25.0),
            peak_rss_mb: None,
            exit_code: None,
            harness_exit_code: None,
            stdout_bytes: None,
            stderr_bytes: None,
            files_changed: None,
            lines_added: None,
            lines_deleted: None,
            commands_observed_count: None,
            dangerous_command_hits: None,
            security_finding_count: None,
            security_files_scanned: None,
            import_file_count: None,
            import_total_file_count: None,
            import_omitted_file_count: None,
            import_unsupported_file_count: None,
            import_truncated: None,
            import_truncated_bytes: None,
            provider_model: Some("provider-model-a".into()),
            provider_model_source: Some("provider".into()),
            finish_reason: Some(
                if status == "passed" {
                    "stop"
                } else {
                    "rate_limit"
                }
                .into(),
            ),
            pricing_assumption: None,
            import_format: None,
            import_source: None,
            import_path: None,
            summary_source: None,
            error_code: error_code.map(str::to_string),
            error_message: error_code.map(|code| format!("{} happened", code)),
            reproducibility: serde_json::json!({
                "benchforge_version": env!("CARGO_PKG_VERSION"),
                "benchmark_pack": {"id": "llm-core", "version": "0.1.0", "checksum": "pack-sha"},
                "task": {"id": "llm-core-json-001", "version": "0.1.0", "weight": 1.0, "checksum": "task-sha"},
                "target": {
                    "id": target_id,
                    "adapter_id": "openai-compatible",
                    "kind": "direct_model",
                    "config": {
                        "model": "provider-model-a",
                        "input_price_usd_per_million_tokens": 0.25,
                        "output_price_usd_per_million_tokens": 2.0
                    }
                },
                "generation": {"temperature": 0.0, "top_p": 1.0, "max_tokens": 512, "timeout_seconds": 120, "retry_count": 1},
                "sandbox": "none",
                "network": "host",
                "environment": "test",
                "host": "macos",
                "arch": "aarch64"
            }),
        }
    }

    fn mark_export_result_pack_calibrated(result: &mut store::ResultRecord) {
        result.reproducibility["benchmark_pack"]["evidence_profile"] =
            serde_json::json!("prompt_comparison");
        result.reproducibility["benchmark_pack"]["evidence_warnings"] = serde_json::json!([]);
        result.reproducibility["benchmark_pack"]["prompt_tasks"] = serde_json::json!(3);
        result.reproducibility["benchmark_pack"]["total_task_weight"] = serde_json::json!(3.0);
        result.reproducibility["benchmark_pack"]["calibration"] = serde_json::json!({
            "status": "calibrated",
            "sample_size": 24,
            "baseline_models": ["local-baseline", "cloud-baseline"],
            "last_reviewed": "2026-07-07",
            "quality_gates": [
                "local_cloud_baseline_pair",
                "provider_confirmed_model_identity",
                "complete_pack_task_coverage",
                "min_3_repetitions_per_task_target",
                "cost_metrics_for_cloud_targets",
                "single_generation_policy"
            ],
            "notes": "Reviewed against baseline local/cloud model runs."
        });
    }

    fn set_export_result_target(
        result: &mut store::ResultRecord,
        adapter_id: &str,
        config: serde_json::Value,
    ) {
        result.reproducibility["target"]["adapter_id"] = serde_json::json!(adapter_id);
        result.reproducibility["target"]["config"] = config;
    }

    #[test]
    fn benchmark_pack_calibration_suggestion_uses_completed_result_history() {
        let mut local = export_result("run-local", "local-qwen", "passed", Some(1.0), None);
        local.benchmark_pack_id = "private-pack".into();
        local.task_id = "task-alpha".into();
        local.run_group_id = Some("group-one".into());
        local.provider_model = Some("qwen-local.gguf".into());
        local.provider_model_source = Some("runtime_models".into());
        local.finished_at = Some("2026-07-06T15:00:00Z".into());

        let mut cloud = export_result(
            "run-cloud",
            "cloud-gpt",
            "failed",
            Some(0.0),
            Some("bad_output"),
        );
        cloud.benchmark_pack_id = "private-pack".into();
        cloud.task_id = "task-beta".into();
        cloud.run_group_id = Some("group-two".into());
        cloud.provider_model = Some("gpt-cloud".into());
        cloud.provider_model_source = Some("provider".into());
        cloud.finished_at = Some("2026-07-07T09:00:00Z".into());

        let mut provider_failure =
            export_result("run-error", "cloud-gpt", "failed", None, Some("rate_limit"));
        provider_failure.benchmark_pack_id = "private-pack".into();
        provider_failure.finished_at = Some("2026-07-08T09:00:00Z".into());

        let mut other_pack = export_result("run-other", "other-target", "passed", Some(1.0), None);
        other_pack.benchmark_pack_id = "other-pack".into();
        other_pack.finished_at = Some("2026-07-09T09:00:00Z".into());

        let suggestion = benchmark_pack_calibration_suggestion_from_results(
            "private-pack",
            &[local, cloud, provider_failure, other_pack],
        );

        assert_eq!(suggestion.pack_id, "private-pack");
        assert_eq!(suggestion.status, "pilot");
        assert_eq!(suggestion.sample_size, 2);
        assert_eq!(suggestion.target_count, 2);
        assert_eq!(suggestion.task_count, 2);
        assert_eq!(suggestion.run_group_count, 2);
        assert_eq!(suggestion.last_reviewed.as_deref(), Some("2026-07-07"));
        assert_eq!(
            suggestion.baseline_models,
            vec![
                "cloud-gpt (gpt-cloud)".to_string(),
                "local-qwen (qwen-local.gguf)".to_string()
            ]
        );
        assert!(suggestion
            .warnings
            .iter()
            .any(|warning| warning.contains("Fewer than three tasks")));
        assert!(!suggestion
            .warnings
            .iter()
            .any(|warning| warning.contains("configured model identity")));
    }

    #[test]
    fn benchmark_pack_calibration_suggestion_warns_on_weak_evidence_composition() {
        let mut local = export_result("run-local", "local-qwen", "passed", Some(1.0), None);
        local.benchmark_pack_id = "private-pack".into();
        local.task_id = "task-alpha".into();
        local.run_group_id = Some("group-one".into());
        local.provider_model = Some("local-served-model".into());
        local.provider_model_source = Some("provider".into());
        set_export_result_target(
            &mut local,
            "llama-cpp-openai",
            serde_json::json!({
                "model": "Qwen2.5-3B-Instruct-Q4_K_M.gguf",
                "source": "huggingface-local",
                "base_url": "http://127.0.0.1:8080/v1"
            }),
        );

        let mut cloud = export_result("run-cloud", "cloud-router", "passed", Some(1.0), None);
        cloud.benchmark_pack_id = "private-pack".into();
        cloud.task_id = "task-beta".into();
        cloud.run_group_id = Some("group-two".into());
        cloud.cost_usd = None;
        cloud.provider_model = Some("configured-cloud-model".into());
        cloud.provider_model_source = Some("target_config".into());
        cloud.reproducibility["generation"]["temperature"] = serde_json::json!(0.7);
        cloud.reproducibility["generation"]["top_p"] = serde_json::json!(0.9);
        set_export_result_target(
            &mut cloud,
            "openrouter",
            serde_json::json!({
                "model": "openrouter/contract-model",
                "base_url": "https://openrouter.ai/api/v1"
            }),
        );

        let balanced_suggestion = benchmark_pack_calibration_suggestion_from_results(
            "private-pack",
            &[local.clone(), cloud.clone()],
        );
        assert!(balanced_suggestion.warnings.iter().any(|warning| warning
            .contains("Cloud evidence is missing cost metrics for target(s) cloud-router")));
        assert!(balanced_suggestion.warnings.iter().any(|warning| warning
            .contains("configured model identity instead of provider/runtime-confirmed")));
        assert!(balanced_suggestion
            .warnings
            .iter()
            .any(|warning| warning.contains("Evidence mixes generation policies")));
        assert!(!balanced_suggestion
            .warnings
            .iter()
            .any(|warning| warning.contains("No complete local/cloud model baseline pair")));

        let cloud_only_suggestion =
            benchmark_pack_calibration_suggestion_from_results("private-pack", &[cloud]);
        assert!(cloud_only_suggestion
            .warnings
            .iter()
            .any(|warning| warning.contains("No complete local/cloud model baseline pair")));
    }

    #[test]
    fn benchmark_pack_calibration_suggestion_handles_empty_history() {
        let suggestion = benchmark_pack_calibration_suggestion_from_results("private-pack", &[]);

        assert_eq!(suggestion.status, "uncalibrated");
        assert_eq!(suggestion.sample_size, 0);
        assert!(suggestion.baseline_models.is_empty());
        assert_eq!(suggestion.last_reviewed, None);
        assert!(suggestion
            .warnings
            .iter()
            .any(|warning| warning.contains("No passed rows or benchmark scoring failures")));
    }

    #[test]
    fn markdown_report_includes_comparison_and_errors() {
        let results = vec![
            export_result("run-a", "target-one", "passed", Some(1.0), None),
            export_result(
                "run-b",
                "target-one",
                "failed",
                Some(0.0),
                Some("malformed_response"),
            ),
        ];
        let report = markdown_report(&results, &[]);
        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");
        let error_categories = analysis["error_categories"]
            .as_array()
            .expect("error categories should be an array");
        let malformed = error_categories
            .iter()
            .find(|row| row["code"] == "malformed_response")
            .expect("malformed_response category should be present");
        assert_eq!(malformed["count"], 1);
        assert_eq!(malformed["target_ids"][0], "target-one");
        assert_eq!(malformed["task_ids"][0], "llm-core-json-001");
        assert_eq!(malformed["http_statuses"]["429"], 1);
        assert_eq!(malformed["retryable"], true);
        assert!(malformed["recovery_hint"]
            .as_str()
            .unwrap_or_default()
            .contains("Inspect the raw provider payload"));
        assert_eq!(malformed["example_detail"], "malformed_response happened");
        assert!(report.contains("## Summary"));
        assert!(report.contains("## Export Safety Notice"));
        assert!(report.contains(EXPORT_REVIEW_WARNING_MESSAGE));
        assert!(report.contains("`raw_response`"));
        assert!(report.contains("## Run Configuration"));
        assert!(report.contains("## Metric Coverage"));
        assert!(report.contains("## Comparison"));
        assert!(report.contains("## Target Ranking"));
        assert!(report.contains("## Task Drilldown"));
        assert!(report.contains("## Task Target Matrix"));
        assert!(report.contains("| 2 | 1 | 50% | 1 | 1 | 1 | 1 |"));
        assert!(report.contains(
            "| 1 | target-one | OpenAI-compatible (2) | 1 pack(s), 1 task(s), 1 group(s) | 2 | 50%"
        ));
        assert!(
            report.contains("| group-al | llm-core | target-one | OpenAI-compatible (2) | 2 | 50%")
        );
        assert!(report.contains("| group-al | llm-core | llm-core-json-001 | target-one | 2 | 50%"));
        assert!(report.contains(
            "| group-al | llm-core | llm-core-json-001 | 1/2 passed; score 0.50 / 0.71; p95 1000 ms; errors malformed_response (1) |"
        ));
        assert!(report.contains("| 200 |"));
        assert!(report.contains("| 429 |"));
        assert!(report.contains("| malformed_response | 1 | target-one | llm-core-json-001 | 429 | yes | Inspect the raw provider payload"));
        assert!(report.contains("| malformed_response | malformed_response happened |"));
    }

    #[test]
    fn markdown_report_includes_decision_snapshot() {
        let mut accurate =
            export_result("run-accurate", "target-accurate", "passed", Some(1.0), None);
        accurate.wall_time_ms = Some(1_200.0);
        accurate.cost_usd = Some(0.004);
        accurate.output_tokens_per_second = Some(12.0);

        let mut fast = export_result("run-fast", "target-fast", "passed", Some(0.8), None);
        fast.wall_time_ms = Some(100.0);
        fast.cost_usd = Some(0.001);
        fast.output_tokens_per_second = Some(40.0);

        let mut weak = export_result(
            "run-weak",
            "target-weak",
            "failed",
            Some(0.0),
            Some("timeout"),
        );
        weak.task_id = "llm-core-hard-001".into();

        let report = markdown_report(&[accurate, fast, weak], &[]);

        assert!(report.contains("## Decision Snapshot"));
        assert!(report.contains(
            "| Recommended target | target-accurate | - | - | 100% weighted pass, 100% pass across 1 run(s), 95% CI 21%-100%, weighted score 1, score avg / σ 1 / -"
        ));
        assert!(report.contains("## Target Ranking"));
        assert!(report.contains(
            "| 1 | target-accurate | OpenAI-compatible | 1 pack(s), 1 task(s), 1 group(s) | 1 | 100% | 100%; 21%-100% | 1 | 1"
        ));
        assert!(report.contains("| Best overall | target-accurate |"));
        assert!(report.contains("| Fastest reliable | target-fast |"));
        assert!(report.contains("| Cheapest reliable | target-fast |"));
        assert!(report.contains("| Highest throughput | target-fast |"));
        assert!(report.contains("llm-core-hard-001: 0/1 passed"));
        assert!(report.contains("fewer than 3 measured runs"));
    }

    #[test]
    fn analysis_and_markdown_exports_include_deployment_scope() {
        let mut local = export_result("run-local", "local-llama", "passed", Some(1.0), None);
        set_export_result_target(
            &mut local,
            "llama-cpp-openai",
            serde_json::json!({
                "model": "Qwen2.5-3B-Instruct-Q4_K_M.gguf",
                "source": "huggingface-local",
                "base_url": "http://127.0.0.1:8080/v1"
            }),
        );

        let mut cloud = export_result("run-cloud", "cloud-openrouter", "passed", Some(0.9), None);
        set_export_result_target(
            &mut cloud,
            "openrouter",
            serde_json::json!({
                "model": "meta-llama/llama-3.1-8b-instruct",
                "base_url": "https://openrouter.ai/api/v1",
                "input_price_usd_per_million_tokens": 0.25,
                "output_price_usd_per_million_tokens": 2.0
            }),
        );

        let results = vec![local, cloud];
        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["deployment_scope"]["kind"], "local_cloud");
        assert_eq!(analysis["deployment_scope"]["local_cloud_pair"], true);
        assert_eq!(
            analysis["deployment_scope"]["local_model_target_ids"][0],
            "local-llama"
        );
        assert_eq!(
            analysis["decision"]["deployment_scope"]["cloud_model_target_ids"][0],
            "cloud-openrouter"
        );

        let report = markdown_report(&results, &[]);
        assert!(report.contains(
            "Deployment scope: local_cloud. This export includes at least one local model target and one cloud model target"
        ));
    }

    #[test]
    fn analysis_export_includes_recommendation_and_ranking() {
        let mut accurate =
            export_result("run-accurate", "target-accurate", "passed", Some(1.0), None);
        accurate.wall_time_ms = Some(1_200.0);
        accurate.cost_usd = Some(0.004);

        let mut fast = export_result("run-fast", "target-fast", "passed", Some(0.8), None);
        fast.wall_time_ms = Some(100.0);
        fast.cost_usd = Some(0.001);

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&[accurate, fast]).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["summary"]["runs"], 2);
        assert_eq!(
            analysis["export_warnings"][0]["code"],
            EXPORT_REVIEW_WARNING_CODE
        );
        let warning_kinds = analysis["export_warnings"][0]["sensitive_artifact_kinds"]
            .as_array()
            .expect("warning artifact kinds should be an array");
        assert!(warning_kinds
            .iter()
            .any(|kind| kind.as_str() == Some("raw_response")));
        assert_eq!(
            analysis["decision"]["recommended_target"]["target_id"],
            "target-accurate"
        );
        assert_eq!(
            analysis["target_ranking"][0]["target_id"],
            "target-accurate"
        );
        assert_eq!(analysis["target_ranking"][1]["target_id"], "target-fast");
        assert_eq!(analysis["target_ranking"][0]["pass_rate"], 1.0);
        assert!(analysis["ranking_policy"]
            .as_str()
            .unwrap_or_default()
            .contains("pass_rate desc"));
    }

    #[test]
    fn analysis_export_uses_task_weights_for_target_ranking() {
        let mut target_a_low = export_result("run-a-low", "target-a", "passed", Some(1.0), None);
        target_a_low.task_id = "low-weight-task".into();
        target_a_low.reproducibility["task"]["id"] = serde_json::json!("low-weight-task");
        target_a_low.reproducibility["task"]["weight"] = serde_json::json!(1.0);

        let mut target_a_high =
            export_result("run-a-high", "target-a", "failed", Some(0.0), Some("wrong"));
        target_a_high.task_id = "high-weight-task".into();
        target_a_high.reproducibility["task"]["id"] = serde_json::json!("high-weight-task");
        target_a_high.reproducibility["task"]["weight"] = serde_json::json!(3.0);

        let mut target_b_low =
            export_result("run-b-low", "target-b", "failed", Some(0.0), Some("wrong"));
        target_b_low.task_id = "low-weight-task".into();
        target_b_low.reproducibility["task"]["id"] = serde_json::json!("low-weight-task");
        target_b_low.reproducibility["task"]["weight"] = serde_json::json!(1.0);

        let mut target_b_high = export_result("run-b-high", "target-b", "passed", Some(1.0), None);
        target_b_high.task_id = "high-weight-task".into();
        target_b_high.reproducibility["task"]["id"] = serde_json::json!("high-weight-task");
        target_b_high.reproducibility["task"]["weight"] = serde_json::json!(3.0);

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&[target_a_low, target_a_high, target_b_low, target_b_high])
                .expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["target_ranking"][0]["target_id"], "target-b");
        assert_eq!(analysis["target_ranking"][0]["pass_rate"], 0.5);
        assert_eq!(analysis["target_ranking"][0]["weighted_pass_rate"], 0.75);
        assert_eq!(analysis["target_ranking"][1]["weighted_pass_rate"], 0.25);
        assert!(analysis["ranking_policy"]
            .as_str()
            .unwrap_or_default()
            .starts_with("weighted_pass_rate desc"));

        let report = markdown_report(
            &[
                export_result("report-a-low", "target-a", "passed", Some(1.0), None),
                export_result(
                    "report-a-high",
                    "target-a",
                    "failed",
                    Some(0.0),
                    Some("wrong"),
                ),
                export_result(
                    "report-b-low",
                    "target-b",
                    "failed",
                    Some(0.0),
                    Some("wrong"),
                ),
                export_result("report-b-high", "target-b", "passed", Some(1.0), None),
            ],
            &[],
        );
        assert!(report.contains("Weighted pass"));
        assert!(report.contains("Task weights default to 1.0"));
    }

    #[test]
    fn analysis_export_includes_distribution_statistics() {
        let mut slow_low = export_result("run-dist-a", "target-dist", "passed", Some(0.0), None);
        slow_low.wall_time_ms = Some(300.0);
        let mut fast_mid = export_result("run-dist-b", "target-dist", "passed", Some(0.5), None);
        fast_mid.wall_time_ms = Some(100.0);
        let mut mid_high = export_result("run-dist-c", "target-dist", "passed", Some(1.0), None);
        mid_high.wall_time_ms = Some(200.0);

        let results = vec![slow_low, fast_mid, mid_high];
        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        let ranking = &analysis["target_ranking"][0];
        assert_eq!(ranking["average_score"], 0.5);
        assert_eq!(ranking["median_score"], 0.5);
        assert_eq!(ranking["min_score"], 0.0);
        assert_eq!(ranking["max_score"], 1.0);
        assert_eq!(ranking["median_wall_time_ms"], 200.0);
        assert_eq!(ranking["min_wall_time_ms"], 100.0);
        assert_eq!(ranking["max_wall_time_ms"], 300.0);

        let comparison = &analysis["comparison"][0];
        assert_eq!(comparison["median_score"], 0.5);
        assert_eq!(comparison["min_wall_time_ms"], 100.0);
        assert_eq!(comparison["max_wall_time_ms"], 300.0);
        assert_eq!(comparison["average_provider_retry_delay_ms"], 0.0);

        let task = &analysis["task_drilldown"][0];
        assert_eq!(task["median_score"], 0.5);
        assert_eq!(task["median_wall_time_ms"], 200.0);
        assert_eq!(task["max_score"], 1.0);

        let report = markdown_report(&results, &[]);
        assert!(report.contains("## Distribution Summary"));
        assert!(report
            .contains("| target-dist | 3 | 0.50 / 0 / 1 | 200 ms / 100 ms / 300 ms | 300 ms |"));
        assert!(report.contains(
            "| group-al | llm-core | llm-core-json-001 | target-dist | 3 | 0.50 / 0 / 1 | 200 ms / 100 ms / 300 ms | 300 ms |"
        ));
    }

    #[test]
    fn analysis_export_flags_close_quality_contenders() {
        let mut fast = export_result("run-fast", "target-fast", "passed", Some(1.0), None);
        fast.wall_time_ms = Some(100.0);
        fast.cost_usd = Some(0.002);

        let mut cheap = export_result("run-cheap", "target-cheap", "passed", Some(1.0), None);
        cheap.wall_time_ms = Some(1_000.0);
        cheap.cost_usd = Some(0.0);

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&[cheap, fast]).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(
            analysis["decision"]["recommended_target"]["target_id"],
            "target-fast"
        );
        assert_eq!(
            analysis["decision"]["close_contenders"][0]["target_id"],
            "target-cheap"
        );
        assert!(analysis["decision"]["tie_note"]
            .as_str()
            .unwrap_or_default()
            .contains("matched the recommended target's pass rate and average score"));
    }

    #[test]
    fn analysis_export_prefers_lower_score_spread_on_quality_tie() {
        let mut stable_a =
            export_result("run-stable-a", "target-stable", "passed", Some(0.5), None);
        stable_a.wall_time_ms = Some(500.0);
        let mut stable_b =
            export_result("run-stable-b", "target-stable", "passed", Some(0.5), None);
        stable_b.wall_time_ms = Some(500.0);

        let mut noisy_a = export_result("run-noisy-a", "target-noisy", "passed", Some(0.0), None);
        noisy_a.wall_time_ms = Some(10.0);
        let mut noisy_b = export_result("run-noisy-b", "target-noisy", "passed", Some(1.0), None);
        noisy_b.wall_time_ms = Some(10.0);

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&[noisy_a, stable_a, noisy_b, stable_b])
                .expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(
            analysis["decision"]["recommended_target"]["target_id"],
            "target-stable"
        );
        assert_eq!(analysis["target_ranking"][0]["target_id"], "target-stable");
        assert_eq!(analysis["target_ranking"][0]["score_std_dev"], 0.0);
        assert!(
            analysis["target_ranking"][1]["score_std_dev"]
                .as_f64()
                .unwrap_or_default()
                > 0.7
        );
        assert!(analysis["decision"]["score_stability_note"]
            .as_str()
            .unwrap_or_default()
            .contains("target-noisy"));
        assert!(analysis["ranking_policy"]
            .as_str()
            .unwrap_or_default()
            .contains("score_std_dev asc"));
    }

    #[test]
    fn analysis_export_warns_on_uneven_target_task_coverage() {
        let mut full_a = export_result("run-full-a", "target-full", "passed", Some(1.0), None);
        full_a.task_id = "task-a".into();
        let mut full_b = export_result("run-full-b", "target-full", "passed", Some(1.0), None);
        full_b.task_id = "task-b".into();
        let mut partial =
            export_result("run-partial-a", "target-partial", "passed", Some(1.0), None);
        partial.task_id = "task-a".into();

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&[full_a, full_b, partial])
                .expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert!(analysis["decision"]["coverage_note"]
            .as_str()
            .unwrap_or_default()
            .contains("Coverage warning"));
        assert_eq!(
            analysis["decision"]["coverage_issues"][0]["target_id"],
            "target-partial"
        );
        assert_eq!(
            analysis["decision"]["coverage_issues"][0]["missing_task_count"],
            1
        );
        assert_eq!(
            analysis["decision"]["coverage_issues"][0]["missing_pack_task_slot_count"],
            1
        );
        assert_eq!(
            analysis["decision"]["coverage_issues"][0]["missing_task_ids"][0],
            "task-b"
        );
        assert_eq!(
            analysis["decision"]["coverage_issues"][0]["missing_pack_task_slot_ids"][0],
            "llm-core/task-b"
        );
        assert_eq!(
            analysis["decision"]["recommended_next_run"]["target_ids"][0],
            "target-partial"
        );
        assert_eq!(
            analysis["decision"]["recommended_next_run"]["benchmark_pack_id"],
            "llm-core"
        );
        assert_eq!(
            analysis["decision"]["recommended_next_run"]["task_ids"][0],
            "task-b"
        );

        let mut report_full_a =
            export_result("run-full-a", "target-full", "passed", Some(1.0), None);
        report_full_a.task_id = "task-a".into();
        let mut report_full_b =
            export_result("run-full-b", "target-full", "passed", Some(1.0), None);
        report_full_b.task_id = "task-b".into();
        let mut report_partial =
            export_result("run-partial-a", "target-partial", "passed", Some(1.0), None);
        report_partial.task_id = "task-a".into();
        let report = markdown_report(&[report_full_a, report_full_b, report_partial], &[]);
        assert!(report.contains("Coverage note: Coverage warning"));
        assert!(report.contains("targets target-partial; tasks task-b; repetitions 3"));
    }

    #[test]
    fn analysis_export_warns_on_crossed_pack_task_coverage() {
        let mut target_a_pack_one = export_result(
            "run-target-a-pack-one",
            "target-a",
            "passed",
            Some(1.0),
            None,
        );
        target_a_pack_one.benchmark_pack_id = "pack-one".into();
        target_a_pack_one.task_id = "task-alpha".into();
        let mut target_a_pack_two = export_result(
            "run-target-a-pack-two",
            "target-a",
            "passed",
            Some(1.0),
            None,
        );
        target_a_pack_two.benchmark_pack_id = "pack-two".into();
        target_a_pack_two.task_id = "task-beta".into();
        let mut target_b_pack_one = export_result(
            "run-target-b-pack-one",
            "target-b",
            "passed",
            Some(1.0),
            None,
        );
        target_b_pack_one.benchmark_pack_id = "pack-one".into();
        target_b_pack_one.task_id = "task-beta".into();
        let mut target_b_pack_two = export_result(
            "run-target-b-pack-two",
            "target-b",
            "passed",
            Some(1.0),
            None,
        );
        target_b_pack_two.benchmark_pack_id = "pack-two".into();
        target_b_pack_two.task_id = "task-alpha".into();

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&[
                target_a_pack_one,
                target_a_pack_two,
                target_b_pack_one,
                target_b_pack_two,
            ])
            .expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert!(analysis["decision"]["coverage_note"]
            .as_str()
            .unwrap_or_default()
            .contains("pack/task slot"));
        assert_eq!(
            analysis["decision"]["coverage_issues"][0]["missing_task_count"],
            0
        );
        assert_eq!(
            analysis["decision"]["coverage_issues"][0]["missing_benchmark_pack_count"],
            0
        );
        assert_eq!(
            analysis["decision"]["coverage_issues"][0]["missing_pack_task_slot_count"],
            2
        );
    }

    #[test]
    fn analysis_export_includes_pass_rate_confidence_interval() {
        let results = vec![
            export_result("run-ci-a", "target-ci", "passed", Some(1.0), None),
            export_result("run-ci-b", "target-ci", "passed", Some(1.0), None),
            export_result("run-ci-c", "target-ci", "passed", Some(1.0), None),
            export_result(
                "run-ci-d",
                "target-ci",
                "failed",
                Some(0.0),
                Some("timeout"),
            ),
        ];

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["target_ranking"][0]["pass_rate"], 0.75);
        assert_eq!(
            analysis["target_ranking"][0]["pass_rate_ci_method"],
            "wilson_95"
        );
        let low = analysis["target_ranking"][0]["pass_rate_ci_low"]
            .as_f64()
            .expect("low CI should be numeric");
        let high = analysis["target_ranking"][0]["pass_rate_ci_high"]
            .as_f64()
            .expect("high CI should be numeric");
        assert!((0.29..0.31).contains(&low), "unexpected low CI: {low}");
        assert!((0.94..0.96).contains(&high), "unexpected high CI: {high}");

        let report = markdown_report(&results, &[]);
        assert!(report.contains("Pass rate / 95% CI"));
        assert!(report.contains("75% pass across 4 run(s), 95% CI 30%-95%"));
        assert!(report.contains(
            "| 1 | target-ci | OpenAI-compatible (4) | 1 pack(s), 1 task(s), 1 group(s) | 4 | 75% | 75%; 30%-95% |"
        ));
    }

    #[test]
    fn analysis_export_warns_when_pass_rate_confidence_intervals_overlap() {
        let results = vec![
            export_result("run-leader-a", "target-leader", "passed", Some(1.0), None),
            export_result("run-leader-b", "target-leader", "passed", Some(1.0), None),
            export_result("run-leader-c", "target-leader", "passed", Some(1.0), None),
            export_result(
                "run-leader-d",
                "target-leader",
                "failed",
                Some(0.0),
                Some("timeout"),
            ),
            export_result(
                "run-challenger-a",
                "target-challenger",
                "passed",
                Some(1.0),
                None,
            ),
            export_result(
                "run-challenger-b",
                "target-challenger",
                "passed",
                Some(1.0),
                None,
            ),
            export_result(
                "run-challenger-c",
                "target-challenger",
                "failed",
                Some(0.0),
                Some("timeout"),
            ),
            export_result(
                "run-challenger-d",
                "target-challenger",
                "failed",
                Some(0.0),
                Some("timeout"),
            ),
        ];

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(
            analysis["decision"]["recommended_target"]["target_id"],
            "target-leader"
        );
        assert_eq!(
            analysis["decision"]["pass_rate_ci_overlap_targets"][0],
            "target-challenger"
        );
        assert_eq!(analysis["decision"]["evidence_grade"], "directional");
        assert_eq!(
            analysis["decision"]["decision_status"],
            "collect_more_evidence"
        );
        assert!(analysis["decision"]["selected_target_id"].is_null());
        assert!(analysis["decision"]["selection_note"]
            .as_str()
            .unwrap_or_default()
            .contains("Collect more evidence before choosing a winner"));
        assert!(analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array")
            .iter()
            .any(|risk| risk.as_str() == Some("pass_rate_ci_overlap")));
        assert!(analysis["decision"]["confidence_note"]
            .as_str()
            .unwrap_or_default()
            .contains("Pass-rate confidence warning"));

        let report = markdown_report(&results, &[]);
        assert!(report.contains(
            "Confidence note: Pass-rate confidence warning: the recommended target's Wilson 95% interval overlaps 1 target(s): target-challenger"
        ));
    }

    #[test]
    fn analysis_export_warns_when_task_target_repetitions_are_sparse() {
        let mut results = Vec::new();
        for target in ["target-a", "target-b"] {
            for task in ["task-a", "task-b", "task-c"] {
                let mut result = export_result(
                    &format!("run-{target}-{task}"),
                    target,
                    "passed",
                    Some(1.0),
                    None,
                );
                result.task_id = task.into();
                results.push(result);
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["low_sample_rows"], 0);
        assert_eq!(analysis["decision"]["task_target_rows"], 6);
        assert_eq!(analysis["decision"]["low_repetition_task_rows"], 6);
        assert_eq!(analysis["decision"]["recommended_task_repetitions"], 3);
        assert_eq!(analysis["decision"]["evidence_grade"], "smoke");
        assert_eq!(
            analysis["decision"]["decision_status"],
            "collect_more_evidence"
        );
        assert!(analysis["decision"]["selected_target_id"].is_null());
        assert!(analysis["decision"]["evidence_note"]
            .as_str()
            .unwrap_or_default()
            .contains("too shallow for model selection"));
        assert!(analysis["decision"]["task_repetition_note"]
            .as_str()
            .unwrap_or_default()
            .contains("task-target row(s) have fewer than 3 measured repetitions"));
        assert!(analysis["decision"]["confidence_note"]
            .as_str()
            .unwrap_or_default()
            .contains("separate task breadth from repeatability"));
        let next_run = &analysis["decision"]["recommended_next_run"];
        assert_eq!(next_run["benchmark_pack_id"], "llm-core");
        assert_eq!(next_run["benchmark_pack_ids"][0], "llm-core");
        assert_eq!(next_run["target_ids"][0], "target-a");
        assert_eq!(next_run["target_ids"][1], "target-b");
        assert_eq!(next_run["repetitions"], 3);
        assert_eq!(next_run["warmup_runs"], 1);
        assert_eq!(next_run["concurrency"], 2);
        assert_eq!(next_run["max_cost_usd"], 1.0);
        assert!(next_run["note"]
            .as_str()
            .unwrap_or_default()
            .contains("Confirm these targets still exist"));

        let report = markdown_report(&results, &[]);
        assert!(report.contains(
            "6/6 task-target row(s) have fewer than 3 measured repetitions for the same task and target"
        ));
        assert!(
            report.contains("All comparison rows in this export have at least 3 measured runs.")
        );
        assert!(report.contains(
            "Suggested next run: pack(s) llm-core; targets target-a, target-b; repetitions 3; warmups 1; concurrency 2; max cost $1.0000."
        ));
    }

    #[test]
    fn analysis_export_marks_balanced_repeated_comparison_ready() {
        let mut results = Vec::new();
        for target in ["target-good", "target-bad"] {
            for task in ["task-a", "task-b"] {
                for repetition in 0..3 {
                    let passed = target == "target-good";
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        if passed { "passed" } else { "failed" },
                        Some(if passed { 1.0 } else { 0.0 }),
                        (!passed).then_some("incorrect_answer"),
                    );
                    result.task_id = task.into();
                    mark_export_result_pack_calibrated(&mut result);
                    result.started_at = Some(format!("2026-07-06T14:00:0{repetition}Z"));
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "comparison_ready");
        assert_eq!(analysis["decision"]["evidence_tone"], "ok");
        assert_eq!(
            analysis["decision"]["decision_status"],
            "select_recommended_target"
        );
        assert_eq!(analysis["decision"]["selected_target_id"], "target-good");
        assert!(analysis["decision"]["selection_note"]
            .as_str()
            .unwrap_or_default()
            .contains("select target-good"));
        assert_eq!(
            analysis["decision"]["evidence_risks"]
                .as_array()
                .expect("evidence risks should be an array")
                .len(),
            0
        );
        assert!(analysis["decision"]["evidence_note"]
            .as_str()
            .unwrap_or_default()
            .contains("Comparison-ready evidence"));
        assert!(analysis["decision"]["recommended_next_run"].is_null());

        let report = markdown_report(&results, &[]);
        assert!(report
            .contains("Decision status: select_recommended_target (selected target: target-good)"));
        assert!(report.contains("Selection note: Evidence is comparison-ready"));
        assert!(report.contains("Evidence grade: Comparison-ready (comparison_ready)"));
        assert!(report.contains("No immediate rerun is required for a first-pass comparison"));
    }

    #[test]
    fn analysis_export_downgrades_comparison_with_pricing_assumptions() {
        let mut results = Vec::new();
        for target in ["target-good", "target-cache-priced"] {
            for task in ["task-a", "task-b"] {
                for repetition in 0..3 {
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        "passed",
                        Some(1.0),
                        None,
                    );
                    result.task_id = task.into();
                    mark_export_result_pack_calibrated(&mut result);
                    if target == "target-cache-priced" {
                        result.pricing_assumption =
                            Some("cache_read_tokens_priced_as_input".into());
                    }
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "directional");
        assert!(analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array")
            .iter()
            .any(|risk| risk.as_str() == Some("pricing_assumption")));
        let cache_priced_row = analysis["comparison"]
            .as_array()
            .expect("comparison should be an array")
            .iter()
            .find(|row| row["target_id"] == "target-cache-priced")
            .expect("cache-priced target should be present");
        assert_eq!(
            cache_priced_row["pricing_assumptions"]["cache_read_tokens_priced_as_input"],
            6
        );
        assert!(analysis["decision"]["minimum_next_run"]
            .as_str()
            .unwrap_or_default()
            .contains("cache read/write pricing"));

        let report = markdown_report(&results, &[]);
        assert!(report.contains("cache_read_tokens_priced_as_input"));
        assert!(report.contains("Pricing assumptions"));
    }

    #[test]
    fn analysis_export_downgrades_weak_private_prompt_pack() {
        let mut results = Vec::new();
        for target in ["target-good", "target-bad"] {
            for task in ["task-a", "task-b", "task-c"] {
                for repetition in 0..3 {
                    let passed = target == "target-good";
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        if passed { "passed" } else { "failed" },
                        Some(if passed { 1.0 } else { 0.0 }),
                        (!passed).then_some("incorrect_answer"),
                    );
                    result.benchmark_pack_id = "weak-private".into();
                    result.reproducibility["benchmark_pack"]["id"] =
                        serde_json::json!("weak-private");
                    result.reproducibility["benchmark_pack"]["evidence_profile"] =
                        serde_json::json!("weak_prompt_suite");
                    result.reproducibility["benchmark_pack"]["evidence_warnings"] =
                        serde_json::json!([
                            "Prompt tasks use no scoring checks; results should not drive model selection."
                        ]);
                    result.task_id = task.into();
                    result.reproducibility["task"]["id"] = serde_json::json!(task);
                    result.started_at = Some(format!("2026-07-06T14:00:0{repetition}Z"));
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "smoke");
        assert!(analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array")
            .iter()
            .any(|risk| risk.as_str() == Some("pack_evidence_profile")));
        assert_eq!(
            analysis["decision"]["pack_evidence_issues"][0]["benchmark_pack_id"],
            "weak-private"
        );
        assert_eq!(
            analysis["decision"]["pack_evidence_issues"][0]["evidence_profile"],
            "weak_prompt_suite"
        );
        assert!(analysis["decision"]["evidence_note"]
            .as_str()
            .unwrap_or_default()
            .contains("pack evidence warning"));
        assert!(analysis["decision"]["minimum_next_run"]
            .as_str()
            .unwrap_or_default()
            .contains("prompt_comparison pack"));
        assert_eq!(
            analysis["decision"]["recommended_next_run"]["benchmark_pack_id"],
            "llm-reliability"
        );

        let report = markdown_report(&results, &[]);
        assert!(report.contains("pack evidence warning"));
        assert!(report.contains("prompt_comparison pack"));
    }

    #[test]
    fn analysis_export_surfaces_pack_calibration_warnings() {
        let mut results = Vec::new();
        for target in ["target-good", "target-bad"] {
            for task in ["task-a", "task-b", "task-c"] {
                for repetition in 0..3 {
                    let passed = target == "target-good";
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        if passed { "passed" } else { "failed" },
                        Some(if passed { 1.0 } else { 0.0 }),
                        (!passed).then_some("incorrect_answer"),
                    );
                    result.benchmark_pack_id = "pilot-private".into();
                    result.reproducibility["benchmark_pack"] = serde_json::json!({
                        "id": "pilot-private",
                        "version": "0.1.0",
                        "evidence_profile": "prompt_comparison",
                        "evidence_warnings": [],
                        "prompt_tasks": 3,
                        "total_task_weight": 3.0,
                        "calibration": {
                            "status": "pilot",
                            "sample_size": 4,
                            "baseline_models": ["local-baseline", "cloud-baseline"],
                            "last_reviewed": "2026-07-07",
                            "notes": "Pilot reviewed against internal smoke baselines."
                        }
                    });
                    result.task_id = task.into();
                    result.reproducibility["task"]["id"] = serde_json::json!(task);
                    result.started_at = Some(format!("2026-07-06T14:00:0{repetition}Z"));
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "directional");
        assert_eq!(analysis["decision"]["evidence_tone"], "warn");
        assert_eq!(
            analysis["decision"]["decision_status"],
            "collect_more_evidence"
        );
        assert!(analysis["decision"]["selected_target_id"].is_null());
        assert!(analysis["decision"]["recommended_next_run"].is_object());
        assert!(analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array")
            .iter()
            .any(|risk| risk.as_str() == Some("pack_calibration")));
        assert_eq!(
            analysis["decision"]["pack_calibration_issues"][0]["benchmark_pack_id"],
            "pilot-private"
        );
        assert_eq!(
            analysis["decision"]["pack_calibration_issues"][0]["statuses"][0],
            "pilot"
        );
        assert_eq!(
            analysis["decision"]["pack_calibration_issues"][0]["sample_sizes"][0],
            4
        );
        assert!(analysis["decision"]["calibration_note"]
            .as_str()
            .unwrap_or_default()
            .contains("will not select a winner as comparison-ready"));
        assert!(analysis["decision"]["evidence_note"]
            .as_str()
            .unwrap_or_default()
            .contains("pack calibration warning"));
        assert!(analysis["decision"]["minimum_next_run"]
            .as_str()
            .unwrap_or_default()
            .contains("Calibrate or review"));

        let report = markdown_report(&results, &[]);
        assert!(report.contains("Calibration note: Pack calibration warning"));
        assert!(report.contains("Decision status: collect_more_evidence"));
        assert!(report.contains("Evidence grade: Directional evidence (directional)"));
        assert!(report.contains("pilot-private is pilot, sample size 4, reviewed 2026-07-07"));
    }

    #[test]
    fn analysis_export_rejects_calibrated_prompt_pack_missing_quality_gates() {
        let mut results = Vec::new();
        for target in ["target-good", "target-bad"] {
            for task in ["task-a", "task-b", "task-c"] {
                for repetition in 0..3 {
                    let passed = target == "target-good";
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        if passed { "passed" } else { "failed" },
                        Some(if passed { 1.0 } else { 0.0 }),
                        (!passed).then_some("incorrect_answer"),
                    );
                    result.benchmark_pack_id = "stale-calibrated-pack".into();
                    result.task_id = task.into();
                    result.reproducibility["task"]["id"] = serde_json::json!(task);
                    result.reproducibility["benchmark_pack"] = serde_json::json!({
                        "id": "stale-calibrated-pack",
                        "version": "0.1.0",
                        "evidence_profile": "prompt_comparison",
                        "evidence_warnings": [],
                        "prompt_tasks": 3,
                        "total_task_weight": 3.0,
                        "calibration": {
                            "status": "calibrated",
                            "sample_size": 24,
                            "baseline_models": ["local-baseline", "cloud-baseline"],
                            "last_reviewed": "2026-07-07",
                            "notes": "Older calibrated metadata before quality gates were introduced."
                        }
                    });
                    result.started_at = Some(format!("2026-07-06T14:00:0{repetition}Z"));
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "directional");
        assert_eq!(
            analysis["decision"]["decision_status"],
            "collect_more_evidence"
        );
        assert!(analysis["decision"]["selected_target_id"].is_null());
        assert_eq!(
            analysis["decision"]["pack_calibration_issues"][0]["benchmark_pack_id"],
            "stale-calibrated-pack"
        );
        assert_eq!(
            analysis["decision"]["pack_calibration_issues"][0]["statuses"][0],
            "calibrated"
        );
        assert!(
            analysis["decision"]["pack_calibration_issues"][0]["missing_quality_gates"]
                .as_array()
                .expect("missing quality gates should be an array")
                .iter()
                .any(|gate| gate.as_str() == Some("local_cloud_baseline_pair"))
        );
        assert!(analysis["decision"]["calibration_note"]
            .as_str()
            .unwrap_or_default()
            .contains("not fully calibrated for model selection"));

        let report = markdown_report(&results, &[]);
        assert!(report.contains("missing gate(s)"));
        assert!(report.contains("local_cloud_baseline_pair"));
    }

    #[test]
    fn analysis_export_downgrades_comparison_when_model_identity_is_unstable() {
        let mut results = Vec::new();
        for target in ["target-good", "target-bad"] {
            for task in ["task-a", "task-b"] {
                for repetition in 0..3 {
                    let passed = target == "target-good";
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        if passed { "passed" } else { "failed" },
                        Some(if passed { 1.0 } else { 0.0 }),
                        (!passed).then_some("incorrect_answer"),
                    );
                    result.task_id = task.into();
                    result.started_at = Some(format!("2026-07-06T14:00:0{repetition}Z"));
                    if target == "target-good" && task == "task-a" && repetition == 0 {
                        result.provider_model = None;
                    }
                    if target == "target-bad" && task == "task-a" && repetition == 0 {
                        result.provider_model = Some("served-model-b-alt".into());
                    }
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "directional");
        assert_eq!(
            analysis["decision"]["decision_status"],
            "collect_more_evidence"
        );
        assert!(analysis["decision"]["selected_target_id"].is_null());
        let risks = analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array");
        assert!(risks
            .iter()
            .any(|risk| risk.as_str() == Some("provider_model_missing")));
        assert!(risks
            .iter()
            .any(|risk| risk.as_str() == Some("provider_model_inconsistent")));
        assert!(analysis["decision"]["evidence_note"]
            .as_str()
            .unwrap_or_default()
            .contains("served model"));
        assert!(analysis["decision"]["minimum_next_run"]
            .as_str()
            .unwrap_or_default()
            .contains("provider-supplied served model id"));
    }

    #[test]
    fn analysis_export_downgrades_comparison_when_model_identity_is_configured_fallback() {
        let mut results = Vec::new();
        for target in ["target-good", "target-fallback"] {
            for task in ["task-a", "task-b"] {
                for repetition in 0..3 {
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        "passed",
                        Some(1.0),
                        None,
                    );
                    result.task_id = task.into();
                    result.started_at = Some(format!("2026-07-06T14:00:0{repetition}Z"));
                    if target == "target-fallback" {
                        result.provider_model_source = Some("target_config".into());
                    }
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "directional");
        assert!(analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array")
            .iter()
            .any(|risk| risk.as_str() == Some("provider_model_configured_fallback")));
        let warnings = analysis["model_identity_warnings"]
            .as_array()
            .expect("model identity warnings should be an array");
        assert!(warnings.iter().any(|warning| warning["issue"]
            == "provider_model_configured_fallback"
            && warning["target_id"] == "target-fallback"
            && warning["provider_model_sources"]["target_config"] == 6));
        assert!(analysis["decision"]["minimum_next_run"]
            .as_str()
            .unwrap_or_default()
            .contains("provider-supplied served model id"));
    }

    #[test]
    fn analysis_export_downgrades_comparison_when_generation_settings_are_mixed() {
        let mut results = Vec::new();
        for target in ["target-deterministic", "target-exploratory"] {
            for task in ["task-a", "task-b"] {
                for repetition in 0..3 {
                    let passed = target == "target-deterministic";
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        if passed { "passed" } else { "failed" },
                        Some(if passed { 1.0 } else { 0.0 }),
                        (!passed).then_some("incorrect_answer"),
                    );
                    result.task_id = task.into();
                    if target == "target-exploratory" {
                        result.reproducibility["generation"]["temperature"] =
                            serde_json::json!(0.7);
                        result.reproducibility["generation"]["top_p"] = serde_json::json!(0.9);
                    }
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "directional");
        assert_eq!(
            analysis["decision"]["decision_status"],
            "collect_more_evidence"
        );
        assert!(analysis["decision"]["selected_target_id"].is_null());
        assert!(analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array")
            .iter()
            .any(|risk| risk.as_str() == Some("generation_settings_mixed")));
        let warnings = analysis["generation_setting_warnings"]
            .as_array()
            .expect("generation setting warnings should be an array");
        assert!(warnings.iter().any(|warning| warning["issue"]
            == "generation_settings_mixed_scope"
            && warning["generation_settings"]
                ["mode deterministic, temp 0, top_p 1, seed not_set"]
                == 6
            && warning["generation_settings"]
                ["mode exploratory, temp 0.7, top_p 0.9, seed not_set"]
                == 6));
        assert!(analysis["decision"]["minimum_next_run"]
            .as_str()
            .unwrap_or_default()
            .contains("one shared generation policy"));

        let report = markdown_report(&results, &[]);
        assert!(report.contains("## Generation Setting Warnings"));
        assert!(report.contains("generation_settings_mixed_scope"));
        assert!(report.contains("deterministic and exploratory sampling"));
    }

    #[test]
    fn analysis_export_downgrades_comparison_when_cost_coverage_is_missing() {
        let mut results = Vec::new();
        for target in ["target-good", "target-unpriced"] {
            for task in ["task-a", "task-b"] {
                for repetition in 0..3 {
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        "passed",
                        Some(1.0),
                        None,
                    );
                    result.task_id = task.into();
                    if target == "target-unpriced" {
                        result.cost_usd = None;
                        set_export_result_target(
                            &mut result,
                            "openrouter",
                            serde_json::json!({
                                "base_url": "https://openrouter.ai/api/v1",
                                "model": "cloud-unpriced"
                            }),
                        );
                    }
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "directional");
        assert_eq!(
            analysis["decision"]["decision_status"],
            "collect_more_evidence"
        );
        assert!(analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array")
            .iter()
            .any(|risk| risk.as_str() == Some("cost_coverage_gap")));
        assert!(analysis["decision"]["evidence_note"]
            .as_str()
            .unwrap_or_default()
            .contains("missing cost metrics"));
        assert!(analysis["decision"]["minimum_next_run"]
            .as_str()
            .unwrap_or_default()
            .contains("Add pricing"));
    }

    #[test]
    fn analysis_export_treats_known_zero_local_unpriced_results_as_cost_covered() {
        let mut results = Vec::new();
        for target in ["target-local", "target-cloud"] {
            for task in ["task-a", "task-b"] {
                for repetition in 0..3 {
                    let passed = target == "target-local";
                    let mut result = export_result(
                        &format!("run-{target}-{task}-{repetition}"),
                        target,
                        if passed { "passed" } else { "failed" },
                        Some(if passed { 1.0 } else { 0.0 }),
                        (!passed).then_some("incorrect_answer"),
                    );
                    result.task_id = task.into();
                    mark_export_result_pack_calibrated(&mut result);
                    result.started_at = Some(format!("2026-07-06T14:00:0{repetition}Z"));
                    if target == "target-local" {
                        result.cost_usd = None;
                        result.estimated_cost_usd = None;
                        result.provider_model = Some("local-gguf".into());
                        set_export_result_target(
                            &mut result,
                            "llama-cpp-openai",
                            serde_json::json!({
                                "source": "huggingface-local",
                                "base_url": "http://127.0.0.1:8080/v1",
                                "model": "local-gguf"
                            }),
                        );
                    } else {
                        result.cost_usd = Some(0.002);
                        result.provider_model = Some("cloud-model".into());
                        set_export_result_target(
                            &mut result,
                            "openrouter",
                            serde_json::json!({
                                "base_url": "https://openrouter.ai/api/v1",
                                "model": "cloud-model",
                                "input_price_usd_per_million_tokens": 0.25,
                                "output_price_usd_per_million_tokens": 2.0
                            }),
                        );
                    }
                    results.push(result);
                }
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "comparison_ready");
        assert!(!analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array")
            .iter()
            .any(|risk| risk.as_str() == Some("cost_coverage_gap")));
        let local_ranking = analysis["target_ranking"]
            .as_array()
            .expect("target rankings should be an array")
            .iter()
            .find(|row| row["target_id"] == "target-local")
            .expect("local target ranking should be present");
        assert_eq!(local_ranking["average_cost_usd"], 0.0);
        assert_eq!(local_ranking["costed_runs"], 6);
    }

    #[test]
    fn analysis_export_smoke_evidence_mentions_missing_cost() {
        let mut results = Vec::new();
        for target in ["target-local", "target-cloud"] {
            for repetition in 0..3 {
                let mut result = export_result(
                    &format!("run-{target}-{repetition}"),
                    target,
                    "passed",
                    Some(1.0),
                    None,
                );
                result.benchmark_pack_id = "llm-connectivity".into();
                result.task_id = "llm-connectivity-nonempty-001".into();
                result.started_at = Some(format!("2026-07-06T14:00:0{repetition}Z"));
                if target == "target-cloud" {
                    result.cost_usd = None;
                    set_export_result_target(
                        &mut result,
                        "openrouter",
                        serde_json::json!({
                            "base_url": "https://openrouter.ai/api/v1",
                            "model": "cloud-unpriced"
                        }),
                    );
                }
                results.push(result);
            }
        }

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");

        assert_eq!(analysis["decision"]["evidence_grade"], "smoke");
        assert!(analysis["decision"]["evidence_risks"]
            .as_array()
            .expect("evidence risks should be an array")
            .iter()
            .any(|risk| risk.as_str() == Some("cost_coverage_gap")));
        assert!(analysis["decision"]["evidence_note"]
            .as_str()
            .unwrap_or_default()
            .contains("missing cost metrics"));
        assert!(analysis["decision"]["minimum_next_run"]
            .as_str()
            .unwrap_or_default()
            .contains("Add pricing"));
        assert!(analysis["decision"]["minimum_next_run"]
            .as_str()
            .unwrap_or_default()
            .contains("llm-reliability"));
    }

    #[test]
    fn scoped_analysis_and_markdown_exports_include_scope_note() {
        let results = vec![export_result(
            "run-scoped",
            "target-scoped",
            "passed",
            Some(1.0),
            None,
        )];
        let note = export_scope_note(results.len());

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json_with_scope(&results, Some(&note))
                .expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");
        assert!(analysis["scope_note"]
            .as_str()
            .unwrap_or_default()
            .contains("not necessarily the full result history"));

        let report = markdown_report_with_scope(&results, &[], Some(&note));
        assert!(report.contains("Scope note: This export contains 1 selected result row"));
        assert!(report.contains("whole-history comparison"));
    }

    #[test]
    fn analysis_and_markdown_exports_include_run_group_trends() {
        let mut previous_a =
            export_result("run-previous-a", "target-trend", "passed", Some(1.0), None);
        previous_a.run_group_id = Some("group-previous".into());
        previous_a.started_at = Some("2026-07-06T10:00:00Z".into());
        previous_a.wall_time_ms = Some(500.0);
        previous_a.cost_usd = Some(0.001);
        let mut previous_b =
            export_result("run-previous-b", "target-trend", "passed", Some(1.0), None);
        previous_b.run_group_id = Some("group-previous".into());
        previous_b.started_at = Some("2026-07-06T10:00:01Z".into());
        previous_b.wall_time_ms = Some(600.0);
        previous_b.cost_usd = Some(0.001);
        let mut current_a =
            export_result("run-current-a", "target-trend", "passed", Some(0.6), None);
        current_a.run_group_id = Some("group-current".into());
        current_a.started_at = Some("2026-07-07T10:00:00Z".into());
        current_a.wall_time_ms = Some(1_500.0);
        current_a.cost_usd = Some(0.003);
        let mut current_b = export_result(
            "run-current-b",
            "target-trend",
            "failed",
            Some(0.0),
            Some("timeout"),
        );
        current_b.run_group_id = Some("group-current".into());
        current_b.started_at = Some("2026-07-07T10:00:01Z".into());
        current_b.wall_time_ms = Some(1_800.0);
        current_b.cost_usd = Some(0.003);
        let results = vec![previous_a, previous_b, current_a, current_b];

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");
        let trend = &analysis["run_group_trends"][0];
        assert_eq!(trend["current_group_id"], "group-current");
        assert_eq!(trend["previous_group_id"], "group-previous");
        assert_eq!(trend["pass_rate_delta"], -0.5);
        assert_eq!(trend["signal_level"], "warn");
        assert!(trend["signal"]
            .as_str()
            .unwrap_or_default()
            .contains("regression"));

        let report = markdown_report(&results, &[]);
        assert!(report.contains("## Run Group Trends"));
        assert!(report.contains("| llm-core | target-trend | group-cu | group-pr | 2/2 | -50 pp"));
        assert!(report.contains("regression: pass rate -50 pp"));
    }

    #[test]
    fn markdown_report_includes_queued_run_configuration() {
        let run_group = store::RunGroupRecord {
            id: "group-alpha".into(),
            benchmark_pack_id: "llm-core".into(),
            target_ids: vec!["target-one".into()],
            status: "completed".into(),
            started_at: "2026-07-06T12:00:00Z".into(),
            finished_at: Some("2026-07-06T12:00:05Z".into()),
            config: serde_json::json!({
                "repetitions": 3,
                "warmup_runs": 1,
                "concurrency": 2,
                "task_count": 1,
                "task_ids": ["llm-core-json-001"],
                "docker": false,
                "max_cost_usd": 0.5,
                "replay": {
                    "mode": "retry",
                    "source_job_id": "source-job-123456",
                    "source_run_group_id": "source-group-abcdef",
                    "source_target_count": 2,
                    "source_task_count": 3,
                    "source_repetitions": 3,
                    "target_count": 1,
                    "task_count": 1,
                    "repetitions": 1,
                    "scoped": true
                },
                "targets": [{
                    "id": "target-one",
                    "adapter_id": "openai-compatible",
                    "model": "provider-model-a",
                    "generation": {
                        "temperature": 0.0,
                        "top_p": 1.0,
                        "max_tokens": 512,
                        "max_tokens_source": "target_config",
                        "timeout_seconds": 120,
                        "retry_count": 1
                    },
                    "pricing": {
                        "input_price_usd_per_million_tokens": 0.25,
                        "output_price_usd_per_million_tokens": 2.0,
                        "pricing_verified_at": "2026-07-06"
                    },
                    "validation": {
                        "status": "ok",
                        "detail": "completion probe succeeded; model listed",
                        "checked_at": "2026-07-07T12:00:00Z"
                    }
                }]
            }),
        };

        let report = markdown_report(
            &[export_result(
                "run-a",
                "target-one",
                "passed",
                Some(1.0),
                None,
            )],
            &[run_group],
        );

        assert!(report.contains("## Run Configuration"));
        assert!(report
            .contains("retry of job source-j (scoped), source group source-g, source targets 2, source tasks 3, source reps 3; repetitions 3; warmups 1; concurrency 2; task count 1; docker false; tasks llm-core-json-001; max cost $0.5000"));
        assert!(report.contains("model provider-model-a"));
        assert!(report.contains("max 512"));
        assert!(report.contains("verified 2026-07-06"));
        assert!(report.contains("validation status ok"));
        assert!(report.contains("completion probe succeeded; model listed"));
        assert!(report.contains("checked 2026-07-07T12:00:00Z"));
    }

    #[test]
    fn markdown_report_explains_missing_metric_coverage() {
        let complete = export_result("run-complete", "target-one", "passed", Some(1.0), None);
        let mut sparse = export_result("run-sparse", "target-two", "passed", Some(1.0), None);
        sparse.provider_time_to_first_byte_ms = None;
        sparse.provider_time_to_first_token_ms = None;
        sparse.ttft_ms = None;
        sparse.provider_request_total_ms = None;
        sparse.prompt_tokens = None;
        sparse.input_tokens = None;
        sparse.completion_tokens = None;
        sparse.output_tokens = None;
        sparse.reasoning_tokens = None;
        sparse.output_tokens_per_second = None;
        sparse.decode_tokens_per_sec = None;
        sparse.provider_attempts = None;
        sparse.provider_retry_after_ms = None;
        sparse.provider_retry_delay_ms = None;
        sparse.http_status = None;
        sparse.provider_model = None;
        sparse.finish_reason = None;
        sparse.cost_usd = None;
        sparse.estimated_cost_usd = None;
        set_export_result_target(
            &mut sparse,
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "cloud-sparse"
            }),
        );
        sparse.security_finding_count = None;
        sparse.security_files_scanned = None;

        let report = markdown_report(&[complete, sparse], &[]);

        assert!(report
            .contains("Blank metric cells mean BenchForge did not receive enough source data"));
        assert!(report.contains(
            "| Cost | 1 | 1 | Requires token usage plus configured pricing, or a known-zero local/mock target. |"
        ));
        assert!(report.contains(
            "| Retry delay | 1 | 1 | Recorded when BenchForge waits before retrying a provider call. |"
        ));
        assert!(report.contains("| TTFT | 1 | 1 | Time to first token is available for streaming provider calls; non-streaming calls leave it blank. |"));
        assert!(report.contains("| Prompt tokens | 1 | 1 | Requires provider token usage or a local runtime that reports prompt tokens. |"));
    }

    #[test]
    fn analysis_export_flags_provider_model_identity_warnings() {
        let mut missing = export_result("run-missing", "target-missing", "passed", Some(1.0), None);
        missing.provider_model = None;
        let mut drift_a = export_result("run-drift-a", "target-drift", "passed", Some(1.0), None);
        drift_a.provider_model = Some("served-model-a".into());
        let mut drift_b = export_result("run-drift-b", "target-drift", "passed", Some(1.0), None);
        drift_b.provider_model = Some("served-model-b".into());
        let results = vec![missing, drift_a, drift_b];

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&results).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");
        let warnings = analysis["model_identity_warnings"]
            .as_array()
            .expect("model identity warnings should be an array");
        assert!(warnings
            .iter()
            .any(|warning| warning["issue"] == "provider_model_missing"
                && warning["target_id"] == "target-missing"
                && warning["missing_provider_model_runs"] == 1));
        assert!(warnings
            .iter()
            .any(|warning| warning["issue"] == "provider_model_inconsistent"
                && warning["target_id"] == "target-drift"
                && warning["provider_models"]["served-model-a"] == 1
                && warning["provider_models"]["served-model-b"] == 1));

        let report = markdown_report(&results, &[]);
        assert!(report.contains("## Model Identity Warnings"));
        assert!(report.contains("provider_model_missing"));
        assert!(report.contains("provider_model_inconsistent"));
        assert!(report.contains("served-model-a, served-model-b"));
    }

    #[test]
    fn exports_include_safety_findings_as_first_class_results() {
        let mut security = export_result(
            "run-security",
            "security-worker",
            "failed",
            Some(0.0),
            Some("security_findings"),
        );
        security.benchmark_pack_id = "security-defensive".into();
        security.task_id = "secrets-basic".into();
        security.security_finding_count = Some(2.0);
        security.security_files_scanned = Some(7.0);

        let report = markdown_report(&[security.clone()], &[]);
        assert!(report.contains("## Safety Findings"));
        assert!(report.contains("| run-secu | group-al | security-defensive | secrets-basic | security-worker | failed | 2 | 7 | security_findings | security_findings happened |"));
        assert!(report.contains("| Safety findings | 1 | 0 | Worker security packs report finding counts as first-class result metrics. |"));

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&[security]).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");
        assert_eq!(analysis["summary"]["security_finding_count"], 2.0);
        assert_eq!(analysis["summary"]["security_files_scanned"], 7.0);
        assert_eq!(analysis["safety_findings"][0]["task_id"], "secrets-basic");
        assert_eq!(
            analysis["safety_findings"][0]["error_message"],
            "security_findings happened"
        );
        assert_eq!(analysis["safety_findings"][0]["finding_count"], 2.0);
    }

    #[test]
    fn exports_include_worker_import_provenance() {
        let mut imported =
            export_result("run-imported", "worker-target", "passed", Some(1.0), None);
        imported.import_file_count = Some(2.0);
        imported.import_total_file_count = Some(5.0);
        imported.import_omitted_file_count = Some(3.0);
        imported.import_unsupported_file_count = Some(2.0);
        imported.import_truncated = Some(1.0);
        imported.import_truncated_bytes = Some(4096.0);
        imported.import_format = Some("junit_xml".into());
        imported.import_source = Some("directory".into());
        imported.import_path = Some("/tmp/benchforge/results,latest".into());
        imported.summary_source = Some("junit_xml".into());
        imported.reproducibility["worker_import"] = serde_json::json!({
            "path": "/tmp/benchforge/results,latest",
            "format": "junit_xml",
            "formats": ["junit_xml"],
            "source": "directory",
            "read_files": ["summary.xml", "latest/results.xml"],
            "hash_algorithm": "sha256",
            "file_details": [
                {"path": "summary.xml", "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
                {"path": "latest/results.xml", "read_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
            ],
            "file_count": 2,
            "total_file_count": 5,
            "omitted_file_count": 3,
            "unsupported_file_count": 2,
            "unsupported_files": ["notes.md", "screenshots/chart.png"],
            "truncated": true,
            "truncated_bytes": 4096,
            "summary_source": "junit_xml"
        });

        let jsonl = results_jsonl(&[imported.clone()]);
        let row: serde_json::Value =
            serde_json::from_str(&jsonl).expect("jsonl row should serialize as JSON");
        assert_eq!(row["import_file_count"], 2.0);
        assert_eq!(row["import_total_file_count"], 5.0);
        assert_eq!(row["import_omitted_file_count"], 3.0);
        assert_eq!(row["import_unsupported_file_count"], 2.0);
        assert_eq!(row["import_truncated"], 1.0);
        assert_eq!(row["import_truncated_bytes"], 4096.0);
        assert_eq!(row["import_format"], "junit_xml");
        assert_eq!(row["import_source"], "directory");
        assert_eq!(row["import_path"], "/tmp/benchforge/results,latest");
        assert_eq!(row["summary_source"], "junit_xml");

        let csv = results_csv(&[imported.clone()]);
        assert!(csv.lines().next().unwrap_or_default().contains(
            "import_file_count,import_total_file_count,import_omitted_file_count,import_unsupported_file_count,import_truncated"
        ));
        assert!(csv.contains(
            "2,5,3,2,1,4096,junit_xml,directory,junit_xml,\"/tmp/benchforge/results,latest\",provider-model-a"
        ));

        let report = markdown_report(&[imported.clone()], &[]);
        assert!(report.contains("| Import format | 1 | 0 | Worker harness imports set this when a run was read from external result files. |"));
        assert!(report.contains("| Import unsupported files | 1 | 0 | Counts unsupported side files ignored during worker directory imports. |"));
        assert!(report.contains("| Import truncated | 1 | 0 | Set by worker imports to show whether imported result evidence was truncated or partially bounded. |"));
        assert!(report.contains("| run-impo | group-al | worker-target | OpenAI-compatible | llm-core | llm-core-json-001 | passed | - | - | - | - | junit_xml | directory | 2 | 5 | 3 | 2 | 1 | 4096 | junit_xml |"));
        assert!(report.contains("## Worker Imports"));
        assert!(report.contains("| run-impo | group-al | llm-core | llm-core-json-001 | worker-target | directory | /tmp/benchforge/results,latest | junit_xml | read 2; total 5; omitted 3; unsupported 2 | summary.xml, latest/results.xml | summary.xml sha256:aaaaaaaaaaaa, latest/results.xml read-sha256:bbbbbbbbbbbb | yes (4096 bytes) | junit_xml |"));

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&[imported]).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");
        let imports = analysis["worker_imports"]
            .as_array()
            .expect("worker imports should be an array");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0]["run_id"], "run-imported");
        assert_eq!(imports[0]["source"], "directory");
        assert_eq!(imports[0]["path"], "/tmp/benchforge/results,latest");
        assert_eq!(imports[0]["formats"][0], "junit_xml");
        assert_eq!(imports[0]["file_count"], 2.0);
        assert_eq!(imports[0]["total_file_count"], 5.0);
        assert_eq!(imports[0]["omitted_file_count"], 3.0);
        assert_eq!(imports[0]["unsupported_file_count"], 2.0);
        assert_eq!(imports[0]["unsupported_files"][0], "notes.md");
        assert_eq!(imports[0]["truncated"], true);
        assert_eq!(imports[0]["truncated_metric"], 1.0);
        assert_eq!(imports[0]["truncated_bytes"], 4096.0);
        assert_eq!(imports[0]["summary_source"], "junit_xml");
        assert_eq!(
            imports[0]["worker_import"]["unsupported_files"][1],
            "screenshots/chart.png"
        );
        let coverage = analysis["metric_coverage"]
            .as_array()
            .expect("metric coverage should be an array");
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Summary parser" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Import omitted files" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Import unsupported files"
                && row["present"] == 1
                && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Import truncated bytes" && row["present"] == 1 && row["missing"] == 0
        }));
    }

    #[test]
    fn exports_include_process_execution_metrics() {
        let mut process = export_result(
            "run-process",
            "code-target",
            "failed",
            Some(0.0),
            Some("test_failed"),
        );
        process.setup_time_ms = Some(111.0);
        process.target_time_ms = Some(222.0);
        process.evaluation_time_ms = Some(432.0);
        process.model_call_wall_time_ms = Some(876.0);
        process.peak_rss_mb = Some(64.5);
        process.exit_code = Some(1.0);
        process.harness_exit_code = Some(2.0);
        process.stdout_bytes = Some(2048.0);
        process.stderr_bytes = Some(512.0);
        process.files_changed = Some(1.0);
        process.lines_added = Some(2.0);
        process.lines_deleted = Some(3.0);
        process.commands_observed_count = Some(2.0);
        process.dangerous_command_hits = Some(4.0);

        let jsonl = results_jsonl(&[process.clone()]);
        let row: serde_json::Value =
            serde_json::from_str(&jsonl).expect("jsonl row should serialize as JSON");
        assert_eq!(row["pass_fail"], false);
        assert_eq!(row["score_numeric"], 0.0);
        assert_eq!(row["setup_time_ms"], 111.0);
        assert_eq!(row["target_time_ms"], 222.0);
        assert_eq!(row["evaluation_time_ms"], 432.0);
        assert_eq!(row["model_call_wall_time_ms"], 876.0);
        assert_eq!(row["input_tokens"], 100.0);
        assert_eq!(row["output_tokens"], 25.0);
        assert_eq!(row["estimated_cost_usd"], 0.001);
        assert_eq!(row["ttft_ms"], 350.0);
        assert_eq!(row["decode_tokens_per_sec"], 25.0);
        assert_eq!(row["peak_rss_mb"], 64.5);
        assert_eq!(row["exit_code"], 1.0);
        assert_eq!(row["harness_exit_code"], 2.0);
        assert_eq!(row["stdout_bytes"], 2048.0);
        assert_eq!(row["stderr_bytes"], 512.0);
        assert_eq!(row["files_changed"], 1.0);
        assert_eq!(row["lines_added"], 2.0);
        assert_eq!(row["lines_deleted"], 3.0);
        assert_eq!(row["commands_observed_count"], 2.0);
        assert_eq!(row["dangerous_command_hits"], 4.0);

        let csv = results_csv(&[process.clone()]);
        assert!(csv.lines().next().unwrap_or_default().contains(
            "wall_time_ms,setup_time_ms,target_time_ms,evaluation_time_ms,model_call_wall_time_ms,provider_time_to_first_byte_ms"
        ));
        assert!(csv.lines().next().unwrap_or_default().contains(
            "output_tokens_per_second,decode_tokens_per_sec,peak_rss_mb,exit_code,harness_exit_code,stdout_bytes,stderr_bytes,files_changed,lines_added,lines_deleted,commands_observed_count,dangerous_command_hits"
        ));
        assert!(csv.contains(",1000,111,222,432,876,150,350,350,900,"));
        assert!(csv.contains(",25,25,64.5,1,2,2048,512,1,2,3,2,4,"));

        let report = markdown_report(&[process.clone()], &[]);
        assert!(
            report.contains("| pass_fail | 1 | 0 | Required v1 alias derived from run status. |")
        );
        assert!(report.contains("| score_numeric | 1 | 0 | Required v1 alias for score. |"));
        assert!(report
            .contains("| input_tokens | 1 | 0 | Required v1 alias for prompt/input tokens. |"));
        assert!(report.contains(
            "| output_tokens | 1 | 0 | Required v1 alias for completion/output tokens. |"
        ));
        assert!(report.contains(
            "| estimated_cost_usd | 1 | 0 | Required v1 alias for estimated benchmark cost. |"
        ));
        assert!(report.contains("| ttft_ms | 1 | 0 | Required v1 alias for time to first token. |"));
        assert!(report.contains(
            "| decode_tokens_per_sec | 1 | 0 | Required v1 alias for output token throughput. |"
        ));
        assert!(report.contains("| Setup time | 1 | 0 | Prompt and repo/code tasks report app/workspace setup time before target execution. |"));
        assert!(report.contains("| Target time | 1 | 0 | Prompt and repo/code tasks report time spent invoking the benchmark target before evaluation. |"));
        assert!(report.contains("| Evaluation time | 1 | 0 | Scoring and repo/code tasks report time spent in the evaluation command after target execution. |"));
        assert!(report.contains("| Commands observed | 1 | 0 | Process-backed repo/code and worker harness runs report benchmark commands BenchForge observed or executed. |"));
        assert!(report.contains("| Peak RSS | 1 | 0 | Process-backed runs report peak resident memory only when BenchForge or a worker can observe it. |"));
        let process_run_line = report
            .lines()
            .find(|line| line.contains("| run-proc |"))
            .expect("markdown report should include process run row");
        assert!(process_run_line.contains("| run-proc | group-al | code-target | OpenAI-compatible | llm-core | llm-core-json-001 | failed | test_failed | test_failed happened |"));
        assert!(process_run_line
            .contains("| 0 | 1000 ms | 111 ms | 222 ms | 432 ms | 876 ms | 1 | 2 | 2048 | 512 | 1 | 2 | 3 | 2 | 4 |"));

        let analysis: serde_json::Value = serde_json::from_str(
            &results_analysis_json(&[process]).expect("analysis export should serialize"),
        )
        .expect("analysis export should be valid JSON");
        let coverage = analysis["metric_coverage"]
            .as_array()
            .expect("metric coverage should be an array");
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Harness exit code" && row["present"] == 1 && row["missing"] == 0
        }));
        for metric in [
            "pass_fail",
            "score_numeric",
            "input_tokens",
            "output_tokens",
            "estimated_cost_usd",
            "ttft_ms",
            "decode_tokens_per_sec",
        ] {
            assert!(
                coverage.iter().any(|row| row["metric"] == metric
                    && row["present"] == 1
                    && row["missing"] == 0),
                "metric coverage should include {metric}"
            );
        }
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Setup time" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Target time" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Stderr bytes" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Files changed" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Lines added" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Lines deleted" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Commands observed" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Dangerous command hits" && row["present"] == 1 && row["missing"] == 0
        }));
        assert!(coverage.iter().any(|row| {
            row["metric"] == "Peak RSS" && row["present"] == 1 && row["missing"] == 0
        }));
    }

    #[test]
    fn csv_export_includes_provider_http_status() {
        let csv = results_csv(&[export_result(
            "run-a",
            "target-one",
            "passed",
            Some(1.0),
            None,
        )]);
        assert!(csv.lines().next().unwrap_or_default().contains(
            "http_status,output_tokens_per_second,decode_tokens_per_sec,peak_rss_mb,exit_code"
        ));
        assert!(csv
            .lines()
            .next()
            .unwrap_or_default()
            .contains("pass_fail,score,score_numeric,wall_time_ms"));
        assert!(csv
            .lines()
            .next()
            .unwrap_or_default()
            .contains("prompt_tokens,input_tokens,completion_tokens,output_tokens,reasoning_tokens,cached_tokens,cache_read_tokens,cache_write_tokens,total_tokens"));
        assert!(csv
            .lines()
            .next()
            .unwrap_or_default()
            .contains("finish_reason,pricing_assumption,cost_usd,estimated_cost_usd,started_at"));
        assert!(csv.lines().next().unwrap_or_default().contains(
            "stdout_bytes,stderr_bytes,files_changed,lines_added,lines_deleted,commands_observed_count,dangerous_command_hits,security_finding_count,security_files_scanned,import_file_count"
        ));
        assert!(csv.lines().next().unwrap_or_default().contains(
            "import_total_file_count,import_omitted_file_count,import_unsupported_file_count,import_truncated,import_truncated_bytes"
        ));
        assert!(csv.contains("target-one,OpenAI-compatible,llm-core"));
        assert!(csv.contains(",150,350,350,900,100,100,25,25,3,12,12,4,125,1,0,0,200,25,25,"));
        assert!(csv.contains("provider-model-a,provider,stop,,0.001,0.001,"));
    }

    #[test]
    fn run_estimate_helpers_read_prices_and_bounds() {
        let config = serde_json::json!({
            "max_tokens": 512,
            "timeout_seconds": 90,
            "input_price_usd_per_million_tokens": 0.25,
            "output_price_usd_per_million_tokens": 2.0
        });
        assert_eq!(estimate_tokens("benchforge"), 3);
        assert_eq!(configured_max_tokens(&config), Some(512));
        assert_eq!(configured_timeout_seconds(&config), 90);
        assert_eq!(configured_timeout_seconds(&serde_json::json!({})), 120);
        assert_eq!(div_ceil_u64(601, 4), 151);
        assert_eq!(
            price_per_million(&config, "input_price_usd_per_million_tokens"),
            Some(0.25)
        );
        assert_eq!(
            price_per_million(&config, "output_price_usd_per_million_tokens"),
            Some(2.0)
        );
        assert_eq!(conservative_prompt_price_per_million(&config, 0.25), 0.25);
        let cache_priced = serde_json::json!({
            "input_price_usd_per_million_tokens": 0.25,
            "cache_read_price_usd_per_million_tokens": 0.05,
            "cache_write_price_usd_per_million_tokens": 1.25
        });
        assert_eq!(
            conservative_prompt_price_per_million(&cache_priced, 0.25),
            1.25
        );
    }

    #[test]
    fn run_estimate_uses_cache_write_price_for_conservative_cost() {
        let conn = store::open_memory().expect("store should open");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "cache-priced-local".into(),
                name: "Cache Priced Local".into(),
                kind: "direct_model".into(),
                adapter_id: "openai-compatible".into(),
                config: serde_json::json!({
                    "model": "local",
                    "base_url": "http://127.0.0.1:8080/v1",
                    "max_tokens": 1,
                    "input_price_usd_per_million_tokens": 1.0,
                    "output_price_usd_per_million_tokens": 0.0,
                    "cache_write_price_usd_per_million_tokens": 10.0
                }),
            },
        )
        .expect("target should save");

        let estimate = estimate_run_plan_for_conn(
            &conn,
            &RunEstimateRequest {
                target_ids: vec!["cache-priced-local".into()],
                benchmark_pack_id: "llm-connectivity".into(),
                task_ids: vec![],
                repetitions: 1,
                warmup_runs: 0,
                concurrency: 1,
            },
        )
        .expect("estimate should work");

        assert_eq!(
            estimate.estimated_max_cost_usd,
            Some(estimate.estimated_prompt_tokens as f64 * 10.0 / 1_000_000.0)
        );
    }

    #[test]
    fn run_estimate_warns_when_prompt_repetitions_are_low() {
        let conn = store::open_memory().expect("store should open");
        let low = estimate_run_plan_for_conn(
            &conn,
            &RunEstimateRequest {
                target_ids: vec!["mock-agent".into()],
                benchmark_pack_id: "llm-basics".into(),
                task_ids: vec![],
                repetitions: 1,
                warmup_runs: 0,
                concurrency: 1,
            },
        )
        .expect("estimate should be available");
        assert_eq!(
            low.estimated_max_completion_tokens,
            low.task_count as u64 * PROMPT_DEFAULT_MAX_TOKENS
        );
        assert!(
            low.notes
                .iter()
                .any(|note| note.contains("Confidence warning")
                    && note.contains("local/cloud models"))
        );

        let repeated = estimate_run_plan_for_conn(
            &conn,
            &RunEstimateRequest {
                target_ids: vec!["mock-agent".into()],
                benchmark_pack_id: "llm-basics".into(),
                task_ids: vec![],
                repetitions: 3,
                warmup_runs: 0,
                concurrency: 1,
            },
        )
        .expect("estimate should be available");
        assert!(!repeated
            .notes
            .iter()
            .any(|note| note.contains("Confidence warning")));
    }

    #[test]
    fn run_estimate_counts_selected_task_subset() {
        let conn = store::open_memory().expect("store should open");
        let estimate = estimate_run_plan_for_conn(
            &conn,
            &RunEstimateRequest {
                target_ids: vec!["mock-agent".into()],
                benchmark_pack_id: "llm-basics".into(),
                task_ids: vec!["llm-json-validity-001".into()],
                repetitions: 2,
                warmup_runs: 1,
                concurrency: 2,
            },
        )
        .expect("subset estimate should work");

        assert_eq!(estimate.task_count, 1);
        assert_eq!(estimate.measured_runs, 2);
        assert_eq!(estimate.warmup_calls, 1);
        assert_eq!(estimate.total_model_calls, 3);

        let err = estimate_run_plan_for_conn(
            &conn,
            &RunEstimateRequest {
                target_ids: vec!["mock-agent".into()],
                benchmark_pack_id: "llm-basics".into(),
                task_ids: vec!["missing-task".into()],
                repetitions: 1,
                warmup_runs: 0,
                concurrency: 1,
            },
        )
        .expect_err("missing task should fail estimate");
        assert!(err.starts_with("task_filter_invalid"));
    }

    #[test]
    fn run_estimate_ignores_warmups_for_non_model_targets() {
        let conn = store::open_memory().expect("store should open");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "security-worker".into(),
                name: "Security Worker".into(),
                kind: "benchmark_harness".into(),
                adapter_id: "benchforge-worker".into(),
                config: serde_json::json!({}),
            },
        )
        .expect("target should save");

        let estimate = estimate_run_plan_for_conn(
            &conn,
            &RunEstimateRequest {
                target_ids: vec!["security-worker".into()],
                benchmark_pack_id: "security-defensive".into(),
                task_ids: vec![],
                repetitions: 1,
                warmup_runs: 5,
                concurrency: 1,
            },
        )
        .expect("harness estimate should work");

        assert!(estimate.task_count > 0);
        assert_eq!(estimate.measured_runs, estimate.task_count);
        assert_eq!(estimate.warmup_calls, 0);
        assert_eq!(estimate.total_model_calls, estimate.measured_runs);
        assert_eq!(estimate.estimated_warmup_timeout_seconds, 0);
    }

    #[test]
    fn run_cost_limit_honors_selected_task_subset() {
        let conn = store::open_memory().expect("store should open");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "subset-priced-local".into(),
                name: "Subset Priced Local".into(),
                kind: "direct_model".into(),
                adapter_id: "openai-compatible".into(),
                config: serde_json::json!({
                    "model": "local",
                    "base_url": "http://127.0.0.1:8080/v1",
                    "max_tokens": 512,
                    "input_price_usd_per_million_tokens": 100.0,
                    "output_price_usd_per_million_tokens": 100.0
                }),
            },
        )
        .expect("target should save");

        let subset_task_ids = vec!["llm-json-validity-001".to_string()];
        let subset_estimate = estimate_run_plan_for_conn(
            &conn,
            &RunEstimateRequest {
                target_ids: vec!["subset-priced-local".into()],
                benchmark_pack_id: "llm-basics".into(),
                task_ids: subset_task_ids.clone(),
                repetitions: 1,
                warmup_runs: 0,
                concurrency: 1,
            },
        )
        .expect("subset estimate should work")
        .estimated_max_cost_usd
        .expect("subset estimate should be priced");
        let full_estimate = estimate_run_plan_for_conn(
            &conn,
            &RunEstimateRequest {
                target_ids: vec!["subset-priced-local".into()],
                benchmark_pack_id: "llm-basics".into(),
                task_ids: vec![],
                repetitions: 1,
                warmup_runs: 0,
                concurrency: 1,
            },
        )
        .expect("full estimate should work")
        .estimated_max_cost_usd
        .expect("full estimate should be priced");
        assert!(
            full_estimate > subset_estimate,
            "full pack estimate should exceed selected-task estimate"
        );

        let cap_between_subset_and_full = (subset_estimate + full_estimate) / 2.0;
        enforce_run_cost_limit(
            &conn,
            &runner::RunQuickSmokeRequest {
                target_ids: vec!["subset-priced-local".into()],
                benchmark_pack_id: "llm-basics".into(),
                task_ids: subset_task_ids,
                repetitions: 1,
                docker: false,
                warmup_runs: 0,
                concurrency: 1,
                max_cost_usd: Some(cap_between_subset_and_full),
                run_group_id: None,
            },
        )
        .expect("selected-task capped run should use selected-task estimate");
    }

    #[test]
    fn run_cost_limit_blocks_over_budget_and_unpriced_targets() {
        let conn = store::open_memory().expect("store should open");
        let remote_key_env: &'static str = Box::leak(
            format!(
                "BENCHFORGE_TEST_REMOTE_COMPATIBLE_KEY_{}",
                uuid::Uuid::new_v4()
            )
            .replace('-', "_")
            .into_boxed_str(),
        );
        let _remote_key = ScopedEnvVar::set(remote_key_env, "benchforge-test-key");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "priced-local".into(),
                name: "Priced Local".into(),
                kind: "direct_model".into(),
                adapter_id: "openai-compatible".into(),
                config: serde_json::json!({
                    "model": "local",
                    "base_url": "http://127.0.0.1:8080/v1",
                    "max_tokens": 512,
                    "timeout_seconds": 30,
                    "input_price_usd_per_million_tokens": 1_000.0,
                    "output_price_usd_per_million_tokens": 1_000.0
                }),
            },
        )
        .expect("target should save");

        let over_budget = runner::RunQuickSmokeRequest {
            target_ids: vec!["priced-local".into()],
            benchmark_pack_id: "llm-basics".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: Some(0.000001),
            run_group_id: None,
        };
        assert!(enforce_run_cost_limit(&conn, &over_budget)
            .expect_err("over-budget request should fail")
            .starts_with("max_cost_exceeded"));

        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "unpriced-remote".into(),
                name: "Unpriced Remote".into(),
                kind: "direct_model".into(),
                adapter_id: "openai-compatible".into(),
                config: serde_json::json!({
                    "model": "remote",
                    "base_url": "https://example.com/v1",
                    "api_key_env": remote_key_env
                }),
            },
        )
        .expect("target should save");
        let unpriced = runner::RunQuickSmokeRequest {
            target_ids: vec!["unpriced-remote".into()],
            benchmark_pack_id: "llm-basics".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: Some(1.0),
            run_group_id: None,
        };
        assert!(enforce_run_cost_limit(&conn, &unpriced)
            .expect_err("unpriced capped request should fail")
            .starts_with("max_cost_unpriced"));
    }

    #[test]
    fn run_estimate_hides_partial_cost_when_remote_targets_are_unpriced() {
        let conn = store::open_memory().expect("store should open");
        let remote_key_env: &'static str = Box::leak(
            format!(
                "BENCHFORGE_TEST_REMOTE_COMPATIBLE_KEY_{}",
                uuid::Uuid::new_v4()
            )
            .replace('-', "_")
            .into_boxed_str(),
        );
        let _remote_key = ScopedEnvVar::set(remote_key_env, "benchforge-test-key");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "priced-remote".into(),
                name: "Priced Remote".into(),
                kind: "direct_model".into(),
                adapter_id: "openai-compatible".into(),
                config: serde_json::json!({
                    "model": "priced",
                    "base_url": "https://priced.example.com/v1",
                    "api_key_env": remote_key_env,
                    "max_tokens": 512,
                    "input_price_usd_per_million_tokens": 0.25,
                    "output_price_usd_per_million_tokens": 2.0
                }),
            },
        )
        .expect("priced target should save");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "unpriced-remote".into(),
                name: "Unpriced Remote".into(),
                kind: "direct_model".into(),
                adapter_id: "openai-compatible".into(),
                config: serde_json::json!({
                    "model": "unpriced",
                    "base_url": "https://unpriced.example.com/v1",
                    "api_key_env": remote_key_env
                }),
            },
        )
        .expect("unpriced target should save");

        let estimate = estimate_run_plan_for_conn(
            &conn,
            &RunEstimateRequest {
                target_ids: vec!["priced-remote".into(), "unpriced-remote".into()],
                benchmark_pack_id: "llm-basics".into(),
                task_ids: vec![],
                repetitions: 1,
                warmup_runs: 0,
                concurrency: 1,
            },
        )
        .expect("estimate should work");

        assert_eq!(estimate.priced_targets, 1);
        assert_eq!(estimate.unpriced_targets, vec!["unpriced-remote"]);
        assert!(estimate.estimated_max_cost_usd.is_none());
        assert!(estimate
            .notes
            .iter()
            .any(|note| note.contains("Missing pricing")));
    }

    #[test]
    fn run_estimate_treats_unpriced_local_targets_as_zero_cost() {
        let conn = store::open_memory().expect("store should open");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "manual-local".into(),
                name: "Manual Local".into(),
                kind: "direct_model".into(),
                adapter_id: "openai-compatible".into(),
                config: serde_json::json!({
                    "model": "local",
                    "base_url": "http://127.0.0.1:8080/v1"
                }),
            },
        )
        .expect("target should save");
        let request = RunEstimateRequest {
            target_ids: vec!["manual-local".into()],
            benchmark_pack_id: "llm-basics".into(),
            task_ids: vec![],
            repetitions: 1,
            warmup_runs: 0,
            concurrency: 1,
        };

        let estimate = estimate_run_plan_for_conn(&conn, &request).expect("estimate should work");

        assert_eq!(estimate.estimated_max_cost_usd, Some(0.0));
        assert_eq!(estimate.priced_targets, 1);
        assert!(estimate.unpriced_targets.is_empty());
        assert!(estimate
            .notes
            .iter()
            .any(|note| note.contains("Assumed $0 cost")));

        let capped = runner::RunQuickSmokeRequest {
            target_ids: vec!["manual-local".into()],
            benchmark_pack_id: "llm-basics".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: Some(0.0),
            run_group_id: None,
        };
        enforce_run_cost_limit(&conn, &capped).expect("zero-cost local target should pass cap");
    }

    #[test]
    fn target_request_validation_accepts_local_openai_target() {
        let request = target_request(
            "hf-local-qwen",
            "HF Local Qwen",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "model": "local-huggingface",
                "base_url": "http://127.0.0.1:8080/v1",
                "temperature": 0,
                "top_p": 1,
                "max_tokens": 512,
                "timeout_seconds": 120,
                "retry_count": 1,
                "port": 8080,
                "context": 2048,
                "input_price_usd_per_million_tokens": 0,
                "output_price_usd_per_million_tokens": 0
            }),
        );

        assert!(validate_create_target_request(&request).is_ok());
    }

    #[test]
    fn target_request_validation_rejects_bad_shape_and_urls() {
        let bad_id = target_request(
            "bad/id",
            "Bad",
            "direct_model",
            "openai-compatible",
            serde_json::json!({"model": "x", "base_url": "http://localhost:8000/v1"}),
        );
        assert!(validate_create_target_request(&bad_id)
            .unwrap_err()
            .contains("id must"));

        let missing_base_url = target_request(
            "generic",
            "Generic",
            "direct_model",
            "openai-compatible",
            serde_json::json!({"model": "x"}),
        );
        assert!(validate_create_target_request(&missing_base_url)
            .unwrap_err()
            .contains("base_url is required"));

        let endpoint_url = target_request(
            "endpoint-url",
            "Endpoint URL",
            "direct_model",
            "openai-compatible",
            serde_json::json!({
                "model": "x",
                "base_url": "http://localhost:8000/v1/chat/completions"
            }),
        );
        assert!(validate_create_target_request(&endpoint_url)
            .unwrap_err()
            .contains("provider root"));
    }

    #[test]
    fn target_request_validation_rejects_raw_secrets_and_bad_numbers() {
        let raw_key = target_request(
            "raw-key",
            "Raw Key",
            "direct_model",
            "openai",
            serde_json::json!({
                "model": "gpt-4.1-mini",
                "api_key": "sk-secret"
            }),
        );
        assert!(validate_create_target_request(&raw_key)
            .unwrap_err()
            .contains("raw secret"));

        let raw_authorization = target_request(
            "raw-authorization",
            "Raw Authorization",
            "benchmark_harness",
            "benchforge-worker",
            serde_json::json!({
                "harness": {
                    "command": ["python3", "-m", "private_harness"],
                    "env": {"authorization": "Bearer secret"}
                }
            }),
        );
        assert!(validate_create_target_request(&raw_authorization)
            .unwrap_err()
            .contains("$.config.harness.env.authorization"));

        let raw_private_key = target_request(
            "raw-private-key",
            "Raw Private Key",
            "benchmark_harness",
            "benchforge-worker",
            serde_json::json!({
                "harness": {
                    "command": ["python3", "-m", "private_harness"],
                    "env": {"service_private_key": "-----BEGIN PRIVATE KEY-----"}
                }
            }),
        );
        assert!(validate_create_target_request(&raw_private_key)
            .unwrap_err()
            .contains("$.config.harness.env.service_private_key"));

        let bad_temperature = target_request(
            "bad-temperature",
            "Bad Temperature",
            "direct_model",
            "openai",
            serde_json::json!({
                "model": "gpt-4.1-mini",
                "temperature": 9
            }),
        );
        assert!(validate_create_target_request(&bad_temperature)
            .unwrap_err()
            .contains("temperature"));

        let partial_pricing = target_request(
            "partial-pricing",
            "Partial Pricing",
            "direct_model",
            "openai",
            serde_json::json!({
                "model": "gpt-4.1-mini",
                "input_price_usd_per_million_tokens": 0.25
            }),
        );
        assert!(validate_create_target_request(&partial_pricing)
            .unwrap_err()
            .contains("input and output pricing"));

        let invalid_api_key_env = target_request(
            "invalid-api-key-env",
            "Invalid API Key Env",
            "direct_model",
            "openai",
            serde_json::json!({
                "model": "gpt-4.1-mini",
                "api_key_env": "not a valid env name"
            }),
        );
        assert!(validate_create_target_request(&invalid_api_key_env)
            .unwrap_err()
            .contains("api_key_env"));

        let non_string_api_key_env = target_request(
            "non-string-api-key-env",
            "Non String API Key Env",
            "direct_model",
            "openai",
            serde_json::json!({
                "model": "gpt-4.1-mini",
                "api_key_env": 123
            }),
        );
        assert!(validate_create_target_request(&non_string_api_key_env)
            .unwrap_err()
            .contains("api_key_env must be a string"));

        let negative_cache_pricing = target_request(
            "negative-cache-pricing",
            "Negative Cache Pricing",
            "direct_model",
            "openai",
            serde_json::json!({
                "model": "gpt-4.1-mini",
                "input_price_usd_per_million_tokens": 0.25,
                "output_price_usd_per_million_tokens": 2.0,
                "cache_read_price_usd_per_million_tokens": -0.01
            }),
        );
        assert!(validate_create_target_request(&negative_cache_pricing)
            .unwrap_err()
            .contains("cache_read_price_usd_per_million_tokens"));

        let cache_pricing_without_base_pricing = target_request(
            "cache-pricing-only",
            "Cache Pricing Only",
            "direct_model",
            "openai",
            serde_json::json!({
                "model": "gpt-4.1-mini",
                "cache_read_price_usd_per_million_tokens": 0.05
            }),
        );
        assert!(
            validate_create_target_request(&cache_pricing_without_base_pricing)
                .unwrap_err()
                .contains("cache pricing requires input and output pricing")
        );
    }

    #[test]
    fn editing_target_preserves_redacted_api_key_references() {
        let conn = store::open_memory().unwrap();
        persist_target_request(
            &conn,
            target_request(
                "remote-compatible",
                "Remote Compatible",
                "direct_model",
                "openai-compatible",
                serde_json::json!({
                    "model": "old-model",
                    "base_url": "https://example.com/v1",
                    "api_key_keychain": "openai-compatible-https-example-com-v1",
                    "api_key_env": "EXAMPLE_API_KEY"
                }),
            ),
        )
        .expect("initial target should save");

        persist_target_request(
            &conn,
            target_request(
                "remote-compatible",
                "Remote Compatible Edited",
                "direct_model",
                "openai-compatible",
                serde_json::json!({
                    "model": "new-model",
                    "base_url": "https://example.com/v1",
                    "api_key_keychain": "[REDACTED]",
                    "api_key_env": "[REDACTED]"
                }),
            ),
        )
        .expect("edited target should preserve stored API key references");

        let stored = store::get_target(&conn, "remote-compatible")
            .unwrap()
            .expect("target should exist");
        let config: serde_json::Value = serde_json::from_str(&stored.config_json).unwrap();
        assert_eq!(config["model"], "new-model");
        assert_eq!(
            config["api_key_keychain"],
            "openai-compatible-https-example-com-v1"
        );
        assert_eq!(config["api_key_env"], "EXAMPLE_API_KEY");

        let err = persist_target_request(
            &conn,
            target_request(
                "new-redacted",
                "New Redacted",
                "direct_model",
                "openai-compatible",
                serde_json::json!({
                    "model": "new-model",
                    "base_url": "https://example.com/v1",
                    "api_key_keychain": "[REDACTED]"
                }),
            ),
        )
        .unwrap_err();
        assert!(err.contains("existing target"), "{err}");
    }

    #[test]
    fn duplicate_target_copies_safe_config_and_clears_validation() {
        let conn = store::open_memory().unwrap();
        persist_target_request(
            &conn,
            target_request(
                "remote-compatible",
                "Remote Compatible",
                "direct_model",
                "openai-compatible",
                serde_json::json!({
                    "model": "old-model",
                    "base_url": "https://example.com/v1",
                    "api_key_keychain": "openai-compatible-https-example-com-v1",
                    "api_key_env": "EXAMPLE_API_KEY",
                    "input_price_usd_per_million_tokens": 0.25,
                    "output_price_usd_per_million_tokens": 2.0
                }),
            ),
        )
        .expect("source target should save");
        store::set_target_validation(
            &conn,
            "remote-compatible",
            "error",
            "model_not_found: old validation",
            "2026-07-09T00:00:00Z",
        )
        .expect("validation should save");

        let duplicated =
            duplicate_target_for_conn(&conn, "remote-compatible").expect("target should clone");

        assert_eq!(duplicated.id, "remote-compatible-copy");
        assert_eq!(duplicated.name, "Remote Compatible Copy");
        assert_eq!(duplicated.model.as_deref(), Some("old-model"));
        assert_eq!(duplicated.validation_status, None);
        assert_eq!(duplicated.validation_detail, None);
        assert_eq!(duplicated.validation_checked_at, None);
        assert_eq!(duplicated.input_price_usd_per_million_tokens, Some(0.25));
        assert_eq!(duplicated.output_price_usd_per_million_tokens, Some(2.0));

        let stored = store::get_target(&conn, "remote-compatible-copy")
            .unwrap()
            .expect("duplicate should exist");
        let config: serde_json::Value = serde_json::from_str(&stored.config_json).unwrap();
        assert_eq!(
            config["api_key_keychain"],
            "openai-compatible-https-example-com-v1"
        );
        assert_eq!(config["api_key_env"], "EXAMPLE_API_KEY");

        let next_duplicate = duplicate_target_for_conn(&conn, "remote-compatible")
            .expect("second duplicate should use a unique id");
        assert_eq!(next_duplicate.id, "remote-compatible-copy-2");

        let source = store::get_target(&conn, "remote-compatible")
            .unwrap()
            .expect("source should still exist");
        assert_eq!(source.validation_status.as_deref(), Some("error"));
    }

    #[test]
    fn provider_api_key_status_reports_keychain_source_without_secret_value() {
        let status = provider_api_key_status_for_with(
            "openai",
            Some("OPENAI_API_KEY".into()),
            &|provider| provider == "openai",
            &|name| (name == "OPENAI_API_KEY").then(|| "sk-test-secret".into()),
        );

        assert!(status.available);
        assert_eq!(status.source, "keychain");
        assert_eq!(status.env_var.as_deref(), Some("OPENAI_API_KEY"));
        assert!(status.detail.contains("Keychain"));
        assert!(!status.detail.contains("sk-test-secret"));
    }

    #[test]
    fn provider_api_key_status_reports_environment_source_without_secret_value() {
        let status = provider_api_key_status_for_with(
            "openai",
            Some("OPENAI_API_KEY".into()),
            &|_| false,
            &|name| (name == "OPENAI_API_KEY").then(|| "sk-test-secret".into()),
        );

        assert!(status.available);
        assert_eq!(status.source, "environment");
        assert_eq!(status.env_var.as_deref(), Some("OPENAI_API_KEY"));
        assert!(status.detail.contains("OPENAI_API_KEY"));
        assert!(!status.detail.contains("sk-test-secret"));
    }

    #[test]
    fn provider_api_key_status_reports_missing_key_repair_hint() {
        let status = provider_api_key_status_for_with(
            "openai",
            Some("OPENAI_API_KEY".into()),
            &|_| false,
            &|_| None,
        );

        assert!(!status.available);
        assert_eq!(status.source, "missing");
        assert_eq!(status.env_var.as_deref(), Some("OPENAI_API_KEY"));
        assert!(status.detail.contains("Keychain"));
        assert!(status.detail.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn model_target_persistence_adds_adapter_secret_env_fallback() {
        let conn = store::open_memory().unwrap();
        let target = persist_target_request(
            &conn,
            target_request(
                "openai-mini",
                "OpenAI Mini",
                "direct_model",
                "openai",
                serde_json::json!({
                    "model": "gpt-4.1-mini",
                    "api_key_keychain": "openai"
                }),
            ),
        )
        .expect("target should save");

        assert_eq!(target.config["api_key_env"], "OPENAI_API_KEY");
        assert_eq!(target.config["api_key_keychain"], "openai");
    }

    #[test]
    fn model_target_persistence_preserves_custom_secret_env_fallback() {
        let conn = store::open_memory().unwrap();
        let target = persist_target_request(
            &conn,
            target_request(
                "openai-custom-key",
                "OpenAI Custom Key",
                "direct_model",
                "openai",
                serde_json::json!({
                    "model": "gpt-4.1-mini",
                    "api_key_keychain": "openai",
                    "api_key_env": "BENCHFORGE_CUSTOM_OPENAI_KEY"
                }),
            ),
        )
        .expect("target should save");

        assert_eq!(target.config["api_key_env"], "BENCHFORGE_CUSTOM_OPENAI_KEY");
    }

    #[test]
    fn model_target_persistence_leaves_generic_compatible_env_unset() {
        let conn = store::open_memory().unwrap();
        let target = persist_target_request(
            &conn,
            target_request(
                "compatible-local",
                "Compatible Local",
                "direct_model",
                "openai-compatible",
                serde_json::json!({
                    "model": "local-model",
                    "base_url": "http://127.0.0.1:8080/v1"
                }),
            ),
        )
        .expect("target should save");

        assert!(target.config.get("api_key_env").is_none());
    }

    #[test]
    fn target_request_validation_checks_benchmark_harness_config() {
        let valid = target_request(
            "evalplus-worker",
            "EvalPlus Worker",
            "benchmark_harness",
            "benchforge-worker",
            serde_json::json!({
                "harness": {
                    "command": ["python3", "-m", "evalplus.evaluate", "--samples", "{workspace}/samples.jsonl"],
                    "env": {"BENCHFORGE_MODE": "contract"},
                    "env_passthrough": ["OPENAI_API_KEY", "HF_TOKEN"],
                    "timeout_seconds": 7200
                }
            }),
        );
        assert!(validate_create_target_request(&valid).is_ok());

        let blank_command = target_request(
            "blank-harness",
            "Blank Harness",
            "benchmark_harness",
            "benchforge-worker",
            serde_json::json!({"harness": {"command": "   "}}),
        );
        assert!(validate_create_target_request(&blank_command)
            .unwrap_err()
            .contains("harness.command"));

        let bad_timeout = target_request(
            "bad-timeout",
            "Bad Timeout",
            "benchmark_harness",
            "benchforge-worker",
            serde_json::json!({"harness": {"command": ["python3"], "timeout_seconds": 0}}),
        );
        assert!(validate_create_target_request(&bad_timeout)
            .unwrap_err()
            .contains("timeout_seconds"));

        let bad_passthrough = target_request(
            "bad-passthrough",
            "Bad Passthrough",
            "benchmark_harness",
            "benchforge-worker",
            serde_json::json!({
                "harness": {
                    "command": ["python3"],
                    "env_passthrough": ["OPENAI_API_KEY", "bad-name"]
                }
            }),
        );
        assert!(validate_create_target_request(&bad_passthrough)
            .unwrap_err()
            .contains("env_passthrough"));
    }

    #[test]
    fn target_validation_reports_benchmark_harness_readiness() {
        let default_worker = target_record(
            "benchforge-worker",
            "BenchForge Worker",
            "benchmark_harness",
            "benchforge-worker",
            serde_json::json!({"command": "benchforge-worker"}),
        );

        let validation = validate_target_record(&default_worker).expect("validation should run");

        assert_eq!(validation.status, "warn");
        assert!(validation.detail.contains("BenchForge Worker"));
        assert!(validation.detail.contains("harness.command"));
    }

    #[test]
    fn target_validation_reports_missing_external_harness_tool() {
        let missing_command =
            format!("benchforge-missing-tool-{}", uuid::Uuid::new_v4()).replace('-', "");
        let target = target_record(
            "evalplus-worker",
            "EvalPlus Worker",
            "benchmark_harness",
            "benchforge-worker",
            serde_json::json!({"harness": {"command": [missing_command]}}),
        );

        let validation = validate_target_record(&target).expect("validation should run");

        assert_eq!(validation.status, "error");
        assert!(validation.detail.contains("tool_missing"));
        assert!(validation.detail.contains("harness command"));
    }

    #[test]
    fn validate_target_persists_last_health() {
        let conn = store::open_memory().expect("db should open");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "persisted-mock".into(),
                name: "Persisted Mock".into(),
                kind: "mock".into(),
                adapter_id: "mock".into(),
                config: serde_json::json!({"mode": "deterministic-fixture-fix"}),
            },
        )
        .expect("target should save");

        let validation =
            validate_target_for_conn(&conn, "persisted-mock").expect("validation should run");
        let stored = store::get_target(&conn, "persisted-mock")
            .expect("target query should work")
            .expect("target should exist");

        assert_eq!(stored.validation_status.as_deref(), Some("ok"));
        assert_eq!(
            stored.validation_detail.as_deref(),
            Some("mock target is deterministic")
        );
        assert_eq!(
            stored.validation_checked_at.as_deref(),
            Some(validation.checked_at.as_str())
        );
    }

    #[test]
    fn target_dto_classifies_local_and_cloud_model_targets() {
        let adapter_map = benchmark_adapter_map();
        let local = target_record(
            "generic-local",
            "Generic Local Endpoint",
            "direct_model",
            "openai-compatible",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "generic-cloud",
            "Generic Cloud Endpoint",
            "direct_model",
            "openai-compatible",
            serde_json::json!({
                "base_url": "https://example.com/v1",
                "model": "remote-model"
            }),
        );

        let local_dto = target_dto_from_record(local, &adapter_map);
        let cloud_dto = target_dto_from_record(cloud, &adapter_map);

        assert!(local_dto.is_local_model);
        assert!(!local_dto.is_cloud_model);
        assert!(cloud_dto.is_cloud_model);
        assert!(!cloud_dto.is_local_model);
    }

    #[test]
    fn target_dto_exposes_persisted_validation_health() {
        let adapter_map = benchmark_adapter_map();
        let mut target = target_record(
            "validated-target",
            "Validated Target",
            "mock",
            "mock",
            serde_json::json!({"mode": "deterministic-fixture-fix"}),
        );
        target.validation_status = Some("warn".into());
        target.validation_detail = Some("last check warning".into());
        target.validation_checked_at = Some("2026-07-07T12:00:00Z".into());

        let dto = target_dto_from_record(target, &adapter_map);

        assert_eq!(dto.validation_status.as_deref(), Some("warn"));
        assert_eq!(dto.validation_detail.as_deref(), Some("last check warning"));
        assert_eq!(
            dto.validation_checked_at.as_deref(),
            Some("2026-07-07T12:00:00Z")
        );
    }

    #[test]
    fn target_dto_exposes_non_secret_pricing_metadata() {
        let adapter_map = benchmark_adapter_map();
        let target = target_record(
            "priced-target",
            "Priced Target",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini",
                "input_usd_per_million_tokens": 0.25,
                "output_usd_per_million_tokens": 2.0,
                "cached_input_price_usd_per_million_tokens": 0.05,
                "cache_creation_price_usd_per_million_tokens": 0.30,
                "api_key_env": "OPENROUTER_API_KEY"
            }),
        );

        let dto = target_dto_from_record(target, &adapter_map);

        assert_eq!(dto.input_price_usd_per_million_tokens, Some(0.25));
        assert_eq!(dto.output_price_usd_per_million_tokens, Some(2.0));
        assert_eq!(dto.cache_read_price_usd_per_million_tokens, Some(0.05));
        assert_eq!(dto.cache_write_price_usd_per_million_tokens, Some(0.30));
    }

    #[test]
    fn target_dto_exposes_safe_identity_without_secret_values() {
        let adapter_map = benchmark_adapter_map();
        let target = target_record(
            "identity-target",
            "Identity Target",
            "direct_model",
            "openai-compatible",
            serde_json::json!({
                "model": "private-model-1",
                "base_url": "https://example.test/v1?api_key=sk-testsecret123456789",
                "command": "bench --token hf_secretvalue123456789 --model private-model-1",
                "api_key_env": "OPENAI_API_KEY"
            }),
        );

        let dto = target_dto_from_record(target, &adapter_map);

        assert_eq!(dto.model.as_deref(), Some("private-model-1"));
        assert!(dto
            .endpoint
            .as_deref()
            .unwrap_or("")
            .contains("example.test"));
        assert!(dto.command.as_deref().unwrap_or("").contains("bench"));
        for public_value in [dto.endpoint.as_deref(), dto.command.as_deref()]
            .into_iter()
            .flatten()
        {
            assert!(!public_value.contains("sk-testsecret123456789"));
            assert!(!public_value.contains("hf_secretvalue123456789"));
        }
    }

    #[test]
    fn hf_download_handoff_builds_server_request_with_benchmark_flags() {
        let job = huggingface::HuggingFaceDownloadJobDto {
            id: "download-job".into(),
            repo_id: "org/model-GGUF".into(),
            selected_file: Some("fallback.Q4_K_M.gguf".into()),
            status: "completed".into(),
            message: "Downloaded".into(),
            started_at: "2026-07-07T12:00:00Z".into(),
            finished_at: Some("2026-07-07T12:01:00Z".into()),
            planned_bytes: Some(123),
            transferred_bytes: 123,
            percent: Some(100.0),
            local_dir: Some("/tmp/model".into()),
            error: None,
            model: Some(huggingface::DownloadedModelDto {
                repo_id: "org/model-GGUF".into(),
                revision: Some("refs/pr/7".into()),
                path: "/tmp/model".into(),
                files: vec!["selected.Q4_K_M.gguf".into()],
                gguf_files: vec!["selected.Q4_K_M.gguf".into()],
                gguf_file_details: vec![],
                size_bytes: 123,
                selected_file: Some("selected.Q4_K_M.gguf".into()),
                download_log: None,
            }),
            start_after_download: true,
            run_connectivity_after_start: true,
            auto_benchmark_pack_id: Some("llm-basics".into()),
            auto_compare_after_start: true,
            auto_benchmark_target_ids: vec!["hf-local-target".into(), "cloud-priced".into()],
            start_port: Some(18080),
            start_context: Some(4096),
        };

        let request = start_request_from_download_job(&job)
            .expect("completed download job should create a start request");

        assert_eq!(request.repo_id, "org/model-GGUF");
        assert_eq!(request.filename.as_deref(), Some("selected.Q4_K_M.gguf"));
        assert_eq!(request.port, 18080);
        assert_eq!(request.context, 4096);
        assert!(request.register_target_after_start);
        assert!(request.run_connectivity_after_start);
        assert_eq!(
            request.auto_benchmark_pack_id.as_deref(),
            Some("llm-basics")
        );
        assert!(request.auto_compare_after_start);
        assert_eq!(
            request.auto_benchmark_target_ids,
            vec!["hf-local-target".to_string(), "cloud-priced".to_string()]
        );
    }

    #[test]
    fn hf_server_handoff_creates_matching_local_target_config() {
        let status = huggingface::HuggingFaceStatusDto {
            token_available: true,
            python_available: true,
            python_supported: true,
            python_version: Some("3.11.0".into()),
            hf_cli_available: true,
            llama_server_available: true,
            server_running: true,
            server_model_id: Some("served-model".into()),
            cache_dir: "/tmp/cache".into(),
            cache_size_bytes: 123,
            cache_free_bytes: Some(456),
            detail: "ready".into(),
            models: vec![huggingface::DownloadedModelDto {
                repo_id: "org/model-GGUF".into(),
                revision: Some("refs/pr/7".into()),
                path: "/tmp/model".into(),
                files: vec!["model.Q4_K_M.gguf".into()],
                gguf_files: vec!["model.Q4_K_M.gguf".into()],
                gguf_file_details: vec![],
                size_bytes: 123,
                selected_file: Some("model.Q4_K_M.gguf".into()),
                download_log: None,
            }],
        };
        let job = huggingface::HuggingFaceServerJobDto {
            id: "server-job".into(),
            repo_id: "org/model-GGUF".into(),
            selected_file: Some("model.Q4_K_M.gguf".into()),
            port: 18080,
            context: 4096,
            status: "completed".into(),
            message: "ready".into(),
            started_at: "2026-07-07T12:00:00Z".into(),
            finished_at: Some("2026-07-07T12:01:00Z".into()),
            error: None,
            server_status: Some(status.clone()),
            register_target_after_start: true,
            run_connectivity_after_start: true,
            auto_benchmark_pack_id: Some("llm-basics".into()),
            auto_compare_after_start: false,
            auto_benchmark_target_ids: vec![],
        };

        let target = target_from_huggingface_server_job(&job, &status);
        let config = target.config;

        assert_eq!(target.id, "hf-local-org-model-gguf-model-q4-k-m-gguf-18080");
        assert_eq!(target.adapter_id, "llama-cpp-openai");
        assert_eq!(config["model"], "served-model");
        assert_eq!(config["base_url"], "http://127.0.0.1:18080/v1");
        assert_eq!(config["repo_id"], "org/model-GGUF");
        assert_eq!(config["revision"], "refs/pr/7");
        assert_eq!(config["gguf_file"], "model.Q4_K_M.gguf");
        assert_eq!(config["model_path"], "/tmp/model");
        assert_eq!(config["max_tokens"], 512);
        assert_eq!(config["input_price_usd_per_million_tokens"], 0);
        assert_eq!(config["output_price_usd_per_million_tokens"], 0);
    }

    #[test]
    fn hf_local_target_caps_max_tokens_to_context_headroom() {
        assert_eq!(hf_local_target_max_tokens(0), 512);
        assert_eq!(hf_local_target_max_tokens(128), 16);
        assert_eq!(hf_local_target_max_tokens(512), 64);
        assert_eq!(hf_local_target_max_tokens(1024), 128);
        assert_eq!(hf_local_target_max_tokens(2048), 512);
        assert_eq!(hf_local_target_max_tokens(4096), 512);
    }

    #[test]
    fn hf_automatic_benchmark_settings_match_ui_defaults() {
        assert_eq!(
            automatic_huggingface_benchmark_settings("llm-connectivity", 1),
            (1, 0, 1, HF_CONNECTIVITY_MAX_COST_USD)
        );
        assert_eq!(
            automatic_huggingface_benchmark_settings("llm-basics", 1),
            (3, 1, 1, HF_QUALITY_MAX_COST_USD)
        );
        assert_eq!(
            automatic_huggingface_benchmark_settings("llm-basics", 2),
            (3, 1, 2, HF_QUALITY_MAX_COST_USD)
        );
    }

    #[test]
    fn hf_auto_compare_picks_priced_cloud_counterpart() {
        let conn = store::open_memory().expect("store should open");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "cloud-unpriced".into(),
                name: "Cloud Unpriced".into(),
                kind: "direct_model".into(),
                adapter_id: "openrouter".into(),
                config: serde_json::json!({
                    "base_url": "https://openrouter.ai/api/v1",
                    "model": "unpriced/model"
                }),
            },
        )
        .expect("unpriced cloud target should save");
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "cloud-priced".into(),
                name: "Cloud Priced".into(),
                kind: "direct_model".into(),
                adapter_id: "openrouter".into(),
                config: serde_json::json!({
                    "base_url": "https://openrouter.ai/api/v1",
                    "model": "priced/model",
                    "input_price_usd_per_million_tokens": 0.25,
                    "output_price_usd_per_million_tokens": 1.0
                }),
            },
        )
        .expect("priced cloud target should save");
        let server_job = huggingface::HuggingFaceServerJobDto {
            id: "server-job".into(),
            repo_id: "org/model-GGUF".into(),
            selected_file: Some("model.Q4_K_M.gguf".into()),
            port: 18080,
            context: 4096,
            status: "completed".into(),
            message: "ready".into(),
            started_at: "2026-07-07T12:00:00Z".into(),
            finished_at: Some("2026-07-07T12:01:00Z".into()),
            error: None,
            server_status: None,
            register_target_after_start: true,
            run_connectivity_after_start: true,
            auto_benchmark_pack_id: Some("llm-basics".into()),
            auto_compare_after_start: true,
            auto_benchmark_target_ids: vec![],
        };

        let target_ids =
            automatic_huggingface_benchmark_target_ids(&conn, "hf-local-target", &server_job)
                .expect("target ids should build");

        assert_eq!(target_ids, vec!["hf-local-target", "cloud-priced"]);
    }

    #[test]
    fn hf_auto_compare_honors_explicit_cloud_counterpart() {
        let conn = store::open_memory().expect("store should open");
        for (id, model) in [("cloud-alpha", "alpha/model"), ("cloud-beta", "beta/model")] {
            store::upsert_target(
                &conn,
                &store::NewTarget {
                    id: id.into(),
                    name: id.into(),
                    kind: "direct_model".into(),
                    adapter_id: "openrouter".into(),
                    config: serde_json::json!({
                        "base_url": "https://openrouter.ai/api/v1",
                        "model": model,
                        "input_price_usd_per_million_tokens": 0.25,
                        "output_price_usd_per_million_tokens": 1.0
                    }),
                },
            )
            .expect("priced cloud target should save");
        }
        let server_job = huggingface::HuggingFaceServerJobDto {
            id: "server-job".into(),
            repo_id: "org/model-GGUF".into(),
            selected_file: Some("model.Q4_K_M.gguf".into()),
            port: 18080,
            context: 4096,
            status: "completed".into(),
            message: "ready".into(),
            started_at: "2026-07-07T12:00:00Z".into(),
            finished_at: Some("2026-07-07T12:01:00Z".into()),
            error: None,
            server_status: None,
            register_target_after_start: true,
            run_connectivity_after_start: true,
            auto_benchmark_pack_id: Some("llm-basics".into()),
            auto_compare_after_start: true,
            auto_benchmark_target_ids: vec!["hf-local-target".into(), "cloud-beta".into()],
        };

        let target_ids =
            automatic_huggingface_benchmark_target_ids(&conn, "hf-local-target", &server_job)
                .expect("target ids should build");

        assert_eq!(target_ids, vec!["hf-local-target", "cloud-beta"]);
    }

    fn target_request(
        id: &str,
        name: &str,
        kind: &str,
        adapter_id: &str,
        config: serde_json::Value,
    ) -> CreateTargetRequest {
        CreateTargetRequest {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            adapter_id: adapter_id.into(),
            config,
        }
    }

    fn target_record(
        id: &str,
        name: &str,
        kind: &str,
        adapter_id: &str,
        config: serde_json::Value,
    ) -> store::TargetRecord {
        store::TargetRecord {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            adapter_id: adapter_id.into(),
            config_json: config.to_string(),
            enabled: true,
            validation_status: None,
            validation_detail: None,
            validation_checked_at: None,
        }
    }

    #[test]
    fn report_folder_export_writes_results_and_artifact_copy() {
        let run_id = uuid::Uuid::new_v4().to_string();
        let artifact_dir = paths::runs_dir().join(&run_id).join("artifacts");
        fs::create_dir_all(&artifact_dir).expect("artifact dir should be created");
        let source = artifact_dir.join("prompt.txt");
        fs::write(&source, "hello benchmark").expect("artifact should be written");

        let mut result = export_result(&run_id, "target-one", "passed", Some(1.0), None);
        result.run_group_id = Some("group-report".into());
        result.reproducibility["host_profile"] = serde_json::json!({
            "os": "macos",
            "arch": "aarch64",
            "kernel": "25.0.0",
            "hardware": {
                "cpu_brand": "Apple M3",
                "logical_cores": 8,
                "memory_bytes": 17179869184_u64,
                "machine_model": "Mac15,3"
            }
        });
        result.reproducibility["sandbox"] = serde_json::json!("docker");
        result.reproducibility["sandbox_level"] = serde_json::json!(2);
        result.reproducibility["permission_mode"] = serde_json::json!("patch-basic-docker-scoring");
        result.reproducibility["network"] = serde_json::json!("none");
        result.reproducibility["environment"] = serde_json::json!("docker-network-none");
        result.reproducibility["docker"] = serde_json::json!({
            "scoring_image": {
                "image": "benchforge-runner:local",
                "image_id": "sha256:abc123",
                "image_digest": "sha256:abc123",
                "repo_digests": [],
                "dockerfile_sha256": "dockerfile-sha"
            },
            "resource_limits": {
                "cpus": "2.0",
                "memory": "2g",
                "pids_limit": "256"
            }
        });
        result.reproducibility["scoring_command_metadata"] = serde_json::json!({
            "command": ["python", "-m", "pytest", "-q"],
            "resolved_command": "/workspace/.benchforge-venv/bin/python",
            "version_probe": ["/workspace/.benchforge-venv/bin/python", "-m", "pytest", "--version"],
            "version_stdout": "pytest 8.0.0",
            "version_stderr": null,
            "version_exit_code": 0,
            "version_timed_out": false
        });
        result.reproducibility["prompts"] = serde_json::json!({
            "hash_algorithm": "sha256",
            "task_prompt_sha256": "task-prompt-sha",
            "task_prompt_chars": 42,
            "system_prompt_sha256": "system-prompt-sha",
            "system_prompt_chars": 21,
            "user_prompt_sha256": "user-prompt-sha",
            "user_prompt_chars": 84
        });
        result.reproducibility["cli_agent"] = serde_json::json!({
            "command_metadata": {
                "command": ["codex", "exec", "--cd", "/tmp/workspace", "<task_prompt>"],
                "resolved_command": "codex",
                "version_probe": ["codex", "--version"],
                "version_stdout": "codex 1.2.3",
                "version_stderr": null,
                "version_exit_code": 0,
                "version_timed_out": false
            },
            "working_dir": "/tmp/workspace",
            "env": {},
            "exit_code": 0,
            "timed_out": false,
            "wall_time_ms": 1234,
            "peak_rss_mb": 64.0,
            "stdout_bytes": 12,
            "stderr_bytes": 0,
            "stdout_sha256": "cli-stdout-sha",
            "stderr_sha256": "cli-stderr-sha",
            "transcript_files": {
                "stdout": "/tmp/run/artifacts/cli-stdout.txt",
                "stderr": "/tmp/run/artifacts/cli-stderr.txt"
            }
        });
        result.reproducibility["worker_import"] = serde_json::json!({
            "path": "results",
            "format": "mixed",
            "formats": ["json", "text"],
            "source": "directory",
            "read_files": ["summary.json", "large.log"],
            "hash_algorithm": "sha256",
            "file_details": [
                {
                    "path": "summary.json",
                    "format": "json",
                    "size_bytes": 128,
                    "read_bytes": 128,
                    "truncated_bytes": 0,
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "read_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                },
                {
                    "path": "large.log",
                    "format": "text",
                    "size_bytes": 5000,
                    "read_bytes": 1024,
                    "truncated_bytes": 3976,
                    "sha256": null,
                    "read_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                }
            ],
            "file_count": 2,
            "total_file_count": 3,
            "omitted_file_count": 1,
            "truncated": true,
            "truncated_bytes": 4096,
            "summary_source": "json"
        });
        result.reproducibility["workspace"] = serde_json::json!({
            "path": "/tmp/benchforge-run/workspace",
            "git": {
                "baseline_commit": "0123456789abcdef0123456789abcdef01234567",
                "baseline_tree": "89abcdef0123456789abcdef0123456789abcdef",
                "diff_sha256": "diff-sha",
                "diff_includes_untracked": true,
                "diff_excluded_paths": [".benchforge-venv", "node_modules"]
            }
        });
        let artifact = store::ArtifactRecord {
            id: "artifact-one".into(),
            run_id: run_id.clone(),
            kind: "prompt".into(),
            path: source.to_string_lossy().to_string(),
            mime_type: Some("text/plain".into()),
            size_bytes: Some(15),
            sha256: Some("abc123".into()),
            metadata: serde_json::json!({}),
        };
        let missing_artifact = store::ArtifactRecord {
            id: "artifact-missing".into(),
            run_id: run_id.clone(),
            kind: "git_diff".into(),
            path: "/tmp/benchforge-missing-artifact.patch".into(),
            mime_type: Some("text/x-diff".into()),
            size_bytes: Some(42),
            sha256: None,
            metadata: serde_json::json!({"source": "test"}),
        };
        let run_group = store::RunGroupRecord {
            id: "group-report".into(),
            benchmark_pack_id: "llm-core".into(),
            target_ids: vec!["target-one".into()],
            status: "completed".into(),
            started_at: "2026-07-06T12:00:00Z".into(),
            finished_at: Some("2026-07-06T12:00:05Z".into()),
            config: serde_json::json!({
                "repetitions": 3,
                "concurrency": 2,
                "task_count": 1,
                "task_ids": ["llm-core-json-001"],
                "targets": [{
                    "id": "target-one",
                    "generation": {
                        "temperature": 0.0,
                        "top_p": 1.0,
                        "max_tokens": 512,
                        "max_tokens_source": "target_config",
                        "timeout_seconds": 120,
                        "retry_count": 1
                    },
                    "pricing": {
                        "input_price_usd_per_million_tokens": 0.25,
                        "output_price_usd_per_million_tokens": 2.0,
                        "cache_read_price_usd_per_million_tokens": 0.025,
                        "cache_write_price_usd_per_million_tokens": 1.25,
                        "pricing_verified_at": "2026-07-06"
                    }
                }]
            }),
        };

        let export_path = export_report_folder_files(
            &[result],
            &[artifact, missing_artifact],
            &[run_group],
            None,
        )
        .expect("report folder should export");
        let export_dir = PathBuf::from(&export_path);
        assert!(export_dir.join("README.md").exists());
        assert!(export_dir.join("results.csv").exists());
        assert!(export_dir.join("results.jsonl").exists());
        assert!(export_dir.join("reproducibility.json").exists());
        assert!(export_dir.join("analysis.json").exists());
        assert!(export_dir.join("artifacts.json").exists());
        assert!(export_dir
            .join("artifacts")
            .join(short_id(&run_id))
            .join("prompt-prompt.txt")
            .exists());
        let repro: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(export_dir.join("reproducibility.json"))
                .expect("reproducibility manifest should be readable"),
        )
        .expect("reproducibility manifest should be valid JSON");
        assert_eq!(repro["result_count"], 1);
        assert_eq!(
            repro["targets"]["target-one"][0]["config"]["input_price_usd_per_million_tokens"],
            0.25
        );
        assert_eq!(
            repro["benchmark_packs"]["llm-core"][0]["checksum"],
            "pack-sha"
        );
        assert_eq!(
            repro["run_groups"][0]["queued_run_group"]["config"]["concurrency"],
            2
        );
        assert_eq!(
            repro["run_groups"][0]["queued_run_group"]["config"]["task_count"],
            1
        );
        assert_eq!(
            repro["run_groups"][0]["queued_run_group"]["config"]["task_ids"][0],
            "llm-core-json-001"
        );
        assert_eq!(
            repro["run_groups"][0]["queued_run_group"]["config"]["targets"][0]["generation"]
                ["max_tokens"],
            512
        );
        assert_eq!(
            repro["environments"][0]["host_profile"]["hardware"]["logical_cores"],
            8
        );
        assert_eq!(
            repro["environments"][0]["host_profile"]["hardware"]["cpu_brand"],
            "Apple M3"
        );
        assert_eq!(
            repro["environments"][0]["docker"]["scoring_image"]["image_digest"],
            "sha256:abc123"
        );
        assert_eq!(
            repro["environments"][0]["docker"]["scoring_image"]["dockerfile_sha256"],
            "dockerfile-sha"
        );
        assert_eq!(
            repro["environments"][0]["docker"]["resource_limits"]["memory"],
            "2g"
        );
        assert_eq!(
            repro["environments"][0]["docker"]["resource_limits"]["pids_limit"],
            "256"
        );
        assert_eq!(repro["environments"][0]["sandbox_level"], 2);
        assert_eq!(
            repro["environments"][0]["permission_mode"],
            "patch-basic-docker-scoring"
        );
        assert_eq!(
            repro["scoring_commands"][0]["version_stdout"],
            "pytest 8.0.0"
        );
        assert_eq!(repro["scoring_commands"][0]["version_probe"][2], "pytest");
        assert_eq!(
            repro["prompt_hashes"][0]["task_prompt_sha256"],
            "task-prompt-sha"
        );
        assert_eq!(repro["prompt_hashes"][0]["user_prompt_chars"], 84);
        assert_eq!(
            repro["cli_agents"][0]["command_metadata"]["command"][4],
            "<task_prompt>"
        );
        assert_eq!(
            repro["cli_agents"][0]["command_metadata"]["version_stdout"],
            "codex 1.2.3"
        );
        assert_eq!(repro["cli_agents"][0]["stdout_sha256"], "cli-stdout-sha");
        assert_eq!(repro["worker_imports"][0]["run_id"], run_id);
        assert_eq!(
            repro["worker_imports"][0]["worker_import"]["read_files"][0],
            "summary.json"
        );
        assert_eq!(
            repro["worker_imports"][0]["worker_import"]["formats"][1],
            "text"
        );
        assert_eq!(
            repro["worker_imports"][0]["worker_import"]["hash_algorithm"],
            "sha256"
        );
        assert_eq!(
            repro["worker_imports"][0]["worker_import"]["file_details"][0]["sha256"],
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            repro["worker_imports"][0]["worker_import"]["truncated_bytes"],
            4096
        );
        assert_eq!(
            repro["workspaces"][0]["git"]["baseline_commit"],
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(
            repro["workspaces"][0]["git"]["diff_excluded_paths"][0],
            ".benchforge-venv"
        );
        let readme = fs::read_to_string(export_dir.join("README.md"))
            .expect("report README should be readable");
        assert!(readme.contains("task count 1"));
        assert!(readme.contains("tasks llm-core-json-001"));
        assert!(readme.contains("## Worker Imports"));
        assert!(readme.contains("These rows were normalized from existing benchmark output"));
        assert!(readme.contains("summary.json, large.log"));
        assert!(readme.contains("summary.json sha256:aaaaaaaaaaaa"));
        assert!(readme.contains("large.log read-sha256:bbbbbbbbbbbb"));
        assert!(readme.contains("read 2; total 3; omitted 1"));
        assert!(readme.contains("yes (4096 bytes)"));
        assert_eq!(
            repro["run_groups"][0]["queued_run_group"]["config"]["targets"][0]["pricing"]
                ["pricing_verified_at"],
            "2026-07-06"
        );
        assert_eq!(
            repro["run_groups"][0]["queued_run_group"]["config"]["targets"][0]["pricing"]
                ["cache_read_price_usd_per_million_tokens"],
            0.025
        );
        assert_eq!(
            repro["run_groups"][0]["queued_run_group"]["config"]["targets"][0]["pricing"]
                ["cache_write_price_usd_per_million_tokens"],
            1.25
        );
        let analysis: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(export_dir.join("analysis.json"))
                .expect("analysis manifest should be readable"),
        )
        .expect("analysis manifest should be valid JSON");
        assert_eq!(analysis["summary"]["runs"], 1);
        assert_eq!(
            analysis["export_warnings"][0]["code"],
            EXPORT_REVIEW_WARNING_CODE
        );
        assert_eq!(
            analysis["decision"]["recommended_target"]["target_id"],
            "target-one"
        );
        assert_eq!(analysis["target_ranking"][0]["target_id"], "target-one");
        assert_eq!(analysis["target_ranking"][0]["pass_rate"], 1.0);
        assert_eq!(analysis["comparison"][0]["average_score"], 1.0);
        assert_eq!(analysis["metric_coverage"][0]["metric"], "Score");
        assert_eq!(analysis["worker_imports"][0]["run_id"], run_id);
        assert_eq!(analysis["worker_imports"][0]["path"], "results");
        assert_eq!(analysis["worker_imports"][0]["formats"][1], "text");
        assert_eq!(
            analysis["worker_imports"][0]["read_files"][0],
            "summary.json"
        );
        assert_eq!(analysis["worker_imports"][0]["hash_algorithm"], "sha256");
        assert_eq!(
            analysis["worker_imports"][0]["file_details"][1]["read_sha256"],
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert_eq!(analysis["worker_imports"][0]["truncated"], true);
        assert_eq!(
            analysis["worker_imports"][0]["worker_import"]["truncated_bytes"],
            4096
        );
        let artifact_manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(export_dir.join("artifacts.json"))
                .expect("artifact manifest should be readable"),
        )
        .expect("artifact manifest should be valid JSON");
        assert_eq!(artifact_manifest["files"]["analysis"], "analysis.json");
        assert_eq!(
            artifact_manifest["export_warnings"][0]["code"],
            EXPORT_REVIEW_WARNING_CODE
        );
        assert_eq!(artifact_manifest["artifact_count"], 2);
        assert_eq!(artifact_manifest["review_summary"]["copied_count"], 1);
        assert_eq!(artifact_manifest["review_summary"]["not_copied_count"], 1);
        assert_eq!(artifact_manifest["review_summary"]["sensitive_count"], 2);
        assert_eq!(
            artifact_manifest["review_summary"]["by_kind"]["prompt"]["copied"],
            1
        );
        assert_eq!(
            artifact_manifest["review_summary"]["by_kind"]["git_diff"]["not_copied"],
            1
        );
        assert_eq!(artifact_manifest["artifacts"][0]["copy_status"], "copied");
        assert_eq!(artifact_manifest["artifacts"][0]["sensitive"], true);
        assert_eq!(
            artifact_manifest["artifacts"][1]["copy_status"],
            "not_copied"
        );
        assert_eq!(
            artifact_manifest["review_summary"]["copy_errors"][0]["kind"],
            "git_diff"
        );

        let _ = fs::remove_dir_all(paths::runs_dir().join(&run_id));
        let _ = fs::remove_dir_all(export_dir);
    }

    #[test]
    fn scoped_results_returns_only_requested_runs() {
        let results = vec![
            export_result("run-a", "target-one", "passed", Some(1.0), None),
            export_result("run-b", "target-two", "failed", Some(0.0), Some("timeout")),
        ];
        let scoped = scoped_results(results, Some(vec!["run-b".into()])).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].id, "run-b");
    }

    #[test]
    fn scoped_results_rejects_missing_runs() {
        let results = vec![export_result(
            "run-a",
            "target-one",
            "passed",
            Some(1.0),
            None,
        )];
        let error = scoped_results(results, Some(vec!["missing-run".into()])).unwrap_err();
        assert!(error.contains("missing-run"));
    }

    #[test]
    fn csv_cell_quotes_commas_quotes_and_newlines() {
        assert_eq!(csv_cell("plain"), "plain");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_cell("a\nb"), "\"a\nb\"");
    }

    #[test]
    fn detects_model_in_openai_style_list() {
        let body = r#"{"data":[{"id":"alpha"},{"id":"beta"}]}"#;
        assert_eq!(model_list_contains(body, "beta"), Some(true));
        assert_eq!(model_list_contains(body, "gamma"), Some(false));
    }

    #[test]
    fn extracts_model_ids_from_local_runtime_lists() {
        assert_eq!(
            openai_model_ids(r#"{"data":[{"id":"alpha"},{"name":"beta"}]}"#),
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert_eq!(
            openai_model_ids(r#"{"models":[{"name":"llama-local"},{"model":"mlx-local"}]}"#),
            vec!["llama-local".to_string(), "mlx-local".to_string()]
        );
        assert_eq!(
            local_probe_model_source("http://localhost:11434/api/tags"),
            "ollama_native_tags"
        );
        assert_eq!(
            local_probe_model_source("http://localhost:11434/v1/models"),
            "openai_models"
        );
    }

    #[test]
    fn local_runtime_dto_records_probe_metadata() {
        let candidate = local_runtime_candidates()
            .into_iter()
            .find(|candidate| candidate.id == "ollama")
            .expect("ollama candidate should exist");
        let runtime = local_runtime_dto(
            &candidate,
            "ok",
            "1 model available",
            vec!["qwen2.5-coder:7b".into()],
            Some("qwen2.5-coder:7b".into()),
            Some("http://localhost:11434/api/tags"),
            Some("ollama_native_tags"),
        );
        assert_eq!(
            runtime.probe_url.as_deref(),
            Some("http://localhost:11434/api/tags")
        );
        assert_eq!(runtime.model_source.as_deref(), Some("ollama_native_tags"));
        assert!(chrono::DateTime::parse_from_rfc3339(&runtime.detected_at).is_ok());
        assert_eq!(runtime.models, vec!["qwen2.5-coder:7b".to_string()]);

        let config = detected_local_runtime_target_config(&runtime, "qwen2.5-coder:7b", 16, 10, 0);
        assert_eq!(
            config.pointer("/runtime/model_source"),
            Some(&serde_json::json!("ollama_native_tags"))
        );
        assert_eq!(
            config.pointer("/runtime/selected_model"),
            Some(&serde_json::json!("qwen2.5-coder:7b"))
        );
        assert_eq!(
            config.pointer("/runtime/model_count"),
            Some(&serde_json::json!(1))
        );
        assert_eq!(config.get("streaming"), Some(&serde_json::json!(false)));
    }

    #[test]
    fn local_runtime_candidates_cover_documented_local_adapters() {
        let candidates = local_runtime_candidates();
        let ids = candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        for expected in ["ollama", "lm-studio", "llama-cpp", "vllm", "mlx-lm", "omlx"] {
            assert!(ids.contains(&expected), "{} should be detected", expected);
        }
        let ollama = candidates
            .iter()
            .find(|candidate| candidate.id == "ollama")
            .expect("ollama candidate should exist");
        assert!(ollama
            .probe_urls
            .iter()
            .any(|url| url.ends_with("/api/tags")));
        assert!(ollama.install_command.contains("ollama"));
        assert!(ollama.start_command.contains("ollama pull"));
        assert!(ollama.model_hint.contains(":"));
    }

    #[test]
    fn doctor_helpers_parse_versions_and_local_ports() {
        assert_eq!(parse_python_version("Python 3.9.6"), Some((3, 9, 6)));
        assert_eq!(parse_python_version("Python 3.10"), Some((3, 10, 0)));
        assert_eq!(parse_python_version("not python"), None);
        assert_eq!(local_base_url_port("http://localhost:8080/v1"), Some(8080));
        assert_eq!(
            local_base_url_port("http://127.0.0.1:11434/v1/models"),
            Some(11434)
        );
        assert_eq!(local_base_url_port("https://localhost:8080/v1"), None);
        assert_eq!(doctor_format_bytes(512), "512 B");
        assert_eq!(doctor_format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn local_model_storage_doctor_check_reports_cache_path() {
        let check = local_model_storage_doctor_check();
        assert_eq!(check.id, "hf-model-storage");
        assert_eq!(check.category, "Local models");
        assert!(check.command.contains(".benchforge"));
        assert!(check.command.contains("models"));
        assert_ne!(check.status, "error");
    }

    #[test]
    fn doctor_readiness_warns_without_local_or_cloud_targets() {
        let checks = benchmark_readiness_doctor_checks(
            &[],
            &[],
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(doctor_check_status(&checks, "benchmark-packs"), Some("ok"));
        assert_eq!(
            doctor_check_status(&checks, "benchmark-target-local"),
            Some("warn")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-target-cloud"),
            Some("warn")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-compare"),
            Some("warn")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-results"),
            Some("warn")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("warn")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-next-step"),
            Some("warn")
        );
        assert!(doctor_check_detail(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("local model target"));
        assert_eq!(
            doctor_check_command(&checks, "benchmark-next-step"),
            Some("Settings > Hugging Face Local Model")
        );
    }

    #[test]
    fn doctor_pack_diagnostics_warns_for_invalid_custom_packs_with_valid_builtins() {
        let diagnostics = vec![
            runner::BenchmarkPackDiagnosticDto {
                id: Some("llm-basics".into()),
                source: "built-in".into(),
                source_path: "benchmark-packs/llm-basics".into(),
                status: "ok".into(),
                detail: "3 task(s) loaded".into(),
            },
            runner::BenchmarkPackDiagnosticDto {
                id: Some("private-broken".into()),
                source: "user".into(),
                source_path: ".benchforge/benchmark-packs/private-broken".into(),
                status: "error".into(),
                detail: "task path must stay inside the benchmark pack".into(),
            },
        ];
        let check = benchmark_pack_diagnostics_doctor_check(&diagnostics);
        assert_eq!(check.id, "benchmark-pack-diagnostics");
        assert_eq!(check.status, "warn");
        assert!(check.detail.contains("1 invalid"));
        assert!(check.detail.contains("1 valid"));
        assert!(check.remediation.contains("built-in packs remain usable"));
    }

    #[test]
    fn doctor_readiness_detects_local_and_cloud_targets() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &[],
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-target-local"),
            Some("ok")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-target-cloud"),
            Some("ok")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-compare"),
            Some("ok")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-results"),
            Some("warn")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("warn")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-next-step"),
            Some("warn")
        );
        assert!(doctor_check_detail(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("LLM Basics"));
        assert!(doctor_check_remediation(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("seeds LLM Basics by default"));
        assert_eq!(
            doctor_check_command(&checks, "benchmark-next-step"),
            Some("Runs > Local + cloud")
        );
    }

    #[test]
    fn doctor_readiness_ignores_targets_with_failed_validation_health() {
        let mut local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        local.validation_status = Some("error".into());
        local.validation_detail = Some("endpoint_unreachable: connection refused".into());
        local.validation_checked_at = Some("2026-07-07T12:00:00Z".into());
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &[],
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-target-local"),
            Some("warn")
        );
        assert!(doctor_check_detail(&checks, "benchmark-target-local")
            .unwrap_or_default()
            .contains("last validation failed"));
        assert_eq!(
            doctor_check_status(&checks, "benchmark-target-cloud"),
            Some("ok")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-compare"),
            Some("warn")
        );
        assert!(
            doctor_check_detail(&checks, "benchmark-local-cloud-compare")
                .unwrap_or_default()
                .contains("ready local model target")
        );
        assert_eq!(
            doctor_check_command(&checks, "benchmark-next-step"),
            Some("Targets > Repair local target")
        );
        assert!(doctor_check_remediation(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("revalidate the failing local target"));
    }

    #[test]
    fn doctor_readiness_routes_failed_cloud_target_to_repair() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let mut cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );
        cloud.validation_status = Some("error".into());
        cloud.validation_detail = Some("auth: provider rejected API key".into());
        cloud.validation_checked_at = Some("2026-07-07T12:00:00Z".into());

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &[],
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-target-cloud"),
            Some("warn")
        );
        assert!(doctor_check_detail(&checks, "benchmark-target-cloud")
            .unwrap_or_default()
            .contains("last validation failed"));
        assert_eq!(
            doctor_check_command(&checks, "benchmark-next-step"),
            Some("Targets > Repair cloud target")
        );
        assert!(doctor_check_remediation(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("fix the failing cloud target"));
    }

    #[test]
    fn doctor_readiness_detects_local_cloud_comparison_results() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );
        let mut local_result = export_result("run-local", "local-llama", "passed", Some(1.0), None);
        local_result.run_group_id = Some("group-local-cloud".into());
        local_result.started_at = Some("2026-07-07T10:00:00Z".into());
        let mut cloud_result = export_result(
            "run-cloud",
            "cloud-openrouter",
            "failed",
            Some(0.0),
            Some("timeout"),
        );
        cloud_result.run_group_id = Some("group-local-cloud".into());
        cloud_result.started_at = Some("2026-07-07T10:00:01Z".into());

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &[local_result, cloud_result],
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-results"),
            Some("ok")
        );
        assert!(
            doctor_check_detail(&checks, "benchmark-local-cloud-results")
                .unwrap_or_default()
                .contains("1 local and 1 cloud result row(s), 1 passed out of 2")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("warn")
        );
        assert!(
            doctor_check_detail(&checks, "benchmark-local-cloud-evidence")
                .unwrap_or_default()
                .contains("fewer than 3 repetition")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-next-step"),
            Some("warn")
        );
        assert!(doctor_check_detail(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("fewer than 3 repetition"));
    }

    #[test]
    fn doctor_readiness_marks_repeated_balanced_local_cloud_evidence_ok() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );
        let mut results = Vec::new();
        for target_id in ["local-llama", "cloud-openrouter"] {
            for repetition in 0..3 {
                let mut result = export_result(
                    &format!("run-{target_id}-{repetition}"),
                    target_id,
                    "passed",
                    Some(1.0),
                    None,
                );
                result.run_group_id = Some("group-local-cloud-ready".into());
                result.started_at = Some(format!("2026-07-07T10:00:0{repetition}Z"));
                results.push(result);
            }
        }

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &results,
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-results"),
            Some("ok")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("ok")
        );
        assert!(
            doctor_check_detail(&checks, "benchmark-local-cloud-evidence")
                .unwrap_or_default()
                .contains("at least 3 repetition")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-next-step"),
            Some("ok")
        );
        assert!(doctor_check_detail(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("inspect and export"));
        assert_eq!(
            doctor_check_command(&checks, "benchmark-next-step"),
            Some("Results > Export report")
        );
    }

    #[test]
    fn doctor_readiness_warns_when_model_identity_uses_configured_fallback() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );
        let mut results = Vec::new();
        for target_id in ["local-llama", "cloud-openrouter"] {
            for repetition in 0..3 {
                let mut result = export_result(
                    &format!("run-{target_id}-{repetition}"),
                    target_id,
                    "passed",
                    Some(1.0),
                    None,
                );
                result.run_group_id = Some("group-local-cloud-fallback-model".into());
                result.started_at = Some(format!("2026-07-07T10:00:0{repetition}Z"));
                if target_id == "cloud-openrouter" {
                    result.provider_model_source = Some("target_config".into());
                }
                results.push(result);
            }
        }

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &results,
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("warn")
        );
        let detail =
            doctor_check_detail(&checks, "benchmark-local-cloud-evidence").unwrap_or_default();
        assert!(detail.contains("unconfirmed served model identity"));
        assert!(detail.contains("cloud-openrouter"));
        assert_eq!(
            doctor_check_status(&checks, "benchmark-next-step"),
            Some("warn")
        );
        assert!(doctor_check_detail(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("unconfirmed served model identity"));
        assert!(doctor_check_remediation(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("provider-supplied served model id"));
    }

    #[test]
    fn doctor_readiness_warns_when_generation_settings_are_mixed() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );
        let mut results = Vec::new();
        for target_id in ["local-llama", "cloud-openrouter"] {
            for repetition in 0..3 {
                let mut result = export_result(
                    &format!("run-mixed-generation-{target_id}-{repetition}"),
                    target_id,
                    "passed",
                    Some(1.0),
                    None,
                );
                result.run_group_id = Some("group-local-cloud-mixed-generation".into());
                result.started_at = Some(format!("2026-07-07T10:00:0{repetition}Z"));
                if target_id == "cloud-openrouter" {
                    result.reproducibility["generation"]["temperature"] = serde_json::json!(0.7);
                    result.reproducibility["generation"]["top_p"] = serde_json::json!(0.9);
                }
                results.push(result);
            }
        }

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &results,
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("warn")
        );
        let detail =
            doctor_check_detail(&checks, "benchmark-local-cloud-evidence").unwrap_or_default();
        assert!(detail.contains("mixes generation settings"));
        assert!(detail.contains("mode deterministic, temp 0, top_p 1, seed not_set"));
        assert!(detail.contains("mode exploratory, temp 0.7, top_p 0.9, seed not_set"));
        assert_eq!(
            doctor_check_status(&checks, "benchmark-next-step"),
            Some("warn")
        );
        assert!(doctor_check_remediation(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("one shared generation policy"));
    }

    #[test]
    fn doctor_readiness_warns_when_local_cloud_evidence_is_missing_cost() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );
        let mut results = Vec::new();
        for target_id in ["local-llama", "cloud-openrouter"] {
            for repetition in 0..3 {
                let mut result = export_result(
                    &format!("run-{target_id}-{repetition}"),
                    target_id,
                    "passed",
                    Some(1.0),
                    None,
                );
                result.run_group_id = Some("group-local-cloud-unpriced".into());
                result.started_at = Some(format!("2026-07-07T10:00:0{repetition}Z"));
                if target_id == "cloud-openrouter" {
                    result.cost_usd = None;
                    set_export_result_target(
                        &mut result,
                        "openrouter",
                        serde_json::json!({
                            "base_url": "https://openrouter.ai/api/v1",
                            "model": "openai/gpt-4.1-mini"
                        }),
                    );
                }
                results.push(result);
            }
        }

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &results,
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-results"),
            Some("ok")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("warn")
        );
        let detail =
            doctor_check_detail(&checks, "benchmark-local-cloud-evidence").unwrap_or_default();
        assert!(detail.contains("missing cost metrics"));
        assert!(detail.contains("cloud-openrouter"));
        assert_eq!(
            doctor_check_status(&checks, "benchmark-next-step"),
            Some("warn")
        );
        assert!(doctor_check_detail(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("missing cost metrics"));
        assert_eq!(
            doctor_check_command(&checks, "benchmark-next-step"),
            Some("Runs > Local + cloud")
        );
    }

    #[test]
    fn doctor_readiness_warns_when_local_cloud_evidence_has_pricing_assumptions() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );
        let mut results = Vec::new();
        for target_id in ["local-llama", "cloud-openrouter"] {
            for repetition in 0..3 {
                let mut result = export_result(
                    &format!("run-cache-pricing-{target_id}-{repetition}"),
                    target_id,
                    "passed",
                    Some(1.0),
                    None,
                );
                result.run_group_id = Some("group-local-cloud-cache-pricing".into());
                result.started_at = Some(format!("2026-07-07T10:00:0{repetition}Z"));
                if target_id == "cloud-openrouter" {
                    result.pricing_assumption = Some("cache_read_tokens_priced_as_input".into());
                }
                results.push(result);
            }
        }

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &results,
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-results"),
            Some("ok")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("warn")
        );
        let detail =
            doctor_check_detail(&checks, "benchmark-local-cloud-evidence").unwrap_or_default();
        assert!(detail.contains("pricing assumptions"));
        assert!(detail.contains("cloud-openrouter"));
        assert!(detail.contains("cache read/write pricing"));
        assert_eq!(
            doctor_check_status(&checks, "benchmark-next-step"),
            Some("warn")
        );
        assert!(doctor_check_detail(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("pricing assumptions"));
        assert!(doctor_check_remediation(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("cache read/write pricing"));
    }

    #[test]
    fn doctor_readiness_warns_when_local_cloud_evidence_is_connectivity_only() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );
        let mut results = Vec::new();
        for target_id in ["local-llama", "cloud-openrouter"] {
            for repetition in 0..3 {
                let mut result = export_result(
                    &format!("run-connectivity-{target_id}-{repetition}"),
                    target_id,
                    "passed",
                    Some(1.0),
                    None,
                );
                result.run_group_id = Some("group-local-cloud-connectivity".into());
                result.benchmark_pack_id = "llm-connectivity".into();
                result.task_id = "llm-connectivity-nonempty-001".into();
                result.started_at = Some(format!("2026-07-07T10:00:0{repetition}Z"));
                results.push(result);
            }
        }

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &results,
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-results"),
            Some("ok")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("warn")
        );
        let detail =
            doctor_check_detail(&checks, "benchmark-local-cloud-evidence").unwrap_or_default();
        assert!(detail.contains("llm-connectivity"));
        assert!(detail.contains("smoke evidence"));
        assert_eq!(
            doctor_check_status(&checks, "benchmark-next-step"),
            Some("warn")
        );
        assert!(doctor_check_detail(&checks, "benchmark-next-step")
            .unwrap_or_default()
            .contains("llm-connectivity"));
        assert_eq!(
            doctor_check_command(&checks, "benchmark-next-step"),
            Some("Runs > Local + cloud")
        );
    }

    #[test]
    fn doctor_readiness_warns_on_partial_local_cloud_evidence_coverage() {
        let local = target_record(
            "local-llama",
            "Local llama.cpp",
            "direct_model",
            "llama-cpp-openai",
            serde_json::json!({
                "base_url": "http://127.0.0.1:8080/v1",
                "model": "local-model"
            }),
        );
        let cloud = target_record(
            "cloud-openrouter",
            "OpenRouter",
            "direct_model",
            "openrouter",
            serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "model": "openai/gpt-4.1-mini"
            }),
        );
        let mut results = Vec::new();
        for task_id in ["task-a", "task-b"] {
            for repetition in 0..3 {
                let mut result = export_result(
                    &format!("run-local-{task_id}-{repetition}"),
                    "local-llama",
                    "passed",
                    Some(1.0),
                    None,
                );
                result.run_group_id = Some("group-local-cloud-partial".into());
                result.task_id = task_id.into();
                result.started_at = Some(format!("2026-07-07T10:00:0{repetition}Z"));
                results.push(result);
            }
        }
        for repetition in 0..3 {
            let mut result = export_result(
                &format!("run-cloud-task-a-{repetition}"),
                "cloud-openrouter",
                "passed",
                Some(1.0),
                None,
            );
            result.run_group_id = Some("group-local-cloud-partial".into());
            result.task_id = "task-a".into();
            result.started_at = Some(format!("2026-07-07T10:00:1{repetition}Z"));
            results.push(result);
        }

        let checks = benchmark_readiness_doctor_checks(
            &[local, cloud],
            &results,
            runner::list_benchmark_packs(),
            adapters::load_builtin_adapters(),
        );

        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-results"),
            Some("ok")
        );
        assert_eq!(
            doctor_check_status(&checks, "benchmark-local-cloud-evidence"),
            Some("warn")
        );
        let detail =
            doctor_check_detail(&checks, "benchmark-local-cloud-evidence").unwrap_or_default();
        assert!(detail.contains("partial"));
        assert!(detail.contains("1 pack/task slot"));
    }

    fn doctor_check_status<'a>(checks: &'a [DoctorCheckDto], id: &str) -> Option<&'a str> {
        checks
            .iter()
            .find(|check| check.id == id)
            .map(|check| check.status.as_str())
    }

    fn doctor_check_detail<'a>(checks: &'a [DoctorCheckDto], id: &str) -> Option<&'a str> {
        checks
            .iter()
            .find(|check| check.id == id)
            .map(|check| check.detail.as_str())
    }

    fn doctor_check_command<'a>(checks: &'a [DoctorCheckDto], id: &str) -> Option<&'a str> {
        checks
            .iter()
            .find(|check| check.id == id)
            .map(|check| check.command.as_str())
    }

    fn doctor_check_remediation<'a>(checks: &'a [DoctorCheckDto], id: &str) -> Option<&'a str> {
        checks
            .iter()
            .find(|check| check.id == id)
            .map(|check| check.remediation.as_str())
    }

    #[test]
    fn unknown_model_list_shape_is_inconclusive() {
        assert_eq!(
            model_list_contains(r#"{"models":["alpha"]}"#, "alpha"),
            None
        );
    }

    #[test]
    fn validation_errors_are_classified_for_user_action() {
        assert_eq!(
            validation_error_code(
                "endpoint check",
                "curl: (7) Failed to connect to localhost port 8080"
            ),
            "endpoint_unreachable"
        );
        assert_eq!(
            validation_error_code(
                "completion probe",
                "curl: (22) The requested URL returned error: 401"
            ),
            "auth"
        );
        assert_eq!(
            validation_error_code("completion probe", "429 too many requests"),
            "rate_limited"
        );
        assert_eq!(
            validation_error_code("completion probe", "model does not exist"),
            "model_not_found"
        );
        assert_eq!(
            validation_error_code(
                "completion probe",
                "provider response did not include choices[0].message.content"
            ),
            "unsupported_shape"
        );
    }

    #[test]
    fn validation_failure_detail_starts_with_code() {
        let detail = format_validation_failure(
            "completion probe",
            "curl: (28) Operation timed out after 30001 milliseconds",
        );
        assert!(detail.starts_with("timeout: completion probe failed:"));
        assert!(detail.contains("Operation timed out"));
    }

    #[test]
    fn validation_requires_key_for_remote_openai_compatible_target() {
        let target = target_record(
            "remote-compatible",
            "Remote Compatible",
            "direct_model",
            "openai-compatible",
            serde_json::json!({
                "model": "contract-ok",
                "base_url": "https://api.example.com/v1",
                "api_key_keychain": "openai-compatible-https-api-example-com-v1"
            }),
        );

        let validation = validate_target_record(&target).expect("validation should return status");

        assert_eq!(validation.status, "error");
        assert!(
            validation.detail.starts_with("missing_key:"),
            "{}",
            validation.detail
        );
        assert!(validation
            .detail
            .contains("openai-compatible-https-api-example-com-v1"));
        assert!(validation.detail.contains("api_key_env"));
    }

    #[test]
    fn validation_allows_local_openai_compatible_target_without_key() {
        let server = ValidationContractServer::start().expect("contract server should start");
        let target = target_record(
            "local-compatible",
            "Local Compatible",
            "direct_model",
            "openai-compatible",
            serde_json::json!({
                "model": "contract-ok",
                "base_url": format!("{}/v1", server.base_url)
            }),
        );

        let validation = validate_target_record(&target).expect("validation should run");

        assert_eq!(validation.status, "ok");
        assert!(validation.detail.contains("completion probe succeeded"));
    }

    #[test]
    fn validation_http_error_preserves_provider_json_body() {
        let stdout = concat!(
            r#"{"error":{"message":"The model `missing-model` does not exist","type":"invalid_request_error","code":"model_not_found"}}"#,
            "\n__BENCHFORGE_VALIDATION_HTTP_STATUS__:404"
        );
        let (body, status) = parse_validation_http_response(stdout);
        assert_eq!(status, Some(404));
        let error = validation_http_error(status.unwrap(), &body, "");
        assert!(error.contains("HTTP 404"));
        assert!(error.contains("missing-model"));
        assert!(error.contains("model_not_found"));
        assert_eq!(
            validation_error_code("completion probe", &error),
            "model_not_found"
        );
    }

    #[test]
    fn validation_http_error_classifies_rate_limit_bodies() {
        let error = validation_http_error(
            429,
            r#"{"error":{"message":"Rate limit reached","code":"rate_limit_exceeded"}}"#,
            "",
        );
        assert!(error.contains("Rate limit reached"));
        assert_eq!(
            validation_error_code("completion probe", &error),
            "rate_limited"
        );
    }

    #[test]
    fn parses_openrouter_model_prices_per_million() {
        let body = r#"{
          "data": [
            {
              "id": "openai/gpt-4.1-mini",
              "name": "OpenAI: GPT-4.1 Mini",
              "description": "Small fast model",
              "context_length": 1048576,
              "pricing": {
                "prompt": "0.0000004",
                "completion": "0.0000016",
                "cache_read": "0.0000001",
                "cache_write": "0.0000008"
              }
            },
            {
              "id": "free/model",
              "name": "Free Model",
              "pricing": {"prompt": "0", "completion": "0"}
            }
          ]
        }"#;
        let models = parse_openrouter_models(body, "4.1", 10).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model, "openai/gpt-4.1-mini");
        assert_price_eq(models[0].input_price_usd_per_million_tokens, 0.4);
        assert_price_eq(models[0].output_price_usd_per_million_tokens, 1.6);
        assert_price_eq(models[0].cache_read_price_usd_per_million_tokens, 0.1);
        assert_price_eq(models[0].cache_write_price_usd_per_million_tokens, 0.8);
        assert_eq!(models[0].context_length, Some(1_048_576));
    }

    #[test]
    fn parses_provider_model_catalog_shapes() {
        let openai = parse_provider_models(
            r#"{"data":[{"id":"gpt-4.1-mini","owned_by":"openai"},{"id":"tts-1"}]}"#,
            "4.1",
            10,
            "OpenAI",
            "openai-live",
            Some("https://developers.openai.com/api/reference/resources/models/methods/list"),
            |item| {
                let model = item.get("id").and_then(|value| value.as_str())?;
                Some(ProviderModelSeed {
                    model: model.to_string(),
                    name: model.to_string(),
                    context_length: None,
                    detail: item
                        .get("owned_by")
                        .and_then(|value| value.as_str())
                        .map(|owner| format!("Owned by {}", owner)),
                })
            },
        )
        .unwrap();
        assert_eq!(openai.len(), 1);
        assert_eq!(openai[0].model, "gpt-4.1-mini");
        assert_eq!(openai[0].detail.as_deref(), Some("Owned by openai"));

        let anthropic = parse_provider_models(
            r#"{"data":[{"id":"claude-sonnet-4-6","display_name":"Claude Sonnet 4.6","created_at":"2026-02-05T00:00:00Z"}]}"#,
            "sonnet",
            10,
            "Anthropic Claude",
            "anthropic-live",
            Some("https://platform.claude.com/docs/en/api/models/list"),
            |item| {
                let model = item.get("id").and_then(|value| value.as_str())?;
                Some(ProviderModelSeed {
                    model: model.to_string(),
                    name: item
                        .get("display_name")
                        .and_then(|value| value.as_str())
                        .unwrap_or(model)
                        .to_string(),
                    context_length: None,
                    detail: item
                        .get("created_at")
                        .and_then(|value| value.as_str())
                        .map(|created| format!("Created {}", created)),
                })
            },
        )
        .unwrap();
        assert_eq!(anthropic[0].name, "Claude Sonnet 4.6");
        assert_eq!(
            anthropic[0].detail.as_deref(),
            Some("Created 2026-02-05T00:00:00Z")
        );

        let mistral = parse_provider_models(
            r#"[{"id":"mistral-small-latest","root":"mistral-small","max_context_length":32768,"capabilities":{"completion_chat":true}},{"id":"embedding","capabilities":{"completion_chat":false}}]"#,
            "",
            10,
            "Mistral API",
            "mistral-live",
            Some("https://docs.mistral.ai/api/endpoint/models"),
            |item| {
                if item
                    .pointer("/capabilities/completion_chat")
                    .and_then(|value| value.as_bool())
                    == Some(false)
                {
                    return None;
                }
                let model = item.get("id").and_then(|value| value.as_str())?;
                Some(ProviderModelSeed {
                    model: model.to_string(),
                    name: item
                        .get("root")
                        .and_then(|value| value.as_str())
                        .unwrap_or(model)
                        .to_string(),
                    context_length: item
                        .get("max_context_length")
                        .and_then(|value| value.as_u64()),
                    detail: None,
                })
            },
        )
        .unwrap();
        assert_eq!(mistral.len(), 1);
        assert_eq!(mistral[0].context_length, Some(32_768));

        let azure = parse_provider_models(
            r#"{"data":[{"id":"gpt-5-mini","display_name":"GPT-5 mini","owned_by":"azure-openai","context_length":128000}]}"#,
            "mini",
            10,
            "Azure OpenAI",
            "azure-openai-live",
            Some("https://learn.microsoft.com/en-us/azure/foundry/openai/"),
            azure_model_seed_from_item,
        )
        .unwrap();
        assert_eq!(azure.len(), 1);
        assert_eq!(azure[0].model, "gpt-5-mini");
        assert_eq!(azure[0].name, "GPT-5 mini");
        assert_eq!(azure[0].context_length, Some(128_000));
        assert_eq!(azure[0].detail.as_deref(), Some("Owned by azure-openai"));

        let gemini = parse_provider_models(
            r#"{"data":[{"id":"models/gemini-2.5-flash-lite","display_name":"Gemini 2.5 Flash-Lite","version":"001","input_token_limit":1048576,"output_token_limit":8192}]}"#,
            "flash-lite",
            10,
            "Google Gemini",
            "gemini-live",
            Some("https://ai.google.dev/gemini-api/docs/openai"),
            gemini_model_seed_from_item,
        )
        .unwrap();
        assert_eq!(gemini.len(), 1);
        assert_eq!(gemini[0].model, "gemini-2.5-flash-lite");
        assert_eq!(gemini[0].name, "Gemini 2.5 Flash-Lite");
        assert_eq!(gemini[0].context_length, Some(1_048_576));
        assert!(gemini[0]
            .detail
            .as_deref()
            .unwrap_or("")
            .contains("Output token limit 8192"));
    }

    #[test]
    fn openai_compatible_catalog_uses_request_base_url_and_env_reference() {
        let server = ValidationContractServer::start().expect("contract server should start");
        let api_key_env: &'static str = Box::leak(
            format!(
                "BENCHFORGE_TEST_COMPAT_CATALOG_KEY_{}",
                uuid::Uuid::new_v4()
            )
            .replace('-', "_")
            .into_boxed_str(),
        );
        let _api_key = ScopedEnvVar::set(api_key_env, "benchforge-test-key");
        let adapter = adapters::find_adapter("openai-compatible")
            .expect("adapter lookup should succeed")
            .expect("openai-compatible adapter should exist")
            .spec;

        let models = openai_compatible_model_catalog(
            &adapter,
            "contract",
            10,
            &CloudModelSearchRequest {
                adapter_id: "openai-compatible".into(),
                query: "contract".into(),
                limit: Some(10),
                base_url: Some(server.base_url.clone()),
                api_key_keychain: Some("openai-compatible-contract".into()),
                api_key_env: Some(api_key_env.into()),
                azure_api_version: None,
            },
        )
        .expect("compatible catalog should read loopback /models");

        let model = require_cloud_model(&models, "openai-compatible", "contract-ok")
            .expect("contract model should be listed");
        assert_eq!(model.provider, "Generic OpenAI-compatible");
        assert_eq!(model.source, "openai-compatible-live");
    }

    #[test]
    fn catalog_api_key_rejects_invalid_request_env_name() {
        let adapter = adapters::find_adapter("openai-compatible")
            .expect("adapter lookup should succeed")
            .expect("openai-compatible adapter should exist")
            .spec;
        let err = catalog_api_key(
            &adapter,
            &CloudModelSearchRequest {
                adapter_id: "openai-compatible".into(),
                query: "".into(),
                limit: Some(10),
                base_url: Some("http://127.0.0.1:8080/v1".into()),
                api_key_keychain: None,
                api_key_env: Some("not a valid env name".into()),
                azure_api_version: None,
            },
        )
        .expect_err("bad env var names should be rejected");

        assert!(err.contains("apiKeyEnv"), "{err}");
    }

    #[test]
    fn enriches_live_models_from_adapter_presets() {
        let mut live = vec![cloud_model("gpt-4.1-mini", "openai-live")];
        let mut preset = cloud_model("gpt-4.1-mini", "adapter-preset");
        preset.input_price_usd_per_million_tokens = Some(0.4);
        preset.output_price_usd_per_million_tokens = Some(1.6);
        preset.cache_read_price_usd_per_million_tokens = Some(0.1);
        preset.cache_write_price_usd_per_million_tokens = Some(0.8);
        preset.source_url = Some("https://openai.com/index/gpt-4-1/".into());

        enrich_models_from_presets(&mut live, &[preset]);

        assert_price_eq(live[0].input_price_usd_per_million_tokens, 0.4);
        assert_price_eq(live[0].output_price_usd_per_million_tokens, 1.6);
        assert_price_eq(live[0].cache_read_price_usd_per_million_tokens, 0.1);
        assert_price_eq(live[0].cache_write_price_usd_per_million_tokens, 0.8);
        assert_eq!(
            live[0].source_url.as_deref(),
            Some("https://openai.com/index/gpt-4-1/")
        );
    }

    fn cloud_model(model: &str, source: &str) -> CloudModelDto {
        CloudModelDto {
            model: model.into(),
            name: model.into(),
            provider: "Test Provider".into(),
            input_price_usd_per_million_tokens: None,
            output_price_usd_per_million_tokens: None,
            cache_read_price_usd_per_million_tokens: None,
            cache_write_price_usd_per_million_tokens: None,
            context_length: None,
            source: source.into(),
            source_url: None,
            detail: None,
        }
    }

    fn assert_price_eq(actual: Option<f64>, expected: f64) {
        assert!((actual.unwrap() - expected).abs() < 0.000_000_001);
    }

    #[test]
    fn detects_openai_completion_content() {
        let body = r#"{"choices":[{"message":{"content":"OK"}}]}"#;
        assert!(openai_completion_has_content(body));
        assert!(!openai_completion_has_content(
            r#"{"choices":[{"message":{"content":"   "}}]}"#
        ));
    }

    #[test]
    fn validation_contract_success_reports_model_and_finish_reason() {
        let request = "POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 27\r\n\r\n{\"model\":\"contract-basics\"}";
        let response = validation_contract_response(request);
        let json: serde_json::Value = serde_json::from_str(&response.body).unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(json["model"], "contract-basics");
        assert_eq!(json["choices"][0]["finish_reason"], "stop");
        assert_eq!(json["choices"][0]["message"]["content"], "benchforge");
        assert_eq!(json["usage"]["prompt_tokens"], 32);
        assert_eq!(json["usage"]["completion_tokens"], 1);
        assert_eq!(json["usage"]["total_tokens"], 33);
    }

    #[test]
    fn detects_openai_responses_content() {
        assert!(openai_responses_has_content(r#"{"output_text":"OK"}"#));
        assert!(openai_responses_has_content(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}]}"#
        ));
        assert!(!openai_responses_has_content(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"   "}]}]}"#
        ));
    }

    #[test]
    fn detects_anthropic_completion_content() {
        let body = r#"{"content":[{"type":"text","text":"OK"}]}"#;
        assert!(anthropic_completion_has_content(body));
        assert!(!anthropic_completion_has_content(
            r#"{"content":[{"type":"text","text":""}]}"#
        ));
    }

    #[test]
    fn azure_openai_chat_url_supports_v1_and_legacy_shapes() {
        assert_eq!(
            azure_openai_chat_url(
                "https://example.openai.azure.com/openai/v1/",
                "deployment",
                &serde_json::json!({})
            ),
            "https://example.openai.azure.com/openai/v1/chat/completions"
        );
        assert_eq!(
            azure_openai_chat_url(
                "https://example.openai.azure.com",
                "deployment",
                &serde_json::json!({"api_version": "2025-04-01-preview"})
            ),
            "https://example.openai.azure.com/openai/deployments/deployment/chat/completions?api-version=2025-04-01-preview"
        );
    }

    #[test]
    fn azure_openai_model_catalog_url_supports_v1_and_legacy_shapes() {
        assert_eq!(
            azure_openai_models_url(
                "https://example.openai.azure.com/openai/v1/",
                Some("2025-04-01-preview")
            ),
            "https://example.openai.azure.com/openai/v1/models"
        );
        assert_eq!(
            azure_openai_models_url(
                "https://example.openai.azure.com",
                Some("2025-04-01-preview")
            ),
            "https://example.openai.azure.com/openai/models?api-version=2025-04-01-preview"
        );
        assert_eq!(
            azure_openai_models_url("https://example.openai.azure.com/openai", None),
            "https://example.openai.azure.com/openai/models?api-version=2024-10-21"
        );
    }

    #[test]
    fn azure_openai_legacy_payload_omits_model_only_for_legacy_url() {
        let mut v1_payload = serde_json::json!({"model": "deployment"});
        remove_azure_legacy_model_field(
            "https://example.openai.azure.com/openai/v1",
            &mut v1_payload,
        );
        assert_eq!(
            v1_payload.get("model").and_then(|value| value.as_str()),
            Some("deployment")
        );

        let mut legacy_payload = serde_json::json!({"model": "deployment"});
        remove_azure_legacy_model_field("https://example.openai.azure.com", &mut legacy_payload);
        assert!(legacy_payload.get("model").is_none());
    }

    #[test]
    fn live_cloud_plan_skips_provider_without_key() {
        let filter = BTreeSet::from(["openai".to_string()]);
        let plan = live_cloud_target_plan(Some(&filter), &|_| None, &|_| false)
            .expect("plan should build");

        assert!(plan.targets.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].provider, "openai");
        assert_eq!(plan.skipped[0].reason, "missing_key");
    }

    #[test]
    fn live_cloud_plan_reports_unknown_provider_filter_without_blocking_known_provider() {
        let filter = BTreeSet::from(["bogus-ai".to_string(), "openai".to_string()]);
        let plan = live_cloud_target_plan(Some(&filter), &|_| None, &|adapter_id| {
            adapter_id == "openai"
        })
        .expect("plan should build");

        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].id, "live-openai");
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].provider, "bogus-ai");
        assert_eq!(plan.skipped[0].reason, "unsupported_provider");
        assert!(plan.skipped[0].detail.contains("openai"));
        assert!(plan.skipped[0].detail.contains("gemini"));
    }

    #[test]
    fn live_cloud_plan_builds_target_without_secret_value() {
        let filter = BTreeSet::from(["openai".to_string()]);
        let plan = live_cloud_target_plan(
            Some(&filter),
            &|name| match name {
                "BENCHFORGE_LIVE_OPENAI_MODEL" => Some("gpt-4.1-mini".into()),
                _ => None,
            },
            &|adapter_id| adapter_id == "openai",
        )
        .expect("plan should build");

        assert_eq!(plan.targets.len(), 1);
        let target = &plan.targets[0];
        assert_eq!(target.id, "live-openai");
        assert_eq!(target.adapter_id, "openai");
        assert_eq!(target.config["model"], "gpt-4.1-mini");
        assert_eq!(target.config["api_key_keychain"], "openai");
        assert_eq!(target.config["api_key_env"], "OPENAI_API_KEY");
        assert_eq!(target.config["max_tokens"], 32);
        assert_eq!(target.config["input_price_usd_per_million_tokens"], 0.4);
        assert_eq!(target.config["output_price_usd_per_million_tokens"], 1.6);
        assert!(!target.config.to_string().contains("sk-"));
    }

    #[test]
    fn live_cloud_pricing_env_requires_complete_non_negative_pair() {
        let adapter = adapters::find_adapter("openai")
            .expect("adapter lookup should succeed")
            .expect("openai adapter should exist")
            .spec;

        assert_eq!(
            live_cloud_pricing(&|_| None, "openai", &adapter, "gpt-4.1-mini")
                .expect("catalog pricing should be available"),
            Some((0.4, 1.6))
        );
        assert_eq!(
            live_cloud_pricing(
                &|name| match name {
                    "BENCHFORGE_LIVE_OPENAI_INPUT_PRICE_USD_PER_MILLION_TOKENS" => {
                        Some("0.12".into())
                    }
                    "BENCHFORGE_LIVE_OPENAI_OUTPUT_PRICE_USD_PER_MILLION_TOKENS" => {
                        Some("0.34".into())
                    }
                    _ => None,
                },
                "openai",
                &adapter,
                "unknown-model",
            )
            .expect("complete override should parse"),
            Some((0.12, 0.34))
        );

        let partial = live_cloud_pricing(
            &|name| match name {
                "BENCHFORGE_LIVE_OPENAI_INPUT_PRICE_USD_PER_MILLION_TOKENS" => Some("0.12".into()),
                _ => None,
            },
            "openai",
            &adapter,
            "gpt-4.1-mini",
        )
        .expect_err("partial override should be rejected");
        assert!(partial.starts_with("pricing_invalid"), "{partial}");
        assert!(partial.contains("set both"), "{partial}");

        for (env_name, value) in [
            (
                "BENCHFORGE_LIVE_OPENAI_INPUT_PRICE_USD_PER_MILLION_TOKENS",
                "NaN",
            ),
            (
                "BENCHFORGE_LIVE_OPENAI_OUTPUT_PRICE_USD_PER_MILLION_TOKENS",
                "-0.01",
            ),
        ] {
            let err = live_cloud_pricing(
                &|name| match name {
                    "BENCHFORGE_LIVE_OPENAI_INPUT_PRICE_USD_PER_MILLION_TOKENS" => {
                        Some(if name == env_name { value } else { "0.12" }.into())
                    }
                    "BENCHFORGE_LIVE_OPENAI_OUTPUT_PRICE_USD_PER_MILLION_TOKENS" => {
                        Some(if name == env_name { value } else { "0.34" }.into())
                    }
                    _ => None,
                },
                "openai",
                &adapter,
                "gpt-4.1-mini",
            )
            .expect_err("invalid override should be rejected");
            assert!(err.starts_with("pricing_invalid"), "{err}");
            assert!(err.contains(env_name), "{err}");
        }
    }

    #[test]
    fn live_cloud_plan_rejects_invalid_pricing_override() {
        let filter = BTreeSet::from(["openai".to_string()]);
        let err = live_cloud_target_plan(
            Some(&filter),
            &|name| match name {
                "BENCHFORGE_LIVE_OPENAI_INPUT_PRICE_USD_PER_MILLION_TOKENS" => Some("bad".into()),
                "BENCHFORGE_LIVE_OPENAI_OUTPUT_PRICE_USD_PER_MILLION_TOKENS" => Some("0.34".into()),
                _ => None,
            },
            &|adapter_id| adapter_id == "openai",
        )
        .expect_err("invalid pricing override should stop target planning");

        assert!(err.starts_with("pricing_invalid"), "{err}");
        assert!(err.contains("BENCHFORGE_LIVE_OPENAI_INPUT_PRICE_USD_PER_MILLION_TOKENS"));
    }

    #[test]
    fn live_cloud_plan_builds_gemini_target_without_secret_value() {
        let filter = BTreeSet::from(["gemini".to_string()]);
        let plan = live_cloud_target_plan(Some(&filter), &|_| None, &|adapter_id| {
            adapter_id == "gemini"
        })
        .expect("plan should build");

        assert_eq!(plan.targets.len(), 1);
        let target = &plan.targets[0];
        assert_eq!(target.id, "live-gemini");
        assert_eq!(target.adapter_id, "gemini");
        assert_eq!(target.config["model"], "gemini-2.5-flash-lite");
        assert_eq!(
            target.config["base_url"],
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
        assert_eq!(target.config["api_key_keychain"], "gemini");
        assert_eq!(target.config["api_key_env"], "GEMINI_API_KEY");
        assert_eq!(target.config["max_tokens"], 32);
        assert_eq!(target.config["input_price_usd_per_million_tokens"], 0.1);
        assert_eq!(target.config["output_price_usd_per_million_tokens"], 0.4);
        assert!(!target.config.to_string().contains("AIza"));
    }

    #[test]
    fn live_cloud_benchmark_skips_unpriced_targets_without_blocking_priced_targets() {
        let conn = store::open_memory().expect("store should open");
        let api_key_env: &'static str = Box::leak(
            format!("BENCHFORGE_TEST_LIVE_CLOUD_KEY_{}", uuid::Uuid::new_v4())
                .replace('-', "_")
                .into_boxed_str(),
        );
        let _api_key = ScopedEnvVar::set(api_key_env, "benchforge-test-key");
        for (id, priced) in [("priced-live", true), ("unpriced-live", false)] {
            let mut config = serde_json::json!({
                "model": id,
                "base_url": "https://example.com/v1",
                "api_key_env": api_key_env,
                "max_tokens": 16
            });
            if priced {
                config["input_price_usd_per_million_tokens"] = serde_json::json!(0.1);
                config["output_price_usd_per_million_tokens"] = serde_json::json!(0.2);
            }
            store::upsert_target(
                &conn,
                &store::NewTarget {
                    id: id.into(),
                    name: id.into(),
                    kind: "direct_model".into(),
                    adapter_id: "openai-compatible".into(),
                    config,
                },
            )
            .expect("target should save");
        }

        let (run_target_ids, skipped) = live_cloud_benchmark_target_ids_for_cap(
            &conn,
            &["priced-live".into(), "unpriced-live".into()],
        )
        .expect("live cloud target partition should work");

        assert_eq!(run_target_ids, vec!["priced-live"]);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0]["targetId"], "unpriced-live");
        assert_eq!(skipped[0]["reason"], "missing_pricing");
    }

    #[test]
    fn live_cloud_smoke_status_marks_subset_benchmarks_partial() {
        assert_eq!(live_cloud_smoke_status(false, false, 0), "validated");
        assert_eq!(live_cloud_smoke_status(true, false, 0), "completed");
        assert_eq!(live_cloud_smoke_status(true, true, 0), "partial");
        assert_eq!(live_cloud_smoke_status(true, false, 1), "partial");
    }

    #[test]
    fn live_cloud_no_targets_message_explains_unsupported_filters() {
        let unsupported = vec![live_cloud_skip_provider(
            "bogus-ai",
            "unsupported_provider",
            "bad filter",
        )];
        let message = live_cloud_no_targets_message(&unsupported);
        assert!(message.contains("BENCHFORGE_LIVE_CLOUD_PROVIDERS"));
        assert!(message.contains("bogus-ai"));
        assert!(message.contains("openai"));
        assert!(message.contains("gemini"));
        assert!(!message.contains("Set a provider API key"));

        let mixed = vec![
            live_cloud_skip_provider("bogus-ai", "unsupported_provider", "bad filter"),
            live_cloud_skip_provider("openai", "missing_key", "No key found"),
        ];
        let mixed_message = live_cloud_no_targets_message(&mixed);
        assert!(mixed_message.contains("Unsupported provider filter value"));
        assert!(mixed_message.contains("missing keys"));

        let missing_key = vec![live_cloud_skip_provider(
            "openai",
            "missing_key",
            "No key found",
        )];
        assert!(live_cloud_no_targets_message(&missing_key).contains("provider API key"));
    }

    #[test]
    fn live_cloud_max_cost_override_requires_non_negative_finite_number() {
        assert_eq!(
            live_cloud_max_cost_usd(&|_| None).expect("default cap should parse"),
            LIVE_CLOUD_DEFAULT_MAX_COST_USD
        );
        assert_eq!(
            live_cloud_max_cost_usd(&|_| Some(" 0.25 ".into())).expect("explicit cap should parse"),
            0.25
        );
        assert_eq!(
            live_cloud_max_cost_usd(&|_| Some("   ".into())).expect("blank cap should use default"),
            LIVE_CLOUD_DEFAULT_MAX_COST_USD
        );
        for bad in ["-0.01", "NaN", "inf", "abc"] {
            let err = live_cloud_max_cost_usd(&|_| Some(bad.into()))
                .expect_err("bad cap should be rejected");
            assert!(err.starts_with("max_cost_invalid"), "{err}");
            assert!(err.contains("BENCHFORGE_LIVE_CLOUD_MAX_COST_USD"), "{err}");
        }
    }

    #[test]
    fn strips_curl_color_codes() {
        assert_eq!(strip_ansi_codes("\u{1b}[31merror\u{1b}[0m"), "error");
    }
}
